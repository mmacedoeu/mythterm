//! Tiny zero-dependency PNG encoder (RGBA8, 8-bit, no compression).
//!
//! Useful for tests that need to dump GPU readback bytes to disk.
//! Not a general-purpose image library — only what's needed for
//! the cinematic-UI prototype's snapshot pipeline.
//!
//! Layout produced: RGBA8, filter type 0 per row, zlib stored blocks
//! (deflate BTYPE=00). Output is deterministic for identical input.
//!
//! The bytes the caller passes in must be in the same row layout
//! that came out of wgpu (i.e. each row is `padded_row` bytes wide
//! where the row is `width * 4` real bytes followed by alignment
//! padding). Padding is dropped before writing.

use std::io::Write;

/// Write an RGBA8 image to `path`.
///
/// `rgba` is the mapped GPU readback buffer. Rows are `padded_row`
/// bytes apart in `rgba` (so it must be at least `padded_row * height`
/// bytes), but each row contains only `width * 4` valid bytes.
pub fn write_png_rgba(
    path: &str,
    width: u32,
    height: u32,
    padded_row: u32,
    rgba: &[u8],
) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(((padded_row + 1) * height) as usize);
    for y in 0..height {
        raw.push(0u8);
        let start = (y * padded_row) as usize;
        let end = start + (width as usize) * 4;
        raw.extend_from_slice(&rgba[start..end]);
    }

    let compressed = zlib_store(&raw);

    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    {
        let mut v = Vec::with_capacity(13);
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.push(8);
        v.push(6);
        v.push(0);
        v.push(0);
        v.push(0);
        write_chunk(&mut out, b"IHDR", &v);
    }
    write_chunk(&mut out, b"IDAT", &compressed);
    write_chunk(&mut out, b"IEND", &[]);

    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = data.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(buf: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[n as usize] = c;
    }
    let mut crc = 0xffff_ffffu32;
    for &b in buf {
        let idx = ((crc ^ b as u32) & 0xff) as usize;
        crc = table[idx] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// Stored (uncompressed) deflate blocks. Valid zlib stream.
fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x78);
    out.push(0x01);

    const MAX_BLOCK: usize = 0xFFFF;
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&adler32(data).to_be_bytes());
        return out;
    }
    let mut blocks = data.chunks(MAX_BLOCK).peekable();
    while let Some(chunk) = blocks.next() {
        let is_last = blocks.peek().is_none();
        out.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    const MOD: u32 = 65_521;
    for &x in data {
        a = (a + x as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a 2x2 image and check the file is a valid PNG
    /// (we don't have a PNG decoder, but we can check the magic
    /// bytes and IHDR chunk).
    #[test]
    fn writes_valid_png_header() {
        let dir = std::env::temp_dir();
        let path = dir.join("wgsl_sdf_png_test.png");
        let path_str = path.to_str().unwrap();
        // 2x2 RGBA: red, green, blue, white
        let rgba = vec![
            255, 0, 0, 255,
            0, 255, 0, 255,
            0, 0, 255, 255,
            255, 255, 255, 255,
        ];
        write_png_rgba(path_str, 2, 2, 8, &rgba).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // PNG signature
        assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        // First chunk is IHDR
        assert_eq!(&bytes[12..16], b"IHDR");
    }
}
