//! The app's state and what each action does.
//!
//! The app is always on one of two screens: the title menu, or playing a
//! game. Keys become actions (`keys.rs`), and this file decides what each
//! action means on the current screen. There's no terminal code here, so all
//! of it can be tested directly.

use nighthawk_sim::{Command, LoadedMap, World};
use ratatui::crossterm::event::KeyEvent;

use crate::keys::{self, GameAction, TitleAction};
use crate::saves::SaveSlot;
use crate::text;
use crate::title::{MenuItem, TitleMenu};

/// How many log lines to keep. The log panel shows as many as fit.
pub const LOG_LIMIT: usize = 100;

/// Which screen is showing. Each variant holds that screen's state, so a
/// game in progress only exists while you're playing it.
#[derive(Debug)]
pub enum Screen {
    Title(TitleMenu),
    Playing(World),
}

pub struct App {
    pub screen: Screen,
    /// The message log of the current game.
    pub log: Vec<String>,
    /// Set when the program should close.
    pub quit: bool,
    /// Where the game is saved, or `None` if this machine has no save folder.
    slot: Option<SaveSlot>,
    /// The map every new game starts on, loaded once at startup.
    start_map: LoadedMap,
    /// Picks the seed for a new game. In the real game it reads the clock;
    /// tests pass a function that returns a fixed number.
    new_seed: fn() -> u64,
    /// True after `c`, while the game waits for the direction to close.
    asking_close_direction: bool,
    /// Whether the map shows the air overlay. A view setting, not part of
    /// the game, so it isn't saved.
    pub show_air: bool,
}

impl App {
    /// The app as it opens. With `seed` (from `--seed`), it goes straight
    /// into a new game on that seed; otherwise it shows the title menu.
    pub fn new(
        start_map: LoadedMap,
        slot: Option<SaveSlot>,
        seed: Option<u64>,
        new_seed: fn() -> u64,
    ) -> Self {
        let mut app = Self {
            screen: Screen::Title(TitleMenu::new(false)),
            log: Vec::new(),
            quit: false,
            slot,
            start_map,
            new_seed,
            asking_close_direction: false,
            show_air: false,
        };
        match seed {
            Some(seed) => app.start_new_game(seed),
            None => app.show_title(None),
        }
        app
    }

    /// Reacts to a key press on whichever screen is showing.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Releases and repeats never count, so they can't answer a question.
        if !keys::is_press(key) {
            return;
        }
        if self.asking_close_direction {
            self.answer_close_direction(key);
            return;
        }
        match self.screen {
            Screen::Title(_) => {
                if let Some(action) = keys::title_action(key) {
                    self.on_title(action);
                }
            }
            Screen::Playing(_) => {
                if let Some(action) = keys::game_action(key) {
                    self.on_game(action);
                }
            }
        }
    }

    pub fn on_title(&mut self, action: TitleAction) {
        let Screen::Title(menu) = &mut self.screen else {
            return;
        };
        match action {
            TitleAction::Up => menu.up(),
            TitleAction::Down => menu.down(),
            TitleAction::Choose => {
                let item = menu.selected();
                self.choose(item);
            }
            // A hotkey for a greyed-out item does nothing.
            TitleAction::Pick(item) if menu.is_enabled(item) => self.choose(item),
            TitleAction::Pick(_) => {}
            TitleAction::Exit => self.quit = true,
        }
    }

    pub fn on_game(&mut self, action: GameAction) {
        match action {
            GameAction::Sim(command) => self.apply(command),
            GameAction::Save => {
                self.save_game();
            }
            GameAction::ToTitle => {
                if self.save_game() {
                    self.show_title(None);
                }
            }
            GameAction::ToTitleWithoutSaving => self.show_title(None),
            GameAction::Exit => self.quit = self.save_game(),
            GameAction::ToggleAirOverlay => self.show_air = !self.show_air,
            GameAction::BeginClose => {
                self.asking_close_direction = true;
                self.push_log("Close which way? (a direction key; anything else cancels)".into());
            }
        }
    }

    /// The key after `c`: a direction closes the door that way; any other
    /// key cancels, costing nothing.
    fn answer_close_direction(&mut self, key: KeyEvent) {
        self.asking_close_direction = false;
        match keys::direction(key) {
            Some(dir) => self.apply(Command::Close(dir)),
            None => self.push_log("Never mind.".into()),
        }
    }

    fn choose(&mut self, item: MenuItem) {
        match item {
            MenuItem::NewGame => self.start_new_game((self.new_seed)()),
            MenuItem::Continue => self.continue_game(),
            MenuItem::Quit => self.quit = true,
        }
    }

    fn start_new_game(&mut self, seed: u64) {
        let world = World::new(self.start_map.clone(), seed);
        self.log = vec![
            format!("Seed {seed}. Run with --seed {seed} to replay this ship."),
            "You wake in the stasis bay. The air is thin and cold.".into(),
            "Emergency lighting flickers. Nothing else is moving.".into(),
        ];
        self.warn_if_unsaved();
        self.screen = Screen::Playing(world);
    }

    /// Loads the save. If that fails, stays on the title menu and says why,
    /// so a damaged save never stops you from starting a new game.
    fn continue_game(&mut self) {
        let loaded = match &self.slot {
            None => Err("This machine has no save folder.".to_string()),
            Some(slot) => slot.load().map_err(|e| format!("Could not load: {e}")),
        };
        match loaded {
            Ok(Some(world)) => {
                self.log = vec![format!(
                    "Game loaded at {}, seed {}. The ship is as you left it.",
                    crate::ui::clock(world.seconds()),
                    world.seed()
                )];
                self.screen = Screen::Playing(world);
            }
            Ok(None) => self.show_title(Some("There's no saved game yet.".into())),
            Err(message) => self.show_title(Some(message)),
        }
    }

    fn show_title(&mut self, message: Option<String>) {
        let can_continue = self.slot.as_ref().is_some_and(SaveSlot::exists);
        self.screen = Screen::Title(TitleMenu::new(can_continue).with_message(message));
    }

    fn warn_if_unsaved(&mut self) {
        if self.slot.is_none() {
            self.push_log(
                "No save folder found (set NIGHTHAWK_SAVE_DIR): this game won't be saved.".into(),
            );
        }
    }

    fn apply(&mut self, command: Command) {
        let Screen::Playing(world) = &mut self.screen else {
            return;
        };
        // Keep only the events worth a log line, then log them.
        let lines: Vec<String> = world
            .apply(command)
            .into_iter()
            .filter_map(text::describe)
            .collect();
        for line in lines {
            self.push_log(line);
        }
    }

    /// Saves the current game and logs the result. Returns `false` only if a
    /// save was attempted and failed; with no game or no save folder there's
    /// nothing to do, which counts as success.
    fn save_game(&mut self) -> bool {
        let (Screen::Playing(world), Some(slot)) = (&self.screen, &self.slot) else {
            return true;
        };
        // Build the message first: `slot` borrows `self`, and `push_log`
        // needs to change `self`, so the borrow has to end before logging.
        let result = match slot.save(world) {
            Ok(()) => Ok(format!("Game saved to {}.", slot.path().display())),
            Err(e) => Err(format!("Could not save: {e}")),
        };
        match result {
            Ok(line) => {
                self.push_log(line);
                true
            }
            Err(line) => {
                self.push_log(line);
                self.push_log("Press Q to leave without saving.".into());
                false
            }
        }
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            self.log.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::load_start_map;
    use crate::keys::GameAction::{Exit, Save, Sim, ToTitle, ToTitleWithoutSaving};
    use nighthawk_sim::{Dir, Sound};

    const CLOCK_SEED: u64 = 1234;

    fn fixed_seed() -> u64 {
        CLOCK_SEED
    }

    /// An app with a save slot in a fresh temporary folder. Keep the
    /// returned `TempDir` alive: the folder is deleted when it's dropped.
    fn new_app(seed: Option<u64>) -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(
            load_start_map().unwrap(),
            Some(SaveSlot::in_dir(dir.path())),
            seed,
            fixed_seed,
        );
        (app, dir)
    }

    fn menu(app: &App) -> &TitleMenu {
        match &app.screen {
            Screen::Title(menu) => menu,
            Screen::Playing(_) => panic!("expected the title menu"),
        }
    }

    /// The game being played, if any.
    fn current_world(app: &App) -> Option<&World> {
        match &app.screen {
            Screen::Playing(world) => Some(world),
            Screen::Title(_) => None,
        }
    }

    fn world(app: &App) -> &World {
        current_world(app).expect("expected a game in progress")
    }

    fn last_log(app: &App) -> &str {
        app.log.last().map(String::as_str).unwrap_or("")
    }

    fn is_ambient_line(line: &str) -> bool {
        Sound::ALL.iter().any(|&s| text::sound_text(s) == line)
    }

    // --- Opening the app -------------------------------------------------

    #[test]
    fn without_a_seed_the_app_opens_on_the_title_menu() {
        let (app, _dir) = new_app(None);
        assert_eq!(menu(&app).selected(), MenuItem::NewGame);
        assert!(!menu(&app).can_continue(), "nothing saved yet");
    }

    #[test]
    fn with_a_seed_the_app_goes_straight_into_that_game() {
        let (app, _dir) = new_app(Some(42));
        assert_eq!(world(&app).seed(), 42);
        assert_eq!(
            app.log[0],
            "Seed 42. Run with --seed 42 to replay this ship."
        );
    }

    // --- The title menu ---------------------------------------------------

    #[test]
    fn new_game_starts_on_a_fresh_seed() {
        let (mut app, _dir) = new_app(None);
        app.on_title(TitleAction::Choose);
        assert_eq!(world(&app).seconds(), 0);
        assert_eq!(world(&app).seed(), CLOCK_SEED);
    }

    #[test]
    fn hotkeys_pick_items_directly() {
        let (mut app, _dir) = new_app(None);
        app.on_title(TitleAction::Pick(MenuItem::NewGame));
        assert!(current_world(&app).is_some());

        let (mut app, _dir) = new_app(None);
        app.on_title(TitleAction::Pick(MenuItem::Quit));
        assert!(app.quit);
    }

    #[test]
    fn a_greyed_out_continue_hotkey_does_nothing() {
        let (mut app, _dir) = new_app(None);
        app.on_title(TitleAction::Pick(MenuItem::Continue));
        assert!(current_world(&app).is_none());
        assert_eq!(menu(&app).message(), None);
    }

    #[test]
    fn up_down_and_exit_work_on_the_menu() {
        let (mut app, _dir) = new_app(None);
        app.on_title(TitleAction::Down);
        assert_eq!(menu(&app).selected(), MenuItem::Quit, "Continue is skipped");
        app.on_title(TitleAction::Up);
        assert_eq!(menu(&app).selected(), MenuItem::NewGame);
        app.on_title(TitleAction::Exit);
        assert!(app.quit);
    }

    // --- Saving, leaving and coming back ------------------------------------

    #[test]
    fn q_saves_and_returns_to_a_menu_that_offers_continue() {
        let (mut app, _dir) = new_app(Some(7));
        app.on_game(Sim(Command::Move(Dir::East)));
        let played = world(&app).clone();

        app.on_game(ToTitle);

        assert_eq!(menu(&app).selected(), MenuItem::Continue);
        app.on_title(TitleAction::Choose);
        assert_eq!(world(&app), &played, "Continue brings back the same game");
        assert_eq!(
            app.log[0],
            "Game loaded at 0:00:03, seed 7. The ship is as you left it."
        );
    }

    #[test]
    fn capital_q_leaves_without_saving() {
        let (mut app, dir) = new_app(Some(7));
        app.on_game(Sim(Command::Wait));
        app.on_game(ToTitleWithoutSaving);
        assert!(!SaveSlot::in_dir(dir.path()).exists());
        assert!(!menu(&app).can_continue());
    }

    #[test]
    fn capital_s_saves_and_keeps_playing() {
        let (mut app, dir) = new_app(Some(7));
        app.on_game(Sim(Command::Wait));

        app.on_game(Save);

        assert!(current_world(&app).is_some());
        assert!(
            last_log(&app).starts_with("Game saved to "),
            "{}",
            last_log(&app)
        );
        assert!(last_log(&app).ends_with("save.json."));
        let saved = SaveSlot::in_dir(dir.path()).load().unwrap().unwrap();
        assert_eq!(&saved, world(&app));
    }

    #[test]
    fn ctrl_c_saves_and_closes_the_program() {
        let (mut app, dir) = new_app(Some(7));
        app.on_game(Sim(Command::Wait));
        app.on_game(Exit);
        assert!(app.quit);
        assert!(SaveSlot::in_dir(dir.path()).exists());
    }

    #[test]
    fn a_failed_save_keeps_you_in_the_game_and_explains() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file_not_folder");
        std::fs::write(&blocker, "").unwrap();
        let slot = SaveSlot::in_dir(&blocker);
        let mut app = App::new(load_start_map().unwrap(), Some(slot), Some(7), fixed_seed);

        app.on_game(ToTitle);
        assert!(
            current_world(&app).is_some(),
            "must not leave when the save failed"
        );
        assert!(app.log.iter().any(|l| l.starts_with("Could not save:")));
        assert_eq!(last_log(&app), "Press Q to leave without saving.");

        app.on_game(Exit);
        assert!(!app.quit, "Ctrl-C also refuses to lose the game silently");

        app.on_game(ToTitleWithoutSaving);
        assert!(current_world(&app).is_none());
    }

    #[test]
    fn a_damaged_save_is_reported_on_the_menu_and_new_game_still_works() {
        let (mut app, dir) = new_app(None);
        std::fs::write(SaveSlot::in_dir(dir.path()).path(), "{ broken").unwrap();
        app.on_game(ToTitleWithoutSaving); // refresh the menu: a save file now exists

        app.on_title(TitleAction::Pick(MenuItem::Continue));

        let message = menu(&app).message().unwrap();
        assert!(
            message.starts_with("Could not load:") && message.contains("damaged"),
            "{message}"
        );
        app.on_title(TitleAction::Pick(MenuItem::NewGame));
        assert_eq!(world(&app).seconds(), 0);
    }

    #[test]
    fn a_save_that_disappears_is_reported_on_the_menu() {
        let (mut app, dir) = new_app(Some(7));
        app.on_game(ToTitle); // saves
        std::fs::remove_file(SaveSlot::in_dir(dir.path()).path()).unwrap();

        app.on_title(TitleAction::Pick(MenuItem::Continue));

        assert_eq!(menu(&app).message(), Some("There's no saved game yet."));
    }

    #[test]
    fn without_a_save_folder_you_can_still_play_and_leave() {
        let mut app = App::new(load_start_map().unwrap(), None, None, fixed_seed);
        assert!(!menu(&app).can_continue());
        app.on_title(TitleAction::Choose);
        assert!(app.log.iter().any(|l| l.contains("won't be saved")));
        app.on_game(ToTitle);
        assert!(current_world(&app).is_none());
        app.on_title(TitleAction::Pick(MenuItem::Quit));
        assert!(app.quit);
    }

    #[test]
    fn continue_without_a_save_folder_explains() {
        // Can't normally be reached (Continue is greyed out), but `choose`
        // must still handle it.
        let mut app = App::new(load_start_map().unwrap(), None, None, fixed_seed);
        app.choose(MenuItem::Continue);
        assert_eq!(
            menu(&app).message(),
            Some("This machine has no save folder.")
        );
    }

    // --- Playing --------------------------------------------------------------

    #[test]
    fn waiting_reaches_the_world_and_the_log() {
        let (mut app, _dir) = new_app(Some(7));
        let log_len = app.log.len();

        app.on_game(Sim(Command::Wait));

        assert_eq!(world(&app).seconds(), nighthawk_sim::WAIT_SECONDS);
        // "You wait." comes first; an ambient sound may follow it.
        assert_eq!(app.log[log_len], "You wait.");
        assert!(app.log[log_len + 1..].iter().all(|l| is_ambient_line(l)));
    }

    #[test]
    fn plain_steps_only_log_ambient_sounds() {
        let (mut app, _dir) = new_app(Some(7));
        let log_len = app.log.len();
        for _ in 0..3 {
            app.on_game(Sim(Command::Move(Dir::East)));
        }
        assert_eq!(world(&app).seconds(), 3 * nighthawk_sim::STEP_SECONDS);
        assert!(app.log[log_len..].iter().all(|l| is_ambient_line(l)));
    }

    #[test]
    fn a_new_game_gets_a_fresh_log() {
        let (mut app, _dir) = new_app(Some(7));
        app.on_game(Sim(Command::Wait));
        app.on_game(ToTitleWithoutSaving);
        app.on_title(TitleAction::Pick(MenuItem::NewGame));
        assert_eq!(app.log.len(), 3);
        assert!(app.log[0].starts_with("Seed 1234."));
    }

    #[test]
    fn log_keeps_only_the_newest_lines() {
        let (mut app, _dir) = new_app(Some(7));
        for i in 0..LOG_LIMIT + 10 {
            app.push_log(format!("line {i}"));
        }
        assert_eq!(app.log.len(), LOG_LIMIT);
        assert_eq!(last_log(&app), "line 109");
    }

    #[test]
    fn keys_reach_the_right_screen() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let press = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let (mut app, _dir) = new_app(None);

        app.handle_key(press('n')); // title: New game
        assert!(current_world(&app).is_some());
        app.handle_key(press('.')); // game: wait
        assert_eq!(world(&app).seconds(), nighthawk_sim::WAIT_SECONDS);
        app.handle_key(press('Q')); // game: leave without saving
        app.handle_key(press('q')); // title: quit
        assert!(app.quit);
    }

    #[test]
    fn actions_meant_for_the_other_screen_are_ignored() {
        let (mut app, _dir) = new_app(None);
        app.on_game(Sim(Command::Wait)); // on the title: nothing to move
        assert!(current_world(&app).is_none());

        let (mut app, _dir) = new_app(Some(7));
        app.on_title(TitleAction::Exit); // in a game: not a menu
        assert!(!app.quit);
    }

    // --- Closing doors (the "which way?" prompt) ------------------------------

    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A game on the real map, standing just north of the crew deck's
    /// south-east door at (27, 7), with that door open.
    fn beside_an_open_door() -> (App, tempfile::TempDir) {
        let (mut app, dir) = new_app(Some(7));
        let route = std::iter::repeat_n('k', 5)
            .chain(std::iter::repeat_n('l', 23))
            .chain(['<', 'j', 'j', 'j', 'j', 'j']); // the last j opens the door
        for c in route {
            app.handle_key(key(KeyCode::Char(c)));
        }
        let world = world(&app);
        assert_eq!(world.player(), nighthawk_sim::Pos::new(27, 6, 1));
        assert_eq!(world.map().get(DOOR), Some(nighthawk_sim::Tile::DoorOpen));
        (app, dir)
    }

    const DOOR: nighthawk_sim::Pos = nighthawk_sim::Pos::new(27, 7, 1);

    #[test]
    fn c_then_a_direction_closes_the_door() {
        let (mut app, _dir) = beside_an_open_door();
        let before = world(&app).seconds();

        app.handle_key(key(KeyCode::Char('c')));
        assert!(last_log(&app).starts_with("Close which way?"));
        app.handle_key(key(KeyCode::Char('j')));

        assert_eq!(
            world(&app).map().get(DOOR),
            Some(nighthawk_sim::Tile::DoorClosed)
        );
        assert_eq!(world(&app).seconds(), before + nighthawk_sim::DOOR_SECONDS);
        assert!(app.log.iter().any(|l| l == "You close the door."));
    }

    #[test]
    fn any_other_key_cancels_closing() {
        let (mut app, _dir) = beside_an_open_door();
        let before = world(&app).seconds();
        app.handle_key(key(KeyCode::Char('c')));
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(last_log(&app), "Never mind.");
        assert_eq!(world(&app).seconds(), before);
        assert_eq!(
            world(&app).map().get(DOOR),
            Some(nighthawk_sim::Tile::DoorOpen)
        );

        // And the prompt is over: the next key is an ordinary key again.
        app.handle_key(key(KeyCode::Char('.')));
        assert_eq!(world(&app).seconds(), before + nighthawk_sim::WAIT_SECONDS);
    }

    #[test]
    fn a_key_release_does_not_answer_the_prompt() {
        let (mut app, _dir) = beside_an_open_door();
        app.handle_key(key(KeyCode::Char('c')));
        let release = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        app.handle_key(release);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(
            world(&app).map().get(DOOR),
            Some(nighthawk_sim::Tile::DoorClosed)
        );
    }

    #[test]
    fn a_toggles_the_air_overlay_without_spending_any_time() {
        let (mut app, _dir) = new_app(Some(7));
        assert!(!app.show_air);
        app.handle_key(key(KeyCode::Char('a')));
        assert!(app.show_air);
        app.handle_key(key(KeyCode::Char('a')));
        assert!(!app.show_air);
        assert_eq!(world(&app).seconds(), 0);
    }
}
