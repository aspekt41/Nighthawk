//! Power cells and the tools that run on them (decision D59).
//!
//! A *cell* is an item that holds energy, counted in whole joules (a watt
//! for one second), so everything stays in whole numbers (decision D22).
//! A *powered* item has a slot for one cell, of the kind its content
//! names, and draws its `watts` from it: a light for every second it's
//! switched on, a tool like the laser cutter for every second of work it's
//! used for. A tool whose cell can't cover the work doesn't count, and a
//! light whose cell runs flat goes out and switches off.
//!
//! This is CDDA's idea of batteries as ammunition, kept small: one size of
//! cell for now, and no battery mods.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::hash::Fnv64;
use crate::item::{Item, ItemType};

/// The most a cell can hold, in kilojoules: about a big power-tool pack.
/// Content is checked against it, which also keeps every charge in
/// joules well inside a `u32`.
pub const MAX_CELL_KJ: u32 = 4000;

/// What makes an item type a power cell: how much it holds when full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CellType {
    /// How much energy it holds when full, in kilojoules (1 to
    /// [`MAX_CELL_KJ`]).
    pub capacity_kj: u32,
}

impl CellType {
    /// How much it holds when full, in joules.
    pub fn capacity_j(&self) -> u32 {
        self.capacity_kj.saturating_mul(1000)
    }
}

/// What makes an item type run on a cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Power {
    /// What it draws while it works, in watts (joules a second; at least 1).
    pub watts: u32,
    /// The id of the cell item it takes.
    pub cell: String,
}

/// Feeds an item type's cell and power into a state hash. `u64::MAX`
/// stands for "none", which no real size could be.
pub(crate) fn hash_power(cell: Option<&CellType>, power: Option<&Power>, h: &mut Fnv64) {
    h.u64(cell.map_or(u64::MAX, |c| u64::from(c.capacity_kj)));
    match power {
        Some(p) => h.u64(u64::from(p.watts)).str(&p.cell),
        None => h.u64(u64::MAX),
    };
}

/// The type of `item`, which the world's checks keep real.
fn kind_of<'a>(item: &Item, types: &'a [ItemType]) -> &'a ItemType {
    &types[usize::from(item.kind)]
}

/// The cell that powers `item`: the item itself if it's a cell, else the
/// cell in its slot, if any.
pub fn cell_in<'a>(item: &'a Item, types: &[ItemType]) -> Option<&'a Item> {
    if kind_of(item, types).cell.is_some() {
        Some(item)
    } else {
        item.cell.as_deref()
    }
}

/// The same, to change.
fn cell_in_mut<'a>(item: &'a mut Item, types: &[ItemType]) -> Option<&'a mut Item> {
    if kind_of(item, types).cell.is_some() {
        Some(item)
    } else {
        item.cell.as_deref_mut()
    }
}

/// The charge `item` can draw on and the most it could hold, in joules:
/// a cell's own, or that of the cell in a powered item's slot. `None` for
/// anything else, and for a powered item with no cell in it.
pub fn charge(item: &Item, types: &[ItemType]) -> Option<(u32, u32)> {
    let cell = cell_in(item, types)?;
    let capacity = kind_of(cell, types).cell.as_ref()?.capacity_j();
    Some((cell.charge_j, capacity))
}

/// The energy `item` needs to work for `seconds`, in joules: 0 for
/// anything that doesn't run on a cell.
pub(crate) fn needed_j(item: &Item, types: &[ItemType], seconds: u32) -> u64 {
    kind_of(item, types)
        .power
        .as_ref()
        .map_or(0, |p| u64::from(p.watts) * u64::from(seconds))
}

/// Whether `item` can work for `seconds`: it needs no cell, or its cell
/// holds enough.
pub(crate) fn can_run(item: &Item, types: &[ItemType], seconds: u32) -> bool {
    let have = charge(item, types).map_or(0, |(have, _)| u64::from(have));
    needed_j(item, types, seconds) <= have
}

/// Takes the energy for `seconds` of work out of `item`'s cell, or all
/// that's left if that's less.
pub(crate) fn draw(item: &mut Item, types: &[ItemType], seconds: u32) {
    let need = needed_j(item, types, seconds);
    if let Some(cell) = cell_in_mut(item, types) {
        let take = u32::try_from(need).unwrap_or(u32::MAX);
        cell.charge_j = cell.charge_j.saturating_sub(take);
    }
}

/// Whether `item` is a light that needs a cell (so it can be switched).
pub fn is_switchable(item: &Item, types: &[ItemType]) -> bool {
    let kind = kind_of(item, types);
    kind.light > 0 && kind.power.is_some()
}

/// How far `item`'s light reaches right now: its `light` if it needs no
/// cell, or if it's switched on and its cell isn't flat; else 0.
pub(crate) fn light_now(item: &Item, types: &[ItemType]) -> u8 {
    let kind = kind_of(item, types);
    let lit = kind.power.is_none() || (item.on && charge(item, types).is_some_and(|(j, _)| j > 0));
    if lit { kind.light } else { 0 }
}

/// One second of a switched-on light using its cell. Returns whether it
/// just went out: its cell ran flat, so it switched itself off.
pub(crate) fn burn_one_second(item: &mut Item, types: &[ItemType]) -> bool {
    if !item.on {
        return false;
    }
    draw(item, types, 1);
    let flat = charge(item, types).is_none_or(|(j, _)| j == 0);
    if flat {
        item.on = false;
    }
    flat
}

/// One second of a charger putting `watts` into the first cell among
/// `items` (on its own, or in a tool's slot) that isn't full yet.
pub(crate) fn charge_one_second(items: &mut [Item], types: &[ItemType], watts: u32) {
    let not_full = items
        .iter_mut()
        .filter_map(|item| cell_in_mut(item, types))
        .find(|cell| charge(cell, types).is_some_and(|(j, full)| j < full));
    if let Some(cell) = not_full {
        let full = charge(cell, types).map_or(0, |(_, full)| full);
        cell.charge_j = cell.charge_j.saturating_add(watts).min(full);
    }
}

/// A charge in joules as whole kilojoules, rounded down: what's shown.
pub fn kj(joules: u64) -> u64 {
    joules / 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::tests::test_items;

    /// The test item numbers: the cell (10 kJ), the torch (a light, 10 W)
    /// and the saw (LASER 2, 100 W), both taking the cell; and scrap.
    const SCRAP: u16 = 2;
    const CELL: u16 = 4;
    const TORCH: u16 = 5;
    const SAW: u16 = 6;

    fn item(kind: u16) -> Item {
        Item::new(kind, 0, test_items().list())
    }

    /// A `kind` with a cell holding `joules` in its slot.
    fn loaded(kind: u16, joules: u32) -> Item {
        let mut cell = item(CELL);
        cell.charge_j = joules;
        let mut tool = item(kind);
        tool.cell = Some(Box::new(cell));
        tool
    }

    #[test]
    fn a_cell_holds_its_own_charge_and_a_tool_holds_its_cells() {
        let types = test_items();
        let types = types.list();
        let mut cell = item(CELL);
        cell.charge_j = 2500;
        assert_eq!(charge(&cell, types), Some((2500, 10_000)));
        assert_eq!(charge(&loaded(SAW, 7000), types), Some((7000, 10_000)));
        assert_eq!(charge(&item(SAW), types), None, "no cell in it");
        assert_eq!(charge(&item(SCRAP), types), None);
        assert_eq!(kj(7999), 7, "rounded down");
    }

    #[test]
    fn a_tool_runs_only_if_its_cell_covers_the_work() {
        let types = test_items();
        let types = types.list();
        let saw = loaded(SAW, 6000);
        assert_eq!(needed_j(&saw, types, 60), 6000, "100 W for a minute");
        assert!(can_run(&saw, types, 60));
        assert!(!can_run(&saw, types, 61));
        assert!(!can_run(&item(SAW), types, 1), "no cell");
        assert!(can_run(&item(SCRAP), types, 1000), "needs no power");
        let mut saw = saw;
        draw(&mut saw, types, 30);
        assert_eq!(charge(&saw, types), Some((3000, 10_000)));
        draw(&mut saw, types, 100);
        assert_eq!(charge(&saw, types), Some((0, 10_000)), "never below 0");
        let mut scrap = item(SCRAP);
        draw(&mut scrap, types, 10);
        assert_eq!(scrap, item(SCRAP), "nothing to draw from");
    }

    #[test]
    fn a_powered_light_shines_only_when_on_and_charged_and_goes_out_when_flat() {
        let types = test_items();
        let types = types.list();
        let mut torch = loaded(TORCH, 15);
        assert!(is_switchable(&torch, types) && !is_switchable(&item(SAW), types));
        assert_eq!(light_now(&torch, types), 0, "off");
        assert!(!burn_one_second(&mut torch, types), "off: uses nothing");
        assert_eq!(charge(&torch, types), Some((15, 10_000)));
        torch.on = true;
        assert_eq!(light_now(&torch, types), 3);
        assert!(!burn_one_second(&mut torch, types));
        assert!(burn_one_second(&mut torch, types), "10 W: flat after 2 s");
        assert!(!torch.on, "it switched itself off");
        assert_eq!(light_now(&torch, types), 0);
        let mut empty = item(TORCH);
        empty.on = true;
        assert!(burn_one_second(&mut empty, types), "no cell at all");
        let lamp = item(3);
        assert_eq!(light_now(&lamp, types), 2, "a light with no cell shines");
    }

    #[test]
    fn a_charger_fills_the_first_cell_that_is_not_full() {
        let types = test_items();
        let types = types.list();
        let mut full = item(CELL);
        full.charge_j = 10_000;
        let mut items = vec![item(SCRAP), full, loaded(TORCH, 9990), item(CELL)];
        charge_one_second(&mut items, types, 25);
        assert_eq!(
            charge(&items[2], types),
            Some((10_000, 10_000)),
            "topped up"
        );
        assert_eq!(items[3].charge_j, 0, "only one at a time");
        charge_one_second(&mut items, types, 25);
        assert_eq!(items[3].charge_j, 25);
        let mut nothing = vec![item(SCRAP)];
        charge_one_second(&mut nothing, types, 25);
        assert_eq!(nothing, vec![item(SCRAP)]);
    }

    #[test]
    fn cells_and_power_are_part_of_the_hash() {
        let hash = |cell: Option<&CellType>, power: Option<&Power>| {
            let mut h = Fnv64::new();
            hash_power(cell, power, &mut h);
            h.finish()
        };
        let cell = CellType { capacity_kj: 10 };
        let power = Power {
            watts: 5,
            cell: "cell".into(),
        };
        let other = Power {
            watts: 6,
            ..power.clone()
        };
        let all = [
            hash(None, None),
            hash(Some(&cell), None),
            hash(None, Some(&power)),
            hash(None, Some(&other)),
        ];
        for (i, h) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(h), "hash {i} repeats");
        }
        assert_eq!(cell.capacity_j(), 10_000);
    }
}
