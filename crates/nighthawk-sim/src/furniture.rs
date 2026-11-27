//! Furniture: things that stand on the floor (decision D53).
//!
//! Furniture is its own layer, on top of a tile's structure: a stasis
//! chamber stands on deck plating, and the deck plating is still there
//! underneath. Kinds of furniture are content, loaded from
//! `data/furniture/*.json`. A *piece* of furniture is one of those kinds
//! placed somewhere in the world, and it can cover more than one tile: a
//! stasis chamber is two tiles long. However many tiles it covers, a piece
//! is one thing, stored once, with its top-left tile and whether it's
//! turned a quarter (which swaps its width and height).
//!
//! What furniture does in the simulation is a couple of named properties,
//! like a structure's: whether you can walk through it (chairs, yes; a
//! bed, no) and whether it hides what's behind it (a locker does). Air
//! always flows past furniture.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::item::Item;
use crate::map::{Map, Tile};
use crate::material::{Colour, Materials};
use crate::pos::Pos;

/// The longest side a piece of furniture may have, in tiles.
pub const MAX_FURNITURE_SIDE: u8 = 4;

/// One kind of furniture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FurnitureType {
    /// How maps refer to it, like `"stasis_chamber"`.
    pub id: String,
    /// How it's shown to the player, like "stasis chamber".
    pub name: String,
    /// A sentence or two for look mode.
    pub description: String,
    /// The character the map draws on every tile it covers.
    pub glyph: char,
    /// The id of the material it's mostly made of.
    pub material: String,
    /// Optional: the colour to draw it in, instead of its material's.
    #[serde(default)]
    pub colour: Option<Colour>,
    /// Its width and height in tiles, from 1 to 4, as it stands unturned:
    /// `[2, 1]` is two tiles across and one down. Optional: `[1, 1]`.
    #[serde(default = "one_tile")]
    pub size: [u8; 2],
    /// You can walk through it, like a chair. Optional: it's in the way.
    #[serde(default)]
    pub walkable: bool,
    /// It's tall enough to hide what's behind it, like a locker.
    /// Optional: you can see over it.
    #[serde(default)]
    pub blocks_sight: bool,
    /// How much it holds, in millilitres, if it's a container like a
    /// locker. Optional: it holds nothing.
    #[serde(default)]
    pub holds_ml: u32,
}

/// The default size: one tile. Serde's `default = "..."` names a function.
fn one_tile() -> [u8; 2] {
    [1, 1]
}

impl FurnitureType {
    /// Its width and height in tiles, turned a quarter or not.
    pub fn footprint(&self, turned: bool) -> (i32, i32) {
        let [w, h] = self.size.map(i32::from);
        if turned { (h, w) } else { (w, h) }
    }

    /// Feeds everything about it into a state hash.
    fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name).str(&self.description);
        h.u64(u64::from(self.glyph))
            .str(&self.material)
            .u64(self.colour.map_or(u64::MAX, |c| u64::from(c.hash_id())))
            .u8(self.size[0])
            .u8(self.size[1])
            .u8(u8::from(self.walkable))
            .u8(u8::from(self.blocks_sight))
            .u64(u64::from(self.holds_ml));
    }
}

/// Every kind of furniture from the files in `data/furniture/`, saved as
/// just the list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FurnitureTypes {
    list: Vec<FurnitureType>,
}

impl FurnitureTypes {
    /// Every kind, in file order. A piece's `kind` is an index into this.
    pub fn list(&self) -> &[FurnitureType] {
        &self.list
    }

    /// The index of the kind with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|f| f.id == id)?;
        Some(u16::try_from(i).expect("furniture count was checked on load"))
    }

    /// Feeds every kind into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.list.len() as u64);
        for kind in &self.list {
            kind.hash_into(h);
        }
    }
}

/// Everything that can be wrong with the furniture files.
#[derive(Debug)]
pub enum FurnitureError {
    /// A file that isn't valid JSON, or has the wrong shape.
    File(FileError),
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    /// Furniture made of a material that isn't defined.
    UnknownMaterial {
        furniture: String,
        material: String,
    },
    /// A side of 0, or longer than [`MAX_FURNITURE_SIDE`].
    BadSize {
        furniture: String,
    },
}

impl fmt::Display for FurnitureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FurnitureError::File(e) => write!(f, "invalid furniture file {e}"),
            FurnitureError::TooMany { count } => write!(
                f,
                "{count} kinds of furniture is too many; the most is {}",
                u16::MAX
            ),
            FurnitureError::DuplicateId { id } => write!(f, "furniture {id:?} is defined twice"),
            FurnitureError::UnknownMaterial {
                furniture,
                material,
            } => write!(
                f,
                "furniture {furniture:?} is made of {material:?}, which isn't a material"
            ),
            FurnitureError::BadSize { furniture } => write!(
                f,
                "furniture {furniture:?} needs a size from 1 to {MAX_FURNITURE_SIDE} tiles each way"
            ),
        }
    }
}

impl std::error::Error for FurnitureError {}

/// The JSON shape of one file in `data/furniture/`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FurnitureFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/furniture.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    furniture: Vec<FurnitureType>,
}

/// Parses and checks every furniture file. Each kind's material must be
/// one of `materials`. A game may have no furniture at all.
pub(crate) fn parse_furniture(
    files: &[ContentFile],
    materials: &Materials,
) -> Result<FurnitureTypes, FurnitureError> {
    let list = read_all(files, |f: FurnitureFile| f.furniture).map_err(FurnitureError::File)?;
    check_list(&list)?;
    list.iter().try_for_each(|f| check_one(f, materials))?;
    Ok(FurnitureTypes { list })
}

/// Checks the whole list: not too many to number, and no id twice.
fn check_list(list: &[FurnitureType]) -> Result<(), FurnitureError> {
    if u16::try_from(list.len()).is_err() {
        return Err(FurnitureError::TooMany { count: list.len() });
    }
    match first_duplicate(list.iter().map(|f| f.id.as_str())) {
        Some(id) => Err(FurnitureError::DuplicateId { id: id.into() }),
        None => Ok(()),
    }
}

fn check_one(kind: &FurnitureType, materials: &Materials) -> Result<(), FurnitureError> {
    if materials.index_of(&kind.material).is_none() {
        return Err(FurnitureError::UnknownMaterial {
            furniture: kind.id.clone(),
            material: kind.material.clone(),
        });
    }
    if !kind
        .size
        .iter()
        .all(|&side| (1..=MAX_FURNITURE_SIDE).contains(&side))
    {
        return Err(FurnitureError::BadSize {
            furniture: kind.id.clone(),
        });
    }
    Ok(())
}

/// A piece of furniture in the world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    /// Which kind of furniture it is, by number in the world's content.
    pub kind: u16,
    /// What it's made of, by number in the world's content.
    pub material: u16,
    /// Its top-left tile.
    pub at: Pos,
    /// Turned a quarter, so its width and height are swapped.
    pub turned: bool,
    /// What's inside it, if it's a container.
    pub contents: Vec<Item>,
}

impl Piece {
    /// Every tile it covers, row by row, given its kind.
    ///
    /// `+ use<>` says the iterator borrows nothing: it only holds copies
    /// of numbers, so callers can keep it while changing the piece.
    pub fn tiles(&self, kind: &FurnitureType) -> impl Iterator<Item = Pos> + use<> {
        let (w, h) = kind.footprint(self.turned);
        let Pos { x, y, z } = self.at;
        (0..h).flat_map(move |dy| (0..w).map(move |dx| Pos::new(x + dx, y + dy, z)))
    }

    fn hash_into(&self, h: &mut Fnv64) {
        h.u64(u64::from(self.kind))
            .u64(u64::from(self.material))
            .i32(self.at.x)
            .i32(self.at.y)
            .i32(self.at.z)
            .u8(u8::from(self.turned))
            .u64(self.contents.len() as u64);
        for item in &self.contents {
            item.hash_into(h);
        }
    }
}

/// Marks the tiles `piece` covers in `covered` (one flag per tile of
/// `map`): each must be a walkable tile that no other piece has claimed.
fn claim(
    map: &Map,
    piece: &Piece,
    kind: &FurnitureType,
    covered: &mut [bool],
) -> Result<(), String> {
    for p in piece.tiles(kind) {
        let on_floor = map.get(p).is_some_and(Tile::is_walkable);
        let i = map
            .index_of(p)
            .filter(|_| on_floor)
            .ok_or_else(|| format!("the {} at {:?} isn't on the floor", kind.name, piece.at))?;
        // `mem::replace` sets the flag and hands back what it was.
        if std::mem::replace(&mut covered[i], true) {
            return Err(format!("two pieces of furniture cover {p:?}"));
        }
    }
    Ok(())
}

/// Marks a tile no piece covers, in [`Furnishings`]' per-tile list.
const NO_PIECE: u32 = u32::MAX;

/// All the furniture in the world: each piece once, and for every tile,
/// which piece covers it (if any), so looking one up is quick.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Furnishings {
    pieces: Vec<Piece>,
    /// For each tile, in map order, the index of the piece covering it, or
    /// [`NO_PIECE`]. Not saved: worked out again from `pieces` on load.
    #[serde(skip)]
    cover: Vec<u32>,
}

impl Furnishings {
    /// `pieces` placed on `map`. Pieces that don't fit are ignored here;
    /// [`Furnishings::check`] reports them.
    pub(crate) fn new(pieces: Vec<Piece>, map: &Map, kinds: &[FurnitureType]) -> Self {
        let mut furnishings = Furnishings {
            pieces,
            cover: Vec::new(),
        };
        furnishings.rebuild(map, kinds);
        furnishings
    }

    /// Works out which piece covers each tile again: after loading, or
    /// after pieces come or go.
    pub(crate) fn rebuild(&mut self, map: &Map, kinds: &[FurnitureType]) {
        // Every (tile, piece number) pair, worked out first as one flat
        // list. `zip(0..)` numbers the pieces; `flatten` turns "the tiles
        // of a known kind, or nothing" into just the tiles.
        let covering: Vec<(usize, u32)> = self
            .pieces
            .iter()
            .zip(0..)
            .flat_map(|(piece, number)| {
                let kind = kinds.get(usize::from(piece.kind));
                let tiles = kind.map(|k| piece.tiles(k)).into_iter().flatten();
                tiles
                    .filter_map(|p| map.index_of(p))
                    .map(move |t| (t, number))
            })
            .collect();
        self.cover = vec![NO_PIECE; map.tile_count()];
        for (t, number) in covering {
            self.cover[t] = number;
        }
    }

    /// Every piece, in the order they were placed.
    pub(crate) fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The piece covering the tile at `p`, if any.
    pub(crate) fn at(&self, map: &Map, p: Pos) -> Option<&Piece> {
        self.pieces.get(self.number_at(map, p)?)
    }

    /// The piece covering the tile at `p`, if any, to change.
    pub(crate) fn at_mut(&mut self, map: &Map, p: Pos) -> Option<&mut Piece> {
        let i = self.number_at(map, p)?;
        self.pieces.get_mut(i)
    }

    /// Takes away the piece covering the tile at `p`, if there is one, and
    /// hands it back.
    pub(crate) fn remove_at(
        &mut self,
        map: &Map,
        p: Pos,
        kinds: &[FurnitureType],
    ) -> Option<Piece> {
        let i = self.number_at(map, p)?;
        let piece = self.pieces.remove(i);
        self.rebuild(map, kinds);
        Some(piece)
    }

    /// The place in `pieces` of the piece covering the tile at `p`.
    fn number_at(&self, map: &Map, p: Pos) -> Option<usize> {
        let i = *self.cover.get(map.index_of(p)?)?;
        usize::try_from(i).ok()
    }

    /// Checks loaded furniture makes sense: every piece is a known kind
    /// and material, lies wholly on walkable tiles of the map, and covers
    /// no tile another piece does.
    pub(crate) fn check(
        &self,
        map: &Map,
        kinds: &[FurnitureType],
        materials: usize,
    ) -> Result<(), String> {
        let mut covered = vec![false; map.tile_count()];
        for piece in &self.pieces {
            let kind = kinds
                .get(usize::from(piece.kind))
                .ok_or_else(|| format!("furniture at {:?} is of an unknown kind", piece.at))?;
            if usize::from(piece.material) >= materials {
                return Err(format!(
                    "the {} at {:?} is made of an unknown material",
                    kind.name, piece.at
                ));
            }
            claim(map, piece, kind, &mut covered)?;
        }
        Ok(())
    }

    /// Feeds every piece into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.pieces.len() as u64);
        for piece in &self.pieces {
            piece.hash_into(h);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::material::tests::test_materials;

    /// Test furniture, all steel: a one-tile `"crate"` in the way, a
    /// `"bench"` two tiles across, a `"stool"` you can walk through and a
    /// `"cabinet"` that hides what's behind it and holds a litre.
    pub(crate) fn test_furniture_json() -> String {
        r#"{ "furniture": [
            { "id": "crate", "name": "crate", "description": "A crate.",
              "glyph": "&", "material": "steel" },
            { "id": "bench", "name": "bench", "description": "A bench.",
              "glyph": "=", "material": "steel", "size": [2, 1] },
            { "id": "stool", "name": "stool", "description": "A stool.",
              "glyph": "h", "material": "steel", "walkable": true },
            { "id": "cabinet", "name": "cabinet", "description": "A cabinet.",
              "glyph": "{", "material": "steel", "blocks_sight": true, "colour": "red",
              "holds_ml": 1000 }
        ] }"#
            .into()
    }

    /// The parsed test furniture, for other modules' tests.
    pub(crate) fn test_furniture() -> FurnitureTypes {
        let files = [ContentFile::new("test.json", test_furniture_json())];
        parse_furniture(&files, &test_materials()).expect("test furniture is valid")
    }

    fn error_of(json: &str) -> String {
        parse_furniture(&[ContentFile::new("beds.json", json)], &test_materials())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_valid_file_loads_with_defaults_filled_in() {
        let all = test_furniture();
        assert_eq!(all.list().len(), 4);
        assert_eq!(all.index_of("bench"), Some(1));
        assert_eq!(all.index_of("sofa"), None);
        let crate_ = &all.list()[0];
        assert_eq!(crate_.size, [1, 1]);
        assert!(!crate_.walkable && !crate_.blocks_sight && crate_.colour.is_none());
        assert!(all.list()[2].walkable && all.list()[3].blocks_sight);
        assert_eq!((crate_.holds_ml, all.list()[3].holds_ml), (0, 1000));
    }

    #[test]
    fn a_game_may_have_no_furniture() {
        let none = [ContentFile::new("none.json", r#"{ "furniture": [] }"#)];
        assert!(
            parse_furniture(&none, &test_materials())
                .unwrap()
                .list()
                .is_empty()
        );
    }

    #[test]
    fn turning_a_piece_swaps_its_width_and_height() {
        let kinds = test_furniture();
        let bench = &kinds.list()[1];
        assert_eq!(bench.footprint(false), (2, 1));
        assert_eq!(bench.footprint(true), (1, 2));
        let along = Piece {
            kind: 1,
            material: 0,
            at: Pos::new(3, 4, 1),
            turned: false,
            contents: Vec::new(),
        };
        let tiles: Vec<Pos> = along.tiles(bench).collect();
        assert_eq!(tiles, [Pos::new(3, 4, 1), Pos::new(4, 4, 1)]);
        let down = Piece {
            turned: true,
            ..along
        };
        let tiles: Vec<Pos> = down.tiles(bench).collect();
        assert_eq!(tiles, [Pos::new(3, 4, 1), Pos::new(3, 5, 1)]);
    }

    #[test]
    fn mistakes_are_errors_naming_the_file_or_the_furniture() {
        let typo = test_furniture_json().replace("\"glyph\"", "\"glyf\"");
        assert!(error_of(&typo).starts_with("invalid furniture file beds.json: unknown field"));
        let twice = test_furniture_json().replace("\"bench\"", "\"crate\"");
        assert_eq!(error_of(&twice), "furniture \"crate\" is defined twice");
        let wood = test_furniture_json().replacen("\"steel\"", "\"oak\"", 1);
        assert_eq!(
            error_of(&wood),
            "furniture \"crate\" is made of \"oak\", which isn't a material"
        );
        for size in ["[0, 1]", "[5, 1]"] {
            let odd = test_furniture_json().replace("[2, 1]", size);
            assert_eq!(
                error_of(&odd),
                "furniture \"bench\" needs a size from 1 to 4 tiles each way"
            );
        }
        let one = test_furniture().list()[0].clone();
        assert_eq!(
            check_list(&vec![one; 70_000]).unwrap_err().to_string(),
            "70000 kinds of furniture is too many; the most is 65535"
        );
    }

    #[test]
    fn everything_about_furniture_is_part_of_the_hash() {
        let hash = |kinds: &FurnitureTypes| {
            let mut h = Fnv64::new();
            kinds.hash_into(&mut h);
            h.finish()
        };
        let base = test_furniture();
        let changes: [fn(&mut FurnitureType); 10] = [
            |f| f.holds_ml = 5,
            |f| f.id.push('!'),
            |f| f.name.push('!'),
            |f| f.description.push('!'),
            |f| f.glyph = '%',
            |f| f.material.push('!'),
            |f| f.colour = Some(Colour::Red),
            |f| f.size = [1, 2],
            |f| f.walkable = true,
            |f| f.blocks_sight = true,
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed.list[0]);
            assert_ne!(hash(&base), hash(&changed), "{:?}", changed.list[0]);
        }
    }

    /// A 6 × 4 floor with walls round it, and two pieces: a bench at
    /// (1, 1)–(2, 1) and a turned bench at (4, 1)–(4, 2).
    fn furnished() -> (Map, Furnishings) {
        let mut map = Map::filled(6, 4, 1, Tile::WALL);
        for y in 1..3 {
            for x in 1..5 {
                map.place(Pos::new(x, y, 0), 0, Tile::FLOOR);
            }
        }
        let bench = |x, turned| Piece {
            kind: 1,
            material: 0,
            at: Pos::new(x, 1, 0),
            turned,
            contents: Vec::new(),
        };
        let pieces = vec![bench(1, false), bench(4, true)];
        let furnishings = Furnishings::new(pieces, &map, test_furniture().list());
        (map, furnishings)
    }

    #[test]
    fn every_tile_a_piece_covers_finds_it_and_others_find_nothing() {
        let (map, furnishings) = furnished();
        let at = |x, y| furnishings.at(&map, Pos::new(x, y, 0)).map(|p| p.at.x);
        assert_eq!((at(1, 1), at(2, 1)), (Some(1), Some(1)), "the bench");
        assert_eq!((at(4, 1), at(4, 2)), (Some(4), Some(4)), "the turned one");
        assert_eq!((at(3, 1), at(1, 2)), (None, None));
        assert_eq!(at(-1, 0), None, "off the map");
        assert_eq!(furnishings.pieces().len(), 2);
    }

    #[test]
    fn loaded_furniture_must_be_known_on_the_floor_and_not_overlap() {
        let (map, furnishings) = furnished();
        let kinds = test_furniture();
        let check = |f: &Furnishings| f.check(&map, kinds.list(), 1);
        assert_eq!(check(&furnishings), Ok(()));
        let with = |change: fn(&mut Piece)| {
            let mut pieces = furnishings.pieces().to_vec();
            change(&mut pieces[0]);
            Furnishings::new(pieces, &map, kinds.list())
        };
        assert!(
            check(&with(|p| p.kind = 9))
                .unwrap_err()
                .contains("unknown kind")
        );
        assert!(
            check(&with(|p| p.material = 3))
                .unwrap_err()
                .contains("unknown material")
        );
        assert!(
            check(&with(|p| p.at.y = 0))
                .unwrap_err()
                .contains("isn't on the floor")
        );
        assert!(
            check(&with(|p| p.at.x = 5))
                .unwrap_err()
                .contains("isn't on the floor")
        );
        assert_eq!(
            check(&with(|p| p.at.x = 3)),
            Err("two pieces of furniture cover Pos { x: 4, y: 1, z: 0 }".into())
        );
    }

    #[test]
    fn where_furniture_stands_is_part_of_the_hash() {
        let hash = |f: &Furnishings| {
            let mut h = Fnv64::new();
            f.hash_into(&mut h);
            h.finish()
        };
        let (map, furnishings) = furnished();
        let kinds = test_furniture();
        let changes: [fn(&mut Piece); 5] = [
            |p| {
                p.contents
                    .push(Item::new(0, 0, crate::item::tests::test_items().list()))
            },
            |p| p.kind = 0,
            |p| p.material = 1,
            |p| p.at.y = 2,
            |p| p.turned = true,
        ];
        for change in changes {
            let mut pieces = furnishings.pieces().to_vec();
            change(&mut pieces[0]);
            let changed = Furnishings::new(pieces, &map, kinds.list());
            assert_ne!(hash(&furnishings), hash(&changed));
        }
    }
}
