//! Structures: what each tile *is* (decisions D37 and D47).
//!
//! A structure is a kind of thing a tile can be: deck plating, a bulkhead,
//! hull plating, a door, a ladder, a crack in the hull. Structures are
//! content, loaded from `data/structures/*.json`. What one does in the
//! simulation is a few named *properties*: whether you can walk on it (and
//! air fills it), how big a hole to space it has, and whether it's a
//! ladder. A door is just a solid structure that a recipe opens into a
//! walkable one. So the data decides what exists and what it does; the
//! code only knows what each property means.
//!
//! What a structure is made of is a separate question, answered by its
//! material (see `material.rs`).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::map::Tile;
use crate::material::{Colour, Materials};
use crate::pos::Vertical;

/// One kind of structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    /// How maps refer to it, like `"bulkhead"`.
    pub id: String,
    /// How it's shown to the player, like "bulkhead".
    pub name: String,
    /// A sentence or two for look mode.
    pub description: String,
    /// The character the map draws it with.
    pub glyph: char,
    /// The id of the material it's made of, unless a map says otherwise.
    pub material: String,
    /// Optional: the colour to draw it in, instead of the usual colour for
    /// what it is (a damaged door is red, though you walk through it).
    #[serde(default)]
    pub colour: Option<Colour>,
    /// You can walk on it, and air fills it. Optional: left out, it's
    /// solid and airtight, like a wall or a shut door.
    #[serde(default)]
    pub walkable: bool,
    /// A hole to space, in cm² (a hull crack is 4, a breach 400): the space
    /// beside it leaks. Only for solid structures. Optional: none.
    #[serde(default)]
    pub leak_cm2: u32,
    /// A ladder, going `"up"` or `"down"`. Only for walkable structures.
    /// Optional: not a ladder.
    #[serde(default)]
    pub climb: Option<Vertical>,
}

impl Structure {
    /// Its properties, as the simulation reads them on every tile of it.
    pub fn tile(&self) -> Tile {
        Tile {
            walkable: self.walkable,
            leak_cm2: self.leak_cm2,
            climb: self.climb,
        }
    }

    /// Feeds everything about it into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name).str(&self.description);
        h.u64(u64::from(self.glyph))
            .str(&self.material)
            .u64(self.colour.map_or(u64::MAX, |c| u64::from(c.hash_id())))
            .u8(u8::from(self.walkable))
            .u64(u64::from(self.leak_cm2))
            .u8(self.climb.map_or(u8::MAX, Vertical::hash_id));
    }
}

/// Every structure from the files in `data/structures/`, saved as just
/// the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Structures {
    list: Vec<Structure>,
}

impl Structures {
    /// Every structure, in file order. A tile's structure is an index into
    /// this list.
    pub fn list(&self) -> &[Structure] {
        &self.list
    }

    /// The index of the structure with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|s| s.id == id)?;
        Some(u16::try_from(i).expect("structure count was checked on load"))
    }

    /// The list, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn list_mut(&mut self) -> &mut Vec<Structure> {
        &mut self.list
    }
}

/// Everything that can be wrong with the structures files.
#[derive(Debug)]
pub enum StructureError {
    /// A file that isn't valid JSON, or has the wrong shape: a missing or
    /// misspelled field, a climb that isn't up or down, a glyph that isn't
    /// one character.
    File(FileError),
    NoStructures,
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    /// A structure made of a material that isn't defined.
    UnknownMaterial {
        structure: String,
        material: String,
    },
    /// Properties that don't go together: a walkable structure with a
    /// leak, or a solid one you could climb.
    Mismatch {
        structure: String,
        problem: &'static str,
    },
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureError::File(e) => write!(f, "invalid structures file {e}"),
            StructureError::NoStructures => write!(f, "there are no structures"),
            StructureError::TooMany { count } => {
                write!(
                    f,
                    "{count} structures is too many; the most is {}",
                    u16::MAX
                )
            }
            StructureError::DuplicateId { id } => write!(f, "structure {id:?} is defined twice"),
            StructureError::UnknownMaterial {
                structure,
                material,
            } => write!(
                f,
                "structure {structure:?} is made of {material:?}, which isn't a material"
            ),
            StructureError::Mismatch { structure, problem } => {
                write!(f, "structure {structure:?}: {problem}")
            }
        }
    }
}

impl std::error::Error for StructureError {}

/// The JSON shape of one file in `data/structures/`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StructuresFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/structures.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    structures: Vec<Structure>,
}

/// Parses and checks every structures file. Each structure's material must
/// be one of `materials`.
pub(crate) fn parse_structures(
    files: &[ContentFile],
    materials: &Materials,
) -> Result<Structures, StructureError> {
    let list = read_all(files, |f: StructuresFile| f.structures).map_err(StructureError::File)?;
    check_list(&list)?;
    list.iter().try_for_each(|s| check_one(s, materials))?;
    Ok(Structures { list })
}

fn check_list(list: &[Structure]) -> Result<(), StructureError> {
    if list.is_empty() {
        return Err(StructureError::NoStructures);
    }
    if u16::try_from(list.len()).is_err() {
        return Err(StructureError::TooMany { count: list.len() });
    }
    match first_duplicate(list.iter().map(|s| s.id.as_str())) {
        Some(id) => Err(StructureError::DuplicateId { id: id.into() }),
        None => Ok(()),
    }
}

/// Checks one structure: its material exists, and its properties go
/// together.
fn check_one(structure: &Structure, materials: &Materials) -> Result<(), StructureError> {
    if materials.index_of(&structure.material).is_none() {
        return Err(StructureError::UnknownMaterial {
            structure: structure.id.clone(),
            material: structure.material.clone(),
        });
    }
    let problem = if structure.walkable && structure.leak_cm2 > 0 {
        Some("only solid structures can leak (leak_cm2), and this one is walkable")
    } else if !structure.walkable && structure.climb.is_some() {
        Some("only walkable structures can be ladders (climb)")
    } else {
        None
    };
    match problem {
        Some(problem) => Err(StructureError::Mismatch {
            structure: structure.id.clone(),
            problem,
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::material::tests::test_materials;

    /// One structure as JSON, for tests: steel, with `properties` (JSON
    /// fields like `"walkable": true`, each starting with a comma).
    fn structure(id: &str, glyph: char, properties: &str) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "{id}", "description": "A test structure.",
                 "glyph": "{glyph}", "material": "steel" {properties} }}"#
        )
    }

    const WALKABLE: &str = r#", "walkable": true"#;

    /// A structures file with one structure for each kind of thing the
    /// tests need, named for what it is (a "wall", a "door"…), all steel.
    /// Tests across the sim use these names in their map legends.
    pub(crate) fn test_structures_json() -> String {
        let all = [
            structure("floor", '.', WALKABLE),
            structure("wall", '#', ""),
            structure("stairs_up", '>', r#", "walkable": true, "climb": "up""#),
            structure("stairs_down", '<', r#", "walkable": true, "climb": "down""#),
            structure("door", '+', ""),
            structure("hull_crack", 'x', r#", "leak_cm2": 4"#),
            structure("hull_breach", 'X', r#", "leak_cm2": 400"#),
            // Walkable, and repairable into a door (see the test recipes).
            structure("frame", '/', WALKABLE),
            // What the test recipes open a door into, and close back.
            structure("open_door", '\'', WALKABLE),
        ];
        format!(r#"{{ "structures": [ {} ] }}"#, all.join(", "))
    }

    /// The parsed test structures, for other modules' tests.
    pub(crate) fn test_structures() -> Structures {
        let files = [ContentFile::new("test.json", test_structures_json())];
        parse_structures(&files, &test_materials()).expect("test structures are valid")
    }

    fn error_of(json: &str) -> String {
        parse_structures(&[ContentFile::new("walls.json", json)], &test_materials())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_valid_file_loads() {
        let structures = test_structures();
        assert_eq!(structures.list().len(), 9);
        assert_eq!(structures.index_of("door"), Some(4));
        assert_eq!(structures.index_of("open_door"), Some(8));
        assert_eq!(structures.index_of("cheese"), None);
    }

    #[test]
    fn properties_are_what_the_simulation_reads_and_default_to_solid() {
        let structures = test_structures();
        let tile =
            |id: &str| structures.list()[usize::from(structures.index_of(id).unwrap())].tile();
        assert_eq!(tile("wall"), Tile::WALL, "solid, airtight, no leak");
        assert_eq!(tile("floor"), Tile::FLOOR);
        assert_eq!(tile("stairs_up"), Tile::LADDER_UP);
        assert_eq!(tile("stairs_down"), Tile::LADDER_DOWN);
        assert_eq!(tile("hull_crack"), Tile::CRACK);
        assert_eq!(tile("hull_breach"), Tile::BREACH);
        assert_eq!(tile("door"), Tile::WALL, "a shut door is just solid");
        assert_eq!(tile("open_door"), Tile::FLOOR, "an open one walkable");
    }

    #[test]
    fn properties_that_do_not_go_together_are_refused() {
        let leaky_floor = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("grate", '=', r#", "walkable": true, "leak_cm2": 10"#)
        );
        assert_eq!(
            error_of(&leaky_floor),
            r#"structure "grate": only solid structures can leak (leak_cm2), and this one is walkable"#
        );
        let solid_ladder = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("pole", '|', r#", "climb": "up""#)
        );
        assert_eq!(
            error_of(&solid_ladder),
            r#"structure "pole": only walkable structures can be ladders (climb)"#
        );
        let sideways = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("slide", '~', r#", "walkable": true, "climb": "sideways""#)
        );
        assert!(error_of(&sideways).contains("sideways"));
    }

    #[test]
    fn mistakes_in_a_file_are_errors_naming_it() {
        let typo = test_structures_json().replace("\"glyph\"", "\"glyf\"");
        assert!(error_of(&typo).starts_with("invalid structures file walls.json: "));
        let long_glyph = test_structures_json().replace("\"#\"", "\"##\"");
        assert!(error_of(&long_glyph).contains("walls.json"));
        let behaviour =
            test_structures_json().replace(r#""walkable": true"#, r#""behaviour": "floor""#);
        assert!(
            error_of(&behaviour).contains("unknown field `behaviour`"),
            "behaviours are gone (D47): {}",
            error_of(&behaviour)
        );
    }

    #[test]
    fn no_structures_is_an_error() {
        assert_eq!(
            error_of(r#"{ "structures": [] }"#),
            "there are no structures"
        );
    }

    #[test]
    fn the_same_id_twice_is_an_error() {
        let json = format!(
            r#"{{ "structures": [ {}, {} ] }}"#,
            structure("wall", '#', ""),
            structure("wall", '#', "")
        );
        assert_eq!(error_of(&json), r#"structure "wall" is defined twice"#);
    }

    #[test]
    fn a_structure_must_be_made_of_a_real_material() {
        let json = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("wall", '#', "").replace("\"steel\"", "\"cheese\"")
        );
        assert_eq!(
            error_of(&json),
            r#"structure "wall" is made of "cheese", which isn't a material"#
        );
    }

    #[test]
    fn open_glyph_is_no_longer_a_field() {
        // An open door is its own structure, with its own glyph (D46).
        let json = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("door", '+', r#", "open_glyph": "'""#)
        );
        assert!(error_of(&json).contains("unknown field `open_glyph`"));
    }

    #[test]
    fn too_many_structures_is_an_error() {
        let one = test_structures().list()[0].clone();
        let err = check_list(&vec![one; 70_000]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "70000 structures is too many; the most is 65535"
        );
    }

    #[test]
    fn everything_about_a_structure_is_part_of_the_hash() {
        let hash = |s: &Structure| {
            let mut h = Fnv64::new();
            s.hash_into(&mut h);
            h.finish()
        };
        let base = test_structures().list()[4].clone();
        let changes: [fn(&mut Structure); 7] = [
            |s| s.glyph = '=',
            |s| s.walkable = !s.walkable,
            |s| s.leak_cm2 += 1,
            |s| s.climb = Some(Vertical::Up),
            |s| s.material.push('!'),
            |s| s.name.push('!'),
            |s| s.colour = Some(Colour::Red),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed);
            assert_ne!(hash(&base), hash(&changed), "{changed:?}");
        }
    }

    #[test]
    fn up_and_down_hash_differently() {
        assert_ne!(Vertical::Up.hash_id(), Vertical::Down.hash_id());
    }
}
