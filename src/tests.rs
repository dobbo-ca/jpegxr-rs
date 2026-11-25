//! Tests that reach past the public API.

use super::*;

/// The container's pixel format is 24bppBGR, 48bppRGB, 8bppGray, 16bppGray or 48bpp3Channels
/// (`gateAFormat`).
pub(crate) fn gate_a_format(data: &[u8]) -> bool {
    container::read(data).is_ok_and(|info| {
        info.pixel_format[..15] == PIXFMT_PREFIX
            && matches!(info.pixel_format[15], 0x0c | 0x15 | 0x08 | 0x0b | 0x26)
    })
}

/// `TestDecodeComponentPlane`: 48bpp3Channels takes the one-channel path, not the full decode.
#[test]
fn decode_component_plane() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/synth/N3x16_lossless_tiled_ov1.jxr"
    );
    let data = std::fs::read(path).unwrap();
    let limits = Limits::default();
    let cancel = AtomicBool::new(false);
    let d = Decoder::new(&data, &limits).unwrap();
    let (w, h, _) = d.config();
    let full = decode(&data, &limits, &cancel).unwrap();
    for k in 0..3 {
        let mut dst = vec![0u8; 2 * w * h];
        let cx = Ctx::new(&limits, None, &cancel, false);
        let mut sc = codec::Scratch::default();
        d.decode_plane(k, &mut dst, 2 * w, &cx, &mut |_, _| Ok(()), &mut sc)
            .unwrap();
        // An odd stride is refused after the size check, as on the other 16-bit paths.
        let mut wide = vec![0u8; 2 * w * h + h];
        assert_eq!(
            d.decode_plane(k, &mut wide, 2 * w + 1, &cx, &mut |_, _| Ok(()), &mut sc),
            Err(Error::OddStride)
        );
        assert_eq!(
            d.decode_plane(k, &mut dst, 2 * w - 1, &cx, &mut |_, _| Ok(()), &mut sc),
            Err(Error::DstTooSmall)
        );
        for i in 0..w * h {
            let le = u16::from_le_bytes([dst[2 * i], dst[2 * i + 1]]);
            let be = u16::from_be_bytes([full.pix[8 * i + 2 * k], full.pix[8 * i + 2 * k + 1]]);
            assert_eq!(le, be, "k {k}: plane differs at pixel {i}");
        }
    }
}
