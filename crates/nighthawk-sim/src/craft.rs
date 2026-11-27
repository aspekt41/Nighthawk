//! Crafting: making items from items (decision D56).
//!
//! A *craft* is content in `data/crafts/*.json`: what it makes (and how
//! many), the items it uses up from your pockets, the tool levels it needs,
//! how long it takes, and what the result is made of: the made item's own
//! material, or the material of the first item used (a shiv made of bone
//! scrap is a bone shiv). Recipes (`recipe.rs`) change the world; crafts
//! change what you carry.
//!
//! You only know a craft once you've carried every kind of item it uses:
//! pick up your first scrap and the idea of a shiv comes to you. The world
//! keeps the list of kinds you've carried (see `World::known_crafts`).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::item::ItemTypes;
use crate::material::Materials;
use crate::quality::{LevelError, Levels, Qualities, hash_levels};
use crate::recipe::Use;

/// What a crafted item is made of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CraftMaterial {
    /// The made item's own material, from `data/items/`.
    #[default]
    Own,
    /// The material of the first item used.
    FromUsed,
}

/// One craft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Craft {
    pub id: String,
    /// The id of the item it makes.
    pub makes: String,
    /// How many it makes. Optional: 1.
    #[serde(default = "one")]
    pub count: u32,
    /// What they're made of: `own` (the default) or `from_used`.
    #[serde(default)]
    pub material: CraftMaterial,
    /// The items it uses up from your pockets, each optionally only of a
    /// material with a tag. You know the craft once you've carried every
    /// kind of item it uses. Optional: none, and then it's always known.
    #[serde(default)]
    pub uses: Vec<Use>,
    /// Tool levels it needs, all of them, like `{ "CUT": 1 }`. Optional.
    #[serde(default)]
    pub needs: Levels,
    /// How long it takes, in seconds.
    pub seconds: u32,
}

/// The default `count`. Serde's `default = "..."` names a function.
fn one() -> u32 {
    1
}

impl Craft {
    fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id)
            .str(&self.makes)
            .u64(u64::from(self.count))
            .u8(u8::from(self.material == CraftMaterial::FromUsed))
            .u64(u64::from(self.seconds))
            .u64(self.uses.len() as u64);
        for used in &self.uses {
            h.str(&used.item)
                .u64(u64::from(used.count))
                .str(used.tag.as_deref().unwrap_or(""));
        }
        hash_levels(&self.needs, h);
    }
}

/// Every craft from the files in `data/crafts/`, saved as just the list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Crafts {
    list: Vec<Craft>,
}

impl Crafts {
    /// Every craft, in file order. Commands name a craft by its place here.
    pub fn list(&self) -> &[Craft] {
        &self.list
    }

    /// Feeds every craft into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.list.len() as u64);
        for craft in &self.list {
            craft.hash_into(h);
        }
    }
}

/// Everything that can be wrong with the crafts files.
#[derive(Debug)]
pub enum CraftError {
    File(FileError),
    DuplicateId {
        id: String,
    },
    /// A craft naming an item, tag or quality that isn't defined: `kind`
    /// is which ("item"), `name` what it said.
    Unknown {
        craft: String,
        kind: &'static str,
        name: String,
    },
    /// Its `needs` names a level out of range.
    Needs {
        craft: String,
        error: LevelError,
    },
    /// A craft using none of something (`"count": 0`).
    UsesNone {
        craft: String,
        item: String,
    },
    /// Fields that make no sense: making none, taking no time, or a
    /// material from items it doesn't use.
    Problem {
        craft: String,
        problem: &'static str,
    },
}

impl fmt::Display for CraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CraftError::File(e) => write!(f, "invalid crafts file {e}"),
            CraftError::DuplicateId { id } => write!(f, "craft {id:?} is defined twice"),
            CraftError::Unknown { craft, kind, name } => {
                write!(f, "craft {craft:?} names {name:?}, which isn't {kind}")
            }
            CraftError::Needs { craft, error } => write!(f, "craft {craft:?}'s needs: {error}"),
            CraftError::UsesNone { craft, item } => {
                write!(f, "craft {craft:?} uses a count of 0 {item:?}")
            }
            CraftError::Problem { craft, problem } => write!(f, "craft {craft:?} {problem}"),
        }
    }
}

impl std::error::Error for CraftError {}

/// The JSON shape of one file in `data/crafts/`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CraftsFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/crafts.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    crafts: Vec<Craft>,
}

/// What crafts may refer to.
pub(crate) struct CraftRefs<'a> {
    pub qualities: &'a Qualities,
    pub materials: &'a Materials,
    pub items: &'a ItemTypes,
}

/// What's wrong with a craft making none.
const MAKES_NONE: &str = "makes a count of 0";
/// What's wrong with a craft that takes no time.
const NO_TIME: &str = "must take at least 1 second";
/// What's wrong with `"material": "from_used"` and no `uses`.
const NOTHING_USED: &str = "takes its material from the items used, but uses none";

/// Parses and checks every crafts file: each makes a real item, uses real
/// items with real tags, needs real qualities, and takes time.
pub(crate) fn parse_crafts(files: &[ContentFile], refs: &CraftRefs) -> Result<Crafts, CraftError> {
    let list = read_all(files, |f: CraftsFile| f.crafts).map_err(CraftError::File)?;
    if let Some(id) = first_duplicate(list.iter().map(|c| c.id.as_str())) {
        return Err(CraftError::DuplicateId { id: id.into() });
    }
    for craft in &list {
        check_one(craft, refs)?;
        check_uses(craft, refs)?;
    }
    Ok(Crafts { list })
}

/// Checks what a craft makes, how long it takes and what it needs.
fn check_one(craft: &Craft, refs: &CraftRefs) -> Result<(), CraftError> {
    let problem = |problem| CraftError::Problem {
        craft: craft.id.clone(),
        problem,
    };
    if refs.items.index_of(&craft.makes).is_none() {
        return Err(unknown(craft, "an item", &craft.makes));
    }
    if craft.count == 0 {
        return Err(problem(MAKES_NONE));
    }
    if craft.seconds == 0 {
        return Err(problem(NO_TIME));
    }
    if craft.material == CraftMaterial::FromUsed && craft.uses.is_empty() {
        return Err(problem(NOTHING_USED));
    }
    refs.qualities
        .check(&craft.needs)
        .map_err(|error| CraftError::Needs {
            craft: craft.id.clone(),
            error,
        })
}

/// Checks every item a craft uses exists, at least once, with a real tag.
fn check_uses(craft: &Craft, refs: &CraftRefs) -> Result<(), CraftError> {
    let tags = refs.materials.tags();
    for used in &craft.uses {
        if refs.items.index_of(&used.item).is_none() {
            return Err(unknown(craft, "an item", &used.item));
        }
        if used.count == 0 {
            return Err(CraftError::UsesNone {
                craft: craft.id.clone(),
                item: used.item.clone(),
            });
        }
        if let Some(tag) = &used.tag
            && !tags.contains(&tag.as_str())
        {
            return Err(unknown(craft, "a material tag", tag));
        }
    }
    Ok(())
}

/// An `Unknown` error: `craft` names `name`, which isn't `kind`.
fn unknown(craft: &Craft, kind: &'static str, name: &str) -> CraftError {
    CraftError::Unknown {
        craft: craft.id.clone(),
        kind,
        name: name.into(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::item::tests::test_items;
    use crate::material::tests::test_materials;
    use crate::quality::tests::test_qualities;

    /// Test crafts: a `"cutter"` from 2 metal scrap, of the scrap's
    /// material (60 s); and two `"lamp"`s from a scrap and a cutter, with
    /// LASER 1 (30 s).
    pub(crate) fn test_crafts_json() -> String {
        r#"{ "crafts": [
            { "id": "cutter", "makes": "cutter", "material": "from_used", "seconds": 60,
              "uses": [ { "item": "scrap", "count": 2, "tag": "metal" } ] },
            { "id": "lamps", "makes": "lamp", "count": 2, "seconds": 30,
              "uses": [ { "item": "scrap", "count": 1 }, { "item": "cutter", "count": 1 } ],
              "needs": { "LASER": 1 } }
        ] }"#
            .into()
    }

    fn parse_test(json: &str) -> Result<Crafts, CraftError> {
        let (qualities, materials, items) = (test_qualities(), test_materials(), test_items());
        let refs = CraftRefs {
            qualities: &qualities,
            materials: &materials,
            items: &items,
        };
        parse_crafts(&[ContentFile::new("tools.json", json)], &refs)
    }

    /// The parsed test crafts, for other modules' tests.
    pub(crate) fn test_crafts() -> Crafts {
        parse_test(&test_crafts_json()).expect("test crafts are valid")
    }

    fn error_of(json: &str) -> String {
        parse_test(json).unwrap_err().to_string()
    }

    #[test]
    fn a_valid_file_loads_with_its_defaults() {
        let crafts = test_crafts();
        assert_eq!(crafts.list().len(), 2);
        let (cutter, lamps) = (&crafts.list()[0], &crafts.list()[1]);
        assert_eq!(
            (cutter.count, cutter.material),
            (1, CraftMaterial::FromUsed)
        );
        assert_eq!(cutter.uses[0].tag.as_deref(), Some("metal"));
        assert_eq!((lamps.count, lamps.material), (2, CraftMaterial::Own));
        assert_eq!(lamps.needs.get("LASER"), Some(&1));
        let none = parse_test(r#"{ "crafts": [] }"#).unwrap();
        assert!(none.list().is_empty(), "a game may have no crafts");
    }

    #[test]
    fn mistakes_are_errors_naming_the_file_or_the_craft() {
        let mangle = |from: &str, to: &str| error_of(&test_crafts_json().replacen(from, to, 1));
        assert!(mangle("\"makes\"", "\"made\"").starts_with("invalid crafts file tools.json"));
        assert_eq!(
            mangle("\"lamps\"", "\"cutter\""),
            "craft \"cutter\" is defined twice"
        );
        assert_eq!(
            mangle("\"makes\": \"cutter\"", "\"makes\": \"sword\""),
            "craft \"cutter\" names \"sword\", which isn't an item"
        );
        assert_eq!(
            mangle("\"item\": \"scrap\"", "\"item\": \"glue\""),
            "craft \"cutter\" names \"glue\", which isn't an item"
        );
        assert_eq!(
            mangle("\"tag\": \"metal\"", "\"tag\": \"shiny\""),
            "craft \"cutter\" names \"shiny\", which isn't a material tag"
        );
        assert_eq!(
            mangle("\"count\": 2,", "\"count\": 0,"),
            "craft \"cutter\" uses a count of 0 \"scrap\""
        );
        assert_eq!(
            mangle("\"seconds\": 60", "\"seconds\": 0"),
            "craft \"cutter\" must take at least 1 second"
        );
        assert_eq!(
            mangle("\"count\": 2, \"seconds\"", "\"count\": 0, \"seconds\""),
            "craft \"lamps\" makes a count of 0"
        );
        assert!(mangle("\"LASER\": 1", "\"LASER\": 11").starts_with("craft \"lamps\"'s needs:"));
    }

    #[test]
    fn a_material_from_nothing_used_is_an_error() {
        let json = r#"{ "crafts": [ { "id": "odd", "makes": "scrap", "seconds": 1,
                         "material": "from_used" } ] }"#;
        assert_eq!(
            error_of(json),
            "craft \"odd\" takes its material from the items used, but uses none"
        );
    }

    #[test]
    fn everything_about_a_craft_is_part_of_the_hash() {
        let hash = |crafts: &Crafts| {
            let mut h = Fnv64::new();
            crafts.hash_into(&mut h);
            h.finish()
        };
        let base = test_crafts();
        let changes: [fn(&mut Craft); 9] = [
            |c| c.id.push('!'),
            |c| c.makes.push('!'),
            |c| c.count += 1,
            |c| c.material = CraftMaterial::Own,
            |c| c.seconds += 1,
            |c| c.uses[0].count += 1,
            |c| c.uses[0].tag = None,
            |c| c.uses[0].item.push('!'),
            |c| {
                c.needs.insert("WELD".into(), 1);
            },
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed.list[0]);
            assert_ne!(hash(&base), hash(&changed), "{:?}", changed.list[0]);
        }
    }
}
