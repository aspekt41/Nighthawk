//! The terminal front-end for Nighthawk.
//!
//! It turns key presses into sim `Command`s, shows the sim's `Event`s in a
//! message log, and draws the world. It blocks waiting for the next key, so it
//! uses no CPU while idle and the game never moves on its own (decision D7).
//! Leaving it running in tmux for a week is fine.

use std::io;
use std::process::ExitCode;

use nighthawk_sim::{Command, Dir, Event, World};
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::{DefaultTerminal, Frame};

mod content;
mod ui;

/// How many log lines to keep. The log panel shows as many as fit.
const LOG_LIMIT: usize = 100;

fn main() -> ExitCode {
    // Load content before touching the terminal, so a bad map file prints a
    // normal error message instead of garbling the screen.
    let world = match content::load_start_world() {
        Ok(world) => world,
        Err(e) => {
            eprintln!("nighthawk: {e}");
            return ExitCode::FAILURE;
        }
    };

    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, runs our closure, and restores the terminal afterwards, even if
    // the game panics.
    match ratatui::run(|terminal| App::new(world).run(terminal)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nighthawk: terminal error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// What a key press means. Keeping this separate from the sim's `Command`
/// leaves room for UI-only actions (open inventory, look mode) later.
enum Action {
    Sim(Command),
    Quit,
}

pub struct App {
    world: World,
    log: Vec<String>,
    quit: bool,
}

impl App {
    fn new(world: World) -> Self {
        Self {
            world,
            log: vec![
                "You wake in the stasis bay. The air is thin and cold.".into(),
                "Emergency lighting flickers. Nothing else is moving.".into(),
            ],
            quit: false,
        }
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            draw(terminal, |frame| ui::render(&self, frame))?;

            // `event::read` blocks until something happens. A resize just
            // falls through to the next loop pass, which redraws at the new size.
            if let TermEvent::Key(key) = event::read()?
                && let Some(action) = action_for(key)
            {
                self.handle(action);
            }
        }
        Ok(())
    }

    fn handle(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Sim(command) => {
                // Keep only the events worth a log line, then log them.
                let lines: Vec<String> = self
                    .world
                    .apply(command)
                    .into_iter()
                    .filter_map(describe)
                    .collect();
                for line in lines {
                    self.push_log(line);
                }
            }
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            self.log.remove(0);
        }
    }
}

/// Draws one frame inside a synchronized update (DEC mode 2026), so terminals
/// that support it show the whole frame at once instead of tearing. Terminals
/// that don't support it ignore the two escape codes.
fn draw(terminal: &mut DefaultTerminal, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
    execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
    terminal.draw(render)?;
    execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
    Ok(())
}

/// The default keymap. Arrows, vi keys (hjkl + yubn) and the numpad all
/// move. Plain keys only, so it works in tmux over SSH with no special setup.
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
];

/// What a key press means, if anything.
fn action_for(key: KeyEvent) -> Option<Action> {
    // Some terminals also report key releases. Only presses count.
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if is_quit(key) {
        return Some(Action::Quit);
    }
    KEYMAP
        .iter()
        .find(|(code, _)| *code == key.code)
        .map(|&(_, command)| Action::Sim(command))
}

/// `q`, or Ctrl-C as an escape hatch.
fn is_quit(key: KeyEvent) -> bool {
    let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
    ctrl_c || key.code == KeyCode::Char('q')
}

/// Turns a sim event into a log line. Ordinary steps aren't worth logging.
fn describe(event: Event) -> Option<String> {
    match event {
        Event::Moved { .. } => None,
        Event::Bumped { tile, .. } => Some(format!("You bump into the {}.", tile.name())),
        Event::BlockedByEdge => Some("There is nowhere to go that way.".into()),
        Event::Waited => Some("You wait. The ventilation ticks.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nighthawk_sim::{Pos, Tile};
    use ratatui::crossterm::event::KeyEventState;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn command_for(code: KeyCode) -> Option<Command> {
        match action_for(press(code)) {
            Some(Action::Sim(c)) => Some(c),
            _ => None,
        }
    }

    #[test]
    fn arrows_vi_keys_and_numpad_agree() {
        assert_eq!(command_for(KeyCode::Up), Some(Command::Move(Dir::North)));
        assert_eq!(
            command_for(KeyCode::Char('k')),
            Some(Command::Move(Dir::North))
        );
        assert_eq!(
            command_for(KeyCode::Char('8')),
            Some(Command::Move(Dir::North))
        );
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
    fn every_direction_has_a_key() {
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
    fn unbound_keys_do_nothing() {
        assert!(action_for(press(KeyCode::Char('z'))).is_none());
        assert!(action_for(press(KeyCode::Tab)).is_none());
    }

    #[test]
    fn period_and_numpad_five_wait() {
        assert_eq!(command_for(KeyCode::Char('.')), Some(Command::Wait));
        assert_eq!(command_for(KeyCode::Char('5')), Some(Command::Wait));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        assert!(matches!(
            action_for(press(KeyCode::Char('q'))),
            Some(Action::Quit)
        ));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(action_for(ctrl_c), Some(Action::Quit)));
    }

    #[test]
    fn plain_c_does_not_quit() {
        assert!(action_for(press(KeyCode::Char('c'))).is_none());
    }

    #[test]
    fn key_releases_are_ignored() {
        // Terminals with the kitty keyboard protocol also send releases.
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert!(action_for(release).is_none());
    }

    #[test]
    fn ordinary_steps_are_not_logged_but_bumps_are() {
        assert_eq!(
            describe(Event::Moved {
                to: Pos::new(0, 0, 0)
            }),
            None
        );
        let bump = Event::Bumped {
            at: Pos::new(0, 0, 0),
            tile: Tile::Wall,
        };
        assert_eq!(
            describe(bump).as_deref(),
            Some("You bump into the bulkhead.")
        );
    }

    fn test_app() -> App {
        App::new(content::load_start_world().expect("starting map loads"))
    }

    #[test]
    fn quit_action_stops_the_game() {
        let mut app = test_app();
        app.handle(Action::Quit);
        assert!(app.quit);
    }

    #[test]
    fn sim_actions_reach_the_world_and_the_log() {
        let mut app = test_app();
        let log_len = app.log.len();

        app.handle(Action::Sim(Command::Wait));

        assert_eq!(app.world.turn(), 1);
        assert_eq!(app.log.len(), log_len + 1);
        assert_eq!(
            app.log.last().map(String::as_str),
            Some("You wait. The ventilation ticks.")
        );
    }

    #[test]
    fn plain_steps_do_not_add_log_lines() {
        let mut app = test_app();
        let log_len = app.log.len();
        app.handle(Action::Sim(Command::Move(Dir::East)));
        assert_eq!(app.world.turn(), 1);
        assert_eq!(app.log.len(), log_len);
    }

    #[test]
    fn log_keeps_only_the_newest_lines() {
        let world = content::load_world(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data")
                .as_path(),
            content::START_MAP,
        )
        .unwrap();
        let mut app = App::new(world);
        for i in 0..LOG_LIMIT + 10 {
            app.push_log(format!("line {i}"));
        }
        assert_eq!(app.log.len(), LOG_LIMIT);
        assert_eq!(app.log.last().map(String::as_str), Some("line 109"));
    }
}
