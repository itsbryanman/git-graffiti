use anyhow::{Result, bail};

pub const INITIAL_STATE: [u32; 5] = [
    0x6745_2301,
    0xefcd_ab89,
    0x98ba_dcfe,
    0x1032_5476,
    0xc3d2_e1f0,
];

#[derive(Clone, Debug)]
pub struct PrefixMask {
    bytes: [u8; 20],
    nibbles: usize,
}

impl PrefixMask {
    pub fn parse(prefix: &str) -> Result<Self> {
        if prefix.is_empty() {
            bail!("prefix is empty. give me 1 to 40 hex characters");
        }
        if prefix.len() > 40 {
            bail!("prefix is {} characters. SHA-1 only has 40", prefix.len());
        }
        if !prefix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("prefix {prefix:?} is not hex. use 0-9 and a-f");
        }

        let mut bytes = [0_u8; 20];
        for (index, byte) in prefix.bytes().enumerate() {
            let value = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => unreachable!(),
            };
            if index % 2 == 0 {
                bytes[index / 2] = value << 4;
            } else {
                bytes[index / 2] |= value;
            }
        }

        Ok(Self {
            bytes,
            nibbles: prefix.len(),
        })
    }

    pub fn matches(&self, digest: &[u8; 20]) -> bool {
        let whole_bytes = self.nibbles / 2;
        if digest[..whole_bytes] != self.bytes[..whole_bytes] {
            return false;
        }
        self.nibbles.is_multiple_of(2)
            || digest[whole_bytes] & 0xf0 == self.bytes[whole_bytes] & 0xf0
    }

    pub fn bytes(&self) -> &[u8; 20] {
        &self.bytes
    }

    pub fn nibbles(&self) -> usize {
        self.nibbles
    }
}

pub fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut words = [0_u32; 80];
    for (index, chunk) in block.chunks_exact(4).enumerate() {
        words[index] = u32::from_be_bytes(chunk.try_into().expect("four-byte chunk"));
    }
    for index in 16..80 {
        words[index] =
            (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                .rotate_left(1);
    }

    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (index, word) in words.iter().enumerate() {
        let (function, constant) = match index {
            0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
            20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
            _ => (b ^ c ^ d, 0xca62_c1d6),
        };
        let next = a
            .rotate_left(5)
            .wrapping_add(function)
            .wrapping_add(e)
            .wrapping_add(constant)
            .wrapping_add(*word);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = next;
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

pub fn digest_bytes(state: [u32; 5]) -> [u8; 20] {
    let mut digest = [0_u8; 20];
    for (chunk, word) in digest.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

pub fn sha1(bytes: &[u8]) -> [u8; 20] {
    let mut state = INITIAL_STATE;
    let mut blocks = bytes.chunks_exact(64);
    for chunk in &mut blocks {
        compress(&mut state, chunk.try_into().expect("64-byte block"));
    }

    let remainder = blocks.remainder();
    let padding_blocks = if remainder.len() < 56 { 1 } else { 2 };
    let mut tail = vec![0_u8; padding_blocks * 64];
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()] = 0x80;
    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    let tail_len = tail.len();
    tail[tail_len - 8..].copy_from_slice(&bit_length.to_be_bytes());
    for chunk in tail.chunks_exact(64) {
        compress(&mut state, chunk.try_into().expect("64-byte block"));
    }
    digest_bytes(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_known_vector() {
        assert_eq!(
            sha1(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
            ]
        );
    }

    #[test]
    fn mask_matches_even_prefix() {
        let mask = PrefixMask::parse("dead").unwrap();
        let mut digest = [0_u8; 20];
        digest[..2].copy_from_slice(&[0xde, 0xad]);
        assert!(mask.matches(&digest));
        digest[1] = 0xae;
        assert!(!mask.matches(&digest));
    }

    #[test]
    fn mask_matches_odd_prefix() {
        let mask = PrefixMask::parse("abc").unwrap();
        let mut digest = [0_u8; 20];
        digest[..2].copy_from_slice(&[0xab, 0xcf]);
        assert!(mask.matches(&digest));
        digest[1] = 0xbf;
        assert!(!mask.matches(&digest));
    }
}
