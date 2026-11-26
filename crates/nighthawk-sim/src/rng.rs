//! Deterministic random numbers.
//!
//! The same seed must always produce the same game, on every machine and
//! every Rust version. That makes bugs reproducible, and lets a test check that
//! nothing changes by accident (see `tests/determinism.rs`). Three rules make
//! it true:
//!
//! 1. **All randomness in the sim goes through [`GameRng`].** Never use
//!    `rand::rng()`, the clock, or anything else that differs between runs.
//! 2. **Each part of the game has its own [`Stream`].** Streams come from one
//!    seed but are independent, so adding a dice roll to one system (say,
//!    loot) doesn't shift the numbers another system (say, ambient sounds)
//!    sees.
//! 3. **We own the maths that turns raw numbers into game choices**
//!    ([`GameRng::below`] and friends). The generator itself is PCG32 from
//!    `rand_pcg`, whose output is fixed by the PCG reference specification and
//!    pinned by a test here. Helper functions in other crates are allowed to
//!    change between versions, so we don't use them.

use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};
// `Rng` is the trait that provides `next_u32`. `as _` imports it only so its
// methods can be called, without putting the name `Rng` in scope.
use rand_pcg::rand_core::Rng as _;

/// The independent random streams, one per game system.
///
/// The numbers are part of the save-game and determinism contract: **never
/// change or reuse one.** Add new streams with new numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Ambient = 1,
}

/// A seeded random number generator for one [`Stream`].
///
/// It can be saved and loaded: the save records the generator's exact
/// position in its sequence, so a loaded game rolls the same numbers next
/// as the game that was saved. `transparent` stores it as just the PCG state,
/// with no extra wrapper in the JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameRng {
    pcg: Pcg32,
}

impl GameRng {
    pub fn new(seed: u64, stream: Stream) -> Self {
        // PCG takes a starting state and a stream number. Same seed, different
        // stream number: a completely different, independent sequence.
        Self {
            pcg: Pcg32::new(seed, stream as u64),
        }
    }

    /// A uniformly random `u32`.
    pub fn next_u32(&mut self) -> u32 {
        self.pcg.next_u32()
    }

    /// A uniformly random number in `0..n`.
    ///
    /// Plain `next_u32() % n` would be slightly biased: unless `n` divides
    /// 2³² exactly, the low results come up a little more often. So values
    /// from the incomplete last "bucket" are thrown away and redrawn. At
    /// most half of all draws can be rejected, so the loop ends quickly.
    ///
    /// # Panics
    /// If `n` is 0, because there's no number in `0..0` to return.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "GameRng::below(0) has no possible result");
        let n = u64::from(n);
        let range = 1u64 << 32;
        // The largest multiple of n that fits in 2³²: draws at or above it
        // would make some results more likely than others.
        let zone = range - range % n;
        loop {
            let x = u64::from(self.next_u32());
            if x < zone {
                // `x % n` is below n, which came from a u32, so this can't fail.
                return u32::try_from(x % n).expect("x % n fits in u32");
            }
        }
    }

    /// True with probability `1/n`. `one_in(1)` is always true.
    pub fn one_in(&mut self, n: u32) -> bool {
        self.below(n) == 0
    }

    /// A uniformly random element of `items`, or `None` if it's empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let len = u32::try_from(items.len()).ok()?;
        if len == 0 {
            return None;
        }
        items.get(self.below(len) as usize)
    }

    /// A number that identifies this generator's exact position in its
    /// sequence, for [`crate::World::state_hash`]. It's the next output,
    /// taken from a copy, so the real generator doesn't move.
    pub(crate) fn fingerprint(&self) -> u64 {
        self.pcg.clone().next_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcg32_matches_the_reference_output() {
        // From the PCG reference implementation (pcg32-demo, seed 42,
        // stream 54). If a dependency upgrade ever changes these, every
        // seeded game would change, so this test must never be edited.
        let mut pcg = Pcg32::new(42, 54);
        let expected = [
            0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e,
        ];
        for want in expected {
            assert_eq!(pcg.next_u32(), want);
        }
    }

    #[test]
    fn same_seed_same_stream_same_numbers() {
        let mut a = GameRng::new(7, Stream::Ambient);
        let mut b = GameRng::new(7, Stream::Ambient);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_give_different_numbers() {
        let a: Vec<u32> = draws(&mut GameRng::new(1, Stream::Ambient), 8);
        let b: Vec<u32> = draws(&mut GameRng::new(2, Stream::Ambient), 8);
        assert_ne!(a, b);
    }

    #[test]
    fn streams_from_one_seed_are_independent() {
        // A stream number nobody uses yet, to compare against Ambient.
        let mut ambient = GameRng::new(7, Stream::Ambient);
        let mut other = GameRng {
            pcg: Pcg32::new(7, 99),
        };
        assert_ne!(draws(&mut ambient, 8), draws(&mut other, 8));
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = GameRng::new(3, Stream::Ambient);
        for n in [1, 2, 3, 7, 100, u32::MAX] {
            for _ in 0..200 {
                assert!(rng.below(n) < n);
            }
        }
    }

    #[test]
    fn below_one_is_always_zero_and_one_in_one_is_always_true() {
        let mut rng = GameRng::new(3, Stream::Ambient);
        assert_eq!(rng.below(1), 0);
        assert!(rng.one_in(1));
    }

    #[test]
    fn below_reaches_every_value_roughly_evenly() {
        let mut rng = GameRng::new(11, Stream::Ambient);
        let mut counts = [0u32; 6];
        for _ in 0..6000 {
            counts[rng.below(6) as usize] += 1;
        }
        // Each face should come up about 1000 times. The bounds are wide
        // enough that a fair generator fails this essentially never.
        for (face, &count) in counts.iter().enumerate() {
            assert!(
                (850..=1150).contains(&count),
                "face {face} came up {count} times"
            );
        }
    }

    #[test]
    fn below_redraws_without_bias_for_awkward_ranges() {
        // n = 2^31 + 1 is the worst case: almost half of all raw draws fall
        // in the rejected zone, so this exercises the redraw path heavily.
        let n = (1u32 << 31) + 1;
        let mut rng = GameRng::new(13, Stream::Ambient);
        let mut low_half = 0;
        for _ in 0..2000 {
            let x = rng.below(n);
            assert!(x < n);
            if x < n / 2 {
                low_half += 1;
            }
        }
        // A biased `% n` would put about 2/3 of results in the low half.
        assert!(
            (850..=1150).contains(&low_half),
            "{low_half} of 2000 in the low half"
        );
    }

    #[test]
    #[should_panic(expected = "no possible result")]
    fn below_zero_panics() {
        GameRng::new(1, Stream::Ambient).below(0);
    }

    #[test]
    fn pick_returns_an_element_or_none_when_empty() {
        let mut rng = GameRng::new(5, Stream::Ambient);
        let items = ["a", "b", "c"];
        for _ in 0..50 {
            assert!(items.contains(rng.pick(&items).unwrap()));
        }
        let empty: [&str; 0] = [];
        assert_eq!(rng.pick(&empty), None);
    }

    #[test]
    fn fingerprint_does_not_advance_the_generator() {
        let rng = GameRng::new(9, Stream::Ambient);
        let before = rng.clone();
        let _ = rng.fingerprint();
        assert_eq!(rng, before);
    }

    #[test]
    fn fingerprint_changes_after_a_draw() {
        let mut rng = GameRng::new(9, Stream::Ambient);
        let before = rng.fingerprint();
        rng.next_u32();
        assert_ne!(rng.fingerprint(), before);
    }

    fn draws(rng: &mut GameRng, n: usize) -> Vec<u32> {
        (0..n).map(|_| rng.next_u32()).collect()
    }
}
