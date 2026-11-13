//! Pure-Rust Native Island Screenshot Exporter
//! Captures the live PixelBuffer containing real optical background blur
//! and exports it as an un-premultiplied 32-bit RGBA PNG with true alpha transparency.

use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

/// Calculates CRC-32 (IEEE 802.3 standard)
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Calculates Adler-32 (RFC 1950 standard)
fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

/// Encodes raw RGBA scanlines into a standard 32-bit PNG byte stream.
/// Uses zero external dependencies with standard uncompressed Deflate blocks (RFC 1951).
pub fn encode_rgba_to_png(width: u32, height: u32, rgba_pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();

    // 1. PNG Header Signature
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    // 2. IHDR Chunk (13 bytes)
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // Bit depth: 8-bit per channel
    ihdr.push(6); // Color type: 6 (RGBA with alpha)
    ihdr.push(0); // Compression method: 0 (Deflate)
    ihdr.push(0); // Filter method: 0 (Standard)
    ihdr.push(0); // Interlace method: 0 (No interlace)
    write_chunk(&mut out, b"IHDR", &ihdr);

    // 3. Prepare scanlines with filter byte 0 (None) at the start of each row
    let row_len = (width as usize) * 4;
    let mut filtered_scanlines = Vec::with_capacity((height as usize) * (row_len + 1));
    for y in 0..height as usize {
        filtered_scanlines.push(0); // Filter type None
        let start = y * row_len;
        let end = start + row_len;
        if end <= rgba_pixels.len() {
            filtered_scanlines.extend_from_slice(&rgba_pixels[start..end]);
        }
    }

    // 4. Deflate stream with uncompressed blocks (RFC 1950 zlib stream)
    let mut zlib_stream = Vec::new();
    zlib_stream.push(0x78); // CMF: Deflate, 32K window
    zlib_stream.push(0x01); // FLG: FCHECK=1 (0x7801 % 31 == 0)

    let mut pos = 0;
    while pos < filtered_scanlines.len() {
        let chunk_len = (filtered_scanlines.len() - pos).min(65535);
        let is_last = (pos + chunk_len) == filtered_scanlines.len();

        zlib_stream.push(if is_last { 0x01 } else { 0x00 }); // BFINAL and BTYPE=00
        zlib_stream.extend_from_slice(&(chunk_len as u16).to_le_bytes());
        zlib_stream.extend_from_slice(&(!(chunk_len as u16)).to_le_bytes());
        zlib_stream.extend_from_slice(&filtered_scanlines[pos..pos + chunk_len]);

        pos += chunk_len;
    }

    // Adler-32 checksum at end of zlib stream
    let checksum = adler32(&filtered_scanlines);
    zlib_stream.extend_from_slice(&checksum.to_be_bytes());

    write_chunk(&mut out, b"IDAT", &zlib_stream);

    // 5. IEND Chunk
    write_chunk(&mut out, b"IEND", &[]);

    out
}

fn write_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);

    let mut crc_buf = Vec::with_capacity(4 + data.len());
    crc_buf.extend_from_slice(chunk_type);
    crc_buf.extend_from_slice(data);
    let crc = crc32(&crc_buf);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Saves the live PixelBuffer to Pictures\NotDynamicIsland as a transparent PNG.
/// Automatically crops around the island bounding box with comfortable padding.
pub fn save_island_screenshot(pixels: &[u32], full_w: usize, full_h: usize) -> Option<PathBuf> {
    if pixels.is_empty() || full_w == 0 || full_h == 0 {
        return None;
    }

    // 1. Find bounding box of visible pixels (alpha > 8)
    let mut min_x = full_w;
    let mut max_x = 0;
    let mut min_y = full_h;
    let mut max_y = 0;
    let mut has_visible = false;

    for y in 0..full_h {
        for x in 0..full_w {
            let p = pixels[y * full_w + x];
            let a = ((p >> 24) & 0xFF) as u8;
            if a > 8 {
                has_visible = true;
                if x < min_x { min_x = x; }
                if x > max_x { max_x = x; }
                if y < min_y { min_y = y; }
                if y > max_y { max_y = y; }
            }
        }
    }

    if !has_visible {
        min_x = 0;
        max_x = full_w - 1;
        max_y = full_h - 1;
    }

    // 2. Add padding around island (flush at top y=0, 16px on sides and bottom)
    let crop_x0 = min_x.saturating_sub(16);
    let crop_x1 = (max_x + 16).min(full_w - 1);
    let crop_y0 = 0usize; // Flush with monitor notch bezel at top
    let crop_y1 = (max_y + 16).min(full_h - 1);

    let crop_w = (crop_x1 - crop_x0 + 1) as u32;
    let crop_h = (crop_y1 - crop_y0 + 1) as u32;

    // 3. Extract and un-premultiply RGBA pixels
    let mut rgba = Vec::with_capacity((crop_w as usize) * (crop_h as usize) * 4);
    for y in crop_y0..=crop_y1 {
        for x in crop_x0..=crop_x1 {
            let p = pixels[y * full_w + x];
            let a = ((p >> 24) & 0xFF) as u8;
            let r_pre = ((p >> 16) & 0xFF) as u32;
            let g_pre = ((p >> 8) & 0xFF) as u32;
            let b_pre = (p & 0xFF) as u32;

            if a > 0 {
                let r = (r_pre * 255 / a as u32).min(255) as u8;
                let g = (g_pre * 255 / a as u32).min(255) as u8;
                let b = (b_pre * 255 / a as u32).min(255) as u8;
                rgba.push(r);
                rgba.push(g);
                rgba.push(b);
                rgba.push(a);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    // 4. Encode to pure PNG
    let png_bytes = encode_rgba_to_png(crop_w, crop_h, &rgba);

    // 5. Determine target folder: %USERPROFILE%\Pictures\NotDynamicIsland
    let target_dir = if let Some(profile) = std::env::var_os("USERPROFILE") {
        PathBuf::from(profile).join("Pictures").join("NotDynamicIsland")
    } else {
        PathBuf::from("screenshots")
    };

    if create_dir_all(&target_dir).is_err() {
        return None;
    }

    // 6. Format filename with timestamp
    let epoch_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let filename = format!("island_snapshot_{}.png", epoch_secs);
    let target_path = target_dir.join(filename);

    if let Ok(mut file) = File::create(&target_path) {
        if file.write_all(&png_bytes).is_ok() {
            return Some(target_path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_rgba_to_png_roundtrip() {
        // Create a 4x4 test pattern with semi-transparent pixels
        let width = 4u32;
        let height = 4u32;
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.push(x as u8 * 60);
                pixels.push(y as u8 * 60);
                pixels.push(180);
                pixels.push(128); // 50% opacity
            }
        }

        let png = encode_rgba_to_png(width, height, &pixels);
        assert!(!png.is_empty());
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn test_save_island_screenshot_crop() {
        let full_w = 100;
        let full_h = 50;
        let mut buffer = vec![0u32; full_w * full_h];

        // Draw a simulated island in center: x=30..70, y=0..20 with alpha=200
        for y in 0..20 {
            for x in 30..70 {
                buffer[y * full_w + x] = 0xC8FFFFFF; // Alpha 200, white
            }
        }

        let temp_dir = std::env::temp_dir();
        unsafe {
            std::env::set_var("USERPROFILE", temp_dir.as_os_str());
        }

        let res = save_island_screenshot(&buffer, full_w, full_h);
        assert!(res.is_some());
        let path = res.unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 100);
        let _ = std::fs::remove_file(path);
    }
}
