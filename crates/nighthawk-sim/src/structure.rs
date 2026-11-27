//! Structures: what each tile *is* (decision D37).
//!
//! A structure is a kind of thing a tile can be: deck plating, a bulkhead,
//! hull plating, a door, a ladder, a crack in the hull. Structures are
//! content, loaded from `data/structures/*.json`. Each one names its
//! *behaviour*, from a short fixed list the code knows how to simulate
//! (floor, wall, door, ladder, hull damage), and the material it's made of
//! unless a map says otherwise. So the data decides what exists and what
//! it's called; the code decides what a door *does*.
//!
//! What a structure is made of is a separate question, answered by its
//! material (see `material.rs`).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::map::Tile;
use crate::material::{Colour, Materials};

/// How a structure behaves in the simulation. Each behaviour is one
/// [`Tile`]. A shut door and an open one are two structures (decision
/// D46): opening a door is a recipe that turns one into the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Behaviour {
    Floor,
    Wall,
    StairsUp,
    StairsDown,
    /// A shut door: solid and airtight.
    Door,
    HullCrack,
    HullBreach,
    /// An open door: walkable, and air passes.
    OpenDoor,
}

impl Behaviour {
    /// The tile a structure with this behaviour is.
    pub fn tile(self) -> Tile {
        match self {
            Behaviour::Floor => Tile::Floor,
            Behaviour::Wall => Tile::Wall,
            Behaviour::StairsUp => Tile::StairsUp,
            Behaviour::StairsDown => Tile::StairsDown,
            Behaviour::Door => Tile::DoorClosed,
            Behaviour::HullCrack => Tile::HullCrack,
            Behaviour::HullBreach => Tile::HullBreach,
            Behaviour::OpenDoor => Tile::DoorOpen,
        }
    }

    /// A fixed number per behaviour, for the state hash. Never change or
    /// reuse one.
    fn hash_id(self) -> u8 {
        match self {
            Behaviour::Floor => 0,
            Behaviour::Wall => 1,
            Behaviour::StairsUp => 2,
            Behaviour::StairsDown => 3,
            Behaviour::Door => 4,
            Behaviour::HullCrack => 5,
            Behaviour::HullBreach => 6,
            Behaviour::OpenDoor => 7,
        }
    }
}

/// One kind of structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub behaviour: Behaviour,
    /// The id of the material it's made of, unless a map says otherwise.
    pub material: String,
    /// Optional: the colour to draw it in, instead of the usual colour for
    /// its behaviour (a damaged door is red, though it behaves like floor).
    #[serde(default)]
    pub colour: Option<Colour>,
}

impl Structure {
    /// Feeds everything about it into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name).str(&self.description);
        h.u64(u64::from(self.glyph))
            .u8(self.behaviour.hash_id())
            .str(&self.material)
            .u64(self.colour.map_or(u64::MAX, |c| u64::from(c.hash_id())));
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
    /// misspelled field, an unknown behaviour, a glyph that isn't one
    /// character.
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
        }
    }
}

impl std::error::Error for StructureError {}

/// The JSON shape of one file in `data/structures/`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuresFile {
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

/// Checks one structure: its material exists.
fn check_one(structure: &Structure, materials: &Materials) -> Result<(), StructureError> {
    if materials.index_of(&structure.material).is_none() {
        return Err(StructureError::UnknownMaterial {
            structure: structure.id.clone(),
            material: structure.material.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::material::tests::test_materials;

    /// One structure as JSON, for tests.
    fn structure(id: &str, behaviour: &str, glyph: char, extra: &str) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "{id}", "description": "A test structure.",
                 "glyph": "{glyph}", "behaviour": "{behaviour}", "material": "steel" {extra} }}"#
        )
    }

    /// A structures file with one structure per behaviour, named after the
    /// behaviour (a "wall", a "door"…), all steel. Tests across the sim use
    /// these names in their map legends.
    pub(crate) fn test_structures_json() -> String {
        let all = [
            structure("floor", "floor", '.', ""),
            structure("wall", "wall", '#', ""),
            structure("stairs_up", "stairs_up", '>', ""),
            structure("stairs_down", "stairs_down", '<', ""),
            structure("door", "door", '+', ""),
            structure("hull_crack", "hull_crack", 'x', ""),
            structure("hull_breach", "hull_breach", 'X', ""),
            // Walkable, and repairable into a door (see the test recipes).
            structure("frame", "floor", '/', ""),
            // What the test recipes open a door into, and close back.
            structure("open_door", "open_door", '\'', ""),
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
    fn each_behaviour_is_one_tile() {
        let all = [
            (Behaviour::Floor, Tile::Floor),
            (Behaviour::Wall, Tile::Wall),
            (Behaviour::StairsUp, Tile::StairsUp),
            (Behaviour::StairsDown, Tile::StairsDown),
            (Behaviour::Door, Tile::DoorClosed),
            (Behaviour::HullCrack, Tile::HullCrack),
            (Behaviour::HullBreach, Tile::HullBreach),
            (Behaviour::OpenDoor, Tile::DoorOpen),
        ];
        for (behaviour, tile) in all {
            assert_eq!(behaviour.tile(), tile);
        }
    }

    #[test]
    fn mistakes_in_a_file_are_errors_naming_it() {
        let typo = test_structures_json().replace("\"glyph\"", "\"glyf\"");
        assert!(error_of(&typo).starts_with("invalid structures file walls.json: "));
        let long_glyph = test_structures_json().replace("\"#\"", "\"##\"");
        assert!(error_of(&long_glyph).contains("walls.json"));
        let behaviour =
            test_structures_json().replace("\"floor\", \"material\"", "\"lava\", \"material\"");
        assert!(
            error_of(&behaviour).contains("lava"),
            "{}",
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
            structure("wall", "wall", '#', ""),
            structure("wall", "wall", '#', "")
        );
        assert_eq!(error_of(&json), r#"structure "wall" is defined twice"#);
    }

    #[test]
    fn a_structure_must_be_made_of_a_real_material() {
        let json = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("wall", "wall", '#', "").replace("\"steel\"", "\"cheese\"")
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
            structure("door", "door", '+', r#", "open_glyph": "'""#)
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
        let changes: [fn(&mut Structure); 5] = [
            |s| s.glyph = '=',
            |s| s.behaviour = Behaviour::Wall,
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
    fn behaviours_have_their_own_hash_ids() {
        let ids = [
            Behaviour::Floor,
            Behaviour::Wall,
            Behaviour::StairsUp,
            Behaviour::StairsDown,
            Behaviour::Door,
            Behaviour::HullCrack,
            Behaviour::HullBreach,
            Behaviour::OpenDoor,
        ]
        .map(Behaviour::hash_id);
        assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
