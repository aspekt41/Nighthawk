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
//! machine computes the same game), and what it takes to work it with each
//! action that needs a tool, CDDA-style (decision D9): `"worked_by": {
//! "cut": { "LASER": 2 } }` means cutting it needs a tool with the `LASER`
//! quality at level 2 or more. The levels are set by hand and tuned as the
//! game grows. Actions and qualities are content too (`data/actions/`,
//! `data/qualities/`), so `worked_by` may only name ones from there.
//!
//! Materials live in `data/materials/*.json`. Which material a structure is
//! made of by default is up to the structure (see `structure.rs`).

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::action::Actions;
use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::quality::{LevelError, Levels, Qualities, hash_levels};

/// The colours a material can be drawn in. A short fixed list rather than
/// any colour at all, so every terminal can show them (decision D16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// Doors, so they stand out from the walls around them.
    LightRed,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    pub worked_by: BTreeMap<String, Levels>,
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
            .u64(u64::from(self.hardness));
        h.u64(self.worked_by.len() as u64);
        for (action, levels) in &self.worked_by {
            h.str(action);
            hash_levels(levels, h);
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
    /// misspelled field, an unknown colour.
    File(FileError),
    NoMaterials,
    /// More materials than a tile can refer to.
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    /// Its `worked_by` names an action that doesn't exist.
    UnknownAction {
        material: String,
        action: String,
    },
    /// Its `worked_by` lists an action with no levels at all. (Leave the
    /// action out instead: then nothing can do it.)
    NoLevels {
        material: String,
        action: String,
    },
    /// Its `worked_by` names a quality that doesn't exist, or a level out
    /// of range.
    WorkedBy {
        material: String,
        action: String,
        error: LevelError,
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
            MaterialError::UnknownAction { material, action } => write!(
                f,
                "material {material:?}'s worked_by names {action:?}, which isn't an action"
            ),
            MaterialError::NoLevels { material, action } => write!(
                f,
                "material {material:?}'s worked_by {action:?} lists no levels; leave it out if \
                 nothing can do it"
            ),
            MaterialError::WorkedBy {
                material,
                action,
                error,
            } => write!(f, "material {material:?}'s worked_by {action:?}: {error}"),
        }
    }
}

impl std::error::Error for MaterialError {}

/// The JSON shape of one file in `data/materials/`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterialsFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/materials.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    materials: Vec<Material>,
}

/// Parses and checks every materials file. Each `worked_by` may only name
/// `actions`, with levels of `qualities`.
pub(crate) fn parse_materials(
    files: &[ContentFile],
    qualities: &Qualities,
    actions: &Actions,
) -> Result<Materials, MaterialError> {
    let list = read_all(files, |f: MaterialsFile| f.materials).map_err(MaterialError::File)?;
    check_list(&list)?;
    list.iter()
        .try_for_each(|m| check_worked_by(m, qualities, actions))?;
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
    Ok(())
}

/// Checks one material's `worked_by`: real actions, real qualities, and
/// levels in range.
fn check_worked_by(
    material: &Material,
    qualities: &Qualities,
    actions: &Actions,
) -> Result<(), MaterialError> {
    for (action, levels) in &material.worked_by {
        let names = || (material.id.clone(), action.clone());
        if actions.index_of(action).is_none() {
            let (material, action) = names();
            return Err(MaterialError::UnknownAction { material, action });
        }
        if levels.is_empty() {
            let (material, action) = names();
            return Err(MaterialError::NoLevels { material, action });
        }
        qualities
            .check(levels)
            .map_err(|error| MaterialError::WorkedBy {
                material: material.id.clone(),
                action: action.clone(),
                error,
            })?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::action::tests::test_actions;
    use crate::quality::tests::test_qualities;

    /// A material with every property filled in, for tests. `worked_by`
    /// is its JSON, like `{ "cut": { "LASER": 2 } }`.
    fn material(id: &str, worked_by: &str) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "{id}", "description": "A test material.",
                 "colour": "grey", "density": 1000, "melting_point": 1000,
                 "specific_heat": 500, "thermal_conductivity": 10000, "hardness": 100,
                 "worked_by": {worked_by} }}"#
        )
    }

    /// A small, valid materials file: steel (cut with LASER 2) and a hard
    /// alloy (cut with LASER 5).
    pub(crate) fn test_materials_json() -> String {
        format!(
            r#"{{ "materials": [ {}, {} ] }}"#,
            material("steel", r#"{ "cut": { "LASER": 2 } }"#),
            material("hard_alloy", r#"{ "cut": { "LASER": 5 } }"#),
        )
    }

    /// The parsed test materials, for other modules' tests.
    pub(crate) fn test_materials() -> Materials {
        parse_materials(
            &[ContentFile::new("test.json", test_materials_json())],
            &test_qualities(),
            &test_actions(),
        )
        .expect("test materials are valid")
    }

    fn parse_one(json: &str) -> Result<Materials, MaterialError> {
        parse_materials(
            &[ContentFile::new("metals.json", json)],
            &test_qualities(),
            &test_actions(),
        )
    }

    #[test]
    fn a_valid_file_loads() {
        let materials = test_materials();
        assert_eq!(materials.list().len(), 2);
        assert_eq!(materials.index_of("hard_alloy"), Some(1));
        assert_eq!(materials.index_of("cheese"), None);
        let alloy = &materials.list()[1];
        assert_eq!(alloy.worked_by["cut"].get("LASER"), Some(&5));
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
        let materials = parse_materials(&files, &test_qualities(), &test_actions()).unwrap();
        assert_eq!(materials.index_of("cotton"), Some(1), "file order");
    }

    #[test]
    fn worked_by_is_optional_and_empty_means_nothing_works_it() {
        let json = format!(
            r#"{{ "materials": [ {} ] }}"#,
            material("x", "{}").replace(r#", "worked_by": {}"#, "")
        );
        let materials = parse_one(&json).unwrap();
        assert!(materials.list()[0].worked_by.is_empty());
    }

    fn error_of(json: &str) -> String {
        parse_one(json).unwrap_err().to_string()
    }

    #[test]
    fn misspelled_fields_and_colours_are_errors_naming_the_file() {
        let typo = test_materials_json().replace("\"density\"", "\"densty\"");
        assert!(error_of(&typo).contains("unknown field `densty`"));
        let colour = test_materials_json().replace("\"grey\"", "\"plaid\"");
        assert!(error_of(&colour).contains("plaid"));
        assert!(error_of("[").starts_with("invalid materials file metals.json: "));
    }

    #[test]
    fn worked_by_may_only_name_real_actions_and_qualities() {
        let quality = test_materials_json().replace("LASER\": 5", "SPOON\": 5");
        assert_eq!(
            error_of(&quality),
            r#"material "hard_alloy"'s worked_by "cut": "SPOON" isn't a quality"#
        );
        let welded = test_materials_json().replace("LASER\": 5", "WELD\": 5");
        assert!(parse_one(&welded).is_ok(), "any quality there is fine");
        let action =
            test_materials_json().replace(r#"{ "cut": { "LASER": 5"#, r#"{ "melt": { "LASER": 5"#);
        assert_eq!(
            error_of(&action),
            r#"material "hard_alloy"'s worked_by names "melt", which isn't an action"#
        );
        let empty =
            test_materials_json().replace(r#"{ "cut": { "LASER": 5 } }"#, r#"{ "cut": {} }"#);
        assert!(error_of(&empty).ends_with("lists no levels; leave it out if nothing can do it"));
    }

    #[test]
    fn no_materials_at_all_is_an_error() {
        assert_eq!(error_of(r#"{ "materials": [] }"#), "there are no materials");
        assert!(matches!(
            parse_materials(&[], &test_qualities(), &test_actions()),
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
                    r#"material "hard_alloy"'s worked_by "cut": LASER {level} is out of range; levels go from 1 to 10"#
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
            |m| m.worked_by.clear(),
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
        assert_eq!(Colour::LightRed.hash_id(), 9, "added last, so none moved");
    }
}
