#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = a JPEG XR file. The container and image header parsers never panic, and a stream whose
// header parses within the limits reports a size no larger than they allow.
fuzz_target!(|data: &[u8]| {
    let limits = limits();
    let cfg = jpegxr::decode_config(data, &limits);
    no_internal(&cfg);
    if let Ok(c) = cfg {
        assert!(c.width as u64 <= limits.max_width && c.height as u64 <= limits.max_height);
        assert!((c.width as u64) * (c.height as u64) <= limits.max_pixels);
        // Decoder::new parses the same header.
        let d = jpegxr::Decoder::new(data, &limits).expect("decode_config ok, Decoder::new failed");
        assert_eq!(d.config().0, c.width);
        assert_eq!(d.config().1, c.height);
    }
});
