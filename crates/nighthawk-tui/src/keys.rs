//! What each key does, on each screen.
//!
//! Plain keys only, so everything works in tmux over SSH with no special
//! terminal setup (D16). Keys are turned into *actions* here, and the app
//! decides what an action does. That keeps this file a simple lookup that is
//! easy to test and, later, to make remappable.

use nighthawk_sim::{Action, Command, Dir, Vertical};
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
    /// Ask which way to close a door; the next key answers.
    BeginClose,
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
}

/// Something the player wants on the inventory screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    /// Highlight the item above or below.
    Up,
    Down,
    /// Drop the highlighted item.
    Drop,
    Close,
}

/// The action for a key pressed on the inventory screen: up and down
/// movement keys choose, `d` drops, Esc, `i` or `q` close.
pub fn inventory_action(key: KeyEvent) -> Option<InventoryAction> {
    if !is_press(key) {
        return None;
    }
    match (key.code, direction(key)) {
        (KeyCode::Esc | KeyCode::Char('i' | 'q'), _) => Some(InventoryAction::Close),
        (KeyCode::Char('d'), _) => Some(InventoryAction::Drop),
        (_, Some(Dir::North)) => Some(InventoryAction::Up),
        (_, Some(Dir::South)) => Some(InventoryAction::Down),
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
    /// Do this action to the tile under the cursor (interact only).
    Do(Action),
    /// Do the first action on the list that can be done (Enter).
    DoFirst,
}

/// The key that does `action` in the interact menu. None of them is a
/// movement key, so moving the cursor and choosing never clash.
pub fn action_key(action: Action) -> char {
    match action {
        Action::Open => 'o',
        Action::Close => 'c',
        Action::Cut => 'x',
        Action::PickUp => 'g',
        Action::Repair => 'r',
    }
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
    ('c', GameAction::BeginClose),
    ('a', GameAction::ToggleOverlay(Overlay::Air)),
    ('t', GameAction::ToggleOverlay(Overlay::Heat)),
    ('V', GameAction::Look),
    ('e', GameAction::Interact),
    ('i', GameAction::Inventory),
];

/// The action for a key pressed while playing, if any. Ctrl-C saves and
/// exits.
pub fn game_action(key: KeyEvent) -> Option<GameAction> {
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
    other.or_else(|| {
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

/// The action for a key pressed while the cursor is up: a movement key
/// moves it; Esc, `V`, `e` or `q` put it away; an action key or Enter
/// chooses. Other keys do nothing, so a stray key can't spend time.
pub fn cursor_action(key: KeyEvent) -> Option<CursorAction> {
    if !is_press(key) {
        return None;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('V' | 'e' | 'q') => Some(CursorAction::Stop),
        KeyCode::Enter => Some(CursorAction::DoFirst),
        code => Action::ALL
            .into_iter()
            .find(|&action| code == KeyCode::Char(action_key(action)))
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

    fn command_for(code: KeyCode) -> Option<Command> {
        match game_action(press(code)) {
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
        assert_eq!(
            game_action(press(KeyCode::Char('S'))),
            Some(GameAction::Save)
        );
        assert_eq!(
            game_action(press(KeyCode::Char('q'))),
            Some(GameAction::ToTitle)
        );
        assert_eq!(
            game_action(press(KeyCode::Char('Q'))),
            Some(GameAction::ToTitleWithoutSaving)
        );
        assert_eq!(game_action(ctrl_c()), Some(GameAction::Exit));
    }

    #[test]
    fn unbound_game_keys_do_nothing() {
        assert_eq!(game_action(press(KeyCode::Char('z'))), None);
        assert_eq!(game_action(press(KeyCode::Tab)), None);
    }

    #[test]
    fn plain_c_starts_closing_a_door_and_is_not_ctrl_c() {
        assert_eq!(
            game_action(press(KeyCode::Char('c'))),
            Some(GameAction::BeginClose)
        );
    }

    #[test]
    fn capital_v_starts_looking() {
        assert_eq!(
            game_action(press(KeyCode::Char('V'))),
            Some(GameAction::Look)
        );
    }

    #[test]
    fn with_the_cursor_up_movement_keys_move_and_esc_v_e_q_stop() {
        assert_eq!(
            cursor_action(press(KeyCode::Char('l'))),
            Some(CursorAction::Move(Dir::East))
        );
        assert_eq!(
            cursor_action(press(KeyCode::Char('y'))),
            Some(CursorAction::Move(Dir::NorthWest))
        );
        for code in [
            KeyCode::Esc,
            KeyCode::Char('V'),
            KeyCode::Char('e'),
            KeyCode::Char('q'),
        ] {
            assert_eq!(
                cursor_action(press(code)),
                Some(CursorAction::Stop),
                "{code:?}"
            );
        }
        assert_eq!(cursor_action(press(KeyCode::Char('.'))), None, "no waiting");
        assert_eq!(cursor_action(release(KeyCode::Esc)), None);
    }

    #[test]
    fn e_starts_interacting_and_o_c_x_enter_choose() {
        assert_eq!(
            game_action(press(KeyCode::Char('e'))),
            Some(GameAction::Interact)
        );
        assert_eq!(
            cursor_action(press(KeyCode::Char('o'))),
            Some(CursorAction::Do(Action::Open))
        );
        assert_eq!(
            cursor_action(press(KeyCode::Char('c'))),
            Some(CursorAction::Do(Action::Close))
        );
        assert_eq!(
            cursor_action(press(KeyCode::Char('x'))),
            Some(CursorAction::Do(Action::Cut))
        );
        assert_eq!(
            cursor_action(press(KeyCode::Enter)),
            Some(CursorAction::DoFirst)
        );
    }

    #[test]
    fn every_action_has_its_own_key_that_is_not_a_movement_key() {
        let keys: Vec<char> = Action::ALL.map(action_key).to_vec();
        assert_eq!(keys, vec!['o', 'c', 'x', 'r', 'g']);
        for key in keys {
            assert_eq!(direction(press(KeyCode::Char(key))), None, "{key}");
        }
    }

    #[test]
    fn a_and_t_toggle_the_air_and_heat_overlays() {
        assert_eq!(
            game_action(press(KeyCode::Char('a'))),
            Some(GameAction::ToggleOverlay(Overlay::Air))
        );
        assert_eq!(
            game_action(press(KeyCode::Char('t'))),
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
        assert_eq!(game_action(release(KeyCode::Char('q'))), None);
        assert_eq!(title_action(release(KeyCode::Enter)), None);
    }

    #[test]
    fn i_opens_the_inventory_where_up_down_choose_and_d_drops() {
        assert_eq!(
            game_action(press(KeyCode::Char('i'))),
            Some(GameAction::Inventory)
        );
        let cases = [
            (KeyCode::Char('k'), Some(InventoryAction::Up)),
            (KeyCode::Up, Some(InventoryAction::Up)),
            (KeyCode::Char('j'), Some(InventoryAction::Down)),
            (KeyCode::Down, Some(InventoryAction::Down)),
            (KeyCode::Char('d'), Some(InventoryAction::Drop)),
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
}
