//! Recipes: how one thing becomes another (decisions D38, D40 and D55).
//!
//! Every change from one structure (or later, item) to another is a recipe
//! in `data/recipes/*.json`: an action, what it acts on, how long it takes,
//! and what it becomes. A recipe can also act on a piece of furniture,
//! which it takes away (smashing or dismantling it), and any recipe can
//! *give* items, left on the floor where the work was done. Cutting a door with a laser is a recipe; patching
//! the hull with scrap will be one; so will building and crafting. Keeping
//! them all as data means the interact menu just looks up which recipes
//! apply to the tile you point at.
//!
//! A recipe's action is one from `data/actions/` whose effect is
//! `recipe`; the action's rules (reach, a clear line…) apply to every
//! recipe that uses it (decision D43). What a recipe *needs* comes from two
//! places: the tool levels it lists itself, in `needs`, and, for actions
//! with `tool_from_material`, the material being worked (decision D35): an
//! aluminium door needs `LASER 1` to cut, a steel one `LASER 2`.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::action::{Actions, Effect};
use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::furniture::FurnitureTypes;
use crate::hash::Fnv64;
use crate::item::ItemTypes;
use crate::material::Materials;
use crate::quality::{LevelError, Levels, Qualities, hash_levels};
use crate::structure::Structures;

/// Some items a recipe uses up: `count` of the item type `item`, made of
/// any material, or only of materials with the tag `tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Use {
    pub item: String,
    pub count: u32,
    /// Only items made of a material with this tag count, like `"metal"`:
    /// bone scrap won't patch the hull. Optional: any material.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Some items a recipe gives, left on the floor where the work was done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Give {
    pub item: String,
    /// Made of this material. Optional: the item's own.
    #[serde(default)]
    pub material: Option<String>,
    /// How many: a number, or `[least, most]` for a random number in
    /// between (both included), rolled each time.
    pub count: Count,
}

/// How many of something: exactly a number, or a random number between two.
///
/// `untagged` lets the JSON be just `3` or `[1, 3]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Count {
    Exactly(u32),
    Between([u32; 2]),
}

impl Count {
    /// The least and most it can be.
    pub fn range(self) -> (u32, u32) {
        match self {
            Count::Exactly(n) => (n, n),
            Count::Between([least, most]) => (least, most),
        }
    }
}

/// What material the new structure is made of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecipeMaterial {
    /// The same as before (a door cut open is still aluminium).
    #[default]
    Kept,
    /// The material of the first item used (a crack patched with steel
    /// scrap is patched with steel).
    FromUsed,
}

/// One recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    /// The id of the action that follows it, like `"cut"`.
    pub action: String,
    /// The id of the structure it acts on, with `to`. (Or give
    /// `furniture` instead.)
    #[serde(default)]
    pub from: Option<String>,
    /// The id of the structure it becomes, with `from`.
    #[serde(default)]
    pub to: Option<String>,
    /// The id of the kind of furniture it acts on, which it takes away
    /// (smashing or dismantling it); anything inside spills on the floor.
    /// Instead of `from` and `to`.
    #[serde(default)]
    pub furniture: Option<String>,
    /// How long it takes, in seconds.
    pub seconds: u32,
    /// The items it uses up. Optional: none if left out.
    #[serde(default)]
    pub uses: Vec<Use>,
    /// The items it gives, left on the floor. Optional: none.
    #[serde(default)]
    pub gives: Vec<Give>,
    /// What the result is made of. Optional: `kept` if left out.
    #[serde(default)]
    pub material: RecipeMaterial,
    /// Tool levels it needs, all of them, like `{ "WELD": 1 }`. Optional:
    /// none if left out.
    #[serde(default)]
    pub needs: Levels,
}

/// Feeds an optional string into a hash, so `None` and `Some("")` differ.
fn hash_option(text: Option<&str>, h: &mut Fnv64) {
    match text {
        Some(text) => h.u8(1).str(text),
        None => h.u8(0),
    };
}

impl Recipe {
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.action);
        for target in [&self.from, &self.to, &self.furniture] {
            hash_option(target.as_deref(), h);
        }
        h.u64(u64::from(self.seconds))
            .u8(u8::from(self.material == RecipeMaterial::FromUsed))
            .u64(self.uses.len() as u64);
        for used in &self.uses {
            h.str(&used.item).u64(u64::from(used.count));
            hash_option(used.tag.as_deref(), h);
        }
        h.u64(self.gives.len() as u64);
        for given in &self.gives {
            h.str(&given.item);
            hash_option(given.material.as_deref(), h);
            let (least, most) = given.count.range();
            h.u64(u64::from(least)).u64(u64::from(most));
        }
        hash_levels(&self.needs, h);
    }

    /// What it acts on: a structure's id or a kind of furniture's id. Two
    /// recipes for one action may not act on the same thing.
    fn target(&self) -> (bool, Option<&str>) {
        match &self.furniture {
            Some(furniture) => (true, Some(furniture)),
            None => (false, self.from.as_deref()),
        }
    }
}

/// Every recipe from the files in `data/recipes/`, saved as just the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Recipes {
    list: Vec<Recipe>,
}

impl Recipes {
    pub fn list(&self) -> &[Recipe] {
        &self.list
    }

    /// The list, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn list_mut(&mut self) -> &mut Vec<Recipe> {
        &mut self.list
    }
}

/// Everything that can be wrong with the recipe files.
#[derive(Debug)]
pub enum RecipeError {
    File(FileError),
    DuplicateId {
        id: String,
    },
    /// A recipe naming an action, structure or item that isn't defined:
    /// `kind` is which ("action"), `name` what it said.
    Unknown {
        recipe: String,
        kind: &'static str,
        name: String,
    },
    /// A recipe for an action whose effect isn't `recipe` (like picking
    /// up), which would never follow it.
    NotARecipeAction {
        recipe: String,
        action: String,
    },
    /// Its `needs` names a quality that doesn't exist, or a level out of
    /// range.
    Needs {
        recipe: String,
        error: LevelError,
    },
    /// A recipe using none of something (`"count": 0`).
    UsesNone {
        recipe: String,
        item: String,
    },
    /// Two recipes doing the same action to the same structure: which one
    /// the menu offered would be a guess.
    Ambiguous {
        recipe: String,
    },
    /// Fields that don't go together or make no sense: no target, half a
    /// structure target (`from` without `to`), a structure and furniture
    /// at once, no time, or a material from items it doesn't use.
    Shape {
        recipe: String,
        problem: &'static str,
    },
    /// A recipe giving an item a count that's never more than 0, or a
    /// range from high to low.
    GivesNone {
        recipe: String,
        item: String,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeError::File(e) => write!(f, "invalid recipes file {e}"),
            RecipeError::DuplicateId { id } => write!(f, "recipe {id:?} is defined twice"),
            RecipeError::Ambiguous { recipe } => write!(
                f,
                "recipe {recipe:?} does the same action to the same thing as an earlier one"
            ),
            RecipeError::Unknown { recipe, kind, name } => write!(
                f,
                "recipe {recipe:?} names {name:?}, which isn't {} {kind}",
                if kind.starts_with(['a', 'i']) {
                    "an"
                } else {
                    "a"
                }
            ),
            RecipeError::NotARecipeAction { recipe, action } => write!(
                f,
                "recipe {recipe:?} is for {action:?}, which doesn't follow recipes"
            ),
            RecipeError::Needs { recipe, error } => {
                write!(f, "recipe {recipe:?}'s needs: {error}")
            }
            RecipeError::UsesNone { recipe, item } => {
                write!(f, "recipe {recipe:?} uses a count of 0 {item:?}")
            }
            RecipeError::Shape { recipe, problem } => write!(f, "recipe {recipe:?} {problem}"),
            RecipeError::GivesNone { recipe, item } => write!(
                f,
                "recipe {recipe:?} gives {item:?} a count that's never above 0, or runs high to low"
            ),
        }
    }
}

impl RecipeError {
    /// An `Unknown` error for `recipe`, which names `name`, a `kind` that
    /// doesn't exist.
    fn unknown(recipe: &Recipe, kind: &'static str, name: &str) -> Self {
        RecipeError::Unknown {
            recipe: recipe.id.clone(),
            kind,
            name: name.into(),
        }
    }
}

impl std::error::Error for RecipeError {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipesFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/recipes.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    recipes: Vec<Recipe>,
}

/// What recipes may refer to.
pub(crate) struct RecipeRefs<'a> {
    pub actions: &'a Actions,
    pub qualities: &'a Qualities,
    pub materials: &'a Materials,
    pub structures: &'a Structures,
    pub furniture: &'a FurnitureTypes,
    pub items: &'a ItemTypes,
}

/// Parses and checks every recipes file. Each recipe's action, structures,
/// items and tool levels must be among `refs`.
pub(crate) fn parse_recipes(
    files: &[ContentFile],
    refs: &RecipeRefs,
) -> Result<Recipes, RecipeError> {
    let list = read_all(files, |f: RecipesFile| f.recipes).map_err(RecipeError::File)?;
    if let Some(id) = first_duplicate(list.iter().map(|r| r.id.as_str())) {
        return Err(RecipeError::DuplicateId { id: id.into() });
    }
    for (i, recipe) in list.iter().enumerate() {
        check_action(recipe, refs)?;
        check_one(recipe, refs)?;
        check_uses(recipe, refs)?;
        check_gives(recipe, refs)?;
        let earlier = &list[..i];
        if earlier
            .iter()
            .any(|r| r.action == recipe.action && r.target() == recipe.target())
        {
            return Err(RecipeError::Ambiguous {
                recipe: recipe.id.clone(),
            });
        }
    }
    Ok(Recipes { list })
}

/// Checks a recipe's action exists and follows recipes, and that what it
/// `needs` is real qualities at levels in range.
fn check_action(recipe: &Recipe, refs: &RecipeRefs) -> Result<(), RecipeError> {
    match refs.actions.get(&recipe.action) {
        None => return Err(RecipeError::unknown(recipe, "action", &recipe.action)),
        Some(action) if action.effect != Effect::Recipe => {
            return Err(RecipeError::NotARecipeAction {
                recipe: recipe.id.clone(),
                action: recipe.action.clone(),
            });
        }
        Some(_) => {}
    }
    refs.qualities
        .check(&recipe.needs)
        .map_err(|error| RecipeError::Needs {
            recipe: recipe.id.clone(),
            error,
        })
}

/// Checks what a recipe acts on: a structure it turns into another
/// (`from` and `to`, both real), or a real kind of furniture it takes
/// away; and that it takes time.
fn check_one(recipe: &Recipe, refs: &RecipeRefs) -> Result<(), RecipeError> {
    let shape = |problem| RecipeError::Shape {
        recipe: recipe.id.clone(),
        problem,
    };
    match (&recipe.from, &recipe.to, &recipe.furniture) {
        (Some(from), Some(to), None) => check_structures(recipe, [from, to], refs.structures)?,
        (None, None, Some(furniture)) => {
            if refs.furniture.index_of(furniture).is_none() {
                return Err(RecipeError::unknown(recipe, "kind of furniture", furniture));
            }
            if recipe.material == RecipeMaterial::FromUsed {
                return Err(shape(MAKES_NO_STRUCTURE));
            }
        }
        (_, _, Some(_)) => return Err(shape(FURNITURE_AND_STRUCTURE)),
        (None, None, None) => return Err(shape(ACTS_ON_NOTHING)),
        _ => return Err(shape(HALF_A_STRUCTURE)),
    }
    if recipe.seconds == 0 {
        return Err(shape(NO_TIME));
    }
    Ok(())
}

/// What's wrong with a recipe naming furniture and a structure.
const FURNITURE_AND_STRUCTURE: &str =
    "acts on furniture, which it takes away, so it can't have \"from\" or \"to\"";
/// What's wrong with a recipe naming nothing to act on.
const ACTS_ON_NOTHING: &str =
    "acts on nothing: give it \"from\" and \"to\" (a structure) or \"furniture\"";
/// What's wrong with `from` without `to`, or the other way round.
const HALF_A_STRUCTURE: &str = "needs both \"from\" and \"to\"";
/// What's wrong with a recipe that takes no time.
const NO_TIME: &str = "must take at least 1 second";
/// What's wrong with `"material": "from_used"` and no `uses`.
const NOTHING_USED: &str = "takes its material from the items used, but uses none";
/// What's wrong with `"material": "from_used"` on furniture.
const MAKES_NO_STRUCTURE: &str =
    "takes its material from the items used, but makes no structure to have it";

/// Checks every structure id in `ids` is a real structure.
fn check_structures(
    recipe: &Recipe,
    ids: [&String; 2],
    structures: &Structures,
) -> Result<(), RecipeError> {
    match ids.into_iter().find(|id| structures.index_of(id).is_none()) {
        Some(structure) => Err(RecipeError::unknown(recipe, "structure", structure)),
        None => Ok(()),
    }
}

/// Checks the items a recipe uses exist, each at least once, with real
/// tags; and that a recipe taking its material from them uses some.
fn check_uses(recipe: &Recipe, refs: &RecipeRefs) -> Result<(), RecipeError> {
    let tags = refs.materials.tags();
    for used in &recipe.uses {
        if refs.items.index_of(&used.item).is_none() {
            return Err(RecipeError::unknown(recipe, "item", &used.item));
        }
        if let Some(tag) = &used.tag
            && !tags.contains(&tag.as_str())
        {
            return Err(RecipeError::unknown(recipe, "material tag", tag));
        }
        if used.count == 0 {
            return Err(RecipeError::UsesNone {
                recipe: recipe.id.clone(),
                item: used.item.clone(),
            });
        }
    }
    if recipe.material == RecipeMaterial::FromUsed && recipe.uses.is_empty() {
        return Err(RecipeError::Shape {
            recipe: recipe.id.clone(),
            problem: NOTHING_USED,
        });
    }
    Ok(())
}

/// Checks the items a recipe gives exist, in real materials, and that
/// each can give at least one.
fn check_gives(recipe: &Recipe, refs: &RecipeRefs) -> Result<(), RecipeError> {
    for given in &recipe.gives {
        if refs.items.index_of(&given.item).is_none() {
            return Err(RecipeError::unknown(recipe, "item", &given.item));
        }
        if let Some(material) = &given.material
            && refs.materials.index_of(material).is_none()
        {
            return Err(RecipeError::unknown(recipe, "material", material));
        }
        let (least, most) = given.count.range();
        if most == 0 || least > most {
            return Err(RecipeError::GivesNone {
                recipe: recipe.id.clone(),
                item: given.item.clone(),
            });
        }
    }
    Ok(())
}

/// Feeds a list of recipes into a state hash.
pub(crate) fn hash_recipes(recipes: &[Recipe], h: &mut Fnv64) {
    h.u64(recipes.len() as u64);
    for recipe in recipes {
        recipe.hash_into(h);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::action::tests::test_actions;
    use crate::furniture::tests::test_furniture;
    use crate::item::tests::test_items;
    use crate::material::tests::test_materials;
    use crate::quality::tests::test_qualities;
    use crate::structure::tests::test_structures;

    /// Test recipes: cutting a door or a wall leaves floor, in 60 seconds;
    /// patching a hull crack with 2 scrap makes a wall of the scrap's
    /// material, in 30 seconds; fixing a frame with 1 scrap makes a door
    /// of the frame's own material, in 20 seconds.
    pub(crate) fn test_recipes_json() -> String {
        r#"{ "recipes": [
            { "id": "cut_door", "action": "cut", "from": "door", "to": "floor", "seconds": 60 },
            { "id": "cut_wall", "action": "cut", "from": "wall", "to": "floor", "seconds": 60 },
            { "id": "patch_crack", "action": "repair", "from": "hull_crack", "to": "wall",
              "seconds": 30, "uses": [ { "item": "scrap", "count": 2, "tag": "metal" } ],
              "material": "from_used" },
            { "id": "fix_frame", "action": "repair", "from": "frame", "to": "door",
              "seconds": 20, "uses": [ { "item": "scrap", "count": 1 } ] },
            { "id": "weld_crack", "action": "weld", "from": "hull_crack", "to": "wall",
              "seconds": 40, "needs": { "WELD": 2 } },
            { "id": "open_door", "action": "open", "from": "door", "to": "open_door",
              "seconds": 2 },
            { "id": "close_door", "action": "close", "from": "open_door", "to": "door",
              "seconds": 2 },
            { "id": "smash_crate", "action": "smash", "furniture": "crate", "seconds": 10,
              "gives": [ { "item": "scrap", "count": [0, 2] } ] },
            { "id": "dismantle_crate", "action": "dismantle", "furniture": "crate",
              "seconds": 30, "needs": { "SCREW": 1 },
              "gives": [ { "item": "scrap", "count": 3 },
                         { "item": "scrap", "material": "hard_alloy", "count": 1 } ] },
            { "id": "smash_cabinet", "action": "smash", "furniture": "cabinet", "seconds": 10 }
        ] }"#
            .to_string()
    }

    /// Parses `json` as a recipes file against the test content.
    fn parse_test(name: &str, json: &str) -> Result<Recipes, RecipeError> {
        let (actions, qualities) = (test_actions(), test_qualities());
        let (structures, items) = (test_structures(), test_items());
        let (materials, furniture) = (test_materials(), test_furniture());
        let refs = RecipeRefs {
            actions: &actions,
            qualities: &qualities,
            materials: &materials,
            structures: &structures,
            furniture: &furniture,
            items: &items,
        };
        let files: Vec<ContentFile> = (!json.is_empty())
            .then(|| ContentFile::new(name, json))
            .into_iter()
            .collect();
        parse_recipes(&files, &refs)
    }

    /// Test recipes: cut a door or a wall into floor (60 s); patch a hull
    /// crack with 2 metal scrap into a wall of the scrap's material (30 s);
    /// fix a frame with 1 scrap into a door (20 s); weld a hull crack shut
    /// into a wall with WELD 2 (40 s); open a door into an open door, and
    /// close it back (2 s each); smash a crate for 0 to 2 scrap (10 s), or
    /// dismantle it with SCREW 1 for 3 steel scrap and 1 hard-alloy scrap
    /// (30 s); smash a cabinet for nothing (10 s).
    pub(crate) fn test_recipes() -> Recipes {
        parse_test("test.json", &test_recipes_json()).expect("test recipes are valid")
    }

    fn error_of(json: &str) -> String {
        parse_test("cutting.json", json).unwrap_err().to_string()
    }

    #[test]
    fn a_valid_file_loads() {
        let recipes = test_recipes();
        assert_eq!(recipes.list().len(), 10);
        let patch = &recipes.list()[2];
        assert_eq!(patch.action, "repair");
        assert_eq!(patch.uses[0].count, 2);
        assert_eq!(patch.material, RecipeMaterial::FromUsed);
        assert_eq!(
            recipes.list()[0].material,
            RecipeMaterial::Kept,
            "the default"
        );
        assert_eq!(recipes.list()[0].action, "cut");
        assert_eq!(recipes.list()[0].to.as_deref(), Some("floor"));
        let smash = &recipes.list()[7];
        assert_eq!(smash.furniture.as_deref(), Some("crate"));
        assert_eq!(smash.gives[0].count, Count::Between([0, 2]));
        assert_eq!(recipes.list()[8].gives[0].count.range(), (3, 3));
        assert_eq!(patch.uses[0].tag.as_deref(), Some("metal"));
        assert!(
            recipes.list()[0].needs.is_empty(),
            "needs nothing by default"
        );
        assert_eq!(recipes.list()[4].needs.get("WELD"), Some(&2));
    }

    #[test]
    fn no_recipes_at_all_is_fine() {
        let recipes = parse_test("none.json", "").unwrap();
        assert!(recipes.list().is_empty());
    }

    #[test]
    fn mistakes_are_errors_naming_the_file_and_recipe() {
        assert!(error_of("[").starts_with("invalid recipes file cutting.json: "));
        let unknown = test_recipes_json().replace(
            "\"to\": \"floor\", \"seconds\": 60 },",
            "\"to\": \"rubble\", \"seconds\": 60 },",
        );
        assert_eq!(
            error_of(&unknown),
            r#"recipe "cut_door" names "rubble", which isn't a structure"#
        );
        let instant = test_recipes_json().replace("\"seconds\": 60 },", "\"seconds\": 0 },");
        assert_eq!(
            error_of(&instant),
            r#"recipe "cut_door" must take at least 1 second"#
        );
        let twice = test_recipes_json().replace("cut_wall", "cut_door");
        assert_eq!(error_of(&twice), r#"recipe "cut_door" is defined twice"#);
    }

    #[test]
    fn a_recipe_must_be_for_a_real_action_that_follows_recipes() {
        let action = test_recipes_json().replace(
            "\"cut\", \"from\": \"door\"",
            "\"bake\", \"from\": \"door\"",
        );
        assert_eq!(
            error_of(&action),
            r#"recipe "cut_door" names "bake", which isn't an action"#
        );
        let pick_up = test_recipes_json().replace(
            "\"cut\", \"from\": \"door\"",
            "\"pick_up\", \"from\": \"door\"",
        );
        assert_eq!(
            error_of(&pick_up),
            r#"recipe "cut_door" is for "pick_up", which doesn't follow recipes"#
        );
    }

    #[test]
    fn what_a_recipe_needs_must_be_real_qualities_in_range() {
        let unknown = test_recipes_json().replace("\"WELD\": 2", "\"GLUE\": 2");
        assert_eq!(
            error_of(&unknown),
            r#"recipe "weld_crack"'s needs: "GLUE" isn't a quality"#
        );
        let high = test_recipes_json().replace("\"WELD\": 2", "\"WELD\": 11");
        assert!(error_of(&high).contains("WELD 11 is out of range"));
    }

    #[test]
    fn the_items_a_recipe_uses_must_exist_and_be_counted() {
        let unknown = test_recipes_json().replace("\"item\": \"scrap\"", "\"item\": \"glue\"");
        assert_eq!(
            error_of(&unknown),
            r#"recipe "patch_crack" names "glue", which isn't an item"#
        );
        let none = test_recipes_json().replace("\"count\": 2", "\"count\": 0");
        assert_eq!(
            error_of(&none),
            r#"recipe "patch_crack" uses a count of 0 "scrap""#
        );
        let nothing = test_recipes_json().replace(
            "\"uses\": [ { \"item\": \"scrap\", \"count\": 2, \"tag\": \"metal\" } ],",
            "",
        );
        assert_eq!(
            error_of(&nothing),
            r#"recipe "patch_crack" takes its material from the items used, but uses none"#
        );
        let shiny = test_recipes_json().replace("\"tag\": \"metal\"", "\"tag\": \"shiny\"");
        assert_eq!(
            error_of(&shiny),
            r#"recipe "patch_crack" names "shiny", which isn't a material tag"#
        );
    }

    /// The test recipes with one more on the end, written as JSON.
    fn with_recipe(json: &str) -> String {
        test_recipes_json().replace("\n        ] }", &format!(", {json} ] }}"))
    }

    #[test]
    fn a_recipe_acts_on_a_structure_or_on_furniture_and_not_both() {
        let shape = |fields: &str| {
            error_of(&with_recipe(&format!(
                r#"{{ "id": "odd", "action": "smash", "seconds": 5 {fields} }}"#
            )))
        };
        assert_eq!(
            shape(r#", "furniture": "crate", "from": "door""#),
            r#"recipe "odd" acts on furniture, which it takes away, so it can't have "from" or "to""#
        );
        assert_eq!(
            shape(""),
            r#"recipe "odd" acts on nothing: give it "from" and "to" (a structure) or "furniture""#
        );
        assert_eq!(
            shape(r#", "from": "door""#),
            r#"recipe "odd" needs both "from" and "to""#
        );
        assert_eq!(
            shape(r#", "furniture": "sofa""#),
            r#"recipe "odd" names "sofa", which isn't a kind of furniture"#
        );
        assert_eq!(
            shape(
                r#", "furniture": "stool", "material": "from_used", "uses": [ { "item": "scrap", "count": 1 } ]"#
            ),
            r#"recipe "odd" takes its material from the items used, but makes no structure to have it"#
        );
        assert_eq!(
            shape(r#", "furniture": "crate""#),
            r#"recipe "odd" does the same action to the same thing as an earlier one"#
        );
    }

    #[test]
    fn what_a_recipe_gives_must_exist_and_come_to_something() {
        let gives = |give: &str| {
            error_of(&with_recipe(&format!(
                r#"{{ "id": "odd", "action": "smash", "furniture": "stool", "seconds": 5,
                     "gives": [ {give} ] }}"#
            )))
        };
        assert_eq!(
            gives(r#"{ "item": "gem", "count": 1 }"#),
            r#"recipe "odd" names "gem", which isn't an item"#
        );
        assert_eq!(
            gives(r#"{ "item": "scrap", "material": "gold", "count": 1 }"#),
            r#"recipe "odd" names "gold", which isn't a material"#
        );
        let never =
            r#"recipe "odd" gives "scrap" a count that's never above 0, or runs high to low"#;
        assert_eq!(gives(r#"{ "item": "scrap", "count": 0 }"#), never);
        assert_eq!(gives(r#"{ "item": "scrap", "count": [2, 1] }"#), never);
        assert_eq!(gives(r#"{ "item": "scrap", "count": [0, 0] }"#), never);
    }

    #[test]
    fn two_recipes_for_the_same_action_on_the_same_thing_are_an_error() {
        let both = test_recipes_json().replace("\"from\": \"wall\"", "\"from\": \"door\"");
        assert_eq!(
            error_of(&both),
            r#"recipe "cut_wall" does the same action to the same thing as an earlier one"#
        );
    }

    fn use_scrap(tag: Option<&str>) -> Use {
        Use {
            item: "scrap".into(),
            count: 1,
            tag: tag.map(Into::into),
        }
    }

    fn give_scrap(material: Option<&str>, count: Count) -> Give {
        Give {
            item: "scrap".into(),
            material: material.map(Into::into),
            count,
        }
    }

    #[test]
    fn everything_about_a_recipe_is_part_of_the_hash() {
        let hash = |recipes: &[Recipe]| {
            let mut h = Fnv64::new();
            hash_recipes(recipes, &mut h);
            h.finish()
        };
        let base = test_recipes().list().to_vec();
        let changes: [fn(&mut Recipe); 12] = [
            |r| r.action.push('!'),
            |r| r.furniture = Some("crate".into()),
            |r| r.uses.push(use_scrap(Some("metal"))),
            |r| r.gives.push(give_scrap(None, Count::Exactly(1))),
            |r| {
                r.gives
                    .push(give_scrap(Some("steel"), Count::Between([0, 1])))
            },
            |r| {
                r.needs.insert("WELD".into(), 1);
            },
            |r| r.material = RecipeMaterial::FromUsed,
            |r| r.uses.push(use_scrap(None)),
            |r| r.seconds += 1,
            |r| r.to = Some("x".into()),
            |r| r.from = None,
            |r| r.id.push('!'),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed[0]);
            assert_ne!(hash(&base), hash(&changed));
        }
    }
}
