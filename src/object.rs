use anyhow::{Context, Result, bail};

use crate::sha1mid::{INITIAL_STATE, compress, digest_bytes};

pub const NONCE_LEN: usize = 64;

#[derive(Clone)]
pub struct PreparedCommit {
    pub content_prefix: Vec<u8>,
    pub midstate: [u32; 5],
    pub padding_block: [u8; 64],
    pub input_bit_length: u64,
}

impl PreparedCommit {
    pub fn content_for_nonce(&self, nonce: u64) -> Vec<u8> {
        let mut content = self.content_prefix.clone();
        content.extend_from_slice(&encode_nonce(nonce));
        content
    }

    pub fn digest_for_nonce(&self, nonce: u64) -> [u8; 20] {
        let mut state = self.midstate;
        compress(&mut state, &encode_nonce(nonce));
        compress(&mut state, &self.padding_block);
        digest_bytes(state)
    }
}

pub fn encode_nonce(value: u64) -> [u8; NONCE_LEN] {
    let mut nonce = [b' '; NONCE_LEN];
    for (bit, byte) in nonce.iter_mut().enumerate() {
        if value & (1_u64 << bit) != 0 {
            *byte = b'\t';
        }
    }
    nonce
}

pub fn prepare(raw_commit: &[u8]) -> Result<PreparedCommit> {
    if !raw_commit.windows(2).any(|pair| pair == b"\n\n") {
        bail!("commit has no blank line between its headers and message");
    }

    // The object header contains the final content length, so its digit count is part
    // of the alignment problem. Trying a small bounded range is simpler than lying to
    // ourselves about that byte.
    for pad_len in 0..=128 {
        let content_len = raw_commit.len() + pad_len + NONCE_LEN;
        let object_header = format!("commit {content_len}\0");
        let nonce_offset = object_header.len() + raw_commit.len() + pad_len;
        if !nonce_offset.is_multiple_of(64) {
            continue;
        }

        let mut object_prefix = Vec::with_capacity(nonce_offset);
        object_prefix.extend_from_slice(object_header.as_bytes());
        object_prefix.extend_from_slice(raw_commit);
        object_prefix.resize(nonce_offset, b' ');

        let mut state = INITIAL_STATE;
        for block in object_prefix.chunks_exact(64) {
            compress(
                &mut state,
                block
                    .try_into()
                    .context("aligned object prefix had a short block")?,
            );
        }

        let input_len = nonce_offset + NONCE_LEN;
        let input_bit_length = (input_len as u64)
            .checked_mul(8)
            .context("commit is too large to hash")?;
        let mut padding_block = [0_u8; 64];
        padding_block[0] = 0x80;
        padding_block[56..].copy_from_slice(&input_bit_length.to_be_bytes());

        let mut content_prefix = Vec::with_capacity(content_len - NONCE_LEN);
        content_prefix.extend_from_slice(raw_commit);
        content_prefix.resize(raw_commit.len() + pad_len, b' ');

        return Ok(PreparedCommit {
            content_prefix,
            midstate: state,
            padding_block,
            input_bit_length,
        });
    }

    bail!("could not align the nonce. this is a bug")
}

pub fn object_bytes(content: &[u8]) -> Vec<u8> {
    let mut object = format!("commit {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    object
}

pub fn strip_gpgsig(raw: &[u8]) -> (Vec<u8>, bool) {
    let Some(split) = raw.windows(2).position(|pair| pair == b"\n\n") else {
        return (raw.to_vec(), false);
    };
    let headers = &raw[..split];
    let message = &raw[split + 2..];
    let mut kept = Vec::with_capacity(headers.len());
    let mut stripping = false;
    let mut stripped = false;

    for line in headers.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"gpgsig ") {
            stripping = true;
            stripped = true;
            continue;
        }
        if stripping && line.starts_with(b" ") {
            continue;
        }
        stripping = false;
        kept.extend_from_slice(line);
    }

    if !stripped {
        return (raw.to_vec(), false);
    }
    while kept.last() == Some(&b'\n') {
        kept.pop();
    }
    kept.extend_from_slice(b"\n\n");
    kept.extend_from_slice(message);
    (kept, true)
}

pub fn parent_hashes(raw: &[u8]) -> Vec<String> {
    let split = raw
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .unwrap_or(raw.len());
    raw[..split]
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"parent "))
        .filter_map(|hash| std::str::from_utf8(hash).ok())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn replace_parent(raw: &[u8], parent: &str) -> Vec<u8> {
    let header_end = raw
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .unwrap_or(raw.len());
    let mut replaced = Vec::with_capacity(raw.len());
    for line in raw[..header_end].split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"parent ") {
            replaced.extend_from_slice(b"parent ");
            replaced.extend_from_slice(parent.as_bytes());
            if line.ends_with(b"\n") {
                replaced.push(b'\n');
            }
        } else {
            replaced.extend_from_slice(line);
        }
    }
    replaced.extend_from_slice(&raw[header_end..]);
    replaced
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::*;

    fn commit_with_message_len(length: usize) -> Vec<u8> {
        let mut raw = b"tree 0123456789012345678901234567890123456789\nauthor a <a@b.c> 1 +0000\ncommitter a <a@b.c> 1 +0000\n\n".to_vec();
        raw.extend(std::iter::repeat_n(b'x', length));
        raw
    }

    #[test]
    fn nonce_encoder_uses_spaces_and_tabs() {
        let nonce = encode_nonce(0b101);
        assert_eq!(&nonce[..4], b"\t \t ");
        assert!(nonce[4..].iter().all(|byte| *byte == b' '));
    }

    #[test]
    fn nonce_is_an_aligned_final_data_block_for_many_message_lengths() {
        for length in 0..=300 {
            let prepared = prepare(&commit_with_message_len(length)).unwrap();
            let content = prepared.content_for_nonce(7);
            let object = object_bytes(&content);
            let nonce_start = object.len() - NONCE_LEN;
            assert_eq!(nonce_start % 64, 0, "message length {length}");
            assert_eq!(object.len() % 64, 0, "message length {length}");
            assert_eq!(prepared.padding_block[0], 0x80);
            assert_eq!(
                u64::from_be_bytes(prepared.padding_block[56..].try_into().unwrap()),
                (object.len() as u64) * 8
            );
        }
    }

    #[test]
    fn cached_hash_matches_independent_sha1() {
        let prepared = prepare(&commit_with_message_len(37)).unwrap();
        let content = prepared.content_for_nonce(0xdead_beef);
        let expected: [u8; 20] = Sha1::digest(object_bytes(&content)).into();
        assert_eq!(prepared.digest_for_nonce(0xdead_beef), expected);
    }

    #[test]
    fn strips_multiline_signature_only() {
        let raw = b"tree abc\ngpgsig -----BEGIN\n line one\n line two\nauthor x\n\nmessage\n";
        let (clean, stripped) = strip_gpgsig(raw);
        assert!(stripped);
        assert_eq!(clean, b"tree abc\nauthor x\n\nmessage\n");
    }

    #[test]
    fn replaces_parent_header_without_touching_message() {
        let raw = b"tree abc\nparent old\nauthor x\n\nparent old\n";
        assert_eq!(
            replace_parent(raw, "new"),
            b"tree abc\nparent new\nauthor x\n\nparent old\n"
        );
    }
}
