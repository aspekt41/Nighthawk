//! What each key does, on each screen.
//!
//! Plain keys only, so everything works in tmux over SSH with no special
//! terminal setup (D16). Keys are turned into *actions* here, and the app
//! decides what an action does. That keeps this file a simple lookup that is
//! easy to test and, later, to make remappable.

use nighthawk_sim::{Command, Dir, Vertical};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::title::MenuItem;

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
/// yubn) and the numpad all move; `<` and `>` climb up and down.
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
    (KeyCode::Char('<'), Command::Climb(Vertical::Up)),
    (KeyCode::Char('>'), Command::Climb(Vertical::Down)),
];

/// The action for a key pressed while playing, if any.
///
/// `S` saves, `q` saves and returns to the menu, `Q` returns without saving,
/// and Ctrl-C saves and exits. The saving keys are capitals so they're hard
/// to hit by accident.
pub fn game_action(key: KeyEvent) -> Option<GameAction> {
    if !is_press(key) {
        return None;
    }
    if is_ctrl_c(key) {
        return Some(GameAction::Exit);
    }
    match key.code {
        KeyCode::Char('q') => Some(GameAction::ToTitle),
        KeyCode::Char('Q') => Some(GameAction::ToTitleWithoutSaving),
        KeyCode::Char('S') => Some(GameAction::Save),
        code => KEYMAP
            .iter()
            .find(|(bound, _)| *bound == code)
            .map(|&(_, command)| GameAction::Sim(command)),
    }
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

/// Some terminals also report key releases and repeats. Only presses count.
fn is_press(key: KeyEvent) -> bool {
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
    fn angle_brackets_climb() {
        assert_eq!(
            command_for(KeyCode::Char('<')),
            Some(Command::Climb(Vertical::Up))
        );
        assert_eq!(
            command_for(KeyCode::Char('>')),
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
        assert_eq!(
            game_action(press(KeyCode::Char('c'))),
            None,
            "plain c isn't Ctrl-C"
        );
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
}
