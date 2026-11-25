// Ported from jxrlib JXRGlueJxr.c (BSD-2, see NOTICE).

//! Parses the HD Photo (TIFF-like) container around a JPEG XR codestream.

use crate::Error;

/// The IFD entries the decoder needs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Info {
    /// GUID bytes as stored.
    pub pixel_format: [u8; 16],
    pub image_offset: u32,
    #[allow(dead_code)] // read for completeness; the public API decodes the primary image only
    pub image_byte_count: u32,
    pub alpha_offset: u32,
    pub alpha_byte_count: u32,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// Parses the container header and first IFD, as `ReadContainer` does.
pub(crate) fn read(data: &[u8]) -> Result<Info, Error> {
    let mut info = Info::default();
    if data.len() < 8 || data[0] != b'I' || data[1] != b'I' {
        return Err(Error::Container);
    }
    let id = u16_at(data, 2);
    if id & 0xff != 0xbc || (id >> 8 != 0 && id >> 8 != 1) {
        return Err(Error::Container);
    }
    let len = data.len() as i64;
    let mut off = i64::from(u32_at(data, 4));
    if off + 2 > len {
        return Err(Error::Container);
    }
    let n = i64::from(u16_at(data, off as usize));
    if n == 0 || n == 0xffff {
        return Err(Error::Container);
    }
    off += 2;
    if off + 12 * n > len {
        return Err(Error::Container);
    }
    let mut pf = false;
    for i in 0..n {
        let e = (off + 12 * i) as usize;
        let tag = u16_at(data, e);
        let count = u32_at(data, e + 4);
        let value = u32_at(data, e + 8);
        match tag {
            0xbc01 => {
                // pixel format
                if i64::from(value) + 16 > len {
                    return Err(Error::Container);
                }
                let v = value as usize;
                info.pixel_format.copy_from_slice(&data[v..v + 16]);
                pf = true;
            }
            0xbc02 | 0xbcc0 | 0xbcc1 | 0xbcc2 | 0xbcc3 | 0xbc82 | 0xbc83 => {
                if count != 1 {
                    return Err(Error::Container);
                }
                match tag {
                    0xbcc0 => info.image_offset = value,
                    0xbcc1 => info.image_byte_count = value,
                    0xbcc2 => info.alpha_offset = value,
                    0xbcc3 => info.alpha_byte_count = value,
                    _ => {}
                }
            }
            // width, height
            0xbc80 | 0xbc81 if value == 0 => return Err(Error::Container),
            _ => {}
        }
    }
    if !pf || i64::from(info.image_offset) >= len {
        return Err(Error::Container);
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FuzzRead`'s seeds: every synth file, and a copy with the IFD entry count corrupted.
    /// `read` never panics, and a valid file parses.
    #[test]
    fn read_synth_seeds() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/synth");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().extension().is_none_or(|x| x != "jxr") {
                continue;
            }
            let b = std::fs::read(e.path()).unwrap();
            let info = read(&b).unwrap();
            assert_eq!(info.pixel_format[..15], crate::PIXFMT_PREFIX);
            let mut m = b.clone();
            m[8] ^= 0xff;
            m[9] ^= 0xff;
            let _ = read(&m);
            for len in 0..b.len().min(64) {
                let _ = read(&b[..len]);
            }
            n += 1;
        }
        assert_eq!(n, 21);
    }
}
