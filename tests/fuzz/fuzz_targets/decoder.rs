#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;
use std::cell::RefCell;

include!("common.rs");

thread_local! {
    /// The destination of the previous input, reused as it is (callers that decode in a loop reuse one buffer).
    static DST: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The buffers of every earlier input, kept (a worker reuses one `Scratch`).
    static SCRATCH: RefCell<jpegxr::Scratch> = RefCell::default();
}

// data = [JPEG XR file][pad: u8]. Decoder::new and decode_into with rows padded by pad % 5 bytes:
// into a fresh buffer and into the reused one (stale bytes from earlier inputs) they give the same
// result and the same pixels, the padding is never written, a dst one byte short is DstTooSmall and
// is not written, an odd stride with 16-bit samples is OddStride and is not written, a set cancel
// flag stops a stream that decodes, and two threads (every image, however small, through the
// pipeline) give the same result and the same bytes as one, and a `Scratch` kept from every
// earlier input gives the result and the pixels of the fresh decoder, after a cancelled call too,
// and on two threads.
fuzz_target!(|data: &[u8]| {
    let Some((&pad, stream)) = data.split_last() else {
        return;
    };
    let limits = limits();
    let Ok(d) = jpegxr::Decoder::new(stream, &limits) else {
        return;
    };
    let (w, h, f) = d.config();
    let row = match f {
        jpegxr::Format::Gray8 => w,
        jpegxr::Format::Gray16 => 2 * w,
        jpegxr::Format::Bgr24 => 3 * w,
        jpegxr::Format::Bgr48 => 6 * w,
    };
    // Odd strides too: with 16-bit samples the decoder refuses them (jxrlib would take stride / 2).
    let stride = row + usize::from(pad % 5);
    let odd16 = matches!(f, jpegxr::Format::Gray16 | jpegxr::Format::Bgr48) && stride % 2 == 1;
    let need = if h == 0 { 0 } else { (h - 1) * stride + row };
    let cancel = no_cancel();

    let mut fresh = vec![0x5a; need];
    if odd16 {
        assert_eq!(
            d.decode_into(&mut fresh, stride, &cancel),
            Err(jpegxr::Error::OddStride)
        );
        assert!(fresh.iter().all(|&b| b == 0x5a), "wrote with an odd stride");
        if need > 0 {
            let mut tight = vec![0x5a; need - 1];
            assert_eq!(
                d.decode_into(&mut tight, stride, &cancel),
                Err(jpegxr::Error::DstTooSmall)
            );
        }
        return;
    }
    let r1 = d.decode_into(&mut fresh, stride, &cancel);
    no_internal(&r1);
    if need > 0 {
        let mut tight = vec![0x5a; need - 1];
        assert_eq!(
            d.decode_into(&mut tight, stride, &cancel),
            Err(jpegxr::Error::DstTooSmall)
        );
        assert!(tight.iter().all(|&b| b == 0x5a), "wrote into a short dst");
    }
    // The pipeline on two threads against the serial decode above.
    let two = Limits {
        max_threads: 2,
        ..limits
    };
    let mut par = vec![0x5a; need];
    let r3 = jpegxr::Decoder::new(stream, &two).map(|d2| {
        let (r3, _) = d2.decode_into_probe(&mut par, stride, &cancel, true);
        assert_eq!(r1, r3, "two threads give another result");
        assert!(fresh == par, "two threads give other bytes");
        r3
    });
    SCRATCH.with_borrow_mut(|scratch| {
        // A cancelled call leaves the scratch half used: the next decode must not see it.
        let stop = AtomicBool::new(true);
        let want = d.decode_into(&mut vec![0x5a; need], stride, &stop);
        if let Ok(c) = jpegxr::Decoder::reusing(stream, &limits, None, scratch) {
            let got = c.decode_into(&mut vec![0x5a; need], stride, &stop);
            assert_eq!(want, got, "a kept scratch cancels differently");
        }
        let Ok(kept) = jpegxr::Decoder::reusing(stream, &limits, None, scratch) else {
            panic!("reusing fails where new does not");
        };
        let mut out = vec![0x5a; need];
        let r4 = kept.decode_into(&mut out, stride, &cancel);
        assert_eq!(r1, r4, "a kept scratch gives another result");
        if r1.is_ok() {
            assert!(fresh == out, "a kept scratch gives other bytes");
        }
        drop(kept);
        if let Ok(r3) = r3 {
            let Ok(kept2) = jpegxr::Decoder::reusing(stream, &two, None, scratch) else {
                panic!("reusing fails where new does not");
            };
            let mut out = vec![0x5a; need];
            let (r5, _) = kept2.decode_into_probe(&mut out, stride, &cancel, true);
            assert_eq!(r3, r5, "a kept scratch on two threads gives another result");
            if r3.is_ok() {
                assert!(par == out, "a kept scratch on two threads gives other bytes");
            }
        }
    });
    DST.with(|dst| {
        let mut dst = dst.borrow_mut();
        // Keep the old bytes; only the new tail gets a value.
        dst.resize(need, 0xc3);
        dst.truncate(need);
        let before = dst.clone();
        let r2 = d.decode_into(&mut dst, stride, &cancel);
        assert_eq!(r1, r2, "fresh and reused dst differ in result");
        for y in 0..h {
            let (a, b) = (y * stride, y * stride + row);
            if r1.is_ok() {
                assert!(fresh[a..b] == dst[a..b], "row {y} depends on the old dst");
            }
            let pad_end = (b + stride - row).min(need);
            assert!(
                dst[b..pad_end] == before[b..pad_end],
                "wrote row {y}'s padding"
            );
            assert!(
                fresh[b..pad_end].iter().all(|&c| c == 0x5a),
                "wrote row {y}'s padding"
            );
        }
        if r1.is_ok() {
            let stop = AtomicBool::new(true);
            assert_eq!(
                d.decode_into(&mut dst, stride, &stop),
                Err(jpegxr::Error::Cancelled)
            );
        }
    });
});
