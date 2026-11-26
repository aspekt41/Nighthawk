//! The terminal front-end for Nighthawk.
//!
//! It turns key presses into sim `Command`s, shows the sim's `Event`s in a
//! message log, and draws the world. It blocks waiting for the next key, so it
//! uses no CPU while idle and the game never moves on its own (decision D7).
//! Leaving it running in tmux for a week is fine.

use std::io;

use nighthawk_sim::{Command, Dir, Event, World};
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::{DefaultTerminal, Frame};

mod ui;

/// How many log lines to keep. The log panel shows as many as fit.
const LOG_LIMIT: usize = 100;

fn main() -> io::Result<()> {
    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, runs our closure, and restores the terminal afterwards, even if
    // the game panics.
    ratatui::run(|terminal| App::new().run(terminal))
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
    fn new() -> Self {
        Self {
            world: World::stasis_bay(),
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
                for event in self.world.apply(command) {
                    if let Some(line) = describe(event) {
                        self.push_log(line);
                    }
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

/// The keymap. Arrows, vi keys (hjkl + yubn) and the numpad all move. Plain
/// keys only, so it works in tmux over SSH with no special setup.
fn action_for(key: KeyEvent) -> Option<Action> {
    // Some terminals also report key releases. Only presses count.
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }

    let dir = match key.code {
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('8') => Dir::North,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('2') => Dir::South,
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('4') => Dir::West,
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('6') => Dir::East,
        KeyCode::Char('y') | KeyCode::Char('7') => Dir::NorthWest,
        KeyCode::Char('u') | KeyCode::Char('9') => Dir::NorthEast,
        KeyCode::Char('b') | KeyCode::Char('1') => Dir::SouthWest,
        KeyCode::Char('n') | KeyCode::Char('3') => Dir::SouthEast,
        KeyCode::Char('.') | KeyCode::Char('5') => return Some(Action::Sim(Command::Wait)),
        KeyCode::Char('q') => return Some(Action::Quit),
        _ => return None,
    };
    Some(Action::Sim(Command::Move(dir)))
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
