//! A small, stable hash for fingerprinting the world state.
//!
//! Rust's built-in `DefaultHasher` may change between Rust versions, and
//! `#[derive(Hash)]` output isn't promised to stay the same either. A
//! determinism test needs a fingerprint that never changes unless the game
//! does, so we use FNV-1a (a simple, well-known 64-bit hash) and feed it bytes
//! explicitly, field by field, always little-endian.

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, 64-bit.
#[derive(Debug, Clone)]
pub struct Fnv64 {
    state: u64,
}

impl Default for Fnv64 {
    fn default() -> Self {
        Self {
            state: OFFSET_BASIS,
        }
    }
}

impl Fnv64 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(PRIME);
        }
        self
    }

    pub fn u8(&mut self, n: u8) -> &mut Self {
        self.bytes(&[n])
    }

    pub fn i32(&mut self, n: i32) -> &mut Self {
        self.bytes(&n.to_le_bytes())
    }

    pub fn u64(&mut self, n: u64) -> &mut Self {
        self.bytes(&n.to_le_bytes())
    }

    /// A string, prefixed with its length so `"ab" + "c"` and `"a" + "bc"`
    /// hash differently.
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u64(s.len() as u64).bytes(s.as_bytes())
    }

    pub fn finish(&self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_fnv1a_test_vectors() {
        // From the FNV reference test suite.
        assert_eq!(Fnv64::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv64::new().bytes(b"a").finish(), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(
            Fnv64::new().bytes(b"foobar").finish(),
            0x8594_4171_f739_67e8
        );
    }

    #[test]
    fn integers_are_hashed_little_endian() {
        let a = Fnv64::new().i32(1).finish();
        let b = Fnv64::new().bytes(&[1, 0, 0, 0]).finish();
        assert_eq!(a, b);
        assert_eq!(
            Fnv64::new().u64(1).finish(),
            Fnv64::new().bytes(&[1, 0, 0, 0, 0, 0, 0, 0]).finish()
        );
    }

    #[test]
    fn strings_are_length_prefixed() {
        let ab_c = Fnv64::new().str("ab").str("c").finish();
        let a_bc = Fnv64::new().str("a").str("bc").finish();
        assert_ne!(ab_c, a_bc);
    }

    #[test]
    fn u8_is_one_byte() {
        assert_eq!(
            Fnv64::new().u8(97).finish(),
            Fnv64::new().bytes(b"a").finish()
        );
    }
}
