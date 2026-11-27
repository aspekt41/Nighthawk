//! Materials: what everything on the ship is made of (decisions D10, D35
//! and D37).
//!
//! Every tile is a *structure* (bulkhead, door, deck plating…; see
//! `structure.rs`), which decides what it is and how it behaves, made of a
//! *material*, which decides what it's made of. The two are separate, the
//! way Dwarf Fortress does it: the same door can be aluminium or steel, and
//! later a wall you build is simply "bulkhead, in whatever you built it
//! from".
//!
//! Materials are content, loaded from `data/materials/*.json`. Each has real
//! physical properties in SI units, as whole numbers (like the air, so every
//! machine computes the same game), and the tool qualities that can cut it,
//! CDDA-style (decision D9): `"cut_by": { "LASER": 2 }` means a cutter with
//! the `LASER` quality at level 2 or more gets through. The levels are set
//! by hand and tuned as the game grows.
//!
//! Materials live in `data/materials/*.json`. Which material a structure is
//! made of by default is up to the structure (see `structure.rs`).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;

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
    pub(crate) fn hash_id(self) -> u8 {
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
    /// Melting point in °C, or `None` for something that doesn't melt
    /// (cotton chars and burns instead).
    #[serde(default)]
    pub melting_point: Option<i32>,
    /// Specific heat in J/(kg·K): how much heat it takes to warm.
    pub specific_heat: u32,
    /// Thermal conductivity in mW/(m·K) (thousandths, so insulators like
    /// cotton at 0.04 W/(m·K) aren't rounded to nothing): how well heat
    /// passes through.
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
            .i32(self.melting_point.unwrap_or(i32::MIN))
            .u64(u64::from(self.specific_heat))
            .u64(u64::from(self.thermal_conductivity))
            .u64(u64::from(self.hardness))
            .u64(self.cut_by.len() as u64);
        for (&quality, &level) in &self.cut_by {
            h.u8(quality.hash_id()).u8(level);
        }
    }
}

/// Every material from the files in `data/materials/`.
///
/// `#[serde(transparent)]` saves it as just the list inside, with no
/// `{ "list": ... }` wrapped around it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Materials {
    list: Vec<Material>,
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

    /// The list, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn list_mut(&mut self) -> &mut Vec<Material> {
        &mut self.list
    }
}

/// Everything that can be wrong with the materials files.
#[derive(Debug)]
pub enum MaterialError {
    /// A file that isn't valid JSON, or has the wrong shape: a missing or
    /// misspelled field, an unknown colour or quality.
    File(FileError),
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
}

impl fmt::Display for MaterialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaterialError::File(e) => write!(f, "invalid materials file {e}"),
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
        }
    }
}

impl std::error::Error for MaterialError {}

/// The JSON shape of one file in `data/materials/`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialsFile {
    materials: Vec<Material>,
}

/// Parses and checks every materials file.
pub(crate) fn parse_materials(files: &[ContentFile]) -> Result<Materials, MaterialError> {
    let list = read_all(files, |f: MaterialsFile| f.materials).map_err(MaterialError::File)?;
    check_list(&list)?;
    Ok(Materials { list })
}

/// Checks the list of materials: not empty, not too long, no id twice,
/// every level in range.
fn check_list(list: &[Material]) -> Result<(), MaterialError> {
    if list.is_empty() {
        return Err(MaterialError::NoMaterials);
    }
    if u16::try_from(list.len()).is_err() {
        return Err(MaterialError::TooMany { count: list.len() });
    }
    if let Some(id) = first_duplicate(list.iter().map(|m| m.id.as_str())) {
        return Err(MaterialError::DuplicateId { id: id.into() });
    }
    list.iter().try_for_each(check_levels)
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
                 "specific_heat": 500, "thermal_conductivity": 10000, "hardness": 100,
                 "cut_by": {cut_by} }}"#
        )
    }

    /// A small, valid materials file: steel (LASER 2) and a hard alloy
    /// (LASER 5).
    pub(crate) fn test_materials_json() -> String {
        format!(
            r#"{{ "materials": [ {}, {} ] }}"#,
            material("steel", r#"{ "LASER": 2 }"#),
            material("hard_alloy", r#"{ "LASER": 5 }"#),
        )
    }

    /// The parsed test materials, for other modules' tests.
    pub(crate) fn test_materials() -> Materials {
        parse_materials(&[ContentFile::new("test.json", test_materials_json())])
            .expect("test materials are valid")
    }

    fn parse_one(json: &str) -> Result<Materials, MaterialError> {
        parse_materials(&[ContentFile::new("metals.json", json)])
    }

    #[test]
    fn a_valid_file_loads() {
        let materials = test_materials();
        assert_eq!(materials.list().len(), 2);
        assert_eq!(materials.index_of("hard_alloy"), Some(1));
        assert_eq!(materials.index_of("cheese"), None);
        let alloy = &materials.list()[1];
        assert_eq!(alloy.cut_by.get(&Quality::Laser), Some(&5));
        assert_eq!(Quality::Laser.name(), "LASER");
    }

    #[test]
    fn materials_can_be_spread_over_several_files() {
        let files = [
            ContentFile::new(
                "metals.json",
                format!(r#"{{ "materials": [ {} ] }}"#, material("steel", "{}")),
            ),
            ContentFile::new(
                "fabrics.json",
                format!(r#"{{ "materials": [ {} ] }}"#, material("cotton", "{}")),
            ),
        ];
        let materials = parse_materials(&files).unwrap();
        assert_eq!(materials.index_of("cotton"), Some(1), "file order");
    }

    #[test]
    fn cut_by_is_optional_and_empty_means_uncuttable() {
        let json = format!(
            r#"{{ "materials": [ {} ] }}"#,
            material("x", "{}").replace(r#", "cut_by": {}"#, "")
        );
        let materials = parse_one(&json).unwrap();
        assert!(materials.list()[0].cut_by.is_empty());
    }

    fn error_of(json: &str) -> String {
        parse_one(json).unwrap_err().to_string()
    }

    #[test]
    fn misspelled_fields_unknown_qualities_and_colours_are_errors_naming_the_file() {
        let typo = test_materials_json().replace("\"density\"", "\"densty\"");
        assert!(error_of(&typo).contains("unknown field `densty`"));
        let quality = test_materials_json().replace("LASER\": 5", "SPOON\": 5");
        assert!(error_of(&quality).contains("SPOON"));
        let colour = test_materials_json().replace("\"grey\"", "\"plaid\"");
        assert!(error_of(&colour).contains("plaid"));
        assert!(error_of("[").starts_with("invalid materials file metals.json: "));
    }

    #[test]
    fn no_materials_at_all_is_an_error() {
        assert_eq!(error_of(r#"{ "materials": [] }"#), "there are no materials");
        assert!(matches!(
            parse_materials(&[]),
            Err(MaterialError::NoMaterials)
        ));
    }

    #[test]
    fn the_same_id_twice_is_an_error() {
        let json = format!(
            r#"{{ "materials": [ {}, {} ] }}"#,
            material("steel", "{}"),
            material("steel", "{}"),
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
            |m| m.melting_point = None,
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
