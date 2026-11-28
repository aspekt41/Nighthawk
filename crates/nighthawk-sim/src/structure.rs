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
use crate::light::MAX_LIGHT;
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
    /// You can see through it, though it's solid, like a window. Only for
    /// solid structures (walkable ones never block sight). Optional: solid
    /// structures block sight.
    #[serde(default)]
    pub see_through: bool,
    /// How many tiles its light reaches, if it's a light (like an
    /// emergency light set into a bulkhead), from 1 to 20. Optional: it
    /// gives no light.
    #[serde(default)]
    pub light: u8,
    /// A pressure door's breach interlock (decision D65): it won't open
    /// while the space on one side of it is venting to space and the space
    /// on another side holds more than this many tiles of air. So only an
    /// airlock, a chamber this small, can open onto a breach, and a
    /// habitat can't be bled through it. Only for solid structures.
    /// Optional: no interlock.
    #[serde(default)]
    pub interlock_tiles: Option<u32>,
    /// The circuit it feeds while it stands, like `"emergency"`: it's a
    /// running power source, such as a working generator (decision D66).
    /// Until the power network (M3), a circuit is only a name. Optional:
    /// it feeds nothing.
    #[serde(default)]
    pub supplies: Option<String>,
    /// The circuit it's on: its light shines only while something on the
    /// ship feeds that circuit (D66). Some structure must supply it.
    /// Optional: it needs no power.
    #[serde(default)]
    pub circuit: Option<String>,
    /// What look mode says about it while its `circuit` has no power, in
    /// place of `description` (a dark lamp, a readout showing a fault).
    /// Only with `circuit`. Optional: the description always.
    #[serde(default)]
    pub unpowered_description: Option<String>,
}

impl Structure {
    /// Its properties, as the simulation reads them on every tile of it.
    pub fn tile(&self) -> Tile {
        Tile {
            walkable: self.walkable,
            leak_cm2: self.leak_cm2,
            climb: self.climb,
            see_through: self.see_through,
            light: self.light,
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
            .u8(self.climb.map_or(u8::MAX, Vertical::hash_id))
            .u8(u8::from(self.see_through))
            .u8(self.light)
            .u64(self.interlock_tiles.map_or(u64::MAX, u64::from));
        // An empty string stands for "none": no real name is empty.
        h.str(self.supplies.as_deref().unwrap_or(""))
            .str(self.circuit.as_deref().unwrap_or(""))
            .str(self.unpowered_description.as_deref().unwrap_or(""));
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

    /// Every circuit some structure feeds, each once, in the order they
    /// first appear (D66). Events name a circuit by its place here.
    pub fn circuits(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for name in self.list.iter().filter_map(|s| s.supplies.as_deref()) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// A structure on a circuit that nothing could ever feed is a typo.
    fn check_circuits(&self) -> Result<(), StructureError> {
        let fed = self.circuits();
        let unfed = self
            .list
            .iter()
            .find(|s| s.circuit.as_deref().is_some_and(|c| !fed.contains(&c)));
        match unfed {
            Some(s) => Err(StructureError::UnfedCircuit {
                structure: s.id.clone(),
                circuit: s.circuit.clone().unwrap_or_default(),
            }),
            None => Ok(()),
        }
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
    /// A structure on a circuit that no structure supplies.
    UnfedCircuit {
        structure: String,
        circuit: String,
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
            StructureError::UnfedCircuit { structure, circuit } => write!(
                f,
                "structure {structure:?} is on circuit {circuit:?}, which no structure supplies"
            ),
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
    let structures = Structures { list };
    structures.check_circuits()?;
    Ok(structures)
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

/// What's wrong with a light that reaches too far. The number is
/// [`MAX_LIGHT`], checked by a test.
const TOO_BRIGHT: &str = "light can reach at most 20 tiles";

/// Checks one structure: its material exists, and its properties go
/// together.
fn check_one(structure: &Structure, materials: &Materials) -> Result<(), StructureError> {
    if materials.index_of(&structure.material).is_none() {
        return Err(StructureError::UnknownMaterial {
            structure: structure.id.clone(),
            material: structure.material.clone(),
        });
    }
    // A table of (what's wrong, what to say): the first that's true is
    // the error. Easier to read and extend than a chain of `else if`s.
    let problems = [
        (
            structure.walkable && structure.leak_cm2 > 0,
            "only solid structures can leak (leak_cm2), and this one is walkable",
        ),
        (
            !structure.walkable && structure.climb.is_some(),
            "only walkable structures can be ladders (climb)",
        ),
        (
            structure.walkable && structure.see_through,
            "walkable structures never block sight, so see_through is only for solid ones",
        ),
        (structure.light > MAX_LIGHT, TOO_BRIGHT),
        (
            structure.walkable && structure.interlock_tiles.is_some(),
            "only solid structures (shut doors) can have a breach interlock (interlock_tiles)",
        ),
        (
            structure.supplies.is_some() && structure.circuit.is_some(),
            "a structure can feed a circuit (supplies) or be on one (circuit), not both",
        ),
        (
            structure.unpowered_description.is_some() && structure.circuit.is_none(),
            "only a structure on a circuit can have an unpowered_description",
        ),
    ];
    match problems.into_iter().find(|&(wrong, _)| wrong) {
        Some((_, problem)) => Err(StructureError::Mismatch {
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
            // A light set into a wall, bright enough for any test room.
            structure("lamp", '*', r#", "light": 20"#),
            // A pressure door that may open onto a breach only from a
            // space of 2 tiles or fewer (the test recipes open it).
            structure("airlock", '+', r#", "interlock_tiles": 2"#),
            // A generator feeding the "mains" circuit, one that's dead (a
            // test recipe repairs it into the first), and a bulb on the
            // circuit, bright enough for any test room while it's fed.
            structure("generator", 'G', r#", "supplies": "mains""#),
            structure("dead_generator", 'g', ""),
            structure(
                "bulb",
                'o',
                r#", "light": 20, "circuit": "mains", "unpowered_description": "A dark bulb.""#,
            ),
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
        assert_eq!(structures.list().len(), 14);
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
        let clear_floor = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure(
                "glass_floor",
                '.',
                r#", "walkable": true, "see_through": true"#
            )
        );
        assert!(error_of(&clear_floor).contains("see_through is only for solid ones"));
        let window = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("window", '=', r#", "see_through": true"#)
        );
        let parsed = parse_structures(&[ContentFile::new("w.json", window)], &test_materials());
        assert!(parsed.unwrap().list()[0].tile().see_through);
        let interlocked_floor = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("hatch", '.', r#", "walkable": true, "interlock_tiles": 2"#)
        );
        assert!(error_of(&interlocked_floor).contains("only solid structures (shut doors)"));
    }

    #[test]
    fn circuits_are_named_by_what_feeds_them_and_must_be_fed() {
        let structures = test_structures();
        assert_eq!(structures.circuits(), ["mains"]);
        let both = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("relay", 'r', r#", "supplies": "a", "circuit": "a""#)
        );
        assert!(error_of(&both).contains("not both"));
        let unpowered_alone = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("lamp", '*', r#", "unpowered_description": "Dark.""#)
        );
        assert!(error_of(&unpowered_alone).contains("only a structure on a circuit"));
        let unfed = format!(
            r#"{{ "structures": [ {} ] }}"#,
            structure("lamp", '*', r#", "circuit": "mians""#)
        );
        assert_eq!(
            error_of(&unfed),
            r#"structure "lamp" is on circuit "mians", which no structure supplies"#
        );
    }

    #[test]
    fn a_light_reaches_as_far_as_it_says_up_to_the_limit() {
        let light = |reach: u8| {
            format!(
                r#"{{ "structures": [ {} ] }}"#,
                structure("emergency_light", '*', &format!(r#", "light": {reach}"#))
            )
        };
        let files = [ContentFile::new("l.json", light(MAX_LIGHT))];
        let parsed = parse_structures(&files, &test_materials()).unwrap();
        assert_eq!(parsed.list()[0].tile().light, MAX_LIGHT);
        assert_eq!(
            test_structures().list()[0].tile().light,
            0,
            "none by default"
        );
        let too_far = error_of(&light(MAX_LIGHT + 1));
        assert!(
            too_far.contains("light can reach at most 20 tiles"),
            "{too_far}"
        );
        assert!(
            TOO_BRIGHT.contains(&MAX_LIGHT.to_string()),
            "the message is right"
        );
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
        let changes: [fn(&mut Structure); 13] = [
            |s| s.supplies = Some("mains".into()),
            |s| s.circuit = Some("mains".into()),
            |s| s.unpowered_description = Some("Dark.".into()),
            |s| s.interlock_tiles = Some(0),
            |s| s.light += 1,
            |s| s.see_through = !s.see_through,
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
