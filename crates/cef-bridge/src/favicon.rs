// Favicons for the built-in pages, captured from the browser's own cache
// when a page announces its icon, so lists never request
// https://<host>/favicon.ico (which told every listed site it was listed).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

/// Largest icon side requested from CEF (lists draw 20 CSS px; 2x for HiDPI).
const MAX_ICON_PX: u32 = 64;

thread_local! {
    // (browser_id, icon URL) from on_favicon_urlchange.
    pub static PENDING_FAVICON_URLS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // (cache key, PNG bytes) from finished downloads.
    pub static PENDING_FAVICON_IMAGES: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// Fetch `icon_url` through `browser`'s cache (no bypass) as PNG; the result
/// lands in PENDING_FAVICON_IMAGES under `key`.
pub fn fetch_favicon(browser: &Browser, icon_url: &str, key: String) {
    let Some(host) = browser.host() else { return };
    let mut callback = FaviconCallbackBuilder::build(FaviconCallback { key });
    host.download_image(
        Some(&icon_url.into()),
        true as _,
        MAX_ICON_PX,
        false as _,
        Some(&mut callback),
    );
}

#[derive(Clone)]
pub struct FaviconCallback {
    key: String,
}

wrap_download_image_callback! {
    pub struct FaviconCallbackBuilder {
        handler: FaviconCallback,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(
            &self,
            _image_url: Option<&CefString>,
            http_status_code: ::std::os::raw::c_int,
            image: Option<&mut Image>,
        ) {
            let key = &self.handler.key;
            let Some(image) = image else {
                log::debug!("favicon {key}: no image (HTTP {http_status_code})");
                return;
            };
            if !(200..300).contains(&http_status_code) && http_status_code != 0 {
                log::debug!("favicon {key}: HTTP {http_status_code}");
                return;
            }
            if image.is_empty() != 0 {
                // Chromium decoded nothing (SVG icons, broken files).
                log::debug!("favicon {key}: empty image (undecodable format)");
                return;
            }
            let bytes = image_png(image);
            if bytes.is_none() {
                log::debug!("favicon {key}: no usable bitmap");
                return;
            }
            let bytes = bytes.unwrap_or_default();
            if !bytes.is_empty() {
                PENDING_FAVICON_IMAGES.with_borrow_mut(|q| q.push((key.clone(), bytes)));
            }
        }
    }
}

/// PNG bytes for the image's best representation. Asks at the scale the
/// image actually holds (a favicon may only have, say, 2x); if CEF won't
/// encode it, takes the RGBA bitmap and encodes it here.
fn image_png(image: &Image) -> Option<Vec<u8>> {
    let mut scale = 1.0f32;
    let (mut w, mut h) = (0, 0);
    if image.representation_info(1.0, Some(&mut scale), Some(&mut w), Some(&mut h)) == 0 {
        scale = 1.0;
    }
    if let Some(png) = image.as_png(scale, true as _, None, None) {
        let bytes = binary_bytes(&png);
        if !bytes.is_empty() {
            return Some(bytes);
        }
    }
    let (mut pw, mut ph) = (0, 0);
    let bitmap = image.as_bitmap(
        scale,
        ColorType::RGBA_8888,
        AlphaType::POSTMULTIPLIED,
        Some(&mut pw),
        Some(&mut ph),
    )?;
    let rgba = binary_bytes(&bitmap);
    encode_png_rgba(u32::try_from(pw).ok()?, u32::try_from(ph).ok()?, &rgba)
}

fn binary_bytes(value: &BinaryValue) -> Vec<u8> {
    let mut bytes = vec![0u8; value.size()];
    let read = value.data(Some(&mut bytes), 0);
    bytes.truncate(read);
    bytes
}

/// Minimal PNG encoder for small RGBA icons: 8-bit RGBA, no filtering,
/// zlib "stored" (uncompressed) blocks. None if `rgba` doesn't match w×h.
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let row = usize::try_from(width).ok()?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() != row.checked_mul(usize::try_from(height).ok()?)? {
        return None;
    }
    // Scanlines, each prefixed with filter type 0.
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for line in rgba.chunks(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // zlib: header, stored deflate blocks (<= 65535 bytes each), Adler-32.
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &zlib);
    push_chunk(&mut out, b"IEND", &[]);
    Some(out)
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for &byte in kind.iter().chain(data) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

impl FaviconCallbackBuilder {
    pub fn build(handler: FaviconCallback) -> DownloadImageCallback {
        Self::new(handler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk the chunks with an independent CRC and zlib decoder and get the
    /// exact pixels back.
    #[test]
    fn encoded_png_decodes_to_the_same_pixels() {
        use std::io::Read;
        let (w, h) = (16u32, 9u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| [i as u8, 0x80, 0x40, (i * 7) as u8])
            .collect();
        let png = encode_png_rgba(w, h, &rgba).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let mut pos = 8;
        let mut kinds = Vec::new();
        let mut idat = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(kind);
            hasher.update(data);
            assert_eq!(
                hasher.finalize(),
                crc,
                "bad CRC in {:?}",
                std::str::from_utf8(kind)
            );
            if kind == b"IHDR" {
                assert_eq!(&data[..8], &[0, 0, 0, 16, 0, 0, 0, 9]);
                assert_eq!(&data[8..], &[8, 6, 0, 0, 0]);
            }
            if kind == b"IDAT" {
                idat.extend_from_slice(data);
            }
            kinds.push(String::from_utf8(kind.to_vec()).unwrap());
            pos += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);

        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&idat[..])
            .read_to_end(&mut raw)
            .unwrap();
        let row = (w * 4) as usize;
        let pixels: Vec<u8> = raw
            .chunks(row + 1)
            .flat_map(|line| {
                assert_eq!(line[0], 0, "filter type");
                line[1..].to_vec()
            })
            .collect();
        assert_eq!(pixels, rgba);
    }

    #[test]
    fn large_icons_span_several_stored_blocks() {
        use std::io::Read;
        // 256x256 RGBA > 65535 bytes per stored block.
        let rgba = vec![7u8; 256 * 256 * 4];
        let png = encode_png_rgba(256, 256, &rgba).unwrap();
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let idat = &png[41..41 + idat_len];
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(idat)
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), 256 * (256 * 4 + 1));
    }

    #[test]
    fn encoder_rejects_mismatched_buffers() {
        assert!(encode_png_rgba(2, 2, &[0; 15]).is_none());
        assert!(encode_png_rgba(0, 2, &[]).is_none());
    }
}
