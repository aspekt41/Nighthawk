//! The terminal front-end for Nighthawk.
//!
//! This file only starts things up and runs the loop that reads keys and
//! draws frames. The rest lives in modules:
//!
//! | Module | What it does |
//! |---|---|
//! | `app` | The screens (title menu, playing) and what each action does |
//! | `keys` | Which key means which action |
//! | `title` | The title menu's state |
//! | `ui` | Drawing everything |
//! | `text` | The log's words |
//! | `content` | Finding `data/` and loading maps |
//! | `saves` | Finding the save folder and reading and writing saves |
//! | `args` | Command-line options |
//!
//! The loop blocks waiting for the next key, so it uses no CPU while idle and
//! the game never moves on its own (decision D7).

use std::io;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event as TermEvent};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::{DefaultTerminal, Frame};

use app::App;
use args::{Parsed, USAGE};
use saves::SaveSlot;

mod app;
mod args;
mod content;
mod keys;
mod saves;
mod text;
mod title;
mod ui;

fn main() -> ExitCode {
    let options = match args::parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Run(options)) => options,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("nighthawk: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    // Load content before touching the terminal, so a bad map file prints a
    // normal error message instead of garbling the screen.
    let start_map = match content::load_start_map() {
        Ok(map) => map,
        Err(e) => {
            eprintln!("nighthawk: {e}");
            return ExitCode::FAILURE;
        }
    };
    let slot = saves::default_save_dir().map(|dir| SaveSlot::in_dir(&dir));
    let app = App::new(start_map, slot, options.seed, seed_from_clock);

    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, runs our closure, and restores the terminal afterwards, even if
    // the game panics.
    match ratatui::run(|terminal| run(app, terminal)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nighthawk: terminal error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Draw, wait for a key, react; until the app says to quit.
fn run(mut app: App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.quit {
        draw(terminal, |frame| ui::render(&app, frame))?;
        // `event::read` blocks until something happens. A resize just falls
        // through to the next loop pass, which redraws at the new size.
        if let TermEvent::Key(key) = event::read()? {
            app.handle_key(key);
        }
    }
    Ok(())
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

/// A seed for a new game when the player didn't pick one: the current time
/// in seconds. Reading the clock is fine here in the front-end. The sim never
/// does it (D18), it just receives the number.
fn seed_from_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_seed_is_a_plausible_time() {
        // Any time after 2025 means the clock was read, not defaulted to 0.
        assert!(seed_from_clock() > 1_735_689_600);
    }
}
