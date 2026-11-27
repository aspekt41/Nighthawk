//! Recipes: how one thing becomes another (decisions D38 and D40).
//!
//! Every change from one structure (or later, item) to another is a recipe
//! in `data/recipes/*.json`: an action, what it acts on, how long it takes,
//! and what it becomes. Cutting a door with a laser is a recipe; patching
//! the hull with scrap will be one; so will building and crafting. Keeping
//! them all as data means the interact menu just looks up which recipes
//! apply to the tile you point at.
//!
//! What a recipe *needs* depends on its action. For cutting, the tool level
//! comes from the material being cut (decision D35): an aluminium door
//! needs `LASER 1`, a steel one `LASER 2`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::item::ItemTypes;
use crate::structure::Structures;

/// What a recipe does. Each action has its own rules in the code; recipes
/// say what it turns what into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeAction {
    /// Cut through a structure with a tool that can cut its material.
    Cut,
    /// Repair or patch a structure, using up the items the recipe lists.
    Repair,
}

impl RecipeAction {
    /// A fixed number per action, for the state hash.
    fn hash_id(self) -> u8 {
        match self {
            RecipeAction::Cut => 0,
            RecipeAction::Repair => 1,
        }
    }
}

/// Some items a recipe uses up: `count` of the item type `item`, made of
/// any material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Use {
    pub item: String,
    pub count: u32,
}

/// What material the new structure is made of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub action: RecipeAction,
    /// The id of the structure it acts on.
    pub from: String,
    /// The id of the structure it becomes.
    pub to: String,
    /// How long it takes, in seconds.
    pub seconds: u32,
    /// The items it uses up. Optional: none if left out.
    #[serde(default)]
    pub uses: Vec<Use>,
    /// What the result is made of. Optional: `kept` if left out.
    #[serde(default)]
    pub material: RecipeMaterial,
}

impl Recipe {
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id)
            .u8(self.action.hash_id())
            .str(&self.from)
            .str(&self.to)
            .u64(u64::from(self.seconds))
            .u8(u8::from(self.material == RecipeMaterial::FromUsed))
            .u64(self.uses.len() as u64);
        for used in &self.uses {
            h.str(&used.item).u64(u64::from(used.count));
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
    /// A recipe naming a structure that isn't defined.
    UnknownStructure {
        recipe: String,
        structure: String,
    },
    /// A recipe using an item that isn't defined.
    UnknownItem {
        recipe: String,
        item: String,
    },
    /// A recipe using none of something (`"count": 0`).
    UsesNone {
        recipe: String,
        item: String,
    },
    /// `"material": "from_used"` on a recipe that uses nothing.
    NothingUsed {
        recipe: String,
    },
    /// A recipe that takes no time.
    NoTime {
        recipe: String,
    },
    /// Two recipes doing the same action to the same structure: which one
    /// the menu offered would be a guess.
    Ambiguous {
        recipe: String,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeError::File(e) => write!(f, "invalid recipes file {e}"),
            RecipeError::DuplicateId { id } => write!(f, "recipe {id:?} is defined twice"),
            RecipeError::UnknownStructure { recipe, structure } => write!(
                f,
                "recipe {recipe:?} names {structure:?}, which isn't a structure"
            ),
            RecipeError::NoTime { recipe } => {
                write!(f, "recipe {recipe:?} must take at least 1 second")
            }
            RecipeError::UnknownItem { recipe, item } => {
                write!(f, "recipe {recipe:?} uses {item:?}, which isn't an item")
            }
            RecipeError::UsesNone { recipe, item } => {
                write!(f, "recipe {recipe:?} uses a count of 0 {item:?}")
            }
            RecipeError::NothingUsed { recipe } => write!(
                f,
                "recipe {recipe:?} takes its material from the items used, but uses none"
            ),
            RecipeError::Ambiguous { recipe } => write!(
                f,
                "recipe {recipe:?} does the same action to the same structure as an earlier one"
            ),
        }
    }
}

impl std::error::Error for RecipeError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipesFile {
    recipes: Vec<Recipe>,
}

/// Parses and checks every recipes file. Each recipe's structures must be
/// in `structures`, and the items it uses in `items`.
pub(crate) fn parse_recipes(
    files: &[ContentFile],
    structures: &Structures,
    items: &ItemTypes,
) -> Result<Recipes, RecipeError> {
    let list = read_all(files, |f: RecipesFile| f.recipes).map_err(RecipeError::File)?;
    if let Some(id) = first_duplicate(list.iter().map(|r| r.id.as_str())) {
        return Err(RecipeError::DuplicateId { id: id.into() });
    }
    for (i, recipe) in list.iter().enumerate() {
        check_one(recipe, structures)?;
        check_uses(recipe, items)?;
        let earlier = &list[..i];
        if earlier
            .iter()
            .any(|r| r.action == recipe.action && r.from == recipe.from)
        {
            return Err(RecipeError::Ambiguous {
                recipe: recipe.id.clone(),
            });
        }
    }
    Ok(Recipes { list })
}

fn check_one(recipe: &Recipe, structures: &Structures) -> Result<(), RecipeError> {
    let unknown = [&recipe.from, &recipe.to]
        .into_iter()
        .find(|id| structures.index_of(id).is_none());
    if let Some(structure) = unknown {
        return Err(RecipeError::UnknownStructure {
            recipe: recipe.id.clone(),
            structure: structure.clone(),
        });
    }
    if recipe.seconds == 0 {
        return Err(RecipeError::NoTime {
            recipe: recipe.id.clone(),
        });
    }
    Ok(())
}

/// Checks the items a recipe uses exist, each at least once, and that a
/// recipe taking its material from them uses some.
fn check_uses(recipe: &Recipe, items: &ItemTypes) -> Result<(), RecipeError> {
    for used in &recipe.uses {
        if items.index_of(&used.item).is_none() {
            return Err(RecipeError::UnknownItem {
                recipe: recipe.id.clone(),
                item: used.item.clone(),
            });
        }
        if used.count == 0 {
            return Err(RecipeError::UsesNone {
                recipe: recipe.id.clone(),
                item: used.item.clone(),
            });
        }
    }
    if recipe.material == RecipeMaterial::FromUsed && recipe.uses.is_empty() {
        return Err(RecipeError::NothingUsed {
            recipe: recipe.id.clone(),
        });
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
    use crate::item::tests::test_items;
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
              "seconds": 30, "uses": [ { "item": "scrap", "count": 2 } ],
              "material": "from_used" },
            { "id": "fix_frame", "action": "repair", "from": "frame", "to": "door",
              "seconds": 20, "uses": [ { "item": "scrap", "count": 1 } ] }
        ] }"#
            .to_string()
    }

    pub(crate) fn test_recipes() -> Recipes {
        parse_recipes(
            &[ContentFile::new("test.json", test_recipes_json())],
            &test_structures(),
            &test_items(),
        )
        .expect("test recipes are valid")
    }

    fn error_of(json: &str) -> String {
        parse_recipes(
            &[ContentFile::new("cutting.json", json)],
            &test_structures(),
            &test_items(),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn a_valid_file_loads() {
        let recipes = test_recipes();
        assert_eq!(recipes.list().len(), 4);
        let patch = &recipes.list()[2];
        assert_eq!(patch.action, RecipeAction::Repair);
        assert_eq!(patch.uses[0].count, 2);
        assert_eq!(patch.material, RecipeMaterial::FromUsed);
        assert_eq!(
            recipes.list()[0].material,
            RecipeMaterial::Kept,
            "the default"
        );
        assert_eq!(recipes.list()[0].action, RecipeAction::Cut);
        assert_eq!(recipes.list()[0].to, "floor");
    }

    #[test]
    fn no_recipes_at_all_is_fine() {
        let recipes = parse_recipes(&[], &test_structures(), &test_items()).unwrap();
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
        let action = test_recipes_json().replace(
            "\"cut\", \"from\": \"door\"",
            "\"bake\", \"from\": \"door\"",
        );
        assert!(error_of(&action).contains("bake"));
    }

    #[test]
    fn the_items_a_recipe_uses_must_exist_and_be_counted() {
        let unknown = test_recipes_json().replace("\"item\": \"scrap\"", "\"item\": \"glue\"");
        assert_eq!(
            error_of(&unknown),
            r#"recipe "patch_crack" uses "glue", which isn't an item"#
        );
        let none = test_recipes_json().replace("\"count\": 2", "\"count\": 0");
        assert_eq!(
            error_of(&none),
            r#"recipe "patch_crack" uses a count of 0 "scrap""#
        );
        let nothing =
            test_recipes_json().replace("\"uses\": [ { \"item\": \"scrap\", \"count\": 2 } ],", "");
        assert_eq!(
            error_of(&nothing),
            r#"recipe "patch_crack" takes its material from the items used, but uses none"#
        );
    }

    #[test]
    fn two_recipes_for_the_same_action_on_the_same_thing_are_an_error() {
        let both = test_recipes_json().replace("\"from\": \"wall\"", "\"from\": \"door\"");
        assert_eq!(
            error_of(&both),
            r#"recipe "cut_wall" does the same action to the same structure as an earlier one"#
        );
    }

    #[test]
    fn everything_about_a_recipe_is_part_of_the_hash() {
        let hash = |recipes: &[Recipe]| {
            let mut h = Fnv64::new();
            hash_recipes(recipes, &mut h);
            h.finish()
        };
        let base = test_recipes().list().to_vec();
        let changes: [fn(&mut Recipe); 6] = [
            |r| r.material = RecipeMaterial::FromUsed,
            |r| {
                r.uses.push(Use {
                    item: "scrap".into(),
                    count: 1,
                })
            },
            |r| r.seconds += 1,
            |r| r.to.push('!'),
            |r| r.from.push('!'),
            |r| r.id.push('!'),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed[0]);
            assert_ne!(hash(&base), hash(&changed));
        }
        assert_eq!(RecipeAction::Cut.hash_id(), 0);
        assert_eq!(RecipeAction::Repair.hash_id(), 1);
    }
}
