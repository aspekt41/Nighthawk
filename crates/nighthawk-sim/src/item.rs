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

use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::material::{Material, Materials};
use crate::pos::Pos;
use crate::quality::{LevelError, Levels, Qualities, hash_levels};

/// One pocket of a wearable item, as content: its name and how much it
/// holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wear {
    pub pockets: Vec<PocketType>,
}

/// A kind of item, from `data/items/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            .u64(u64::from(self.solid_percent));
        hash_levels(&self.qualities, h);
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
/// lists), and what's in each of its pockets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub kind: u16,
    pub material: u16,
    /// One list per pocket of its type, in the same order.
    pub pockets: Vec<Vec<Item>>,
}

impl Item {
    /// A new, empty item of type `kind` (an index into `types`).
    pub fn new(kind: u16, material: u16, types: &[ItemType]) -> Item {
        let pockets = types[usize::from(kind)].pockets().len();
        Item {
            kind,
            material,
            pockets: vec![Vec::new(); pockets],
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
        own + contents
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

/// Takes the item at `path` out of its pocket, if there is one there.
pub(crate) fn take(worn: &mut [Item], path: ItemPath) -> Option<Item> {
    let pocket = worn.get_mut(path.worn)?.pockets.get_mut(path.pocket)?;
    (path.index < pocket.len()).then(|| pocket.remove(path.index))
}

/// Takes `count` items of type `kind` out of the pockets of `worn`, the
/// last-pocketed first, and returns them in the order they were found. If
/// there are fewer than `count`, takes all there are.
pub(crate) fn take_kind(worn: &mut [Item], kind: u16, count: usize) -> Vec<Item> {
    let paths: Vec<ItemPath> = worn
        .iter()
        .enumerate()
        .flat_map(|(w, wearing)| pocketed(w, wearing))
        .filter(|(_, item)| item.kind == kind)
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
    item.pockets
        .iter()
        .flatten()
        .try_for_each(|inner| check_item(inner, ctx))
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
        }
    }
}

impl std::error::Error for ItemError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemsFile {
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
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::material::tests::test_materials;
    use crate::quality::tests::test_qualities;

    /// Test items: a "suit" with two 1 L pockets (2.5 kg each), a "cutter"
    /// (LASER 1, 800 ml, half solid) and "scrap" (100 ml, solid), all
    /// made of the test "steel" (1000 kg/m³ in the test materials).
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
              "material": "steel", "volume_ml": 100, "solid_percent": 100 }
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
        assert_eq!(items.list().len(), 3);
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
        // Each change, and which type (0 the suit, 1 the cutter) it's made to.
        type Change = (usize, fn(&mut ItemType));
        let changes: [Change; 5] = [
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
            material: 0,
            pockets: vec![],
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
mod take_kind_tests {
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
        let taken = take_kind(&mut worn, 2, 2);
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
        worn[0].pockets[1].push(Item::new(2, 0, items.list()));
        assert_eq!(take_kind(&mut worn, 2, 5).len(), 1);
        assert!(take_kind(&mut worn, 2, 1).is_empty());
    }
}
