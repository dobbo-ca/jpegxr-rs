#![no_main]
#![allow(dead_code)]
use libfuzzer_sys::fuzz_target;

include!("common.rs");

// data = a JPEG XR file. Decode never panics (an internal error is a bug, as in Go's FuzzDecode),
// and the image has the size and colour model decode_config reports and the pixel count they
// imply.
fuzz_target!(|data: &[u8]| {
    let (limits, cancel) = (limits(), no_cancel());
    let cfg = jpegxr::decode_config(data, &limits);
    let img = jpegxr::decode(data, &limits, &cancel);
    no_internal(&cfg);
    no_internal(&img);
    if let Ok(img) = img {
        let c = cfg.expect("decode succeeded but decode_config failed");
        assert_eq!(
            (c.width, c.height, c.color_model),
            (img.width, img.height, img.color_model)
        );
        assert_eq!(
            img.pix.len(),
            img.width * img.height * img.color_model.bytes_per_pixel()
        );
    }
});
