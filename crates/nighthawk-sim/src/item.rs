//! Items: things you can pick up, carry and wear (decisions D38 and D39).
//!
//! An *item type* is content, loaded from `data/items/*.json`: a jumpsuit, a
//! laser cutter, a piece of scrap. An *item* is one actual thing in the
//! game: a type, made of a material (scrap can be steel or aluminium), and,
//! if the type has pockets, whatever is in them.
//!
//! An item's weight comes from physics: its volume × its material's density
//! × how solid it is (a laser cutter is mostly air and wiring; a piece of
//! scrap is solid metal), plus whatever it holds.
//!
//! Items you *wear* (the jumpsuit) have pockets; items you *carry* go in
//! those pockets. A pocket holds a set volume and weight.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::light::MAX_LIGHT;
use crate::material::{Material, Materials};
use crate::pos::Pos;
use crate::power::{self, CellType, MAX_CELL_KJ, Power};
use crate::quality::{LevelError, Levels, Qualities, hash_levels};

/// One pocket of a wearable item, as content: its name and how much it
/// holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PocketType {
    /// Like "left hip pocket".
    pub name: String,
    /// How much fits, in millilitres.
    pub volume_ml: u32,
    /// How much weight it holds, in grams.
    pub max_mass_g: u32,
}

/// What wearing an item gives you. For now, just its pockets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Wear {
    pub pockets: Vec<PocketType>,
}

/// A kind of item, from `data/items/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemType {
    /// How maps refer to it, like `"basic_laser_cutter"`.
    pub id: String,
    /// How it's shown to the player, like "basic laser cutter".
    pub name: String,
    pub description: String,
    /// The character the map draws it with, lying on the floor.
    pub glyph: char,
    /// The id of the material it's made of, unless a map says otherwise.
    pub material: String,
    /// How much room it takes up, in millilitres.
    pub volume_ml: u32,
    /// How much of its volume is solid material, in percent (1 to 100).
    pub solid_percent: u32,
    /// The tool qualities it has, like `{ "LASER": 1 }`.
    #[serde(default)]
    pub qualities: Levels,
    /// If it can be worn, what wearing it gives.
    #[serde(default)]
    pub wear: Option<Wear>,
    /// How many tiles its light reaches, if it gives light (like a hand
    /// lamp), from 1 to 20: around it where it lies, or around you when
    /// you carry it. Optional: it gives no light. With `power`, only while
    /// it's switched on and its cell isn't flat.
    #[serde(default)]
    pub light: u8,
    /// If it's a power cell, how much it holds (decision D59).
    #[serde(default)]
    pub cell: Option<CellType>,
    /// If it runs on a power cell, what it draws and which cell it takes.
    #[serde(default)]
    pub power: Option<Power>,
}

impl ItemType {
    /// The pockets this item has (none unless it's wearable with pockets).
    pub fn pockets(&self) -> &[PocketType] {
        self.wear.as_ref().map_or(&[], |w| w.pockets.as_slice())
    }

    fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name).str(&self.description);
        h.u64(u64::from(self.glyph))
            .str(&self.material)
            .u64(u64::from(self.volume_ml))
            .u64(u64::from(self.solid_percent))
            .u8(self.light);
        hash_levels(&self.qualities, h);
        power::hash_power(self.cell.as_ref(), self.power.as_ref(), h);
        let pockets = self.wear.as_ref().map(|w| w.pockets.as_slice());
        h.u64(pockets.map_or(u64::MAX, |p| p.len() as u64));
        for pocket in pockets.unwrap_or_default() {
            h.str(&pocket.name)
                .u64(u64::from(pocket.volume_ml))
                .u64(u64::from(pocket.max_mass_g));
        }
    }
}

/// Feeds a list of item types into a state hash.
pub(crate) fn hash_types(types: &[ItemType], h: &mut Fnv64) {
    h.u64(types.len() as u64);
    for t in types {
        t.hash_into(h);
    }
}

/// Every item type from the files in `data/items/`, saved as just the
/// list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemTypes {
    list: Vec<ItemType>,
}

impl ItemTypes {
    pub fn list(&self) -> &[ItemType] {
        &self.list
    }

    /// The index of the item type with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|t| t.id == id)?;
        Some(u16::try_from(i).expect("item type count was checked on load"))
    }

    /// The list, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn list_mut(&mut self) -> &mut Vec<ItemType> {
        &mut self.list
    }
}

/// One actual item: its type and material (indexes into the world's
/// lists), what's in each of its pockets, and, for power cells and the
/// things that run on them (decision D59), its charge, its cell and
/// whether it's switched on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub kind: u16,
    pub material: u16,
    /// One list per pocket of its type, in the same order.
    pub pockets: Vec<Vec<Item>>,
    /// For a power cell: the energy in it, in joules.
    #[serde(default)]
    pub charge_j: u32,
    /// For something that runs on a cell: the cell in its slot, if any.
    /// `Box` keeps it on the heap, since an item can't hold a whole item
    /// directly inside itself (it would have no fixed size).
    #[serde(default)]
    pub cell: Option<Box<Item>>,
    /// For a light that runs on a cell: whether it's switched on.
    #[serde(default)]
    pub on: bool,
}

impl Item {
    /// A new, empty item of type `kind` (an index into `types`).
    pub fn new(kind: u16, material: u16, types: &[ItemType]) -> Item {
        let pockets = types[usize::from(kind)].pockets().len();
        Item {
            kind,
            material,
            pockets: vec![Vec::new(); pockets],
            charge_j: 0,
            cell: None,
            on: false,
        }
    }

    /// Which item this is, without its contents: what events report.
    pub fn id(&self) -> ItemRef {
        ItemRef {
            kind: self.kind,
            material: self.material,
        }
    }

    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(u64::from(self.kind))
            .u64(u64::from(self.material))
            .u64(self.pockets.len() as u64);
        for pocket in &self.pockets {
            h.u64(pocket.len() as u64);
            for item in pocket {
                item.hash_into(h);
            }
        }
        h.u64(u64::from(self.charge_j)).u8(u8::from(self.on));
        match &self.cell {
            Some(cell) => cell.hash_into(h.u8(1)),
            None => {
                h.u8(0);
            }
        }
    }
}

/// An item lying on the floor at `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorItem {
    pub at: Pos,
    pub item: Item,
}

/// An item's type and material, small enough to copy into an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRef {
    pub kind: u16,
    pub material: u16,
}

/// The item types and materials, which together say how big and heavy an
/// item is. A `Copy` bundle of two borrowed lists.
#[derive(Debug, Clone, Copy)]
pub struct ItemContext<'a> {
    pub types: &'a [ItemType],
    pub materials: &'a [Material],
}

impl ItemContext<'_> {
    /// The item's own volume, in millilitres. A pocket's contents don't add
    /// to it: they're inside.
    pub fn volume_ml(&self, item: &Item) -> u64 {
        u64::from(self.types[usize::from(item.kind)].volume_ml)
    }

    /// The item's weight in grams, with everything in its pockets:
    /// volume (ml) × density (kg/m³) × solid share ÷ 1000, since a
    /// millilitre of something with a density of 1000 kg/m³ weighs 1 g.
    pub fn mass_g(&self, item: &Item) -> u64 {
        let kind = &self.types[usize::from(item.kind)];
        let density = u64::from(self.materials[usize::from(item.material)].density);
        let own = u64::from(kind.volume_ml) * density * u64::from(kind.solid_percent) / 100 / 1000;
        let contents: u64 = item.pockets.iter().flatten().map(|i| self.mass_g(i)).sum();
        let cell = item.cell.as_deref().map_or(0, |c| self.mass_g(c));
        own + contents + cell
    }

    /// Whether `item` fits in `pocket` alongside what's already in it.
    pub fn fits(&self, pocket: &PocketType, contents: &[Item], item: &Item) -> bool {
        let volume: u64 = contents.iter().map(|i| self.volume_ml(i)).sum();
        let mass: u64 = contents.iter().map(|i| self.mass_g(i)).sum();
        volume + self.volume_ml(item) <= u64::from(pocket.volume_ml)
            && mass + self.mass_g(item) <= u64::from(pocket.max_mass_g)
    }
}

/// Where a carried item is: which worn item, which of its pockets, and
/// which item in that pocket (counting from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemPath {
    pub worn: usize,
    pub pocket: usize,
    pub index: usize,
}

/// Everything in the pockets of `wearing` (worn item number `worn`), with
/// where each one is.
pub(crate) fn pocketed(worn: usize, wearing: &Item) -> Vec<(ItemPath, &Item)> {
    let mut all = Vec::new();
    for (pocket, held) in wearing.pockets.iter().enumerate() {
        for (index, item) in held.iter().enumerate() {
            let path = ItemPath {
                worn,
                pocket,
                index,
            };
            all.push((path, item));
        }
    }
    all
}

/// Puts `item` in the first pocket of `worn` it fits in, in order: the
/// first worn item's pockets first. If it fits nowhere, hands it back.
pub(crate) fn stow(worn: &mut [Item], item: Item, ctx: ItemContext) -> Result<(), Item> {
    for wearing in worn.iter_mut() {
        let pocket_types = ctx.types[usize::from(wearing.kind)].pockets();
        let room = pocket_types
            .iter()
            .zip(&wearing.pockets)
            .position(|(pocket, held)| ctx.fits(pocket, held, &item));
        if let Some(p) = room {
            wearing.pockets[p].push(item);
            return Ok(());
        }
    }
    Err(item)
}

/// The item at `path`, if there is one there.
pub(crate) fn get(worn: &[Item], path: ItemPath) -> Option<&Item> {
    worn.get(path.worn)?
        .pockets
        .get(path.pocket)?
        .get(path.index)
}

/// The item at `path`, if there is one there, to change.
pub(crate) fn get_mut(worn: &mut [Item], path: ItemPath) -> Option<&mut Item> {
    let pocket = worn.get_mut(path.worn)?.pockets.get_mut(path.pocket)?;
    pocket.get_mut(path.index)
}

/// Takes the item at `path` out of its pocket, if there is one there.
pub(crate) fn take(worn: &mut [Item], path: ItemPath) -> Option<Item> {
    let pocket = worn.get_mut(path.worn)?.pockets.get_mut(path.pocket)?;
    (path.index < pocket.len()).then(|| pocket.remove(path.index))
}

/// Takes `count` carried items that `wanted` says yes to (like "scrap
/// made of metal") out of the pockets of `worn`, the last-pocketed first,
/// and returns them in the order they were found. If there are fewer than
/// `count`, takes all there are.
pub(crate) fn take_matching(
    worn: &mut [Item],
    count: usize,
    wanted: impl Fn(&Item) -> bool,
) -> Vec<Item> {
    let paths: Vec<ItemPath> = worn
        .iter()
        .enumerate()
        .flat_map(|(w, wearing)| pocketed(w, wearing))
        .filter(|(_, item)| wanted(item))
        .map(|(path, _)| path)
        .collect();
    let chosen = &paths[paths.len().saturating_sub(count)..];
    // Remove from the end backwards, so the earlier paths stay right.
    let mut taken: Vec<Item> = chosen
        .iter()
        .rev()
        .filter_map(|&path| take(worn, path))
        .collect();
    taken.reverse();
    taken
}

/// Takes `count` items that `wanted` says yes to out of `pile` (the
/// contents of a locker, say), the last put in first, and returns them in
/// the order they lay. If there are fewer than `count`, takes all there
/// are.
pub(crate) fn take_from_pile(
    pile: &mut Vec<Item>,
    count: usize,
    wanted: impl Fn(&Item) -> bool,
) -> Vec<Item> {
    let found: Vec<usize> = (0..pile.len()).filter(|&i| wanted(&pile[i])).collect();
    let chosen = &found[found.len().saturating_sub(count)..];
    // As in `take_matching`: from the end backwards, then put in order.
    let mut taken: Vec<Item> = chosen.iter().rev().map(|&i| pile.remove(i)).collect();
    taken.reverse();
    taken
}

/// Checks a saved item makes sense: its type and material exist, it has one
/// list per pocket of its type, and so does everything in its pockets.
pub(crate) fn check_item(item: &Item, ctx: ItemContext) -> Result<(), String> {
    let Some(kind) = ctx.types.get(usize::from(item.kind)) else {
        return Err(format!(
            "an item has type {}, which doesn't exist",
            item.kind
        ));
    };
    if usize::from(item.material) >= ctx.materials.len() {
        return Err(format!(
            "a {} is made of a material that doesn't exist",
            kind.id
        ));
    }
    if item.pockets.len() != kind.pockets().len() {
        return Err(format!(
            "a {} has {} pockets but should have {}",
            kind.id,
            item.pockets.len(),
            kind.pockets().len()
        ));
    }
    check_power(item, kind, ctx)?;
    item.pockets
        .iter()
        .flatten()
        .try_for_each(|inner| check_item(inner, ctx))
}

/// Checks a saved item's power makes sense: only a cell holds charge, and
/// no more than it can; only something that takes a cell has one, of the
/// kind it takes; and only a light that runs on a cell is switched on.
fn check_power(item: &Item, kind: &ItemType, ctx: ItemContext) -> Result<(), String> {
    let capacity = kind.cell.as_ref().map_or(0, CellType::capacity_j);
    if item.charge_j > capacity {
        return Err(format!("a {} holds more charge than it can", kind.id));
    }
    if item.on && !power::is_switchable(item, ctx.types) {
        return Err(format!("a {} is switched on, but can't be", kind.id));
    }
    let Some(cell) = item.cell.as_deref() else {
        return Ok(());
    };
    let takes = kind.power.as_ref().map(|p| p.cell.as_str());
    let cell_id = ctx.types.get(usize::from(cell.kind)).map(|t| t.id.as_str());
    if takes.is_none() || takes != cell_id {
        return Err(format!("a {} has a cell it can't take", kind.id));
    }
    check_item(cell, ctx)
}

/// Everything that can be wrong with the item files.
#[derive(Debug)]
pub enum ItemError {
    File(FileError),
    NoItems,
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    UnknownMaterial {
        item: String,
        material: String,
    },
    /// A solid share outside 1 to 100%, or no volume at all.
    BadSize {
        item: String,
    },
    /// Its qualities name one that doesn't exist, or a level out of range.
    Qualities {
        item: String,
        error: LevelError,
    },
    /// Its light reaches further than any light can ([`MAX_LIGHT`]).
    TooBright {
        item: String,
        light: u8,
    },
    /// Something wrong with its `cell` or `power`; `problem` says what.
    Power {
        item: String,
        problem: &'static str,
    },
}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemError::File(e) => write!(f, "invalid items file {e}"),
            ItemError::NoItems => write!(f, "there are no items"),
            ItemError::TooMany { count } => {
                write!(
                    f,
                    "{count} item types is too many; the most is {}",
                    u16::MAX
                )
            }
            ItemError::DuplicateId { id } => write!(f, "item {id:?} is defined twice"),
            ItemError::UnknownMaterial { item, material } => write!(
                f,
                "item {item:?} is made of {material:?}, which isn't a material"
            ),
            ItemError::BadSize { item } => write!(
                f,
                "item {item:?} needs a volume above 0 and a solid_percent from 1 to 100"
            ),
            ItemError::Qualities { item, error } => {
                write!(f, "item {item:?}'s qualities: {error}")
            }
            ItemError::TooBright { item, light } => write!(
                f,
                "item {item:?} has a light of {light}, but light can reach at most {MAX_LIGHT} tiles"
            ),
            ItemError::Power { item, problem } => write!(f, "item {item:?} {problem}"),
        }
    }
}

impl std::error::Error for ItemError {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemsFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/items.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    items: Vec<ItemType>,
}

/// Parses and checks every items file. Each item's material must be one of
/// `materials`, and its qualities among `qualities`.
pub(crate) fn parse_items(
    files: &[ContentFile],
    materials: &Materials,
    qualities: &Qualities,
) -> Result<ItemTypes, ItemError> {
    let list = read_all(files, |f: ItemsFile| f.items).map_err(ItemError::File)?;
    check_list(&list)?;
    list.iter().try_for_each(|t| check_one(t, materials))?;
    list.iter().try_for_each(|t| check_power_type(t, &list))?;
    for item in &list {
        qualities
            .check(&item.qualities)
            .map_err(|error| ItemError::Qualities {
                item: item.id.clone(),
                error,
            })?;
    }
    Ok(ItemTypes { list })
}

/// Checks the list as a whole: not empty, not too long, no id twice.
fn check_list(list: &[ItemType]) -> Result<(), ItemError> {
    if list.is_empty() {
        return Err(ItemError::NoItems);
    }
    if u16::try_from(list.len()).is_err() {
        return Err(ItemError::TooMany { count: list.len() });
    }
    match first_duplicate(list.iter().map(|t| t.id.as_str())) {
        Some(id) => Err(ItemError::DuplicateId { id: id.into() }),
        None => Ok(()),
    }
}

fn check_one(item: &ItemType, materials: &Materials) -> Result<(), ItemError> {
    if materials.index_of(&item.material).is_none() {
        return Err(ItemError::UnknownMaterial {
            item: item.id.clone(),
            material: item.material.clone(),
        });
    }
    if item.volume_ml == 0 || !(1..=100).contains(&item.solid_percent) {
        return Err(ItemError::BadSize {
            item: item.id.clone(),
        });
    }
    if item.light > MAX_LIGHT {
        return Err(ItemError::TooBright {
            item: item.id.clone(),
            light: item.light,
        });
    }
    Ok(())
}

/// Checks an item type's `cell` and `power`: a cell holds 1 to
/// [`MAX_CELL_KJ`] kJ; a powered item draws at least 1 W and takes a cell
/// that's in `list`; and nothing is both.
fn check_power_type(item: &ItemType, list: &[ItemType]) -> Result<(), ItemError> {
    let problem = |problem| {
        Err(ItemError::Power {
            item: item.id.clone(),
            problem,
        })
    };
    if item
        .cell
        .as_ref()
        .is_some_and(|c| !(1..=MAX_CELL_KJ).contains(&c.capacity_kj))
    {
        return problem("is a cell that must hold 1 to 4000 kJ");
    }
    let Some(power) = &item.power else {
        return Ok(());
    };
    if item.cell.is_some() {
        return problem("can't both be a cell and take one");
    }
    if power.watts == 0 {
        return problem("must draw at least 1 watt");
    }
    let takes = list.iter().find(|t| t.id == power.cell);
    if !takes.is_some_and(|t| t.cell.is_some()) {
        return problem("takes a cell that isn't an item with a \"cell\"");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::material::tests::test_materials;
    use crate::quality::tests::test_qualities;

    /// Test items: a "suit" with two 1 L pockets (2.5 kg each), a "cutter"
    /// (LASER 1, 800 ml, half solid), "scrap" (100 ml, solid) and a
    /// "lamp" (light 2, 100 ml, half solid), all made of the test "steel"
    /// (1000 kg/m³ in the test materials).
    pub(crate) fn test_items_json() -> String {
        r#"{ "items": [
            { "id": "suit", "name": "suit", "description": "A suit.", "glyph": "[",
              "material": "steel", "volume_ml": 2000, "solid_percent": 30,
              "wear": { "pockets": [
                  { "name": "left pocket", "volume_ml": 1000, "max_mass_g": 2500 },
                  { "name": "right pocket", "volume_ml": 1000, "max_mass_g": 2500 } ] } },
            { "id": "cutter", "name": "cutter", "description": "A cutter.", "glyph": "(",
              "material": "steel", "volume_ml": 800, "solid_percent": 50,
              "qualities": { "LASER": 1 } },
            { "id": "scrap", "name": "scrap", "description": "Scrap.", "glyph": "*",
              "material": "steel", "volume_ml": 100, "solid_percent": 100 },
            { "id": "lamp", "name": "lamp", "description": "A lamp.", "glyph": "!",
              "material": "steel", "volume_ml": 100, "solid_percent": 50, "light": 2 },
            { "id": "cell", "name": "cell", "description": "A cell.", "glyph": "=",
              "material": "steel", "volume_ml": 50, "solid_percent": 100,
              "cell": { "capacity_kj": 10 } },
            { "id": "torch", "name": "torch", "description": "A torch.", "glyph": "!",
              "material": "steel", "volume_ml": 100, "solid_percent": 50, "light": 3,
              "power": { "watts": 10, "cell": "cell" } },
            { "id": "saw", "name": "saw", "description": "A saw.", "glyph": "(",
              "material": "steel", "volume_ml": 300, "solid_percent": 50,
              "qualities": { "LASER": 2 }, "power": { "watts": 100, "cell": "cell" } }
        ] }"#
            .to_string()
    }

    pub(crate) fn test_items() -> ItemTypes {
        parse_items(
            &[ContentFile::new("test.json", test_items_json())],
            &test_materials(),
            &test_qualities(),
        )
        .expect("test items are valid")
    }

    fn error_of(json: &str) -> String {
        parse_items(
            &[ContentFile::new("tools.json", json)],
            &test_materials(),
            &test_qualities(),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn a_valid_file_loads() {
        let items = test_items();
        assert_eq!(items.list().len(), 7);
        assert_eq!(items.index_of("cutter"), Some(1));
        assert_eq!(items.index_of("cheese"), None);
        assert_eq!(items.list()[0].pockets().len(), 2);
        assert!(items.list()[2].pockets().is_empty());
        assert_eq!(items.list()[1].qualities.get("LASER"), Some(&1));
    }

    #[test]
    fn weight_comes_from_volume_density_and_solidity() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = ItemContext {
            types: items.list(),
            materials: materials.list(),
        };
        let scrap = Item::new(2, 0, items.list());
        let cutter = Item::new(1, 0, items.list());
        assert_eq!(ctx.mass_g(&scrap), 100, "100 ml × 1000 kg/m³ × 100%");
        assert_eq!(ctx.mass_g(&cutter), 400, "800 ml × 1000 kg/m³ × 50%");
        assert_eq!(ctx.volume_ml(&cutter), 800);
    }

    #[test]
    fn what_a_pocket_holds_adds_to_the_weight_but_not_the_volume() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = ItemContext {
            types: items.list(),
            materials: materials.list(),
        };
        let mut suit = Item::new(0, 0, items.list());
        assert_eq!(ctx.mass_g(&suit), 600);
        suit.pockets[0].push(Item::new(2, 0, items.list()));
        assert_eq!(ctx.mass_g(&suit), 700);
        assert_eq!(ctx.volume_ml(&suit), 2000);
    }

    #[test]
    fn a_pocket_holds_a_set_volume_and_weight() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = ItemContext {
            types: items.list(),
            materials: materials.list(),
        };
        let pocket = &items.list()[0].pockets()[0];
        let cutter = Item::new(1, 0, items.list());
        let scrap = Item::new(2, 0, items.list());
        assert!(ctx.fits(pocket, &[], &cutter));
        // 800 ml + 2 × 100 ml = exactly 1 L: fits. A third scrap doesn't.
        let mut held = vec![cutter.clone(), scrap.clone()];
        assert!(ctx.fits(pocket, &held, &scrap));
        held.push(scrap.clone());
        assert!(!ctx.fits(pocket, &held, &scrap), "too big");
        // Ten dense scraps would be light enough by volume but too heavy.
        let heavy = ItemContext {
            types: items.list(),
            materials: &[Material {
                density: 30_000,
                ..materials.list()[0].clone()
            }],
        };
        let one = vec![scrap.clone()];
        assert!(!heavy.fits(pocket, &one, &scrap), "3 kg is over 2.5 kg");
    }

    #[test]
    fn mistakes_in_a_file_are_errors() {
        assert_eq!(error_of(r#"{ "items": [] }"#), "there are no items");
        assert!(error_of("[").starts_with("invalid items file tools.json: "));
        let unknown = test_items_json().replace(
            "\"material\": \"steel\", \"volume_ml\": 800",
            "\"material\": \"cheese\", \"volume_ml\": 800",
        );
        assert_eq!(
            error_of(&unknown),
            r#"item "cutter" is made of "cheese", which isn't a material"#
        );
        let twice = test_items_json().replace("\"id\": \"scrap\"", "\"id\": \"cutter\"");
        assert_eq!(error_of(&twice), r#"item "cutter" is defined twice"#);
    }

    #[test]
    fn an_item_s_light_reaches_at_most_the_limit() {
        let lamp =
            |reach: u8| test_items_json().replace("\"light\": 2", &format!("\"light\": {reach}"));
        let files = [ContentFile::new("lamp.json", lamp(MAX_LIGHT))];
        let items = parse_items(&files, &test_materials(), &test_qualities()).unwrap();
        assert_eq!(items.list()[3].light, MAX_LIGHT);
        assert_eq!(items.list()[0].light, 0, "none by default");
        assert_eq!(
            error_of(&lamp(MAX_LIGHT + 1)),
            r#"item "lamp" has a light of 21, but light can reach at most 20 tiles"#
        );
    }

    #[test]
    fn sizes_and_levels_must_make_sense() {
        let solid = test_items_json().replace("\"solid_percent\": 100", "\"solid_percent\": 101");
        assert_eq!(
            error_of(&solid),
            r#"item "scrap" needs a volume above 0 and a solid_percent from 1 to 100"#
        );
        let empty = test_items_json().replace("\"volume_ml\": 100,", "\"volume_ml\": 0,");
        assert!(error_of(&empty).contains("needs a volume above 0"));
        let level = test_items_json().replace("\"LASER\": 1", "\"LASER\": 0");
        assert_eq!(
            error_of(&level),
            r#"item "cutter"'s qualities: LASER 0 is out of range; levels go from 1 to 10"#
        );
        let unknown = test_items_json().replace("\"LASER\": 1", "\"SPORK\": 1");
        assert_eq!(
            error_of(&unknown),
            r#"item "cutter"'s qualities: "SPORK" isn't a quality"#
        );
    }

    #[test]
    fn cells_and_what_runs_on_them_must_make_sense() {
        let json = test_items_json();
        let error = |from: &str, to: &str| error_of(&json.replace(from, to));
        let cell = "\"cell\": { \"capacity_kj\": 10 }";
        for capacity in ["0", "4001"] {
            let odd = format!("\"cell\": {{ \"capacity_kj\": {capacity} }}");
            assert_eq!(
                error(cell, &odd),
                r#"item "cell" is a cell that must hold 1 to 4000 kJ"#
            );
        }
        let torch_power = "\"power\": { \"watts\": 10, \"cell\": \"cell\" }";
        assert_eq!(
            error(
                torch_power,
                "\"power\": { \"watts\": 0, \"cell\": \"cell\" }"
            ),
            r#"item "torch" must draw at least 1 watt"#
        );
        let takes = r#"item "torch" takes a cell that isn't an item with a "cell""#;
        let elsewhere = |cell: &str| torch_power.replace("\"cell\": \"cell\"", cell);
        assert_eq!(error(torch_power, &elsewhere("\"cell\": \"scrap\"")), takes);
        assert_eq!(error(torch_power, &elsewhere("\"cell\": \"jam\"")), takes);
        let both = format!("{cell}, {torch_power}");
        assert_eq!(
            error(cell, &both),
            r#"item "cell" can't both be a cell and take one"#
        );
    }

    #[test]
    fn a_cell_in_its_slot_weighs_with_what_it_is_in() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = context(&items, &materials);
        let mut saw = Item::new(6, 0, items.list());
        assert_eq!(ctx.mass_g(&saw), 150, "300 ml, half solid, 1 g/ml");
        saw.cell = Some(Box::new(Item::new(4, 0, items.list())));
        assert_eq!(ctx.mass_g(&saw), 200, "and a 50 ml solid cell");
    }

    #[test]
    fn saved_power_is_checked_too() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = context(&items, &materials);
        let new = |kind| Item::new(kind, 0, items.list());
        let mut cell = new(4);
        cell.charge_j = 10_001;
        assert_eq!(
            check_item(&cell, ctx),
            Err("a cell holds more charge than it can".to_string())
        );
        let mut saw = new(6);
        saw.on = true;
        assert_eq!(
            check_item(&saw, ctx),
            Err("a saw is switched on, but can't be".to_string())
        );
        let mut scrap = new(2);
        scrap.cell = Some(Box::new(new(4)));
        let wrong = Err("a scrap has a cell it can't take".to_string());
        assert_eq!(check_item(&scrap, ctx), wrong);
        let mut torch = new(5);
        torch.cell = Some(Box::new(new(2)));
        let wrong = Err("a torch has a cell it can't take".to_string());
        assert_eq!(check_item(&torch, ctx), wrong, "scrap is no cell");
        let mut overfull = new(4);
        overfull.charge_j = 20_000;
        torch.cell = Some(Box::new(overfull));
        assert!(check_item(&torch, ctx).is_err(), "the cell is checked too");
        torch.cell = Some(Box::new(new(4)));
        torch.on = true;
        assert_eq!(check_item(&torch, ctx), Ok(()));
    }

    #[test]
    fn too_many_item_types_is_an_error() {
        let one = test_items().list()[0].clone();
        let err = check_list(&vec![one; 70_000]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "70000 item types is too many; the most is 65535"
        );
    }

    #[test]
    fn everything_about_items_is_part_of_the_hash() {
        let hash_types_of = |types: &[ItemType]| {
            let mut h = Fnv64::new();
            hash_types(types, &mut h);
            h.finish()
        };
        let base = test_items().list().to_vec();
        // Each change, and which type (0 the suit, 1 the cutter, 4 the cell,
        // 5 the torch) it's made to.
        type Change = (usize, fn(&mut ItemType));
        let changes: [Change; 8] = [
            (4, |t| t.cell.as_mut().unwrap().capacity_kj += 1),
            (5, |t| t.power.as_mut().unwrap().watts += 1),
            (1, |t| t.light += 1),
            (0, |t| t.glyph = '%'),
            (0, |t| t.volume_ml += 1),
            (1, |t| t.qualities.clear()),
            (0, |t| t.wear = None),
            (0, |t| t.wear.as_mut().unwrap().pockets[0].max_mass_g += 1),
        ];
        for (which, change) in changes {
            let mut changed = base.clone();
            change(&mut changed[which]);
            assert_ne!(hash_types_of(&base), hash_types_of(&changed));
        }
        let item_hash = |item: &Item| {
            let mut h = Fnv64::new();
            item.hash_into(&mut h);
            h.finish()
        };
        let empty = Item::new(0, 0, &base);
        let mut full = empty.clone();
        full.pockets[1].push(Item::new(2, 0, &base));
        assert_ne!(item_hash(&empty), item_hash(&full));
    }

    fn context<'a>(items: &'a ItemTypes, materials: &'a Materials) -> ItemContext<'a> {
        ItemContext {
            types: items.list(),
            materials: materials.list(),
        }
    }

    #[test]
    fn stowing_fills_the_first_pocket_with_room() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = context(&items, &materials);
        let mut worn = vec![Item::new(0, 0, items.list())];
        let cutter = Item::new(1, 0, items.list());
        let scrap = Item::new(2, 0, items.list());
        assert_eq!(stow(&mut worn, cutter.clone(), ctx), Ok(()));
        assert_eq!(
            stow(&mut worn, cutter.clone(), ctx),
            Ok(()),
            "second pocket"
        );
        for _ in 0..4 {
            assert_eq!(stow(&mut worn, scrap.clone(), ctx), Ok(()));
        }
        assert_eq!(worn[0].pockets[0].len(), 3, "cutter and two scraps: 1 L");
        assert_eq!(worn[0].pockets[1].len(), 3);
        assert_eq!(stow(&mut worn, scrap.clone(), ctx), Err(scrap), "full");
        assert_eq!(
            stow(&mut [], cutter.clone(), ctx),
            Err(cutter),
            "no pockets at all"
        );
    }

    #[test]
    fn taking_removes_exactly_the_item_named() {
        let items = test_items();
        let mut worn = vec![Item::new(0, 0, items.list())];
        worn[0].pockets[1].push(Item::new(1, 0, items.list()));
        worn[0].pockets[1].push(Item::new(2, 0, items.list()));
        let path = ItemPath {
            worn: 0,
            pocket: 1,
            index: 1,
        };
        assert_eq!(take(&mut worn, path).map(|i| i.kind), Some(2));
        assert_eq!(worn[0].pockets[1].len(), 1);
        assert_eq!(take(&mut worn, path), None, "nothing there now");
        let nowhere = ItemPath {
            worn: 5,
            pocket: 0,
            index: 0,
        };
        assert_eq!(take(&mut worn, nowhere), None);
    }

    #[test]
    fn saved_items_are_checked_all_the_way_down() {
        let (items, materials) = (test_items(), test_materials());
        let ctx = context(&items, &materials);
        let mut suit = Item::new(0, 0, items.list());
        assert_eq!(check_item(&suit, ctx), Ok(()));
        suit.pockets[0].push(Item {
            kind: 9,
            ..Item::new(2, 0, items.list())
        });
        assert_eq!(
            check_item(&suit, ctx),
            Err("an item has type 9, which doesn't exist".to_string())
        );
        let odd_material = Item {
            material: 9,
            ..Item::new(2, 0, items.list())
        };
        assert!(
            check_item(&odd_material, ctx)
                .unwrap_err()
                .contains("made of a material")
        );
        let no_pockets = Item {
            pockets: vec![],
            ..Item::new(0, 0, items.list())
        };
        assert_eq!(
            check_item(&no_pockets, ctx),
            Err("a suit has 0 pockets but should have 2".to_string())
        );
    }
}

#[cfg(test)]
mod take_matching_tests {
    use super::tests::test_items;
    use super::*;

    #[test]
    fn taking_several_of_a_kind_takes_the_last_ones_and_leaves_the_rest() {
        let items = test_items();
        let mut worn = vec![Item::new(0, 0, items.list())];
        // Pocket 0: cutter, scrap(material 0); pocket 1: scrap(1), scrap(2).
        worn[0].pockets[0].push(Item::new(1, 0, items.list()));
        worn[0].pockets[0].push(Item::new(2, 0, items.list()));
        worn[0].pockets[1].push(Item::new(2, 1, items.list()));
        worn[0].pockets[1].push(Item::new(2, 2, items.list()));
        let taken = take_matching(&mut worn, 2, |i| i.kind == 2);
        let materials: Vec<u16> = taken.iter().map(|i| i.material).collect();
        assert_eq!(materials, vec![1, 2], "in the order they were found");
        assert_eq!(
            worn[0].pockets[0].len(),
            2,
            "the first scrap and the cutter stay"
        );
        assert!(worn[0].pockets[1].is_empty());
    }

    #[test]
    fn asking_for_more_than_there_are_takes_all_there_are() {
        let items = test_items();
        let mut worn = vec![Item::new(0, 0, items.list())];
        worn[0].pockets[0].push(Item::new(1, 0, items.list())); // a cutter
        worn[0].pockets[1].push(Item::new(2, 0, items.list()));
        assert_eq!(take_matching(&mut worn, 5, |i| i.kind == 2).len(), 1);
        assert!(take_matching(&mut worn, 1, |i| i.kind == 2).is_empty());
        assert_eq!(worn[0].pockets[0].len(), 1, "the cutter wasn't wanted");
    }

    #[test]
    fn taking_from_a_pile_takes_the_last_put_in_and_keeps_their_order() {
        let items = test_items();
        // Scrap of materials 0 and 1, a cutter, then scrap of material 2.
        let mut pile = vec![
            Item::new(2, 0, items.list()),
            Item::new(2, 1, items.list()),
            Item::new(1, 0, items.list()),
            Item::new(2, 2, items.list()),
        ];
        let taken = take_from_pile(&mut pile, 2, |i| i.kind == 2);
        let materials: Vec<u16> = taken.iter().map(|i| i.material).collect();
        assert_eq!(materials, vec![1, 2], "the last two, in the order they lay");
        assert_eq!(pile.len(), 2, "the first scrap and the cutter stay");
        assert_eq!(take_from_pile(&mut pile, 5, |i| i.kind == 2).len(), 1);
        assert!(take_from_pile(&mut pile, 1, |i| i.kind == 2).is_empty());
        assert_eq!(pile.len(), 1, "the cutter wasn't wanted");
    }

    #[test]
    fn get_finds_a_carried_item_by_its_path_or_nothing() {
        let items = test_items();
        let mut worn = vec![Item::new(0, 0, items.list())];
        worn[0].pockets[1].push(Item::new(1, 0, items.list()));
        let at = |worn: usize, pocket: usize, index: usize| ItemPath {
            worn,
            pocket,
            index,
        };
        assert_eq!(get(&worn, at(0, 1, 0)).map(|i| i.kind), Some(1));
        assert!(
            get(&worn, at(0, 1, 1)).is_none(),
            "past the end of a pocket"
        );
        assert!(get(&worn, at(0, 9, 0)).is_none(), "no such pocket");
        assert!(get(&worn, at(3, 0, 0)).is_none(), "nothing worn there");
    }
}
