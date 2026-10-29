//! The content blocks a Horizon client carries (docs/modpacks.md): a content
//! mod's blocks get block-state ids after vanilla's, assigned from the mods'
//! own files, so two games agree on a state id only when they carry the same
//! blocks. A client sends a [`Fingerprint`] of its blocks in the configuration
//! phase; a Horizon server that has content blocks refuses a client whose
//! fingerprint differs from its own.
use crate::{
    Error, Result,
    buf::{Reader, Writer},
};
use sha2::{Digest, Sha256};

/// The custom payload channel, serverbound, in the configuration phase.
pub const CHANNEL: &str = "horizon:content_blocks";

/// What a game's content blocks are: how many, and a hash of their ordered
/// identities (block id and template, in block-state id order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fingerprint {
    pub count: u32,
    pub hash: String,
}

impl Fingerprint {
    /// The fingerprint of `blocks`, one entry per block in state-id order.
    /// No blocks is the default.
    pub fn of(blocks: &[String]) -> Self {
        if blocks.is_empty() {
            return Self::default();
        }
        let mut hasher = Sha256::new();
        for block in blocks {
            hasher.update((block.len() as u64).to_le_bytes());
            hasher.update(block.as_bytes());
        }
        Self {
            count: blocks.len() as u32,
            hash: format!("{:x}", hasher.finalize()),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(self.count as i32);
        w.string(&self.hash);
        w.data
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let count = r.varint()?;
        let hash = r.string(64)?;
        if count < 0 || r.remaining() > 0 {
            return Err(Error::new("content blocks: malformed payload"));
        }
        Ok(Self {
            count: count as u32,
            hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_blocks_is_the_default() {
        assert_eq!(Fingerprint::of(&[]), Fingerprint::default());
        let none = Fingerprint::default();
        assert_eq!(Fingerprint::decode(&none.encode()).unwrap(), none);
    }

    #[test]
    fn a_fingerprint_round_trips() {
        let f = Fingerprint::of(&blocks(&["demo:a minecraft:gold_block"]));
        assert_eq!(f.count, 1);
        assert_eq!(Fingerprint::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn the_blocks_and_their_order_decide_it() {
        let ab = Fingerprint::of(&blocks(&["demo:a t", "demo:z t"]));
        let z = Fingerprint::of(&blocks(&["demo:z t"]));
        let ba = Fingerprint::of(&blocks(&["demo:z t", "demo:a t"]));
        assert_ne!(ab, z);
        assert_ne!(ab, ba);
        assert_eq!(ab, Fingerprint::of(&blocks(&["demo:a t", "demo:z t"])));
    }

    #[test]
    fn a_bad_payload_is_refused() {
        assert!(Fingerprint::decode(&[]).is_err());
        let mut p = Fingerprint::of(&blocks(&["x:y t"])).encode();
        p.push(0);
        assert!(Fingerprint::decode(&p).is_err());
    }
}
