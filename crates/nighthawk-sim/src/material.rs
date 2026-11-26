//! Materials: what everything on the ship is made of (decisions D10 and D35).
//!
//! Every tile has a *shape* (a [`Tile`]: wall, floor, door…), which decides
//! how it behaves, and a *material*, which decides what it's made of. The
//! two are separate, the way Dwarf Fortress does it: a door that opens
//! changes shape but not material, and later a wall you build is simply
//! "wall, in whatever you built it from".
//!
//! Materials are content, loaded from `data/materials.json`. Each has real
//! physical properties in SI units, as whole numbers (like the air, so every
//! machine computes the same game), and the tool qualities that can cut it,
//! CDDA-style (decision D9): `"cut_by": { "LASER": 2 }` means a cutter with
//! the `LASER` quality at level 2 or more gets through. The levels are set
//! by hand and tuned as the game grows.
//!
//! The file also names a default material for every shape, so a map only
//! has to spell out the exceptions (the hull, say).

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::hash::Fnv64;
use crate::map::Tile;

/// A tool quality (decision D9). Recipes and actions ask for a quality at a
/// level, never for a particular tool, so there can be many ways to get one.
///
/// `Ord` lets qualities be keys of a `BTreeMap`, which always lists them in
/// the same order (a `HashMap` wouldn't: its order changes from run to run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Quality {
    /// Cutting with a beam of light: melts a slot through a tile.
    #[serde(rename = "LASER")]
    Laser,
}

impl Quality {
    /// The name shown to the player and written in data files.
    pub fn name(self) -> &'static str {
        match self {
            Quality::Laser => "LASER",
        }
    }

    /// A fixed number per quality, for the state hash. Never change or
    /// reuse one.
    fn hash_id(self) -> u8 {
        match self {
            Quality::Laser => 0,
        }
    }
}

/// The highest level a quality can have. Levels run from 1 to this.
pub const MAX_QUALITY_LEVEL: u8 = 10;

/// The colours a material can be drawn in. A short fixed list rather than
/// any colour at all, so every terminal can show them (decision D16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    White,
    Grey,
    DarkGrey,
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    Magenta,
}

impl Colour {
    /// A fixed number per colour, for the state hash: its place in the
    /// list above (`as u8` gives that for an enum like this one). So never
    /// reorder the list; add new colours at the end.
    fn hash_id(self) -> u8 {
        self as u8
    }
}

/// One material and its properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    /// How data files refer to it, like `"steel"`.
    pub id: String,
    /// How it's shown to the player, like "Steel".
    pub name: String,
    /// A sentence or two for look mode.
    pub description: String,
    /// The colour walls made of it are drawn in.
    pub colour: Colour,
    /// Density in kg/m³.
    pub density: u32,
    /// Melting point in °C.
    pub melting_point: i32,
    /// Specific heat in J/(kg·K): how much heat it takes to warm.
    pub specific_heat: u32,
    /// Thermal conductivity in W/(m·K): how well heat passes through.
    pub thermal_conductivity: u32,
    /// Vickers hardness (HV): how hard it is to scratch, drill or grind.
    pub hardness: u32,
    /// The tool qualities that cut it, and the level each needs. Any one
    /// is enough. Empty means nothing can cut it.
    #[serde(default)]
    pub cut_by: BTreeMap<Quality, u8>,
}

impl Material {
    /// Feeds every property into a state hash. Materials are game state
    /// (decision D35): a save carries its own copy.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name).str(&self.description);
        h.u8(self.colour.hash_id())
            .u64(u64::from(self.density))
            .i32(self.melting_point)
            .u64(u64::from(self.specific_heat))
            .u64(u64::from(self.thermal_conductivity))
            .u64(u64::from(self.hardness))
            .u64(self.cut_by.len() as u64);
        for (&quality, &level) in &self.cut_by {
            h.u8(quality.hash_id()).u8(level);
        }
    }
}

/// The parsed `materials.json`: every material, and which one each tile
/// shape is made of unless a map says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Materials {
    list: Vec<Material>,
    /// The index into `list` of each shape's default material.
    defaults: BTreeMap<Tile, u16>,
}

impl Materials {
    /// Every material, in file order. A tile's material is an index into
    /// this list.
    pub fn list(&self) -> &[Material] {
        &self.list
    }

    /// The index of the material with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|m| m.id == id)?;
        Some(u16::try_from(i).expect("material count was checked on load"))
    }

    /// The index of the material `shape` is made of by default.
    pub fn default_for(&self, shape: Tile) -> u16 {
        self.defaults[&shape]
    }
}

/// Everything that can be wrong with a materials file.
#[derive(Debug)]
pub enum MaterialError {
    /// Not valid JSON, or the wrong shape: a missing or misspelled field,
    /// an unknown colour or quality.
    Json(serde_json::Error),
    NoMaterials,
    /// More materials than a tile can refer to.
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    /// A quality level outside 1 to [`MAX_QUALITY_LEVEL`].
    BadLevel {
        material: String,
        quality: Quality,
        level: u8,
    },
    /// A tile shape with no default material.
    MissingDefault {
        shape: Tile,
    },
    /// A default that names a material that isn't in the file.
    UnknownDefault {
        shape: Tile,
        material: String,
    },
}

impl fmt::Display for MaterialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaterialError::Json(e) => write!(f, "invalid materials JSON: {e}"),
            MaterialError::NoMaterials => write!(f, "there are no materials"),
            MaterialError::TooMany { count } => {
                write!(f, "{count} materials is too many; the most is {}", u16::MAX)
            }
            MaterialError::DuplicateId { id } => write!(f, "material {id:?} is defined twice"),
            MaterialError::BadLevel {
                material,
                quality,
                level,
            } => write!(
                f,
                "material {material:?} needs {} {level} to cut; levels go from 1 to {MAX_QUALITY_LEVEL}",
                quality.name()
            ),
            MaterialError::MissingDefault { shape } => {
                write!(f, "no default material for {}", shape.json_name())
            }
            MaterialError::UnknownDefault { shape, material } => write!(
                f,
                "the default material for {} is {material:?}, which isn't defined",
                shape.json_name()
            ),
        }
    }
}

impl std::error::Error for MaterialError {}

/// The JSON shape of `materials.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialsFile {
    materials: Vec<Material>,
    defaults: BTreeMap<Tile, String>,
}

/// Parses and checks a materials file's text.
pub fn parse_materials(json: &str) -> Result<Materials, MaterialError> {
    let file: MaterialsFile = serde_json::from_str(json).map_err(MaterialError::Json)?;
    check_list(&file.materials)?;
    let mut defaults = BTreeMap::new();
    for shape in Tile::ALL {
        let id = file
            .defaults
            .get(&shape)
            .ok_or(MaterialError::MissingDefault { shape })?;
        let index = file
            .materials
            .iter()
            .position(|m| &m.id == id)
            .ok_or_else(|| MaterialError::UnknownDefault {
                shape,
                material: id.clone(),
            })?;
        defaults.insert(shape, u16::try_from(index).expect("checked by check_list"));
    }
    Ok(Materials {
        list: file.materials,
        defaults,
    })
}

/// Checks the list of materials on its own: not empty, not too long, no id
/// twice, every level in range.
fn check_list(list: &[Material]) -> Result<(), MaterialError> {
    if list.is_empty() {
        return Err(MaterialError::NoMaterials);
    }
    if u16::try_from(list.len()).is_err() {
        return Err(MaterialError::TooMany { count: list.len() });
    }
    let mut seen = HashSet::new();
    for material in list {
        if !seen.insert(material.id.as_str()) {
            return Err(MaterialError::DuplicateId {
                id: material.id.clone(),
            });
        }
        check_levels(material)?;
    }
    Ok(())
}

fn check_levels(material: &Material) -> Result<(), MaterialError> {
    let bad = material
        .cut_by
        .iter()
        .find(|&(_, &level)| !(1..=MAX_QUALITY_LEVEL).contains(&level));
    match bad {
        Some((&quality, &level)) => Err(MaterialError::BadLevel {
            material: material.id.clone(),
            quality,
            level,
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A material with every property filled in, for tests.
    fn material(id: &str, cut_by: &str) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "{id}", "description": "A test material.",
                 "colour": "grey", "density": 1000, "melting_point": 1000,
                 "specific_heat": 500, "thermal_conductivity": 10, "hardness": 100,
                 "cut_by": {cut_by} }}"#
        )
    }

    /// Every shape made of `id` by default.
    fn defaults_all(id: &str) -> String {
        let pairs: Vec<String> = Tile::ALL
            .iter()
            .map(|t| format!(r#""{}": "{id}""#, t.json_name()))
            .collect();
        format!("{{ {} }}", pairs.join(", "))
    }

    /// A small, valid materials file: steel (LASER 2) as every default,
    /// and a hard alloy (LASER 5).
    pub(crate) fn test_materials_json() -> String {
        format!(
            r#"{{ "materials": [ {}, {} ], "defaults": {} }}"#,
            material("steel", r#"{ "LASER": 2 }"#),
            material("hard_alloy", r#"{ "LASER": 5 }"#),
            defaults_all("steel")
        )
    }

    /// The parsed test materials, for other modules' tests.
    pub(crate) fn test_materials() -> Materials {
        parse_materials(&test_materials_json()).expect("test materials are valid")
    }

    #[test]
    fn a_valid_file_loads_with_its_defaults() {
        let materials = test_materials();
        assert_eq!(materials.list().len(), 2);
        assert_eq!(materials.index_of("hard_alloy"), Some(1));
        assert_eq!(materials.index_of("cheese"), None);
        assert_eq!(materials.default_for(Tile::Wall), 0);
        let alloy = &materials.list()[1];
        assert_eq!(alloy.cut_by.get(&Quality::Laser), Some(&5));
        assert_eq!(Quality::Laser.name(), "LASER");
    }

    #[test]
    fn cut_by_is_optional_and_empty_means_uncuttable() {
        let json = format!(
            r#"{{ "materials": [ {} ], "defaults": {} }}"#,
            material("x", "{}").replace(r#", "cut_by": {}"#, ""),
            defaults_all("x")
        );
        let materials = parse_materials(&json).unwrap();
        assert!(materials.list()[0].cut_by.is_empty());
    }

    fn error_of(json: &str) -> String {
        parse_materials(json).unwrap_err().to_string()
    }

    #[test]
    fn misspelled_fields_unknown_qualities_and_colours_are_errors() {
        let typo = test_materials_json().replace("\"density\"", "\"densty\"");
        assert!(error_of(&typo).contains("unknown field `densty`"));
        let quality = test_materials_json().replace("LASER\": 5", "SPOON\": 5");
        assert!(error_of(&quality).contains("SPOON"));
        let colour = test_materials_json().replace("\"grey\"", "\"plaid\"");
        assert!(error_of(&colour).contains("plaid"));
        assert!(error_of("[").starts_with("invalid materials JSON"));
    }

    #[test]
    fn an_empty_list_is_an_error() {
        let json = format!(
            r#"{{ "materials": [], "defaults": {} }}"#,
            defaults_all("x")
        );
        assert_eq!(error_of(&json), "there are no materials");
    }

    #[test]
    fn the_same_id_twice_is_an_error() {
        let json = format!(
            r#"{{ "materials": [ {}, {} ], "defaults": {} }}"#,
            material("steel", "{}"),
            material("steel", "{}"),
            defaults_all("steel")
        );
        assert_eq!(error_of(&json), r#"material "steel" is defined twice"#);
    }

    #[test]
    fn levels_must_be_1_to_10() {
        for level in [0, 11] {
            let json = test_materials_json().replace("LASER\": 5", &format!("LASER\": {level}"));
            assert_eq!(
                error_of(&json),
                format!(
                    r#"material "hard_alloy" needs LASER {level} to cut; levels go from 1 to 10"#
                )
            );
        }
    }

    #[test]
    fn every_shape_needs_a_default_that_exists() {
        let missing = test_materials_json().replace(r#""door_open": "steel","#, "");
        assert_eq!(error_of(&missing), "no default material for door_open");
        let unknown = test_materials_json().replace(r#""floor": "steel""#, r#""floor": "wood""#);
        assert_eq!(
            error_of(&unknown),
            r#"the default material for floor is "wood", which isn't defined"#
        );
    }

    #[test]
    fn too_many_materials_is_an_error() {
        let err = check_list(&vec![test_materials().list()[0].clone(); 70_000]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "70000 materials is too many; the most is 65535"
        );
    }

    #[test]
    fn every_property_is_part_of_the_hash() {
        let hash = |m: &Material| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        let base = test_materials().list()[1].clone();
        let changes: [fn(&mut Material); 6] = [
            |m| m.density += 1,
            |m| m.melting_point += 1,
            |m| m.colour = Colour::Red,
            |m| m.hardness += 1,
            |m| m.cut_by.clear(),
            |m| m.name.push('!'),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed);
            assert_ne!(hash(&base), hash(&changed), "{changed:?}");
        }
    }

    #[test]
    fn colours_have_their_own_hash_ids() {
        assert_eq!(Colour::White.hash_id(), 0);
        assert_eq!(Colour::Magenta.hash_id(), 8);
    }
}
