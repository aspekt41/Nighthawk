//! The terminal front-end for Nighthawk.
//!
//! It turns key presses into sim `Command`s, shows the sim's `Event`s in a
//! message log, and draws the world. It blocks waiting for the next key, so it
//! uses no CPU while idle and the game never moves on its own (decision D7).
//! Leaving it running in tmux for a week is fine.

use std::io;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use args::{Options, Parsed, USAGE};
use saves::SaveSlot;

use nighthawk_sim::{Command, Dir, Event, Sound, World};
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::{DefaultTerminal, Frame};

mod args;
mod content;
mod saves;
mod ui;

/// How many log lines to keep. The log panel shows as many as fit.
const LOG_LIMIT: usize = 100;

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
    let slot = saves::default_save_dir().map(|dir| SaveSlot::in_dir(&dir));

    // Load content and saves before touching the terminal, so a bad file
    // prints a normal error message instead of garbling the screen.
    let app = match start(options, slot) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("nighthawk: {e}");
            return ExitCode::FAILURE;
        }
    };

    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, runs our closure, and restores the terminal afterwards, even if
    // the game panics.
    match ratatui::run(|terminal| app.run(terminal)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nighthawk: terminal error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Builds the app: resumes the saved game if there is one, otherwise starts
/// a new game. `--seed` always starts a new game, because it asks for a
/// specific ship.
fn start(options: Options, slot: Option<SaveSlot>) -> Result<App, String> {
    if options.seed.is_none()
        && let Some(slot) = &slot
        && let Some(world) = slot
            .load()
            .map_err(|e| format!("{e}\nMove or delete that file to start a new game."))?
    {
        return Ok(App::resumed(world, Some(slot.clone())));
    }
    let seed = options.seed.unwrap_or_else(seed_from_clock);
    let world = content::load_start_world(seed).map_err(|e| e.to_string())?;
    Ok(App::new_game(world, slot))
}

/// A seed for a new game when the player didn't pick one: the current time
/// in seconds. Reading the clock is fine here in the front-end. The sim never
/// does it (D18), it just receives the number.
fn seed_from_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What a key press means. Keeping this separate from the sim's `Command`
/// leaves room for UI-only actions (open inventory, look mode) later.
enum Action {
    Sim(Command),
    Save,
    /// Save, then quit. If saving fails, stay so the player sees why.
    Quit,
    QuitWithoutSaving,
}

pub struct App {
    world: World,
    log: Vec<String>,
    quit: bool,
    /// Where the game is saved, or `None` if this machine has no save folder.
    slot: Option<SaveSlot>,
}

impl App {
    /// A fresh game, just started.
    fn new_game(world: World, slot: Option<SaveSlot>) -> Self {
        let seed = world.seed();
        let log = vec![
            format!("Seed {seed}. Run with --seed {seed} to replay this ship."),
            "You wake in the stasis bay. The air is thin and cold.".into(),
            "Emergency lighting flickers. Nothing else is moving.".into(),
        ];
        Self::with_log(world, slot, log)
    }

    /// A game loaded from the save slot.
    fn resumed(world: World, slot: Option<SaveSlot>) -> Self {
        let log = vec![format!(
            "Game loaded: turn {}, seed {}. The ship is as you left it.",
            world.turn(),
            world.seed()
        )];
        Self::with_log(world, slot, log)
    }

    fn with_log(world: World, slot: Option<SaveSlot>, mut log: Vec<String>) -> Self {
        if slot.is_none() {
            log.push(
                "No save folder found (set NIGHTHAWK_SAVE_DIR): this game won't be saved.".into(),
            );
        }
        Self {
            world,
            log,
            quit: false,
            slot,
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
            Action::Save => {
                self.save_game();
            }
            Action::Quit => self.quit = self.save_game(),
            Action::QuitWithoutSaving => self.quit = true,
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

    /// Saves to the slot and logs the result. Returns `false` only if a save
    /// was attempted and failed. With no save folder there's nothing to do.
    fn save_game(&mut self) -> bool {
        let Some(slot) = &self.slot else {
            return true;
        };
        match slot.save(&self.world) {
            Ok(()) => {
                let line = format!("Game saved to {}.", slot.path().display());
                self.push_log(line);
                true
            }
            Err(e) => {
                self.push_log(format!("Could not save: {e}"));
                self.push_log("Press Q to quit without saving.".into());
                false
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
    if let Some(action) = game_action(key) {
        return Some(action);
    }
    KEYMAP
        .iter()
        .find(|(code, _)| *code == key.code)
        .map(|&(_, command)| Action::Sim(command))
}

/// Keys that act on the game itself rather than inside it: `S` saves, `q`
/// (or Ctrl-C, the escape hatch) saves and quits, `Q` quits without saving.
/// Saving keys are capitals so they're hard to hit by accident.
fn game_action(key: KeyEvent) -> Option<Action> {
    let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
    match key.code {
        _ if ctrl_c => Some(Action::Quit),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('Q') => Some(Action::QuitWithoutSaving),
        KeyCode::Char('S') => Some(Action::Save),
        _ => None,
    }
}

/// Turns a sim event into a log line. Ordinary steps aren't worth logging.
fn describe(event: Event) -> Option<String> {
    match event {
        Event::Moved { .. } => None,
        Event::Bumped { tile, .. } => Some(format!("You bump into the {}.", tile.name())),
        Event::BlockedByEdge => Some("There is nowhere to go that way.".into()),
        Event::Waited => Some("You wait.".into()),
        Event::Ambient(sound) => Some(sound_text(sound).into()),
    }
}

/// What each ambient sound sounds like, in the gritty tone of D4.
fn sound_text(sound: Sound) -> &'static str {
    match sound {
        Sound::VentTick => "A ventilation fan ticks somewhere overhead.",
        Sound::HullCreak => "The hull creaks as it cools.",
        Sound::DistantClang => "Something metal clangs, far off.",
        Sound::CoolantDrip => "Coolant drips onto a grating nearby.",
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
        App::new_game(
            content::load_start_world(TEST_SEED).expect("starting map loads"),
            None,
        )
    }

    const TEST_SEED: u64 = 1234;

    /// A new game that saves into a temporary folder. The folder is deleted
    /// when the returned `TempDir` is dropped, so keep it alive in the test.
    fn app_with_slot() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        let world = content::load_start_world(TEST_SEED).unwrap();
        (App::new_game(world, Some(slot)), dir)
    }

    fn slot_in(dir: &tempfile::TempDir) -> SaveSlot {
        SaveSlot::in_dir(dir.path())
    }

    fn last_log(app: &App) -> &str {
        app.log.last().map(String::as_str).unwrap_or("")
    }

    #[test]
    fn capital_s_saves_and_capital_q_quits_without_saving() {
        assert!(matches!(
            action_for(press(KeyCode::Char('S'))),
            Some(Action::Save)
        ));
        assert!(matches!(
            action_for(press(KeyCode::Char('Q'))),
            Some(Action::QuitWithoutSaving)
        ));
    }

    #[test]
    fn save_writes_the_game_and_keeps_playing() {
        let (mut app, dir) = app_with_slot();
        app.handle(Action::Sim(Command::Wait));

        app.handle(Action::Save);

        assert!(!app.quit);
        assert!(
            last_log(&app).starts_with("Game saved to "),
            "{}",
            last_log(&app)
        );
        assert!(last_log(&app).ends_with("save.json."));
        let saved = slot_in(&dir).load().unwrap().unwrap();
        assert_eq!(saved, app.world);
    }

    #[test]
    fn quit_saves_then_stops() {
        let (mut app, dir) = app_with_slot();
        app.handle(Action::Sim(Command::Move(Dir::East)));

        app.handle(Action::Quit);

        assert!(app.quit);
        assert_eq!(slot_in(&dir).load().unwrap().map(|w| w.turn()), Some(1));
    }

    #[test]
    fn a_failed_save_keeps_the_game_open_and_explains() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file_not_folder");
        std::fs::write(&blocker, "").unwrap();
        let world = content::load_start_world(TEST_SEED).unwrap();
        let mut app = App::new_game(world, Some(SaveSlot::in_dir(&blocker)));

        app.handle(Action::Quit);

        assert!(!app.quit, "must not quit when the save failed");
        assert!(app.log.iter().any(|l| l.starts_with("Could not save:")));
        assert_eq!(last_log(&app), "Press Q to quit without saving.");

        app.handle(Action::QuitWithoutSaving);
        assert!(app.quit);
    }

    #[test]
    fn without_a_save_folder_the_game_says_so_and_quits_freely() {
        let mut app = test_app();
        assert!(app.log.iter().any(|l| l.contains("won't be saved")));
        app.handle(Action::Save);
        app.handle(Action::Quit);
        assert!(app.quit);
    }

    #[test]
    fn start_begins_a_new_game_when_there_is_no_save() {
        let dir = tempfile::tempdir().unwrap();
        let app = start(Options { seed: None }, Some(slot_in(&dir))).unwrap();
        assert_eq!(app.world.turn(), 0);
        assert!(app.log[0].starts_with("Seed "));
    }

    #[test]
    fn start_resumes_a_saved_game() {
        let (mut app, dir) = app_with_slot();
        for _ in 0..5 {
            app.handle(Action::Sim(Command::Wait));
        }
        app.handle(Action::Save);

        let resumed = start(Options { seed: None }, Some(slot_in(&dir))).unwrap();

        assert_eq!(resumed.world, app.world);
        assert_eq!(
            resumed.log[0],
            "Game loaded: turn 5, seed 1234. The ship is as you left it."
        );
    }

    #[test]
    fn start_with_a_seed_ignores_the_save() {
        let (mut app, dir) = app_with_slot();
        app.handle(Action::Sim(Command::Wait));
        app.handle(Action::Save);

        let fresh = start(Options { seed: Some(77) }, Some(slot_in(&dir))).unwrap();

        assert_eq!(fresh.world.turn(), 0);
        assert_eq!(fresh.world.seed(), 77);
    }

    #[test]
    fn start_refuses_a_damaged_save_and_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(slot_in(&dir).path(), "{ broken").unwrap();
        let err = start(Options { seed: None }, Some(slot_in(&dir)))
            .err()
            .unwrap();
        assert!(
            err.contains("save.json") && err.contains("Move or delete"),
            "{err}"
        );
    }

    #[test]
    fn start_without_a_save_folder_still_plays() {
        let app = start(Options { seed: Some(3) }, None).unwrap();
        assert_eq!(app.world.seed(), 3);
        assert!(app.slot.is_none());
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
        // "You wait." comes first; an ambient sound may follow it.
        assert_eq!(app.log[log_len], "You wait.");
        assert!(app.log[log_len + 1..].iter().all(|l| is_ambient_line(l)));
    }

    #[test]
    fn plain_steps_only_log_ambient_sounds() {
        let mut app = test_app();
        let log_len = app.log.len();
        for _ in 0..3 {
            app.handle(Action::Sim(Command::Move(Dir::East)));
        }
        assert_eq!(app.world.turn(), 3);
        assert!(app.log[log_len..].iter().all(|l| is_ambient_line(l)));
    }

    fn is_ambient_line(line: &str) -> bool {
        Sound::ALL.iter().any(|&s| sound_text(s) == line)
    }

    #[test]
    fn the_log_starts_with_the_seed_so_a_game_can_be_replayed() {
        let app = test_app();
        assert_eq!(
            app.log[0],
            "Seed 1234. Run with --seed 1234 to replay this ship."
        );
    }

    #[test]
    fn every_ambient_sound_has_its_own_line() {
        let lines: Vec<_> = Sound::ALL
            .iter()
            .map(|&s| describe(Event::Ambient(s)).unwrap())
            .collect();
        for (i, line) in lines.iter().enumerate() {
            assert!(!line.is_empty());
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
    }

    #[test]
    fn the_clock_seed_is_a_plausible_time() {
        // Any time after 2025 means the clock was read, not defaulted to 0.
        assert!(seed_from_clock() > 1_735_689_600);
    }

    #[test]
    fn log_keeps_only_the_newest_lines() {
        let world = content::load_world(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data")
                .as_path(),
            content::START_MAP,
            TEST_SEED,
        )
        .unwrap();
        let mut app = App::new_game(world, None);
        for i in 0..LOG_LIMIT + 10 {
            app.push_log(format!("line {i}"));
        }
        assert_eq!(app.log.len(), LOG_LIMIT);
        assert_eq!(app.log.last().map(String::as_str), Some("line 109"));
    }
}
