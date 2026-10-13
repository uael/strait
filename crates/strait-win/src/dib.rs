//! Conversions between PNG, which the Mac pastes, and the device-independent
//! bitmaps (`CF_DIB`) that most Windows programs copy and paste.

use std::io;

const HEADER: usize = 40;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_owned())
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// Encodes a `CF_DIB` (a BITMAPINFOHEADER followed by pixels) as PNG.
/// Handles the 24- and 32-bit uncompressed layouts that clipboards carry.
pub fn dib_to_png(dib: &[u8]) -> io::Result<Vec<u8>> {
    if dib.len() < HEADER {
        return Err(invalid("bitmap header is truncated"));
    }
    let header_size = u32_at(dib, 0) as usize;
    let width = u32_at(dib, 4) as i32;
    let height = u32_at(dib, 8) as i32;
    let bits = u16_at(dib, 14);
    let compression = u32_at(dib, 16);
    let masks = if compression == BI_BITFIELDS && header_size == HEADER { 12 } else { 0 };
    if width <= 0 || height == 0 || !(bits == 24 || bits == 32) || !(compression == BI_RGB || compression == BI_BITFIELDS) {
        return Err(invalid("unsupported bitmap layout"));
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let stride = (w * usize::from(bits) / 8).div_ceil(4) * 4;
    let start = header_size + masks;
    let pixels = dib.get(start..start + stride * h).ok_or_else(|| invalid("bitmap pixels are truncated"))?;
    let step = usize::from(bits / 8);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        // Positive heights store the bottom row first.
        let src = if height > 0 { h - 1 - row } else { row };
        for px in pixels[src * stride..][..w * step].chunks(step) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], if step == 4 { px[3] } else { 255 }]);
        }
    }
    // Most programs leave the alpha byte of 32-bit bitmaps at zero.
    if step == 4 && rgba.chunks(4).all(|p| p[3] == 0) {
        rgba.chunks_mut(4).for_each(|p| p[3] = 255);
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(&rgba).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)?;
    Ok(png)
}

/// Decodes PNG into a bottom-up 32-bit `CF_DIB`.
pub fn png_to_dib(png_bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(png_bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(io::Error::other)?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(io::Error::other)?;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let rgba = |x: usize, y: usize| -> [u8; 4] {
        let line = &buf[y * frame.line_size..];
        match frame.color_type {
            png::ColorType::Rgba => line[x * 4..x * 4 + 4].try_into().expect("4 bytes"),
            png::ColorType::Rgb => [line[x * 3], line[x * 3 + 1], line[x * 3 + 2], 255],
            png::ColorType::GrayscaleAlpha => [line[x * 2], line[x * 2], line[x * 2], line[x * 2 + 1]],
            png::ColorType::Grayscale => [line[x], line[x], line[x], 255],
            png::ColorType::Indexed => unreachable!("normalize_to_color8 expands palettes"),
        }
    };
    let mut dib = Vec::with_capacity(HEADER + w * h * 4);
    dib.extend_from_slice(&(HEADER as u32).to_le_bytes());
    dib.extend_from_slice(&(w as i32).to_le_bytes());
    dib.extend_from_slice(&(h as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&BI_RGB.to_le_bytes());
    dib.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
    dib.extend_from_slice(&[0; 16]); // resolution and palette fields
    for y in (0..h).rev() {
        for x in 0..w {
            let [r, g, b, a] = rgba(x, y);
            dib.extend_from_slice(&[b, g, r, a]);
        }
    }
    Ok(dib)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(png_bytes).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        (info.width, info.height, buf)
    }

    #[test]
    fn png_survives_a_dib_round_trip() {
        let pixels: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 10) as u8).collect();
        let mut png_bytes = Vec::new();
        let mut e = png::Encoder::new(&mut png_bytes, 3, 2);
        e.set_color(png::ColorType::Rgba);
        e.write_header().unwrap().write_image_data(&pixels).unwrap();
        let back = dib_to_png(&png_to_dib(&png_bytes).unwrap()).unwrap();
        assert_eq!(decode(&back), (3, 2, pixels));
    }

    #[test]
    fn top_down_24_bit_dib_with_padding() {
        // 1x2, rows padded from 3 to 4 bytes, first row is the top.
        let mut dib = vec![0; HEADER];
        dib[0] = 40;
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&(-2i32).to_le_bytes());
        dib[14] = 24;
        dib.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        let (w, h, rgba) = decode(&dib_to_png(&dib).unwrap());
        assert_eq!((w, h), (1, 2));
        assert_eq!(rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn rejects_truncated_pixels() {
        let mut dib = vec![0; HEADER];
        dib[0] = 40;
        dib[4] = 4;
        dib[8] = 4;
        dib[14] = 32;
        assert!(dib_to_png(&dib).is_err());
    }
}
