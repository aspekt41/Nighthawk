//! Air: the gases in each tile (decision D28).
//!
//! Every tile that can hold air stores how much of each gas it contains, as a
//! *partial pressure* in pascals (Pa). Tiles are all the same size, so a
//! pressure is a direct measure of an amount, and adding the three gives
//! the tile's total pressure. Normal ship air is 101,325 Pa (one Earth
//! atmosphere): 21% oxygen, 78.96% nitrogen, 0.04% carbon dioxide.
//!
//! The amounts are whole numbers (`u32`), never floating point. Moving air
//! between tiles only ever moves whole pascals from one tile to another, so
//! air is never created or destroyed by rounding. A sealed ship keeps exactly
//! the same total forever, and every machine computes exactly the same
//! result (D22).

use serde::{Deserialize, Serialize};

/// The gases the game tracks. A tile's air is made of these and nothing else,
/// so their percentages always add up to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gas {
    O2,
    N2,
    CO2,
}

impl Gas {
    /// Every gas, in the order they're shown and stored.
    pub const ALL: [Gas; 3] = [Gas::O2, Gas::N2, Gas::CO2];

    /// The gas's formula, in plain ASCII so it's one cell per character in
    /// every terminal.
    pub fn symbol(self) -> &'static str {
        match self {
            Gas::O2 => "O2",
            Gas::N2 => "N2",
            Gas::CO2 => "CO2",
        }
    }
}

/// `diff / SPREAD_DIVISOR`, rounded to the nearest whole number, with
/// halves rounded away from zero (5 → 1, -5 → -1, 4 → 0).
fn divide_rounding_to_nearest(diff: i64) -> i64 {
    let half = SPREAD_DIVISOR / 2;
    if diff >= 0 {
        (diff + half) / SPREAD_DIVISOR
    } else {
        (diff - half) / SPREAD_DIVISOR
    }
}

/// How fast air evens out. Each turn, each pair of connected tiles moves
/// 1/`SPREAD_DIVISOR` of their difference. A tile has at most five
/// connections (four sides, plus a ladder), and five tenths is one half, so
/// a tile can never give away more than half its air in one turn. That keeps
/// the spreading smooth: values approach each other without overshooting.
pub const SPREAD_DIVISOR: i64 = 10;

/// The air in one tile: a partial pressure in pascals for each gas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mix {
    pub o2: u32,
    pub n2: u32,
    pub co2: u32,
}

impl Mix {
    /// No air at all.
    pub const VACUUM: Mix = Mix {
        o2: 0,
        n2: 0,
        co2: 0,
    };

    /// Normal ship air: 101,325 Pa, 21% O2, 78.96% N2, 0.04% CO2.
    pub const STANDARD: Mix = Mix {
        o2: 21_278,
        n2: 80_006,
        co2: 41,
    };

    /// The partial pressure of one gas.
    pub fn get(self, gas: Gas) -> u32 {
        match gas {
            Gas::O2 => self.o2,
            Gas::N2 => self.n2,
            Gas::CO2 => self.co2,
        }
    }

    /// The three amounts as an array, in [`Gas::ALL`] order.
    fn amounts(self) -> [u64; 3] {
        Gas::ALL.map(|gas| u64::from(self.get(gas)))
    }

    /// Total pressure in pascals.
    pub fn total(self) -> u64 {
        self.amounts().iter().sum()
    }

    /// The same mix at `percent` of the pressure, for example 60 for thin
    /// air. Each gas is scaled separately, rounding down.
    pub fn scaled(self, percent: u32) -> Mix {
        let scale = |amount: u32| {
            let scaled = u64::from(amount) * u64::from(percent) / 100;
            u32::try_from(scaled).expect("a scaled tile of air fits in u32")
        };
        Mix {
            o2: scale(self.o2),
            n2: scale(self.n2),
            co2: scale(self.co2),
        }
    }

    /// Each gas's share of the total in tenths of a percent (per mille), in
    /// [`Gas::ALL`] order, or `None` in a vacuum where shares mean nothing.
    ///
    /// The shares always add up to exactly 1000 (100.0%). Rounding each
    /// share on its own could give 99.9% or 100.1%, so this uses the
    /// *largest remainder* method: round every share down, then hand the
    /// missing tenths to the shares that lost the most in rounding.
    pub fn per_mille(self) -> Option<[u32; 3]> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let exact = self.amounts().map(|amount| amount * 1000);
        let mut shares = exact.map(|scaled| scaled / total);
        let handed_out: u64 = shares.iter().sum();
        // Biggest remainder first; ties go to the gas listed first.
        let mut by_remainder = [0, 1, 2];
        by_remainder.sort_by_key(|&i| std::cmp::Reverse(exact[i] % total));
        for &i in by_remainder.iter().take((1000 - handed_out) as usize) {
            shares[i] += 1;
        }
        Some(shares.map(|share| u32::try_from(share).expect("a share is at most 1000")))
    }

    /// How much of each gas should move from `self` to `other` in one turn
    /// (negative means it moves the other way): 1/[`SPREAD_DIVISOR`] of
    /// the difference, rounded to the nearest whole pascal.
    ///
    /// Why nearest, not down: flow stops once neighbours differ by less than
    /// half a divisor (5 Pa), and those small leftovers add up along a long
    /// corridor into a faint slope. Rounding to nearest keeps that slope half
    /// as steep as rounding down would. It's still safe: a tile with `a` Pa
    /// next to empty tiles gives at most `round(a / 10)` to each of at most
    /// five neighbours, which is never more than `a`, so no gas goes negative.
    pub(crate) fn flow_to(self, other: Mix) -> [i64; 3] {
        let (here, there) = (self.amounts(), other.amounts());
        [0, 1, 2].map(|i| divide_rounding_to_nearest(here[i] as i64 - there[i] as i64))
    }

    /// This mix with `delta` of each gas added (or removed, when negative).
    ///
    /// # Panics
    /// If a gas would go below zero. The spreading rules guarantee it never
    /// does, so this would be a bug worth stopping for.
    pub(crate) fn plus(self, delta: [i64; 3]) -> Mix {
        let add = |amount: u32, change: i64| {
            u32::try_from(i64::from(amount) + change).expect("air never goes negative or overflows")
        };
        Mix {
            o2: add(self.o2, delta[0]),
            n2: add(self.n2, delta[1]),
            co2: add(self.co2, delta[2]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_air_is_one_atmosphere() {
        assert_eq!(Mix::STANDARD.total(), 101_325);
    }

    #[test]
    fn standard_air_is_21_percent_oxygen() {
        // 21.0% O2, 79.0% N2 (78.96 rounded), 0.0% CO2 (0.04 rounded).
        assert_eq!(Mix::STANDARD.per_mille(), Some([210, 790, 0]));
    }

    #[test]
    fn shares_always_add_up_to_exactly_100_percent() {
        // Mixes chosen so that rounding each share on its own would not
        // add up to 1000: thirds, and a mix of awkward numbers.
        let mixes = [
            Mix {
                o2: 1,
                n2: 1,
                co2: 1,
            },
            Mix {
                o2: 2,
                n2: 2,
                co2: 2,
            },
            Mix {
                o2: 1234,
                n2: 5678,
                co2: 91,
            },
            Mix {
                o2: 1,
                n2: 0,
                co2: 0,
            },
            Mix {
                o2: 0,
                n2: 7,
                co2: 3,
            },
            Mix::STANDARD,
            Mix::STANDARD.scaled(37),
        ];
        for mix in mixes {
            let shares = mix.per_mille().unwrap();
            assert_eq!(shares.iter().sum::<u32>(), 1000, "{mix:?} gave {shares:?}");
        }
    }

    #[test]
    fn largest_remainder_rounds_thirds_to_334_333_333() {
        assert_eq!(
            Mix {
                o2: 1,
                n2: 1,
                co2: 1
            }
            .per_mille(),
            Some([334, 333, 333])
        );
    }

    #[test]
    fn a_vacuum_has_no_shares() {
        assert_eq!(Mix::VACUUM.per_mille(), None);
        assert_eq!(Mix::VACUUM.total(), 0);
    }

    #[test]
    fn scaling_keeps_the_proportions() {
        let half = Mix::STANDARD.scaled(50);
        assert_eq!(
            half,
            Mix {
                o2: 10_639,
                n2: 40_003,
                co2: 20
            }
        );
        assert_eq!(half.per_mille(), Some([210, 790, 0]));
        assert_eq!(Mix::STANDARD.scaled(0), Mix::VACUUM);
        assert_eq!(Mix::STANDARD.scaled(100), Mix::STANDARD);
    }

    #[test]
    fn get_reads_each_gas() {
        let mix = Mix {
            o2: 1,
            n2: 2,
            co2: 3,
        };
        let read: Vec<u32> = Gas::ALL.iter().map(|&g| mix.get(g)).collect();
        assert_eq!(read, vec![1, 2, 3]);
        assert_eq!(Gas::ALL.map(Gas::symbol), ["O2", "N2", "CO2"]);
    }

    #[test]
    fn flow_is_a_tenth_of_the_difference_and_runs_downhill() {
        let full = Mix {
            o2: 1000,
            n2: 500,
            co2: 9,
        };
        let empty = Mix::VACUUM;
        assert_eq!(full.flow_to(empty), [100, 50, 1], "9 / 10 rounds to 1");
        assert_eq!(empty.flow_to(full), [-100, -50, -1]);
        assert_eq!(full.flow_to(full), [0, 0, 0]);
    }

    #[test]
    fn flows_round_to_the_nearest_pascal_symmetrically() {
        let cases = [
            (0, 0),
            (4, 0),
            (5, 1),
            (14, 1),
            (15, 2),
            (-4, 0),
            (-5, -1),
            (-15, -2),
        ];
        for (diff, expected) in cases {
            assert_eq!(divide_rounding_to_nearest(diff), expected, "diff {diff}");
        }
    }

    #[test]
    fn no_tile_can_give_away_more_air_than_it_has() {
        // The worst case: a tile with five links, all to empty tiles. For
        // every small amount, the total given away must fit in the tile.
        for amount in 0..=1000u32 {
            let tile = Mix {
                o2: amount,
                n2: 0,
                co2: 0,
            };
            let given = 5 * tile.flow_to(Mix::VACUUM)[0];
            assert!(
                given <= i64::from(amount),
                "{amount} Pa would give away {given}"
            );
        }
    }

    #[test]
    fn plus_adds_and_removes() {
        let mix = Mix {
            o2: 10,
            n2: 10,
            co2: 10,
        };
        assert_eq!(
            mix.plus([5, -10, 0]),
            Mix {
                o2: 15,
                n2: 0,
                co2: 10
            }
        );
    }

    #[test]
    #[should_panic(expected = "never goes negative")]
    fn plus_refuses_to_go_negative() {
        Mix::VACUUM.plus([-1, 0, 0]);
    }
}
