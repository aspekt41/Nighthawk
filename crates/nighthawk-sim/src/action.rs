//! Actions: what the player can do to a tile, as content in `data/actions/`
//! (decision D43).
//!
//! An action is a verb, like "Cut" or "Repair", with the rules for doing it:
//! how far it reaches, whether it needs a clear line, whether the tile must
//! be clear of items, whether a tool is needed and what it does. What each
//! action *does* is one of a few fixed [`Effect`]s: most follow a recipe
//! (`data/recipes/`) that turns one structure into another. The rules
//! themselves are checked in `interact.rs`.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;

/// What doing an action does: a small, fixed list, so the sim knows how to
/// carry each one out. Everything else about an action is data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Follow this action's recipe for the structure there: take its time,
    /// use up its items, and turn the tile into what it makes. The usual
    /// effect, so it's the default (`#[default]`).
    #[default]
    Recipe,
    /// Pick up the items lying there.
    PickUp,
}

impl Effect {
    /// A fixed number per effect, for the state hash. Never change or
    /// reuse one. (2 and 3 were opening and shutting doors, which are
    /// recipes now: decision D46.)
    fn hash_id(self) -> u8 {
        match self {
            Effect::Recipe => 0,
            Effect::PickUp => 1,
        }
    }
}

/// One thing the player can do to a tile.
///
/// The `bool` fields all default to `false` (`#[serde(default)]`), so a
/// data file only mentions the ones that are true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// How recipes and materials refer to it, like `"cut"`.
    pub id: String,
    /// How the menu shows it, like "Cut".
    pub name: String,
    /// The key that does it in the interact menu.
    pub key: char,
    /// How far away it works, in steps (diagonals count as one), at least 1.
    pub reach: u32,
    /// A solid tile on the straight line to the target blocks it.
    #[serde(default)]
    pub clear_line: bool,
    /// It can be done to the tile you're standing on.
    #[serde(default)]
    pub on_own_tile: bool,
    /// Items lying on the tile block it (they'd end up inside).
    #[serde(default)]
    pub clear_of_items: bool,
    /// It needs a tool at whatever level the target's material lists for
    /// this action in its `worked_by`; a material that lists none can't
    /// have it done at all.
    #[serde(default)]
    pub tool_from_material: bool,
    /// What doing it does.
    #[serde(default)]
    pub effect: Effect,
    /// How long it takes, for effects without a recipe to say: for picking
    /// up, the time per item.
    #[serde(default)]
    pub seconds: Option<u32>,
    /// The log line when a recipe action is done. `{was}` is replaced with
    /// the structure's name before, and `{now}` with its name after, as in
    /// "You cut through the {was}."
    #[serde(default)]
    pub done: Option<String>,
    /// Walking into a solid tile does this action, if it can be done there
    /// (walking into a shut door opens it).
    #[serde(default)]
    pub on_bump: bool,
    /// Its key also works while playing, followed by a direction, to do it
    /// to the tile that way (`c` then a direction closes a door).
    #[serde(default)]
    pub shortcut: bool,
}

impl Action {
    /// The log line for a finished recipe action, with `was` and `now`
    /// filled in.
    pub fn done_text(&self, was: &str, now: &str) -> String {
        self.done
            .as_deref()
            .unwrap_or_default()
            .replace("{was}", was)
            .replace("{now}", now)
    }

    fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id).str(&self.name);
        h.u64(u64::from(self.key))
            .u64(u64::from(self.reach))
            .u8(u8::from(self.clear_line))
            .u8(u8::from(self.on_own_tile))
            .u8(u8::from(self.clear_of_items))
            .u8(u8::from(self.tool_from_material))
            .u8(self.effect.hash_id())
            .u64(self.seconds.map_or(u64::MAX, u64::from))
            .str(self.done.as_deref().unwrap_or_default())
            .u8(u8::from(self.on_bump))
            .u8(u8::from(self.shortcut));
    }
}

/// Feeds a list of actions into a state hash.
pub(crate) fn hash_actions(actions: &[Action], h: &mut Fnv64) {
    h.u64(actions.len() as u64);
    for action in actions {
        action.hash_into(h);
    }
}

/// Every action from the files in `data/actions/`, saved as just the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Actions {
    list: Vec<Action>,
}

impl Actions {
    /// Every action, in the order the interact menu lists them.
    pub fn list(&self) -> &[Action] {
        &self.list
    }

    /// The number of the action with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|a| a.id == id)?;
        Some(u16::try_from(i).expect("action count was checked on load"))
    }

    /// The action with this `id`, if there is one.
    pub fn get(&self, id: &str) -> Option<&Action> {
        self.list.iter().find(|a| a.id == id)
    }
}

/// Everything that can be wrong with the actions files.
#[derive(Debug)]
pub enum ActionError {
    File(FileError),
    /// More actions than a command can refer to.
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
    /// Two actions with the same key.
    SameKey {
        key: char,
        first: String,
        second: String,
    },
    /// A reach of 0.
    NoReach {
        action: String,
    },
    /// `seconds` given for a recipe action (the recipe says how long), or
    /// missing (or 0) for any other.
    Seconds {
        action: String,
    },
    /// `done` missing for a recipe action, or given for any other (their
    /// log lines are built in).
    Done {
        action: String,
    },
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::File(e) => write!(f, "invalid actions file {e}"),
            ActionError::TooMany { count } => {
                write!(f, "{count} actions is too many; the most is {}", u16::MAX)
            }
            ActionError::DuplicateId { id } => write!(f, "action {id:?} is defined twice"),
            ActionError::SameKey { key, first, second } => write!(
                f,
                "actions {first:?} and {second:?} both use the key {key:?}"
            ),
            ActionError::NoReach { action } => {
                write!(f, "action {action:?} needs a reach of at least 1")
            }
            ActionError::Seconds { action } => write!(
                f,
                "action {action:?}: a recipe action takes its time from the recipe, and any other \
                 needs \"seconds\" of at least 1"
            ),
            ActionError::Done { action } => write!(
                f,
                "action {action:?}: a recipe action needs a \"done\" log line, and no other may \
                 have one"
            ),
        }
    }
}

impl std::error::Error for ActionError {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionsFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/actions.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    actions: Vec<Action>,
}

/// Parses and checks every actions file.
pub(crate) fn parse_actions(files: &[ContentFile]) -> Result<Actions, ActionError> {
    let list = read_all(files, |f: ActionsFile| f.actions).map_err(ActionError::File)?;
    if u16::try_from(list.len()).is_err() {
        return Err(ActionError::TooMany { count: list.len() });
    }
    if let Some(id) = first_duplicate(list.iter().map(|a| a.id.as_str())) {
        return Err(ActionError::DuplicateId { id: id.into() });
    }
    check_keys(&list)?;
    list.iter().try_for_each(check_one)?;
    Ok(Actions { list })
}

/// No two actions share a key.
fn check_keys(list: &[Action]) -> Result<(), ActionError> {
    for (i, action) in list.iter().enumerate() {
        if let Some(first) = list[..i].iter().find(|a| a.key == action.key) {
            return Err(ActionError::SameKey {
                key: action.key,
                first: first.id.clone(),
                second: action.id.clone(),
            });
        }
    }
    Ok(())
}

/// One action's own rules: a reach, and `seconds` and `done` exactly where
/// its effect needs them.
fn check_one(action: &Action) -> Result<(), ActionError> {
    let named = || action.id.clone();
    if action.reach == 0 {
        return Err(ActionError::NoReach { action: named() });
    }
    let recipe = action.effect == Effect::Recipe;
    let timed = action.seconds.is_some_and(|s| s > 0);
    if recipe == action.seconds.is_some() || (!recipe && !timed) {
        return Err(ActionError::Seconds { action: named() });
    }
    if recipe != action.done.is_some() {
        return Err(ActionError::Done { action: named() });
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The test actions, in menu order: open, close, cut (reach 2, clear
    /// line, tool from the material), repair (clear of items), pick up
    /// (your own tile too, 2 s per item) and weld (a second tool action).
    pub(crate) const TEST_ACTIONS: &str = r#"{ "actions": [
        { "id": "open", "name": "Open", "key": "o", "reach": 1, "on_bump": true,
          "done": "You open the {was}." },
        { "id": "close", "name": "Close", "key": "c", "reach": 1, "shortcut": true,
          "done": "You close the {now}." },
        { "id": "cut", "name": "Cut", "key": "x", "reach": 2, "clear_line": true,
          "tool_from_material": true, "done": "You cut through the {was}." },
        { "id": "repair", "name": "Repair", "key": "r", "reach": 1, "clear_of_items": true,
          "done": "You repair the {was}." },
        { "id": "pick_up", "name": "Pick up", "key": "g", "reach": 1, "on_own_tile": true,
          "effect": "pick_up", "seconds": 2 },
        { "id": "weld", "name": "Weld", "key": "w", "reach": 1,
          "done": "You weld the {was} into {now}." }
    ] }"#;

    /// The test actions' numbers, for tests that name one.
    pub(crate) const OPEN: u16 = 0;
    pub(crate) const CLOSE: u16 = 1;
    pub(crate) const CUT: u16 = 2;
    pub(crate) const REPAIR: u16 = 3;
    pub(crate) const PICK_UP: u16 = 4;
    pub(crate) const WELD: u16 = 5;

    #[test]
    fn the_named_numbers_match_the_test_actions() {
        let actions = test_actions();
        let named = [("open", OPEN), ("close", CLOSE), ("cut", CUT)];
        let more = [("repair", REPAIR), ("pick_up", PICK_UP), ("weld", WELD)];
        for (id, number) in named.into_iter().chain(more) {
            assert_eq!(actions.index_of(id), Some(number), "{id}");
        }
    }

    pub(crate) fn test_actions() -> Actions {
        parse_actions(&[ContentFile::new("basic.json", TEST_ACTIONS)])
            .expect("test actions are valid")
    }

    fn error_of(json: &str) -> String {
        parse_actions(&[ContentFile::new("basic.json", json)])
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn actions_load_in_menu_order_with_their_rules() {
        let actions = test_actions();
        let ids: Vec<&str> = actions.list().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["open", "close", "cut", "repair", "pick_up", "weld"]);
        assert_eq!(actions.index_of("cut"), Some(2));
        assert_eq!(actions.index_of("dance"), None);
        let cut = actions.get("cut").unwrap();
        assert_eq!((cut.key, cut.reach), ('x', 2));
        assert!(cut.clear_line && cut.tool_from_material && !cut.on_own_tile);
        assert_eq!(cut.effect, Effect::Recipe, "the default effect");
        assert!(actions.get("open").unwrap().on_bump);
        assert!(actions.get("close").unwrap().shortcut);
        assert!(actions.get("dance").is_none());
    }

    #[test]
    fn the_done_line_names_the_structure_before_and_after() {
        let actions = test_actions();
        let weld = actions.get("weld").unwrap();
        assert_eq!(
            weld.done_text("cracked hull", "hull plating"),
            "You weld the cracked hull into hull plating."
        );
        assert_eq!(
            actions.get("close").unwrap().done_text("open door", "door"),
            "You close the door."
        );
        assert_eq!(actions.get("pick_up").unwrap().done_text("a", "b"), "");
    }

    #[test]
    fn no_actions_at_all_is_fine() {
        assert!(parse_actions(&[]).unwrap().list().is_empty());
    }

    #[test]
    fn ids_and_keys_must_be_unique() {
        let twice = TEST_ACTIONS.replace(r#""id": "weld""#, r#""id": "cut""#);
        assert_eq!(error_of(&twice), r#"action "cut" is defined twice"#);
        let same_key = TEST_ACTIONS.replace(r#""key": "w""#, r#""key": "x""#);
        assert_eq!(
            error_of(&same_key),
            r#"actions "cut" and "weld" both use the key 'x'"#
        );
    }

    #[test]
    fn each_action_needs_a_reach_and_its_time_and_text_in_the_right_place() {
        let no_reach =
            TEST_ACTIONS.replace(r#""key": "w", "reach": 1"#, r#""key": "w", "reach": 0"#);
        assert_eq!(
            error_of(&no_reach),
            r#"action "weld" needs a reach of at least 1"#
        );
        let timed_recipe = TEST_ACTIONS.replace(
            r#""key": "w", "reach": 1,"#,
            r#""key": "w", "reach": 1, "seconds": 5,"#,
        );
        assert!(
            error_of(&timed_recipe).starts_with(r#"action "weld": a recipe action takes its time"#)
        );
        let untimed = TEST_ACTIONS.replace(
            r#""effect": "pick_up", "seconds": 2"#,
            r#""effect": "pick_up""#,
        );
        assert!(error_of(&untimed).starts_with(r#"action "pick_up": a recipe action"#));
        let zero = TEST_ACTIONS.replace(
            r#""effect": "pick_up", "seconds": 2"#,
            r#""effect": "pick_up", "seconds": 0"#,
        );
        assert!(error_of(&zero).contains("at least 1"));
        // Swap the repair action's "done" for a field it already has false.
        let silent =
            TEST_ACTIONS.replace(r#""done": "You repair the {was}.""#, r#""on_bump": false"#);
        assert!(
            error_of(&silent).starts_with(r#"action "repair": a recipe action needs a "done""#)
        );
        let chatty = TEST_ACTIONS.replace(
            r#""effect": "pick_up", "seconds": 2"#,
            r#""effect": "pick_up", "seconds": 2, "done": "x""#,
        );
        assert!(error_of(&chatty).starts_with(r#"action "pick_up": a recipe action needs"#));
    }

    #[test]
    fn mistakes_in_the_file_name_it() {
        let typo = TEST_ACTIONS.replace(r#""reach": 2"#, r#""raech": 2"#);
        assert!(error_of(&typo).starts_with("invalid actions file basic.json: "));
        let effect = TEST_ACTIONS.replace(r#""effect": "pick_up""#, r#""effect": "teleport""#);
        assert!(error_of(&effect).contains("teleport"));
    }

    #[test]
    fn too_many_actions_is_an_error() {
        // More than a command's u16 action number can tell apart (the
        // count is checked before anything else, so copies are fine).
        let one = r#"{ "id": "a", "name": "A", "key": "a", "reach": 1, "done": "D." }"#;
        let many = vec![one; usize::from(u16::MAX) + 1].join(",");
        let json = format!(r#"{{ "actions": [{many}] }}"#);
        assert_eq!(
            error_of(&json),
            "65536 actions is too many; the most is 65535"
        );
    }

    #[test]
    fn every_rule_is_part_of_the_hash() {
        let hash = |actions: &[Action]| {
            let mut h = Fnv64::new();
            hash_actions(actions, &mut h);
            h.finish()
        };
        let base = test_actions().list().to_vec();
        let changes: [fn(&mut Action); 8] = [
            |a| a.reach += 1,
            |a| a.clear_line = !a.clear_line,
            |a| a.key = 'z',
            |a| a.effect = Effect::PickUp,
            |a| a.done = None,
            |a| a.on_bump = !a.on_bump,
            |a| a.shortcut = !a.shortcut,
            |a| a.name.push('!'),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed[2]);
            assert_ne!(hash(&base), hash(&changed), "{:?}", changed[2]);
        }
        let ids = [Effect::Recipe, Effect::PickUp].map(Effect::hash_id);
        assert_eq!(ids, [0, 1]);
    }
}
