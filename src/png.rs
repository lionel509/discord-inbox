//! U4: indexed-colour PNG writer (std only: stored deflate, hand CRC-32 and Adler-32).

/// A row-major 8-bit indexed pixel buffer. Each byte indexes the palette given to `encode`.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, bg: u8) -> Canvas {
        Canvas { w, h, px: vec![bg; (w as usize) * (h as usize)] }
    }

    /// Fills a rectangle, clipped to the canvas.
    pub fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: u8) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w as i64);
        let y1 = (y + h).min(self.h as i64);
        for yy in y0..y1 {
            let row = yy as usize * self.w as usize;
            for xx in x0..x1 {
                self.px[row + xx as usize] = c;
            }
        }
    }

    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.px[(y as usize) * (self.w as usize) + x as usize]
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut covered = ty.to_vec();
    covered.extend_from_slice(data);
    out.extend_from_slice(&covered);
    out.extend_from_slice(&crc32(&covered).to_be_bytes());
}

/// zlib stream made of stored (uncompressed) deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        let last = blocks.peek().is_none();
        let len = block.len() as u16;
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Encodes the canvas as an 8-bit indexed PNG. `palette` entries are RGB triples.
pub fn encode(c: &Canvas, palette: &[[u8; 3]]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&c.w.to_be_bytes());
    ihdr.extend_from_slice(&c.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let plte: Vec<u8> = palette.iter().flatten().copied().collect();
    chunk(&mut out, b"PLTE", &plte);
    let stride = c.w as usize;
    let mut raw = Vec::with_capacity((stride + 1) * c.h as usize);
    for row in c.px.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}
