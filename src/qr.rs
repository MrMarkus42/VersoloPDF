//! QR codes (ISO/IEC 18004) for the "Barcode" form field: byte mode,
//! error correction level M (about 15 % of the code may be damaged),
//! versions 1–40. Written for VersoloPDF (no extra library), following the
//! structure of the specification step by step.

use crate::{Error, Result};

/// Error correction codewords per block and number of blocks for level M,
/// index = version (index 0 unused).
const ECC_PER_BLOCK: [u8; 41] = [
    0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28,
    28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
];
const BLOCKS: [u8; 41] = [
    0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28,
    29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49,
];

/// A finished QR code: `size` × `size` modules, true = dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrCode {
    pub size: usize,
    pub version: usize,
    modules: Vec<bool>,
}

impl QrCode {
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }
}

/// Modules available for data and error correction.
fn raw_data_modules(ver: usize) -> usize {
    let mut n = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let align = ver / 7 + 2;
        n -= (25 * align - 10) * align - 55;
        if ver >= 7 {
            n -= 36;
        }
    }
    n
}

fn data_codewords(ver: usize) -> usize {
    raw_data_modules(ver) / 8 - usize::from(ECC_PER_BLOCK[ver]) * usize::from(BLOCKS[ver])
}

// Galois field GF(2^8) with the polynomial x^8 + x^4 + x^3 + x^2 + 1.
fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u16 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11D);
        z ^= u16::from((y >> i) & 1) * u16::from(x);
    }
    z as u8
}

fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut result = vec![0u8; degree];
    result[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            result[j] = gf_mul(result[j], root);
            if j + 1 < degree {
                result[j] ^= result[j + 1];
            }
        }
        root = gf_mul(root, 0x02);
    }
    result
}

fn rs_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; divisor.len()];
    for &b in data {
        let factor = b ^ result.remove(0);
        result.push(0);
        for (r, &d) in result.iter_mut().zip(divisor) {
            *r ^= gf_mul(d, factor);
        }
    }
    result
}

/// Data codewords of `text` (byte mode, UTF-8) for the smallest version.
fn data_bits(text: &[u8]) -> Result<(usize, Vec<u8>)> {
    let ver = (1..=40)
        .find(|&v| {
            let count_bits = if v < 10 { 8 } else { 16 };
            4 + count_bits + text.len() * 8 <= data_codewords(v) * 8
        })
        .ok_or_else(|| Error::Invalid("Text zu lang für einen QR-Code".into()))?;
    let mut bits: Vec<bool> = Vec::new();
    let mut push = |value: usize, n: usize| {
        for i in (0..n).rev() {
            bits.push((value >> i) & 1 == 1);
        }
    };
    push(0b0100, 4);
    push(text.len(), if ver < 10 { 8 } else { 16 });
    for &b in text {
        push(usize::from(b), 8);
    }
    let capacity = data_codewords(ver) * 8;
    let terminator = (capacity - bits.len()).min(4);
    bits.extend(std::iter::repeat_n(false, terminator));
    bits.extend(std::iter::repeat_n(false, (8 - bits.len() % 8) % 8));
    let mut bytes: Vec<u8> =
        bits.chunks(8).map(|c| c.iter().fold(0u8, |acc, &b| (acc << 1) | u8::from(b))).collect();
    let mut pad = [0xEC, 0x11].into_iter().cycle();
    while bytes.len() < data_codewords(ver) {
        bytes.push(pad.next().unwrap_or(0xEC));
    }
    Ok((ver, bytes))
}

/// Splits into blocks, adds error correction and interleaves.
fn with_error_correction(ver: usize, data: &[u8]) -> Vec<u8> {
    let blocks = usize::from(BLOCKS[ver]);
    let ecc_len = usize::from(ECC_PER_BLOCK[ver]);
    let raw = raw_data_modules(ver) / 8;
    let short_blocks = blocks - raw % blocks;
    let short_len = raw / blocks;
    let divisor = rs_divisor(ecc_len);
    let mut all: Vec<Vec<u8>> = Vec::with_capacity(blocks);
    let mut k = 0;
    for i in 0..blocks {
        let len = short_len - ecc_len + usize::from(i >= short_blocks);
        let dat = &data[k..k + len];
        k += len;
        let mut block = dat.to_vec();
        if i < short_blocks {
            // A placeholder, so all blocks have the same length.
            block.push(0);
        }
        block.extend(rs_remainder(dat, &divisor));
        all.push(block);
    }
    let mut out = Vec::with_capacity(raw);
    for i in 0..all[0].len() {
        for (j, block) in all.iter().enumerate() {
            if i != short_len - ecc_len || j >= short_blocks {
                out.push(block[i]);
            }
        }
    }
    out
}

struct Grid {
    size: usize,
    dark: Vec<bool>,
    function: Vec<bool>,
}

impl Grid {
    fn set(&mut self, x: usize, y: usize, dark: bool) {
        let i = y * self.size + x;
        self.dark[i] = dark;
        self.function[i] = true;
    }

    fn finder(&mut self, cx: isize, cy: isize) {
        for dy in -4..=4isize {
            for dx in -4..=4isize {
                let (x, y) = (cx + dx, cy + dy);
                if (0..self.size as isize).contains(&x) && (0..self.size as isize).contains(&y) {
                    let dist = dx.abs().max(dy.abs());
                    self.set(x as usize, y as usize, dist != 2 && dist != 4);
                }
            }
        }
    }

    fn alignment(&mut self, cx: usize, cy: usize) {
        for dy in -2..=2isize {
            for dx in -2..=2isize {
                let dist = dx.abs().max(dy.abs());
                self.set((cx as isize + dx) as usize, (cy as isize + dy) as usize, dist != 1);
            }
        }
    }

    fn format_bits(&mut self, mask: u32) {
        // Level M has the format value 0.
        let data = mask;
        let mut rem = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        }
        let bits = ((data << 10) | rem) ^ 0x5412;
        let bit = |i: u32| (bits >> i) & 1 == 1;
        let s = self.size;
        for i in 0..=5 {
            self.set(8, i, bit(i as u32));
        }
        self.set(8, 7, bit(6));
        self.set(8, 8, bit(7));
        self.set(7, 8, bit(8));
        for i in 9..15 {
            self.set(14 - i, 8, bit(i as u32));
        }
        for i in 0..8 {
            self.set(s - 1 - i, 8, bit(i as u32));
        }
        for i in 8..15 {
            self.set(8, s - 15 + i, bit(i as u32));
        }
        self.set(8, s - 8, true);
    }
}

fn alignment_positions(ver: usize, size: usize) -> Vec<usize> {
    if ver == 1 {
        return Vec::new();
    }
    let count = ver / 7 + 2;
    let step = (ver * 8 + count * 3 + 5) / (count * 4 - 4) * 2;
    let mut result = vec![6];
    let mut pos = size - 7;
    for _ in 0..count - 1 {
        result.insert(1, pos);
        pos -= step;
    }
    result
}

fn masked(mask: u8, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => (x / 3 + y / 2).is_multiple_of(2),
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3).is_multiple_of(2),
        _ => ((x + y) % 2 + x * y % 3).is_multiple_of(2),
    }
}

/// Penalty of a finished symbol (runs, 2×2 blocks and balance; the finder
/// like patterns are left out – any mask gives a valid code).
fn penalty(size: usize, dark: &[bool]) -> usize {
    let at = |x: usize, y: usize| dark[y * size + x];
    let mut p = 0;
    for line in 0..size {
        for horizontal in [true, false] {
            let mut run = 1;
            for i in 1..size {
                let (a, b) =
                    if horizontal { (at(i, line), at(i - 1, line)) } else { (at(line, i), at(line, i - 1)) };
                if a == b {
                    run += 1;
                    if run == 5 {
                        p += 3;
                    } else if run > 5 {
                        p += 1;
                    }
                } else {
                    run = 1;
                }
            }
        }
    }
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let c = at(x, y);
            if c == at(x + 1, y) && c == at(x, y + 1) && c == at(x + 1, y + 1) {
                p += 3;
            }
        }
    }
    let dark_count = dark.iter().filter(|d| **d).count();
    let total = size * size;
    let k = (dark_count * 20).abs_diff(total * 10).div_ceil(total).saturating_sub(1);
    p + k * 10
}

/// Makes the QR code for `text`.
pub fn encode(text: &str) -> Result<QrCode> {
    let (ver, data) = data_bits(text.as_bytes())?;
    let codewords = with_error_correction(ver, &data);
    let size = ver * 4 + 17;
    let mut g = Grid { size, dark: vec![false; size * size], function: vec![false; size * size] };
    // Timing patterns, finders, alignment patterns.
    for i in 0..size {
        g.set(6, i, i % 2 == 0);
        g.set(i, 6, i % 2 == 0);
    }
    g.finder(3, 3);
    g.finder(size as isize - 4, 3);
    g.finder(3, size as isize - 4);
    let pos = alignment_positions(ver, size);
    let n = pos.len();
    for i in 0..n {
        for j in 0..n {
            if !(i == 0 && j == 0 || i == 0 && j == n - 1 || i == n - 1 && j == 0) {
                g.alignment(pos[i], pos[j]);
            }
        }
    }
    g.format_bits(0); // reserves the area; drawn again with the mask
    if ver >= 7 {
        let mut rem = ver as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
        }
        let bits = ((ver as u32) << 12) | rem;
        for i in 0..18 {
            let bit = (bits >> i) & 1 == 1;
            let (a, b) = (size - 11 + i % 3, i / 3);
            g.set(a, b, bit);
            g.set(b, a, bit);
        }
    }
    // Data in the zigzag order, two columns at a time from the right.
    let mut i = 0usize;
    let mut right = size as isize - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = (right + 1) & 2 == 0;
                let y = if upward { size - 1 - vert } else { vert };
                let idx = y * size + x;
                if !g.function[idx] && i < codewords.len() * 8 {
                    g.dark[idx] = (codewords[i >> 3] >> (7 - (i & 7))) & 1 == 1;
                    i += 1;
                }
            }
        }
        right -= 2;
    }
    // The mask with the lowest penalty.
    let mut best: Option<(usize, Vec<bool>)> = None;
    for mask in 0..8u8 {
        let mut candidate = Grid { size, dark: g.dark.clone(), function: g.function.clone() };
        for y in 0..size {
            for x in 0..size {
                let idx = y * size + x;
                if !candidate.function[idx] && masked(mask, x, y) {
                    candidate.dark[idx] = !candidate.dark[idx];
                }
            }
        }
        candidate.format_bits(u32::from(mask));
        let p = penalty(size, &candidate.dark);
        if best.as_ref().is_none_or(|(bp, _)| p < *bp) {
            best = Some((p, candidate.dark));
        }
    }
    let modules = best.map(|(_, d)| d).unwrap_or_default();
    Ok(QrCode { size, version: ver, modules })
}

/// PDF drawing operators for the code in a `width` × `height` box: dark
/// modules as filled rectangles on a white background with a quiet zone.
pub fn pdf_content(code: &QrCode, width: f32, height: f32) -> String {
    let quiet = 2.0;
    let side = width.min(height);
    let unit = side / (code.size as f32 + 2.0 * quiet);
    let x0 = (width - side) / 2.0 + quiet * unit;
    let y0 = (height - side) / 2.0 + quiet * unit;
    let mut s = format!("1 g 0 0 {width:.2} {height:.2} re f 0 g\n");
    for y in 0..code.size {
        // Runs of dark modules in a row as one rectangle.
        let mut x = 0;
        while x < code.size {
            if code.get(x, y) {
                let start = x;
                while x < code.size && code.get(x, y) {
                    x += 1;
                }
                let px = x0 + start as f32 * unit;
                let py = y0 + (code.size - 1 - y) as f32 * unit;
                s.push_str(&format!("{px:.3} {py:.3} {:.3} {unit:.3} re\n", (x - start) as f32 * unit));
            } else {
                x += 1;
            }
        }
    }
    s.push_str("f\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacities_and_sizes() {
        // Byte capacities of level M (from the specification's table).
        assert_eq!(data_codewords(1), 16);
        assert_eq!(data_codewords(2), 28);
        assert_eq!(data_codewords(10), 216);
        assert_eq!(data_codewords(40), 2334);
        let c = encode("https://mrmarkus42.github.io/VersoloPDF/").unwrap();
        assert_eq!((c.version, c.size), (3, 29));
        assert!(encode(&"x".repeat(2400)).is_err());
        assert_eq!(encode(&"x".repeat(1000)).unwrap().version, 26);
    }

    /// Writes sample codes as PNG for checking them with an independent
    /// reader (`target/qr-check`, run by hand with `-- --ignored`).
    #[test]
    #[ignore = "writes pictures for a check by hand"]
    fn sample_pictures() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qr-check");
        std::fs::create_dir_all(&dir).unwrap();
        let long: String = (0..60).map(|i| format!("Zeile {i}: Grüße aus Köln! ")).collect();
        let samples = [
            "Hallo".to_string(),
            "https://mrmarkus42.github.io/VersoloPDF/".to_string(),
            "Grüße aus Köln – äöüß €".to_string(),
            long.chars().take(200).collect(),
            long.chars().take(600).collect(),
            long.chars().take(1500).collect(),
        ];
        let mut list = String::new();
        for (n, text) in samples.iter().enumerate() {
            let c = encode(text).unwrap();
            let scale = 6u32;
            let quiet = 4u32;
            let px = (c.size as u32 + 2 * quiet) * scale;
            let img = image::GrayImage::from_fn(px, px, |x, y| {
                let (mx, my) = ((x / scale) as i64 - quiet as i64, (y / scale) as i64 - quiet as i64);
                let dark = mx >= 0 && my >= 0 && c.get(mx as usize, my as usize);
                image::Luma([if dark { 0 } else { 255 }])
            });
            img.save(dir.join(format!("qr{n}.png"))).unwrap();
            list.push_str(&format!("{n}\t{}\t{}\n", c.version, text));
        }
        std::fs::write(dir.join("texts.txt"), list).unwrap();
    }

    #[test]
    fn finder_patterns_and_reed_solomon() {
        let c = encode("Hallo").unwrap();
        // Corner of the top left finder: dark, then the white separator ring.
        assert!(c.get(0, 0) && c.get(6, 6) && !c.get(7, 7) && c.get(2, 2) && !c.get(1, 1));
        // Dark module above the bottom left finder.
        assert!(c.get(8, c.size - 8));
        // The remainder of a codeword sequence that already is a code word is zero.
        let div = rs_divisor(10);
        let data = [32u8, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17];
        let ecc = rs_remainder(&data, &div);
        // Known error correction of "HELLO WORLD" 1-M (from the specification example).
        assert_eq!(ecc, [196, 35, 39, 119, 235, 215, 231, 226, 93, 23]);
    }
}
