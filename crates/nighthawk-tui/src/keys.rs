//! What each key does, on each screen.
//!
//! Plain keys only, so everything works in tmux over SSH with no special
//! terminal setup (D16). Keys are turned into *actions* here, and the app
//! decides what an action does. That keeps this file a simple lookup that is
//! easy to test and, later, to make remappable.

use nighthawk_sim::{Action, Command, Dir, PowerAct, Vertical};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::title::MenuItem;
use crate::ui::Overlay;

/// Something the player wants while playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAction {
    /// Do something in the world: move, wait.
    Sim(Command),
    Save,
    /// Save, then go back to the title menu.
    ToTitle,
    /// Back to the title menu without saving.
    ToTitleWithoutSaving,
    /// Save, then close the program (Ctrl-C).
    Exit,
    /// Ask which way to do action number `.0`, a shortcut action (`c` to
    /// close a door); the next key answers.
    BeginShortcut(u16),
    /// Show or hide an overlay (`a` air, `t` heat). Only changes the view;
    /// no time passes.
    ToggleOverlay(Overlay),
    /// Start look mode (`V`): a cursor to inspect any tile.
    Look,
    /// Start the interact menu (`e`): the same cursor, listing what you
    /// can do to the tile under it.
    Interact,
    /// Show what you're wearing and carrying (`i`).
    Inventory,
    /// Show what you know how to make (`C`).
    Crafting,
}

/// Something the player wants on the inventory screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    /// Highlight the item above or below.
    Up,
    Down,
    /// Drop the highlighted item.
    Drop,
    /// Put the highlighted item into a container within reach.
    PutAway,
    /// Load a cell into the highlighted item, take its cell out, or
    /// switch it on or off.
    Power(PowerAct),
    Close,
}

/// The action for a key pressed on the inventory screen: up and down
/// movement keys choose, `d` drops, `p` puts away, `r` loads a cell, `u`
/// takes it out, `a` switches on or off, and Esc, `i` or `q` close. The
/// letter keys come first, so `u` means unload here, not north-east.
pub fn inventory_action(key: KeyEvent) -> Option<InventoryAction> {
    if !is_press(key) {
        return None;
    }
    match (key.code, direction(key)) {
        (KeyCode::Esc | KeyCode::Char('i' | 'q'), _) => Some(InventoryAction::Close),
        (KeyCode::Char('d'), _) => Some(InventoryAction::Drop),
        (KeyCode::Char('p'), _) => Some(InventoryAction::PutAway),
        (KeyCode::Char('r'), _) => Some(InventoryAction::Power(PowerAct::Reload)),
        (KeyCode::Char('u'), _) => Some(InventoryAction::Power(PowerAct::Unload)),
        (KeyCode::Char('a'), _) => Some(InventoryAction::Power(PowerAct::Switch)),
        (_, Some(Dir::North)) => Some(InventoryAction::Up),
        (_, Some(Dir::South)) => Some(InventoryAction::Down),
        _ => None,
    }
}

/// Something the player wants on the crafting screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftingAction {
    /// Highlight the craft above or below.
    Up,
    Down,
    /// Make the highlighted craft.
    Make,
    Close,
}

/// The action for a key pressed on the crafting screen: up and down
/// movement keys choose, Enter makes, Esc, `C` or `q` close.
pub fn crafting_action(key: KeyEvent) -> Option<CraftingAction> {
    if !is_press(key) {
        return None;
    }
    match (key.code, direction(key)) {
        (KeyCode::Esc | KeyCode::Char('C' | 'q'), _) => Some(CraftingAction::Close),
        (KeyCode::Enter, _) => Some(CraftingAction::Make),
        (_, Some(Dir::North)) => Some(CraftingAction::Up),
        (_, Some(Dir::South)) => Some(CraftingAction::Down),
        _ => None,
    }
}

/// Something the player wants while the look or interact cursor is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorAction {
    /// Move the cursor one tile.
    Move(Dir),
    /// Put the cursor away.
    Stop,
    /// Do action number `.0` to the tile under the cursor (interact only).
    Do(u16),
    /// Do the first action on the list that can be done (Enter).
    DoFirst,
}

/// The number of the action in `actions` whose key is `code`, if any.
fn action_for(code: KeyCode, actions: &[Action]) -> Option<u16> {
    (0..)
        .zip(actions)
        .find(|(_, action)| code == KeyCode::Char(action.key))
        .map(|(number, _)| number)
}

/// Checks the actions' keys (from `data/actions/`) can't be confused with
/// the game's own: none may be a movement key or one that puts the cursor
/// away, and a shortcut action's key, which also works while playing, may
/// not be one of the other keys used then. Two actions sharing a key is
/// already a content error.
pub fn check_action_keys(actions: &[Action]) -> Result<(), String> {
    for action in actions {
        let key = action.key;
        let code = KeyCode::Char(key);
        let taken = if KEYMAP.iter().any(|&(bound, _)| bound == code) {
            Some("a movement key")
        } else if CURSOR_STOP_KEYS.contains(&key) {
            Some("a key that puts the cursor away")
        } else if action.shortcut && GAME_KEYS.iter().any(|&(c, _)| c == key) {
            Some("already used while playing")
        } else {
            None
        };
        if let Some(why) = taken {
            return Err(format!(
                "action {:?} uses the key {key:?}, which is {why}",
                action.id
            ));
        }
    }
    Ok(())
}

/// Something the player wants on the title menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    Up,
    Down,
    /// Choose the highlighted item.
    Choose,
    /// Choose an item directly by its hotkey.
    Pick(MenuItem),
    /// Close the program (Ctrl-C).
    Exit,
}

/// Movement, waiting and climbing while playing. Arrows, vi keys (hjkl +
/// yubn) and the numpad all move; `>` climbs up and `<` climbs down, the
/// same characters the map draws ladders with (decision D31).
///
/// It's a table rather than a big `match`, so adding a key is one line, and
/// later the table can be loaded from a settings file to make keys remappable.
const KEYMAP: &[(KeyCode, Command)] = &[
    (KeyCode::Up, Command::Move(Dir::North)),
    (KeyCode::Char('k'), Command::Move(Dir::North)),
    (KeyCode::Char('8'), Command::Move(Dir::North)),
    (KeyCode::Down, Command::Move(Dir::South)),
    (KeyCode::Char('j'), Command::Move(Dir::South)),
    (KeyCode::Char('2'), Command::Move(Dir::South)),
    (KeyCode::Left, Command::Move(Dir::West)),
    (KeyCode::Char('h'), Command::Move(Dir::West)),
    (KeyCode::Char('4'), Command::Move(Dir::West)),
    (KeyCode::Right, Command::Move(Dir::East)),
    (KeyCode::Char('l'), Command::Move(Dir::East)),
    (KeyCode::Char('6'), Command::Move(Dir::East)),
    (KeyCode::Char('y'), Command::Move(Dir::NorthWest)),
    (KeyCode::Char('7'), Command::Move(Dir::NorthWest)),
    (KeyCode::Char('u'), Command::Move(Dir::NorthEast)),
    (KeyCode::Char('9'), Command::Move(Dir::NorthEast)),
    (KeyCode::Char('b'), Command::Move(Dir::SouthWest)),
    (KeyCode::Char('1'), Command::Move(Dir::SouthWest)),
    (KeyCode::Char('n'), Command::Move(Dir::SouthEast)),
    (KeyCode::Char('3'), Command::Move(Dir::SouthEast)),
    (KeyCode::Char('.'), Command::Wait),
    (KeyCode::Char('5'), Command::Wait),
    (KeyCode::Char('>'), Command::Climb(Vertical::Up)),
    (KeyCode::Char('<'), Command::Climb(Vertical::Down)),
];

/// The keys that aren't moves, while playing. `S` saves, `q` saves and
/// returns to the menu, `Q` returns without saving. The saving keys are
/// capitals so they're hard to hit by accident. A table, like `KEYMAP`, so
/// adding a key is one line.
const GAME_KEYS: &[(char, GameAction)] = &[
    ('q', GameAction::ToTitle),
    ('Q', GameAction::ToTitleWithoutSaving),
    ('S', GameAction::Save),
    ('a', GameAction::ToggleOverlay(Overlay::Air)),
    ('t', GameAction::ToggleOverlay(Overlay::Heat)),
    ('V', GameAction::Look),
    ('e', GameAction::Interact),
    ('i', GameAction::Inventory),
    ('C', GameAction::Crafting),
];

/// The action for a key pressed while playing, if any: the game's own
/// keys, then a shortcut action's key (from `actions`), then moving.
/// Ctrl-C saves and exits.
pub fn game_action(key: KeyEvent, actions: &[Action]) -> Option<GameAction> {
    if !is_press(key) {
        return None;
    }
    if is_ctrl_c(key) {
        return Some(GameAction::Exit);
    }
    let other = GAME_KEYS
        .iter()
        .find(|&&(c, _)| key.code == KeyCode::Char(c))
        .map(|&(_, action)| action);
    let shortcut = || {
        action_for(key.code, actions)
            .filter(|&n| actions[usize::from(n)].shortcut)
            .map(GameAction::BeginShortcut)
    };
    other.or_else(shortcut).or_else(|| {
        KEYMAP
            .iter()
            .find(|(bound, _)| *bound == key.code)
            .map(|&(_, command)| GameAction::Sim(command))
    })
}

/// The action for a key pressed on the title menu, if any.
pub fn title_action(key: KeyEvent) -> Option<TitleAction> {
    if !is_press(key) {
        return None;
    }
    if is_ctrl_c(key) {
        return Some(TitleAction::Exit);
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(TitleAction::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(TitleAction::Down),
        KeyCode::Enter => Some(TitleAction::Choose),
        code => MenuItem::ALL
            .into_iter()
            .find(|item| code == KeyCode::Char(item.hotkey()))
            .map(TitleAction::Pick),
    }
}

/// The keys (besides Esc) that put the look or interact cursor away.
const CURSOR_STOP_KEYS: [char; 3] = ['V', 'e', 'q'];

/// The action for a key pressed while the cursor is up: a movement key
/// moves it; Esc, `V`, `e` or `q` put it away; an action's key (from
/// `actions`) or Enter chooses. Other keys do nothing, so a stray key
/// can't spend time.
pub fn cursor_action(key: KeyEvent, actions: &[Action]) -> Option<CursorAction> {
    if !is_press(key) {
        return None;
    }
    match key.code {
        KeyCode::Esc => Some(CursorAction::Stop),
        KeyCode::Char(c) if CURSOR_STOP_KEYS.contains(&c) => Some(CursorAction::Stop),
        KeyCode::Enter => Some(CursorAction::DoFirst),
        code => action_for(code, actions)
            .map(CursorAction::Do)
            .or_else(|| direction(key).map(CursorAction::Move)),
    }
}

/// The direction a key means when the game asks "which way?": any of the
/// movement keys (arrows, vi keys, numpad).
pub fn direction(key: KeyEvent) -> Option<Dir> {
    KEYMAP.iter().find_map(|&(code, command)| match command {
        Command::Move(dir) if code == key.code => Some(dir),
        _ => None,
    })
}

/// Some terminals also report key releases and repeats. Only presses count.
pub fn is_press(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventState;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        }
    }

    /// The shipped actions, from `data/actions/`.
    fn actions() -> Vec<Action> {
        let map = crate::content::load_start_map().unwrap();
        map.content.actions().to_vec()
    }

    /// The number of the shipped action with this id.
    fn number(id: &str) -> u16 {
        let map = crate::content::load_start_map().unwrap();
        map.content.action_index(id).unwrap()
    }

    fn game(key: KeyEvent) -> Option<GameAction> {
        game_action(key, &actions())
    }

    fn cursor(key: KeyEvent) -> Option<CursorAction> {
        cursor_action(key, &actions())
    }

    fn command_for(code: KeyCode) -> Option<Command> {
        match game(press(code)) {
            Some(GameAction::Sim(c)) => Some(c),
            _ => None,
        }
    }

    #[test]
    fn arrows_vi_keys_and_numpad_agree() {
        for code in [KeyCode::Up, KeyCode::Char('k'), KeyCode::Char('8')] {
            assert_eq!(command_for(code), Some(Command::Move(Dir::North)));
        }
        assert_eq!(
            command_for(KeyCode::Char('y')),
            Some(Command::Move(Dir::NorthWest))
        );
        assert_eq!(
            command_for(KeyCode::Char('3')),
            Some(Command::Move(Dir::SouthEast))
        );
    }

    #[test]
    fn every_direction_has_a_vi_key() {
        let keys = ['k', 'u', 'l', 'n', 'j', 'b', 'h', 'y'];
        let dirs: Vec<_> = keys
            .iter()
            .filter_map(|&c| command_for(KeyCode::Char(c)))
            .collect();
        assert_eq!(dirs.len(), 8);
    }

    #[test]
    fn no_key_is_bound_twice() {
        for (i, (code, _)) in KEYMAP.iter().enumerate() {
            let dupes = KEYMAP[i + 1..].iter().filter(|(c, _)| c == code).count();
            assert_eq!(dupes, 0, "{code:?} is bound more than once");
        }
    }

    #[test]
    fn greater_than_climbs_up_and_less_than_climbs_down() {
        assert_eq!(
            command_for(KeyCode::Char('>')),
            Some(Command::Climb(Vertical::Up))
        );
        assert_eq!(
            command_for(KeyCode::Char('<')),
            Some(Command::Climb(Vertical::Down))
        );
    }

    #[test]
    fn period_and_numpad_five_wait() {
        assert_eq!(command_for(KeyCode::Char('.')), Some(Command::Wait));
        assert_eq!(command_for(KeyCode::Char('5')), Some(Command::Wait));
    }

    #[test]
    fn game_keys_for_saving_and_leaving() {
        assert_eq!(game(press(KeyCode::Char('S'))), Some(GameAction::Save));
        assert_eq!(game(press(KeyCode::Char('q'))), Some(GameAction::ToTitle));
        assert_eq!(
            game(press(KeyCode::Char('Q'))),
            Some(GameAction::ToTitleWithoutSaving)
        );
        assert_eq!(game(ctrl_c()), Some(GameAction::Exit));
    }

    #[test]
    fn unbound_game_keys_do_nothing() {
        assert_eq!(game(press(KeyCode::Char('z'))), None);
        assert_eq!(game(press(KeyCode::Tab)), None);
    }

    #[test]
    fn plain_c_starts_closing_a_door_and_is_not_ctrl_c() {
        // `c` is the shipped close action's key, and it's a shortcut.
        assert_eq!(
            game(press(KeyCode::Char('c'))),
            Some(GameAction::BeginShortcut(number("close")))
        );
        assert_eq!(
            game(press(KeyCode::Char('x'))),
            None,
            "cutting isn't a shortcut: only the menu does it"
        );
    }

    #[test]
    fn capital_v_starts_looking() {
        assert_eq!(game(press(KeyCode::Char('V'))), Some(GameAction::Look));
    }

    #[test]
    fn with_the_cursor_up_movement_keys_move_and_esc_v_e_q_stop() {
        assert_eq!(
            cursor(press(KeyCode::Char('l'))),
            Some(CursorAction::Move(Dir::East))
        );
        assert_eq!(
            cursor(press(KeyCode::Char('y'))),
            Some(CursorAction::Move(Dir::NorthWest))
        );
        for code in [
            KeyCode::Esc,
            KeyCode::Char('V'),
            KeyCode::Char('e'),
            KeyCode::Char('q'),
        ] {
            assert_eq!(cursor(press(code)), Some(CursorAction::Stop), "{code:?}");
        }
        assert_eq!(cursor(press(KeyCode::Char('.'))), None, "no waiting");
        assert_eq!(cursor(release(KeyCode::Esc)), None);
    }

    #[test]
    fn e_starts_interacting_and_o_c_x_enter_choose() {
        assert_eq!(game(press(KeyCode::Char('e'))), Some(GameAction::Interact));
        assert_eq!(
            cursor(press(KeyCode::Char('o'))),
            Some(CursorAction::Do(number("open")))
        );
        assert_eq!(
            cursor(press(KeyCode::Char('c'))),
            Some(CursorAction::Do(number("close")))
        );
        assert_eq!(
            cursor(press(KeyCode::Char('x'))),
            Some(CursorAction::Do(number("cut")))
        );
        assert_eq!(cursor(press(KeyCode::Enter)), Some(CursorAction::DoFirst));
    }

    #[test]
    fn the_shipped_action_keys_are_the_agreed_ones_and_clash_with_nothing() {
        let actions = actions();
        let keys: Vec<char> = actions.iter().map(|a| a.key).collect();
        assert_eq!(keys, vec!['o', 'c', 'x', 'r', 'g', 't', 's', 'd', 'p']);
        assert_eq!(check_action_keys(&actions), Ok(()));
    }

    #[test]
    fn action_keys_may_not_be_movement_cursor_or_playing_keys() {
        let with_key = |key: char, shortcut: bool| {
            let mut actions = actions();
            actions[2].key = key;
            actions[2].shortcut = shortcut;
            check_action_keys(&actions)
        };
        assert_eq!(
            with_key('h', false),
            Err(r#"action "cut" uses the key 'h', which is a movement key"#.into())
        );
        assert!(
            with_key('V', false)
                .unwrap_err()
                .contains("puts the cursor away")
        );
        assert_eq!(with_key('S', false), Ok(()), "the menu doesn't save");
        assert!(
            with_key('S', true)
                .unwrap_err()
                .contains("already used while playing")
        );
    }

    #[test]
    fn a_and_t_toggle_the_air_and_heat_overlays() {
        assert_eq!(
            game(press(KeyCode::Char('a'))),
            Some(GameAction::ToggleOverlay(Overlay::Air))
        );
        assert_eq!(
            game(press(KeyCode::Char('t'))),
            Some(GameAction::ToggleOverlay(Overlay::Heat))
        );
    }

    #[test]
    fn every_movement_key_is_also_a_direction() {
        for &(code, command) in KEYMAP {
            if let Command::Move(dir) = command {
                assert_eq!(direction(press(code)), Some(dir), "{code:?}");
            }
        }
    }

    #[test]
    fn non_movement_keys_are_not_directions() {
        for code in [
            KeyCode::Char('.'),
            KeyCode::Char('<'),
            KeyCode::Char('z'),
            KeyCode::Esc,
        ] {
            assert_eq!(direction(press(code)), None, "{code:?}");
        }
    }

    #[test]
    fn title_menu_keys() {
        assert_eq!(title_action(press(KeyCode::Up)), Some(TitleAction::Up));
        assert_eq!(
            title_action(press(KeyCode::Char('k'))),
            Some(TitleAction::Up)
        );
        assert_eq!(title_action(press(KeyCode::Down)), Some(TitleAction::Down));
        assert_eq!(
            title_action(press(KeyCode::Char('j'))),
            Some(TitleAction::Down)
        );
        assert_eq!(
            title_action(press(KeyCode::Enter)),
            Some(TitleAction::Choose)
        );
        assert_eq!(title_action(ctrl_c()), Some(TitleAction::Exit));
    }

    #[test]
    fn every_menu_item_has_a_working_hotkey() {
        for item in MenuItem::ALL {
            let key = press(KeyCode::Char(item.hotkey()));
            assert_eq!(title_action(key), Some(TitleAction::Pick(item)));
        }
    }

    #[test]
    fn unbound_title_keys_do_nothing() {
        assert_eq!(title_action(press(KeyCode::Char('z'))), None);
        assert_eq!(title_action(press(KeyCode::Left)), None);
    }

    #[test]
    fn key_releases_are_ignored_everywhere() {
        // Terminals with the kitty keyboard protocol also send releases.
        assert_eq!(game(release(KeyCode::Char('q'))), None);
        assert_eq!(title_action(release(KeyCode::Enter)), None);
    }

    #[test]
    fn i_opens_the_inventory_where_up_down_choose_and_d_drops() {
        assert_eq!(game(press(KeyCode::Char('i'))), Some(GameAction::Inventory));
        let cases = [
            (KeyCode::Char('k'), Some(InventoryAction::Up)),
            (KeyCode::Up, Some(InventoryAction::Up)),
            (KeyCode::Char('j'), Some(InventoryAction::Down)),
            (KeyCode::Down, Some(InventoryAction::Down)),
            (KeyCode::Char('d'), Some(InventoryAction::Drop)),
            (KeyCode::Char('p'), Some(InventoryAction::PutAway)),
            (
                KeyCode::Char('r'),
                Some(InventoryAction::Power(PowerAct::Reload)),
            ),
            (
                KeyCode::Char('u'),
                Some(InventoryAction::Power(PowerAct::Unload)),
            ),
            (
                KeyCode::Char('a'),
                Some(InventoryAction::Power(PowerAct::Switch)),
            ),
            (KeyCode::Esc, Some(InventoryAction::Close)),
            (KeyCode::Char('i'), Some(InventoryAction::Close)),
            (KeyCode::Char('q'), Some(InventoryAction::Close)),
            (KeyCode::Char('l'), None),
            (KeyCode::Char('.'), None),
        ];
        for (code, action) in cases {
            assert_eq!(inventory_action(press(code)), action, "{code:?}");
        }
        assert_eq!(inventory_action(release(KeyCode::Char('d'))), None);
    }

    #[test]
    fn capital_c_opens_crafting_where_up_down_choose_and_enter_makes() {
        assert_eq!(game(press(KeyCode::Char('C'))), Some(GameAction::Crafting));
        let cases = [
            (KeyCode::Char('k'), Some(CraftingAction::Up)),
            (KeyCode::Up, Some(CraftingAction::Up)),
            (KeyCode::Char('j'), Some(CraftingAction::Down)),
            (KeyCode::Down, Some(CraftingAction::Down)),
            (KeyCode::Enter, Some(CraftingAction::Make)),
            (KeyCode::Esc, Some(CraftingAction::Close)),
            (KeyCode::Char('C'), Some(CraftingAction::Close)),
            (KeyCode::Char('q'), Some(CraftingAction::Close)),
            (KeyCode::Char('l'), None),
            (KeyCode::Char('d'), None),
        ];
        for (code, action) in cases {
            assert_eq!(crafting_action(press(code)), action, "{code:?}");
        }
        assert_eq!(crafting_action(release(KeyCode::Enter)), None);
    }
}
