//! Sealed suits and O₂ canisters (decision D67).
//!
//! A *canister* is an item that holds oxygen, counted in whole millilitres
//! at normal pressure (decision D22). A *sealed suit* is something you
//! wear with a slot for one canister, of the kind its content names: the
//! same slot a tool keeps its power cell in (D59), so canisters load and
//! unload the same way. While you wear one in air you can't breathe, it
//! feeds you from its canister, `ml_per_s` for every second that passes,
//! so its air runs down with game time, never real time (D7). In air you
//! can breathe it takes nothing: the visor opens.
//!
//! The emergency rescue suit is open-loop (it flushes its visor with
//! oxygen as you breathe, and dumps the rest), so a canister lasts it only
//! minutes; a closed-loop EVA suit could take the same canisters and make
//! them last far longer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::hash::Fnv64;
use crate::item::{Item, ItemType};

/// The most a canister can hold, in millilitres of oxygen at normal
/// pressure (100 L). Content is checked against it.
pub const MAX_CANISTER_ML: u32 = 100_000;

/// What makes an item type an O₂ canister: how much oxygen it holds when
/// full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanisterType {
    /// How much oxygen it holds when full, in millilitres at normal
    /// pressure (1 to [`MAX_CANISTER_ML`]).
    pub capacity_ml: u32,
}

/// What makes something you wear a sealed suit: what it draws from its
/// canister each second you can't breathe the air around you, and which
/// canister it takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SuitAir {
    /// Oxygen it uses each second while sealed, in millilitres (at least 1).
    pub ml_per_s: u32,
    /// The id of the canister item it takes.
    pub canister: String,
}

/// Feeds a canister and a suit's air into a state hash. `u64::MAX` stands
/// for "none".
pub(crate) fn hash_air(canister: Option<&CanisterType>, air: Option<&SuitAir>, h: &mut Fnv64) {
    h.u64(canister.map_or(u64::MAX, |c| u64::from(c.capacity_ml)));
    match air {
        Some(a) => h.u64(u64::from(a.ml_per_s)).str(&a.canister),
        None => h.u64(u64::MAX),
    };
}

/// What sealing up gives `suit`, if it's a sealed suit.
pub fn air_of<'a>(suit: &Item, types: &'a [ItemType]) -> Option<&'a SuitAir> {
    types.get(usize::from(suit.kind))?.air()
}

/// The oxygen left in `suit`'s canister and the most it holds, in ml:
/// `None` if it isn't a sealed suit or has no canister.
pub fn o2(suit: &Item, types: &[ItemType]) -> Option<(u32, u32)> {
    air_of(suit, types)?;
    let canister = suit.cell.as_deref()?;
    let capacity = types.get(usize::from(canister.kind))?.canister.as_ref()?;
    Some((canister.o2_ml, capacity.capacity_ml))
}

/// How many seconds of sealed breathing `suit`'s canister has left, at
/// its draw: `None` if it isn't a sealed suit or has no canister.
pub fn seconds_left(suit: &Item, types: &[ItemType]) -> Option<u32> {
    let (have, _) = o2(suit, types)?;
    let draw = air_of(suit, types)?.ml_per_s.max(1);
    Some(have / draw)
}

/// One second sealed in `suit`: takes its draw from the canister, or all
/// that's left. Returns the oxygen before and after, or `None` if there was
/// none to breathe (no canister, or an empty one).
pub(crate) fn breathe_one_second(suit: &mut Item, types: &[ItemType]) -> Option<(u32, u32)> {
    let draw = air_of(suit, types)?.ml_per_s;
    let canister = suit.cell.as_deref_mut()?;
    let before = canister.o2_ml;
    if before == 0 {
        return None;
    }
    canister.o2_ml = before.saturating_sub(draw);
    Some((before, canister.o2_ml))
}

/// How low a suit's air has run, for warnings: each is said once, as the
/// canister falls past it, while there's still time to turn back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuitAirLevel {
    /// Half the canister is gone.
    Half,
    /// A quarter is left.
    Quarter,
    /// A tenth is left.
    Low,
    /// It's empty: you're breathing whatever is around you.
    Empty,
}

/// The warning levels, as the share of a full canister left (in percent),
/// highest first.
const WARNINGS: [(u32, SuitAirLevel); 3] = [
    (50, SuitAirLevel::Half),
    (25, SuitAirLevel::Quarter),
    (10, SuitAirLevel::Low),
];

/// The warnings for a canister holding `capacity` going from `before` to
/// `after` millilitres in one second.
pub(crate) fn levels_crossed(before: u32, after: u32, capacity: u32) -> Vec<SuitAirLevel> {
    let at = |percent: u32| u64::from(capacity) * u64::from(percent) / 100;
    let mut levels: Vec<SuitAirLevel> = WARNINGS
        .iter()
        .filter(|&&(percent, _)| u64::from(before) > at(percent) && u64::from(after) <= at(percent))
        .map(|&(_, level)| level)
        .collect();
    if before > 0 && after == 0 {
        levels.push(SuitAirLevel::Empty);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::tests::test_items;

    /// The test items' sealed suit and its canister (see `test_items`).
    const HOOD: u16 = 8;
    const CANISTER: u16 = 9;

    fn item(kind: u16) -> Item {
        Item::new(kind, 0, test_items().list())
    }

    /// The test hood with a canister holding `ml` in its slot.
    fn hood(ml: u32) -> Item {
        let mut canister = item(CANISTER);
        canister.o2_ml = ml;
        let mut hood = item(HOOD);
        hood.cell = Some(Box::new(canister));
        hood
    }

    #[test]
    fn a_sealed_suit_knows_its_canister_and_how_long_it_lasts() {
        let types = test_items();
        let types = types.list();
        assert_eq!(air_of(&item(HOOD), types).map(|a| a.ml_per_s), Some(10));
        assert_eq!(o2(&hood(600), types), Some((600, 1000)));
        assert_eq!(seconds_left(&hood(605), types), Some(60), "rounded down");
        assert_eq!(o2(&item(HOOD), types), None, "no canister in it");
        assert_eq!(o2(&item(CANISTER), types), None, "not a suit");
        assert_eq!(seconds_left(&item(0), types), None);
    }

    #[test]
    fn each_second_sealed_takes_its_draw_until_it_runs_out() {
        let types = test_items();
        let types = types.list();
        let mut suit = hood(25);
        assert_eq!(breathe_one_second(&mut suit, types), Some((25, 15)));
        assert_eq!(breathe_one_second(&mut suit, types), Some((15, 5)));
        assert_eq!(
            breathe_one_second(&mut suit, types),
            Some((5, 0)),
            "the last of it"
        );
        assert_eq!(breathe_one_second(&mut suit, types), None, "empty");
        assert_eq!(
            breathe_one_second(&mut item(HOOD), types),
            None,
            "no canister"
        );
        assert_eq!(breathe_one_second(&mut item(0), types), None, "not a suit");
    }

    #[test]
    fn warnings_come_once_each_as_the_canister_runs_down() {
        assert_eq!(levels_crossed(510, 500, 1000), [SuitAirLevel::Half]);
        assert_eq!(levels_crossed(500, 490, 1000), []);
        assert_eq!(levels_crossed(260, 250, 1000), [SuitAirLevel::Quarter]);
        assert_eq!(levels_crossed(101, 91, 1000), [SuitAirLevel::Low]);
        assert_eq!(levels_crossed(5, 0, 1000), [SuitAirLevel::Empty]);
        assert_eq!(
            levels_crossed(1000, 0, 1000),
            [
                SuitAirLevel::Half,
                SuitAirLevel::Quarter,
                SuitAirLevel::Low,
                SuitAirLevel::Empty
            ],
            "all at once"
        );
        assert_eq!(levels_crossed(0, 0, 1000), []);
    }

    #[test]
    fn canisters_and_suit_air_are_part_of_the_hash() {
        let hash = |canister: Option<&CanisterType>, air: Option<&SuitAir>| {
            let mut h = Fnv64::new();
            hash_air(canister, air, &mut h);
            h.finish()
        };
        let canister = CanisterType { capacity_ml: 10 };
        let air = SuitAir {
            ml_per_s: 5,
            canister: "can".into(),
        };
        let thirstier = SuitAir {
            ml_per_s: 6,
            ..air.clone()
        };
        let all = [
            hash(None, None),
            hash(Some(&canister), None),
            hash(None, Some(&air)),
            hash(None, Some(&thirstier)),
        ];
        for (i, h) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(h), "hash {i} repeats");
        }
    }
}
