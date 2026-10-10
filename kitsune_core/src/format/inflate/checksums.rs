//! checksums (split out of `inflate.rs`).

pub(super) const fn make_crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            let prev = t[s - 1][i];
            t[s][i] = t[0][(prev & 0xFF) as usize] ^ (prev >> 8);
            s += 1;
        }
        i += 1;
    }
    t
}

pub(super) static CRC_TABLES: [[u32; 256]; 8] = make_crc_tables();

/// Incremental CRC-32 (IEEE 802.3, as used by PNG and zlib's `crc32`).
#[derive(Clone, Copy, Debug)]
pub struct Crc32 {
    pub(super) state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    /// Feeds `data` (slicing-by-8).
    pub fn update(&mut self, data: &[u8]) {
        let t = &CRC_TABLES;
        let mut c = self.state;
        let (chunks, rest) = data.as_chunks::<8>();
        for ch in chunks {
            let lo = c ^ u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
            let hi = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
            c = t[7][(lo & 0xFF) as usize]
                ^ t[6][((lo >> 8) & 0xFF) as usize]
                ^ t[5][((lo >> 16) & 0xFF) as usize]
                ^ t[4][(lo >> 24) as usize]
                ^ t[3][(hi & 0xFF) as usize]
                ^ t[2][((hi >> 8) & 0xFF) as usize]
                ^ t[1][((hi >> 16) & 0xFF) as usize]
                ^ t[0][(hi >> 24) as usize];
        }
        for &b in rest {
            c = t[0][((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    /// The CRC of everything fed so far (the hasher stays usable).
    pub const fn finish(&self) -> u32 {
        !self.state
    }
}

/// One-shot CRC-32.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finish()
}

/// Incremental Adler-32 (RFC 1950).
#[derive(Clone, Copy, Debug)]
pub struct Adler32 {
    pub(super) a: u32,
    pub(super) b: u32,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    pub const fn new() -> Self {
        Self { a: 1, b: 0 }
    }

    pub fn update(&mut self, data: &[u8]) {
        // 5552 is the largest block for which `b` cannot overflow a u32
        // before the modulo (zlib's NMAX).
        for block in data.chunks(5552) {
            let (mut a, mut b) = (self.a, self.b);
            for &x in block {
                a += x as u32;
                b += a;
            }
            self.a = a % 65521;
            self.b = b % 65521;
        }
    }

    pub const fn finish(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

/// One-shot Adler-32.
pub fn adler32(data: &[u8]) -> u32 {
    let mut a = Adler32::new();
    a.update(data);
    a.finish()
}
