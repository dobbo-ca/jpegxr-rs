#![no_main]
#![allow(dead_code)]
use jpegxr::ColorModel;
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = [JPEG XR file][k: u8]. decode_component(k % 4) equals channel k of decode's image
// (k 0 of Gray16, k 0 to 2 of Rgba64) and fails exactly when decode does or k is not a channel,
// as Go's checkComponents; neither panics.
fuzz_target!(|data: &[u8]| {
    let Some((&k, stream)) = data.split_last() else {
        return;
    };
    let k = usize::from(k % 4);
    let (limits, cancel) = (limits(), no_cancel());
    let img = jpegxr::decode(stream, &limits, &cancel);
    let got = jpegxr::decode_component(stream, k, &limits, &cancel);
    no_internal(&img);
    no_internal(&got);
    // The two calls charge differently, so a limit error on either side proves nothing.
    if img.as_ref().err().is_some_and(is_limit) || got.as_ref().err().is_some_and(is_limit) {
        return;
    }
    let want = img.ok().and_then(|i| match i.color_model {
        ColorModel::Gray16 if k == 0 => Some(i.pix),
        ColorModel::Rgba64 if k < 3 => Some(
            i.pix
                .chunks_exact(8)
                .flat_map(|p| [p[2 * k], p[2 * k + 1]])
                .collect(),
        ),
        _ => None,
    });
    match (want, got) {
        (Some(w), Ok(g)) => {
            assert_eq!(g.color_model, ColorModel::Gray16);
            assert!(g.pix == w, "component {k} differs from decode's channel");
        }
        (None, Err(_)) => {}
        (Some(_), Err(e)) => panic!("decode_component {k} failed ({e}), decode did not"),
        (None, Ok(_)) => panic!("decode_component {k} succeeded, decode or the channel did not"),
    }
});
