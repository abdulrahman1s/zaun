//! Streaming Poly1305 over segments of any length, for the lazy secretbox (Crypto/Lazy.hs; crypto spike).
//! The `poly1305` crate has block updates and a one-shot `compute_unpadded` only, so this is a
//! small soft implementation (donna 26-bit limbs, no secret-dependent branches).
use zeroize::Zeroize;

pub struct Poly1305 {
    r: [u64; 5],
    s: [u32; 4],
    h: [u64; 5],
    buf: [u8; 16],
    n: usize,
}

fn le32(b: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Poly1305 {
    #[must_use]
    pub fn new(key: &[u8; 32]) -> Self {
        let r = [
            le32(&key[0..]) & 0x3ffffff,
            (le32(&key[3..]) >> 2) & 0x3ffff03,
            (le32(&key[6..]) >> 4) & 0x3ffc0ff,
            (le32(&key[9..]) >> 6) & 0x3f03fff,
            (le32(&key[12..]) >> 8) & 0x00fffff,
        ];
        let s = [
            le32(&key[16..]) as u32,
            le32(&key[20..]) as u32,
            le32(&key[24..]) as u32,
            le32(&key[28..]) as u32,
        ];
        Poly1305 {
            r,
            s,
            h: [0; 5],
            buf: [0; 16],
            n: 0,
        }
    }

    fn block(&mut self, m: &[u8; 16], hibit: u64) {
        let [r0, r1, r2, r3, r4] = self.r;
        let (s1, s2, s3, s4) = (r1 * 5, r2 * 5, r3 * 5, r4 * 5);
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;
        h0 += le32(&m[0..]) & 0x3ffffff;
        h1 += (le32(&m[3..]) >> 2) & 0x3ffffff;
        h2 += (le32(&m[6..]) >> 4) & 0x3ffffff;
        h3 += (le32(&m[9..]) >> 6) & 0x3ffffff;
        h4 += (le32(&m[12..]) >> 8) | hibit;
        let d0 = h0 * r0 + h1 * s4 + h2 * s3 + h3 * s2 + h4 * s1;
        let mut d1 = h0 * r1 + h1 * r0 + h2 * s4 + h3 * s3 + h4 * s2;
        let mut d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * s4 + h4 * s3;
        let mut d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * s4;
        let mut d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;
        let mut c = d0 >> 26;
        h0 = d0 & 0x3ffffff;
        d1 += c;
        c = d1 >> 26;
        h1 = d1 & 0x3ffffff;
        d2 += c;
        c = d2 >> 26;
        h2 = d2 & 0x3ffffff;
        d3 += c;
        c = d3 >> 26;
        h3 = d3 & 0x3ffffff;
        d4 += c;
        c = d4 >> 26;
        h4 = d4 & 0x3ffffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ffffff;
        h1 += c;
        self.h = [h0, h1, h2, h3, h4];
    }

    pub fn update(&mut self, mut data: &[u8]) {
        if self.n > 0 {
            let take = (16 - self.n).min(data.len());
            self.buf[self.n..self.n + take].copy_from_slice(&data[..take]);
            self.n += take;
            data = &data[take..];
            if self.n < 16 {
                return;
            }
            let b = self.buf;
            self.block(&b, 1 << 24);
            self.n = 0;
        }
        let (blocks, rem) = data.as_chunks::<16>();
        for b in blocks {
            self.block(b, 1 << 24);
        }
        self.buf[..rem.len()].copy_from_slice(rem);
        self.n = rem.len();
    }

    #[must_use]
    pub fn finalize(mut self) -> [u8; 16] {
        if self.n > 0 {
            let mut b = [0u8; 16];
            b[..self.n].copy_from_slice(&self.buf[..self.n]);
            b[self.n] = 1;
            self.block(&b, 0);
        }
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;
        let mut c = h1 >> 26;
        h1 &= 0x3ffffff;
        h2 += c;
        c = h2 >> 26;
        h2 &= 0x3ffffff;
        h3 += c;
        c = h3 >> 26;
        h3 &= 0x3ffffff;
        h4 += c;
        c = h4 >> 26;
        h4 &= 0x3ffffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ffffff;
        h1 += c;
        let mut g0 = h0 + 5;
        c = g0 >> 26;
        g0 &= 0x3ffffff;
        let mut g1 = h1 + c;
        c = g1 >> 26;
        g1 &= 0x3ffffff;
        let mut g2 = h2 + c;
        c = g2 >> 26;
        g2 &= 0x3ffffff;
        let mut g3 = h3 + c;
        c = g3 >> 26;
        g3 &= 0x3ffffff;
        let g4 = (h4 + c).wrapping_sub(1 << 26);
        let mask = (g4 >> 63).wrapping_sub(1); // all ones when h >= p
        let nm = !mask;
        h0 = (h0 & nm) | (g0 & mask);
        h1 = (h1 & nm) | (g1 & mask);
        h2 = (h2 & nm) | (g2 & mask);
        h3 = (h3 & nm) | (g3 & mask);
        h4 = (h4 & nm) | (g4 & mask);
        let w0 = (h0 | (h1 << 26)) as u32 as u64;
        let w1 = ((h1 >> 6) | (h2 << 20)) as u32 as u64;
        let w2 = ((h2 >> 12) | (h3 << 14)) as u32 as u64;
        let w3 = ((h3 >> 18) | (h4 << 8)) as u32 as u64;
        let mut f = w0 + self.s[0] as u64;
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&(f as u32).to_le_bytes());
        f = w1 + self.s[1] as u64 + (f >> 32);
        out[4..8].copy_from_slice(&(f as u32).to_le_bytes());
        f = w2 + self.s[2] as u64 + (f >> 32);
        out[8..12].copy_from_slice(&(f as u32).to_le_bytes());
        f = w3 + self.s[3] as u64 + (f >> 32);
        out[12..16].copy_from_slice(&(f as u32).to_le_bytes());
        out
    }
}

impl Drop for Poly1305 {
    fn drop(&mut self) {
        self.r.zeroize();
        self.s.zeroize();
        self.h.zeroize();
        self.buf.zeroize();
    }
}
