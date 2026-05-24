//! Explicit stable hashing for restart/process-stable identifiers.
//!
//! Rust's `DefaultHasher` is intentionally unspecified. Use this module for
//! IDs that are persisted, displayed as stable diagnostics, or reused across
//! process boundaries.

use std::hash::{Hash, Hasher};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 64-bit FNV-1a hasher with explicit little-endian integer encoding.
#[derive(Clone, Debug)]
pub struct StableHasher(u64);

impl StableHasher {
    #[must_use]
    pub fn new() -> Self {
        Self(FNV_OFFSET)
    }

    pub fn finish64(&self) -> u64 {
        self.0
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write(&[i]);
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_u128(&mut self, i: u128) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    fn write_i8(&mut self, i: i8) {
        self.write(&i.to_le_bytes());
    }

    fn write_i16(&mut self, i: i16) {
        self.write(&i.to_le_bytes());
    }

    fn write_i32(&mut self, i: i32) {
        self.write(&i.to_le_bytes());
    }

    fn write_i64(&mut self, i: i64) {
        self.write(&i.to_le_bytes());
    }

    fn write_i128(&mut self, i: i128) {
        self.write(&i.to_le_bytes());
    }

    fn write_isize(&mut self, i: isize) {
        self.write_i64(i as i64);
    }
}

#[must_use]
pub fn stable_hash64(value: impl Hash) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_known_value_is_pinned() {
        assert_eq!(
            stable_hash64(("nmp", 42u64, "stable")),
            0x12fd_5f39_8ee9_ec51
        );
    }

    #[test]
    fn stable_hash_distinguishes_typed_parts() {
        assert_ne!(stable_hash64(("a", "bc")), stable_hash64(("ab", "c")));
        assert_ne!(stable_hash64(("id", 1u64)), stable_hash64(("id", 2u64)));
    }
}
