//! The app's state and what each action does.
//!
//! The app is always on one of two screens: the title menu, or playing a
//! game. Keys become actions (`keys.rs`), and this file decides what each
//! action means on the current screen. There's no terminal code here, so all
//! of it can be tested directly.

use nighthawk_sim::{
    ActionOption, Blocked, Command, Dress, ItemAt, ItemPath, LoadedMap, Place, Pos, World,
    options_at,
};
use ratatui::crossterm::event::KeyEvent;

use crate::keys::{
    self, BlackoutAction, CursorAction, GameAction, InventoryAction, ListAction, TitleAction,
};
use crate::saves::SaveSlot;
use crate::text;
use crate::title::{MenuItem, TitleMenu};
use crate::ui::{self, BlackoutOption, Cursor, CursorMode, InvEntry, InventoryMode, Overlay};

/// How many log lines to keep. The log panel shows as many as fit.
pub const LOG_LIMIT: usize = 100;

/// Which screen is showing. Each variant holds that screen's state, so a
/// game in progress only exists while you're playing it.
#[derive(Debug)]
pub enum Screen {
    Title(TitleMenu),
    /// Boxed (kept on the heap) because a `World` is much bigger than a
    /// title menu, and an enum is always as big as its biggest variant.
    /// The box keeps `Screen` itself small either way.
    Playing(Box<World>),
}

/// A question the game is waiting for a direction key to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    /// Which way to do action number `.0`, after its shortcut key.
    Action(u16),
    /// Which container to put the carried item at `.0` in, when more than
    /// one is within reach.
    PutAway(ItemPath),
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
    /// What the game is asking a direction for, if anything: after a
    /// shortcut key (`c` to close a door), or which container to put
    /// something in.
    asking_direction: Option<Asking>,
    /// Which overlay the map shows, if any. A view setting, not part of
    /// the game, so it isn't saved.
    pub overlay: Option<Overlay>,
    /// The look (`V`) or interact (`e`) cursor, while it's up. Moving it
    /// never changes the world or spends time; only choosing an action does.
    pub cursor: Option<Cursor>,
    /// While the inventory screen is open (`i`), which carried item is
    /// highlighted, counting from 0.
    pub inventory: Option<usize>,
    /// Whether the inventory shows its list, a closer look at the
    /// highlighted line, or the pocket chooser.
    pub inventory_mode: InventoryMode,
    /// While the crafting screen is open (`C`), which of the crafts you
    /// know is highlighted.
    pub crafting: Option<usize>,
    /// While the wear screen is open (`W`), which line is highlighted
    /// (see [`ui::wear_entries`]).
    pub wearing: Option<usize>,
    /// While you're blacked out, which option of the box is highlighted
    /// (see [`ui::blackout_options`]).
    pub blackout: usize,
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
            asking_direction: None,
            overlay: None,
            cursor: None,
            inventory: None,
            inventory_mode: InventoryMode::List,
            crafting: None,
            wearing: None,
            blackout: 0,
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
        if !keys::is_press(key) || self.on_open_screen(key) {
            return;
        }
        match self.screen {
            Screen::Title(_) => {
                if let Some(action) = keys::title_action(key) {
                    self.on_title(action);
                }
            }
            Screen::Playing(_) => {
                if let Some(action) = keys::game_action(key, self.actions()) {
                    self.on_game(action);
                }
            }
        }
    }

    /// Sends `key` to whatever is open over the game, if anything: a
    /// question waiting for a direction, the inventory or crafting screen,
    /// or a cursor. Returns whether something was open, and so took the
    /// key (even a key it does nothing with).
    fn on_open_screen(&mut self, key: KeyEvent) -> bool {
        // `||` stops at the first that takes the key, so they're tried in
        // order: the blacked-out box first, since it takes every key.
        self.on_blackout_key(key)
            || self.on_question_key(key)
            || self.on_list_screen_key(key)
            || self.on_cursor_key(key)
    }

    /// While you're blacked out, the box asking to wake up or quit takes
    /// every key. Returns whether it's up.
    fn on_blackout_key(&mut self, key: KeyEvent) -> bool {
        let out_cold = matches!(&self.screen, Screen::Playing(world) if !world.is_conscious());
        if out_cold && let Some(action) = keys::blackout_action(key) {
            self.on_blackout(action);
        }
        out_cold
    }

    /// The key that answers a question waiting for a direction, if one is.
    fn on_question_key(&mut self, key: KeyEvent) -> bool {
        let Some(asking) = self.asking_direction else {
            return false;
        };
        self.answer_direction(key, asking);
        true
    }

    /// A key for the inventory, crafting or wear screen, whichever is open.
    fn on_list_screen_key(&mut self, key: KeyEvent) -> bool {
        if self.inventory.is_some() {
            self.on_inventory_key(key);
        } else if self.crafting.is_some() {
            if let Some(action) = keys::crafting_action(key) {
                self.on_crafting(action);
            }
        } else if self.wearing.is_some() {
            if let Some(action) = keys::wear_action(key) {
                self.on_wear(action);
            }
        } else {
            return false;
        }
        true
    }

    /// A key for the look or interact cursor, if it's up.
    fn on_cursor_key(&mut self, key: KeyEvent) -> bool {
        if self.cursor.is_none() {
            return false;
        }
        if let Some(action) = keys::cursor_action(key, self.actions()) {
            self.on_cursor(action);
        }
        true
    }

    /// The actions of the game being played (from `data/actions/`), or
    /// none on the title menu.
    fn actions(&self) -> &[nighthawk_sim::Action] {
        match &self.screen {
            Screen::Playing(world) => world.content().actions(),
            Screen::Title(_) => &[],
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
            GameAction::ToTitle => self.quit_to_title(),
            GameAction::ToTitleWithoutSaving => self.show_title(None),
            GameAction::Exit => self.quit = self.save_game(),
            GameAction::ToggleOverlay(_)
            | GameAction::Look
            | GameAction::Interact
            | GameAction::Inventory
            | GameAction::Crafting
            | GameAction::Wear => self.change_view(action),
            GameAction::BeginShortcut(action) => {
                let name = match &self.screen {
                    Screen::Playing(world) => text::action_name(world, action).to_string(),
                    Screen::Title(_) => return,
                };
                self.asking_direction = Some(Asking::Action(action));
                self.push_log(format!(
                    "{name} which way? (a direction key; anything else cancels)"
                ));
            }
        }
    }

    /// The actions that only change what's shown: overlays, the look and
    /// interact cursors, the inventory, crafting and wear screens. None of
    /// them spends any time. `fn on_game` sends only those here, so the
    /// last arm is the wear screen.
    fn change_view(&mut self, action: GameAction) {
        match action {
            GameAction::ToggleOverlay(overlay) => self.toggle_overlay(overlay),
            GameAction::Look => self.raise_cursor(CursorMode::Look),
            GameAction::Interact => self.raise_cursor(CursorMode::Interact),
            GameAction::Inventory => {
                if let Screen::Playing(world) = &self.screen {
                    self.inventory = Some(ui::first_carried(world));
                }
                self.inventory_mode = InventoryMode::List;
            }
            GameAction::Crafting => self.crafting = Some(0),
            _ => self.wearing = Some(0),
        }
    }

    /// Moves the wear screen's highlight, puts on or takes off the
    /// highlighted thing, or closes the screen, which stays open after
    /// dressing so you can do more.
    fn on_wear(&mut self, action: ListAction) {
        let (Screen::Playing(world), Some(selected)) = (&self.screen, self.wearing) else {
            return;
        };
        let entries = ui::wear_entries(world);
        let last = entries.len().saturating_sub(1);
        match action {
            ListAction::Close => self.wearing = None,
            ListAction::Up => self.wearing = Some(selected.saturating_sub(1)),
            ListAction::Down => self.wearing = Some((selected + 1).min(last)),
            ListAction::Choose => match entries.get(selected) {
                Some(&dress) => self.dress(dress),
                None => self.push_log("You have nothing to put on or take off.".into()),
            },
        }
    }

    /// Puts something on or takes it off, keeping the wear screen's
    /// highlight on its list.
    fn dress(&mut self, dress: Dress) {
        self.apply(Command::Dress(dress));
        if let (Screen::Playing(world), Some(selected)) = (&self.screen, self.wearing) {
            let last = ui::wear_entries(world).len().saturating_sub(1);
            self.wearing = Some(selected.min(last));
        }
    }

    /// Moves the crafting highlight, makes the highlighted craft, or
    /// closes the screen. The screen stays open after making something, so
    /// you can make another.
    fn on_crafting(&mut self, action: ListAction) {
        let (Screen::Playing(world), Some(selected)) = (&self.screen, self.crafting) else {
            return;
        };
        let known = world.known_crafts();
        let last = known.len().saturating_sub(1);
        match action {
            ListAction::Close => self.crafting = None,
            ListAction::Up => self.crafting = Some(selected.saturating_sub(1)),
            ListAction::Down => self.crafting = Some((selected + 1).min(last)),
            ListAction::Choose => match known.get(selected) {
                Some(&craft) => self.apply(Command::Craft(craft)),
                None => self.push_log("You don't know how to make anything yet.".into()),
            },
        }
    }

    /// A key on the inventory screen: for the pocket chooser while it's
    /// up, else for the list or the closer look.
    fn on_inventory_key(&mut self, key: KeyEvent) {
        if matches!(self.inventory_mode, InventoryMode::Moving { .. }) {
            if let Some(action) = keys::pocket_action(key) {
                self.on_pocket_choice(action);
            }
        } else if let Some(action) = keys::inventory_action(key) {
            self.on_inventory(action);
        }
    }

    /// Moves the inventory highlight, opens or closes a closer look, or
    /// does something to the highlighted line (decision D62).
    fn on_inventory(&mut self, action: InventoryAction) {
        let (Screen::Playing(world), Some(selected)) = (&self.screen, self.inventory) else {
            return;
        };
        let entries = ui::inventory_entries(world);
        let last = entries.len().saturating_sub(1);
        match action {
            InventoryAction::Close if self.inventory_mode == InventoryMode::Examine => {
                self.inventory_mode = InventoryMode::List;
            }
            InventoryAction::Close => self.inventory = None,
            InventoryAction::Up => self.inventory = Some(selected.saturating_sub(1)),
            InventoryAction::Down => self.inventory = Some((selected + 1).min(last)),
            InventoryAction::Examine if !entries.is_empty() => {
                self.inventory_mode = InventoryMode::Examine;
            }
            _ => match entries.get(selected) {
                Some(&entry) => self.act_on(entry, action),
                None => self.push_log("You have nothing on or with you.".into()),
            },
        }
    }

    /// Does `action` to the inventory line `entry`, then shows the list
    /// again, keeping the highlight on it as it changes.
    fn act_on(&mut self, entry: InvEntry, action: InventoryAction) {
        self.inventory_mode = InventoryMode::List;
        match (entry, action) {
            (InvEntry::Worn(i), InventoryAction::Dress) => {
                self.apply(Command::Dress(Dress::TakeOff(i)));
            }
            (InvEntry::Worn(_), _) => {
                self.push_log("You're wearing that. Take it off first (W).".into());
            }
            (InvEntry::Carried(path), action) => self.act_on_carried(path, action),
        }
        self.keep_highlight_on_list();
    }

    /// Does `action` to the carried item at `path`. `on_inventory` handles
    /// the rest first, so the last arm is moving it to another pocket.
    fn act_on_carried(&mut self, path: ItemPath, action: InventoryAction) {
        match action {
            InventoryAction::Drop => self.apply(Command::Drop(path)),
            InventoryAction::PutAway => {
                if let Screen::Playing(world) = &self.screen {
                    let containers = world.containers_in_reach();
                    self.begin_put_away(path, &containers);
                }
            }
            InventoryAction::Power(act) => self.apply(Command::Power { item: path, act }),
            InventoryAction::Dress => {
                self.apply(Command::Dress(Dress::PutOn(ItemAt::Carried(path))));
            }
            _ => {
                self.inventory_mode = InventoryMode::Moving {
                    item: path,
                    chosen: 0,
                };
            }
        }
    }

    /// Moves the pocket chooser's highlight, moves the item into the
    /// highlighted pocket, or goes back to the list.
    fn on_pocket_choice(&mut self, action: ListAction) {
        let (Screen::Playing(world), InventoryMode::Moving { item: path, chosen }) =
            (&self.screen, self.inventory_mode)
        else {
            return;
        };
        let pockets = ui::pockets_to_move_to(world, path);
        let last = pockets.len().saturating_sub(1);
        let moving = |chosen| InventoryMode::Moving { item: path, chosen };
        match action {
            ListAction::Close => self.inventory_mode = InventoryMode::List,
            ListAction::Up => self.inventory_mode = moving(chosen.saturating_sub(1)),
            ListAction::Down => self.inventory_mode = moving((chosen + 1).min(last)),
            ListAction::Choose => {
                self.inventory_mode = InventoryMode::List;
                if let Some(&(worn, pocket)) = pockets.get(chosen) {
                    let into = Place::Pocket { worn, pocket };
                    self.apply(Command::Store { item: path, into });
                }
            }
        }
    }

    /// Moves the inventory highlight up onto the list, if it has shrunk
    /// under it.
    fn keep_highlight_on_list(&mut self) {
        if let (Screen::Playing(world), Some(selected)) = (&self.screen, self.inventory) {
            let last = ui::inventory_entries(world).len().saturating_sub(1);
            self.inventory = Some(selected.min(last));
        }
    }

    /// Puts the carried item at `path` into the only one of `containers`
    /// (those within reach), or, with more than one, asks which way.
    fn begin_put_away(&mut self, path: ItemPath, containers: &[Pos]) {
        // A slice pattern matches on the list's length: none, exactly one
        // (named `into`), or anything else.
        match *containers {
            [] => self.push_log("There's nothing within reach to put things in.".into()),
            [into] => self.put_away(path, into),
            _ => {
                self.asking_direction = Some(Asking::PutAway(path));
                self.push_log("Put it in which? (a direction key; anything else cancels)".into());
            }
        }
    }

    /// Puts the carried item at `path` into the furniture at `into`, and
    /// keeps the inventory highlight on the list as it shrinks.
    fn put_away(&mut self, path: ItemPath, into: Pos) {
        let into = Place::Furniture(into);
        self.apply(Command::Store { item: path, into });
        self.keep_highlight_on_list();
    }

    /// Puts the cursor up on the player, for looking or interacting.
    fn raise_cursor(&mut self, mode: CursorMode) {
        if let Screen::Playing(world) = &self.screen {
            self.cursor = Some(Cursor {
                at: world.player(),
                mode,
            });
        }
    }

    /// Moves the cursor (staying on the map), puts it away, or chooses an
    /// action in the interact menu.
    fn on_cursor(&mut self, action: CursorAction) {
        let (Screen::Playing(world), Some(cursor)) = (&self.screen, self.cursor) else {
            return;
        };
        match action {
            CursorAction::Stop => self.cursor = None,
            CursorAction::Move(dir) => {
                let next = cursor.at.step(dir);
                if world.map().in_bounds(next) {
                    self.cursor = Some(Cursor { at: next, ..cursor });
                }
            }
            CursorAction::Do(_) | CursorAction::DoFirst if cursor.mode == CursorMode::Look => {}
            CursorAction::Do(chosen) => self.choose_action(cursor.at, Some(chosen)),
            CursorAction::DoFirst => self.choose_action(cursor.at, None),
        }
    }

    /// Does `chosen` (or, for `None`, the first action that can be done)
    /// to the tile at `at`, and puts the cursor away. If it can't be done,
    /// says why and leaves the cursor up, so you can pick something else.
    fn choose_action(&mut self, at: Pos, chosen: Option<u16>) {
        let Screen::Playing(world) = &self.screen else {
            return;
        };
        let options = options_at(world, at);
        let pick = match chosen {
            Some(action) => options.iter().find(|o| o.action == action),
            None => options
                .iter()
                .find(|o| o.blocked.is_none())
                .or(options.first()),
        };
        // Work out what to say first: `world` borrows `self`, and logging
        // needs to change `self`, so the borrow has to end before logging.
        let refusal = match pick {
            None if !world.is_visible(at) => {
                Some(text::blocked_text(Blocked::CantSee, 0, Some(world)))
            }
            None => Some("There's nothing to do with that here.".to_string()),
            Some(&ActionOption {
                action,
                blocked: Some(blocked),
            }) => Some(text::blocked_text(blocked, action, Some(world))),
            Some(_) => None,
        };
        match (refusal, pick) {
            (Some(line), _) => self.push_log(line),
            (None, Some(&ActionOption { action, .. })) => {
                self.cursor = None;
                self.apply(Command::Interact { at, action });
            }
            (None, None) => {}
        }
    }

    /// Shows `overlay`, or hides it if it's already showing. Asking for a
    /// different overlay swaps to it.
    fn toggle_overlay(&mut self, overlay: Overlay) {
        self.overlay = if self.overlay == Some(overlay) {
            None
        } else {
            Some(overlay)
        };
    }

    /// The key after a question that wants a direction: it does the
    /// action to the tile that way (`c` then `l` closes the door to the
    /// east), or puts the item in the container that way; any other key
    /// cancels, costing nothing.
    fn answer_direction(&mut self, key: KeyEvent, asking: Asking) {
        self.asking_direction = None;
        let at = match (&self.screen, keys::direction(key)) {
            (Screen::Playing(world), Some(dir)) => world.player().step(dir),
            _ => {
                self.push_log("Never mind.".into());
                return;
            }
        };
        match asking {
            Asking::Action(action) => self.apply(Command::Interact { at, action }),
            Asking::PutAway(item) => self.put_away(item, at),
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
        self.screen = Screen::Playing(Box::new(world));
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
                    "Game loaded: {}, seed {}. The ship is as you left it.",
                    world.now(),
                    world.seed()
                )];
                self.screen = Screen::Playing(Box::new(world));
            }
            Ok(None) => self.show_title(Some("There's no saved game yet.".into())),
            Err(message) => self.show_title(Some(message)),
        }
    }

    fn show_title(&mut self, message: Option<String>) {
        self.cursor = None;
        self.inventory = None;
        self.crafting = None;
        self.wearing = None;
        self.inventory_mode = InventoryMode::List;
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
        let events = world.apply(command);
        let lines: Vec<String> = events
            .into_iter()
            .filter_map(|event| text::describe_in(world, event))
            .collect();
        for line in lines {
            self.push_log(line);
        }
        self.close_screens_if_blacked_out();
    }

    /// Blacked out, nothing else stays open: the box asking to wake up or
    /// quit takes over (decision D63), its first option highlighted.
    fn close_screens_if_blacked_out(&mut self) {
        if let Screen::Playing(world) = &self.screen
            && !world.is_conscious()
        {
            self.inventory = None;
            self.inventory_mode = InventoryMode::List;
            self.crafting = None;
            self.wearing = None;
            self.cursor = None;
            self.asking_direction = None;
            self.blackout = 0;
        }
    }

    /// Moves the blacked-out box's highlight, wakes up beside your stasis
    /// chamber, or saves and goes back to the title menu.
    fn on_blackout(&mut self, action: BlackoutAction) {
        let Screen::Playing(world) = &self.screen else {
            return;
        };
        let options = ui::blackout_options(world);
        let last = options.len().saturating_sub(1);
        let chosen = match action {
            BlackoutAction::Up => {
                self.blackout = self.blackout.saturating_sub(1);
                return;
            }
            BlackoutAction::Down => {
                self.blackout = (self.blackout + 1).min(last);
                return;
            }
            BlackoutAction::Choose => options.get(self.blackout).copied(),
            BlackoutAction::Wake => options
                .first()
                .copied()
                .filter(|&o| o == BlackoutOption::WakeUp),
            BlackoutAction::Quit => Some(BlackoutOption::Quit),
        };
        match chosen {
            Some(BlackoutOption::WakeUp) => self.apply(Command::WakeUp),
            Some(BlackoutOption::Quit) => self.quit_to_title(),
            None => {}
        }
    }

    /// Saves, then goes back to the title menu; if the save fails, stays
    /// in the game (the log says why).
    fn quit_to_title(&mut self) {
        if self.save_game() {
            self.show_title(None);
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

    /// Adds a line to the log. The same line twice in a row becomes one
    /// line with a count ("You wait. (×3)"), so picking up four pieces of
    /// scrap doesn't push everything else off the screen.
    pub fn push_log(&mut self, line: String) {
        if let Some(last) = self.log.last_mut()
            && let Some(times) = repeats(last, &line)
        {
            *last = format!("{line} (×{})", times + 1);
            return;
        }
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            self.log.remove(0);
        }
    }
}

/// How many times `line` is already said by the log line `last`: 1 if
/// they're the same, `n` if `last` is `line` followed by " (×n)", and
/// `None` if `last` is something else.
fn repeats(last: &str, line: &str) -> Option<u32> {
    if last == line {
        return Some(1);
    }
    let count = last
        .strip_prefix(line)?
        .strip_prefix(" (×")?
        .strip_suffix(')')?;
    count.parse().ok()
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

    /// The newest log line that isn't a ship noise: what the player did.
    fn last_said(app: &App) -> &str {
        let said = app.log.iter().rev().find(|l| !is_ambient_line(l));
        said.map_or("", String::as_str)
    }

    /// Whether `line` is a ship noise, counted ("… (×2)") or not.
    fn is_ambient_line(line: &str) -> bool {
        Sound::ALL
            .iter()
            .any(|&s| line.starts_with(text::sound_text(s)))
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
            "Game loaded: 25 Nov 2631 08:42:03, seed 7. The ship is as you left it."
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
            .chain(['>', 'j', 'j', 'j', 'j', 'j']); // > climbs; the last j opens the door
        for c in route {
            app.handle_key(key(KeyCode::Char(c)));
        }
        let world = world(&app);
        assert_eq!(world.player(), nighthawk_sim::Pos::new(33, 6, 1));
        assert_eq!(
            world.structure_at(DOOR).map(|s| s.id.as_str()),
            Some("door_open")
        );
        (app, dir)
    }

    const DOOR: nighthawk_sim::Pos = nighthawk_sim::Pos::new(33, 7, 1);

    #[test]
    fn c_then_a_direction_closes_the_door() {
        let (mut app, _dir) = beside_an_open_door();
        let before = world(&app).seconds();

        app.handle_key(key(KeyCode::Char('c')));
        assert!(last_log(&app).starts_with("Close which way?"));
        app.handle_key(key(KeyCode::Char('j')));

        assert_eq!(
            world(&app).structure_at(DOOR).map(|s| s.id.as_str()),
            Some("door")
        );
        // The shipped close action takes 2 seconds.
        assert_eq!(world(&app).seconds(), before + 2);
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
            world(&app).structure_at(DOOR).map(|s| s.id.as_str()),
            Some("door_open")
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
            world(&app).structure_at(DOOR).map(|s| s.id.as_str()),
            Some("door")
        );
    }

    #[test]
    fn a_toggles_the_air_overlay_without_spending_any_time() {
        let (mut app, _dir) = new_app(Some(7));
        assert_eq!(app.overlay, None);
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.overlay, Some(Overlay::Air));
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.overlay, None);
        assert_eq!(world(&app).seconds(), 0);
    }

    #[test]
    fn t_shows_the_heat_overlay_and_swaps_with_air() {
        let (mut app, _dir) = new_app(Some(7));
        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.overlay, Some(Overlay::Heat));
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.overlay, Some(Overlay::Air), "swapped, not stacked");
        app.handle_key(key(KeyCode::Char('t')));
        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.overlay, None);
        assert_eq!(world(&app).seconds(), 0);
    }

    // --- Blacking out ---------------------------------------------------------

    /// An app on a deck with no air, wearing a jumpsuit, two steps from
    /// your stasis chamber (`pod`) or with none, and already blacked out:
    /// with the crafting screen open, to show blacking out closes it.
    fn airless(pod: bool) -> (App, tempfile::TempDir) {
        let rows = if pod {
            ["HHHHHH", "H@.PPH", "HHHHHH"]
        } else {
            ["HHHHHH", "H@...H", "HHHHHH"]
        };
        let json = format!(
            r##"{{ "id": "t", "name": "T", "player_wears": [ {{ "item": "jumpsuit" }} ],
                 "legend": {{ "H": "hull_plating", ".": "deck_plating", "@": "deck_plating",
                     "P": {{ "structure": "deck_plating", "furniture": "your_stasis_chamber" }} }},
                 "decks": [ {{ "rows": {rows:?}, "pressure": 0 }} ] }}"##
        );
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let content = crate::content::load_content(&data).unwrap();
        let map = nighthawk_sim::parse_map(&json, &content).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(map, Some(SaveSlot::in_dir(dir.path())), Some(1), fixed_seed);
        app.crafting = Some(0);
        for _ in 0..50 {
            app.apply(Command::Wait);
        }
        assert!(!world(&app).is_conscious(), "out cold");
        (app, dir)
    }

    #[test]
    fn blacking_out_opens_a_box_that_takes_every_key_and_wakes_you_by_the_chamber() {
        let (mut app, _dir) = airless(true);
        assert_eq!(
            (app.crafting, app.blackout),
            (None, 0),
            "the box, not crafting"
        );
        let before = world(&app).seconds();
        press_all(&mut app, "l.iCe");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(world(&app).seconds(), before, "nothing else happens");
        assert_eq!((app.inventory, app.cursor), (None, None));
        press_all(&mut app, "jj");
        assert_eq!(app.blackout, 1, "Quit, the last");
        press_all(&mut app, "k");
        assert_eq!(app.blackout, 0, "Wake up");
        app.handle_key(key(KeyCode::Enter));
        assert!(world(&app).is_conscious());
        assert_eq!(
            world(&app).player(),
            Pos::new(2, 1, 0),
            "beside the chamber"
        );
        assert!(
            last_said(&app).starts_with("You come to beside the stasis chamber"),
            "{}",
            last_said(&app)
        );
        press_all(&mut app, "....");
        assert!(world(&app).is_conscious(), "its air keeps you going");
    }

    #[test]
    fn quitting_while_blacked_out_saves_and_continue_brings_the_box_back() {
        let (mut app, dir) = airless(true);
        press_all(&mut app, "q");
        assert!(current_world(&app).is_none(), "on the title menu");
        assert!(SaveSlot::in_dir(dir.path()).exists(), "saved");
        press_all(&mut app, "c");
        assert!(!world(&app).is_conscious(), "still out cold");
        press_all(&mut app, "w");
        assert!(world(&app).is_conscious(), "w wakes you");
    }

    #[test]
    fn with_nothing_to_revive_you_only_quitting_is_offered() {
        let (mut app, dir) = airless(false);
        press_all(&mut app, "wj");
        assert!(!world(&app).is_conscious(), "w does nothing");
        assert_eq!(app.blackout, 0, "one option");
        app.handle_key(key(KeyCode::Enter));
        assert!(current_world(&app).is_none(), "Enter quits");
        assert!(SaveSlot::in_dir(dir.path()).exists());
        app.on_blackout(BlackoutAction::Up); // no game: ignored
    }

    // --- Look mode ------------------------------------------------------------

    #[test]
    fn capital_v_looks_from_where_you_stand_and_movement_moves_the_cursor() {
        let (mut app, _dir) = new_app(Some(7));
        let start = world(&app).player();
        app.handle_key(key(KeyCode::Char('V')));
        assert_eq!(app.cursor.map(|c| c.at), Some(start));
        app.handle_key(key(KeyCode::Char('l')));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(
            app.cursor.map(|c| c.at),
            Some(Pos::new(start.x + 1, start.y + 1, start.z))
        );
        assert_eq!(world(&app).player(), start, "only the cursor moved");
        assert_eq!(world(&app).seconds(), 0, "looking takes no time");
    }

    #[test]
    fn other_keys_do_nothing_while_looking_and_esc_stops() {
        let (mut app, _dir) = new_app(Some(7));
        app.handle_key(key(KeyCode::Char('V')));
        app.handle_key(key(KeyCode::Char('.')));
        app.handle_key(key(KeyCode::Char('S')));
        assert_eq!(world(&app).seconds(), 0, "no waiting");
        assert!(app.cursor.is_some());
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.cursor, None);
        app.handle_key(key(KeyCode::Char('.')));
        assert!(world(&app).seconds() > 0, "keys play normally again");
    }

    #[test]
    fn the_look_cursor_stays_on_the_map() {
        let (mut app, _dir) = new_app(Some(7));
        app.handle_key(key(KeyCode::Char('V')));
        for c in ['h', 'k'] {
            for _ in 0..100 {
                app.handle_key(key(KeyCode::Char(c)));
            }
        }
        assert_eq!(
            app.cursor.map(|c| c.at),
            Some(Pos::new(0, 0, 0)),
            "stopped at the corner"
        );
    }

    #[test]
    fn leaving_the_game_stops_looking() {
        let (mut app, _dir) = new_app(Some(7));
        app.handle_key(key(KeyCode::Char('V')));
        app.show_title(None);
        assert_eq!(app.cursor, None);
        // Looking with no game is ignored rather than crashing.
        app.on_cursor(CursorAction::Move(nighthawk_sim::Dir::East));
        assert_eq!(app.cursor, None);
    }

    // --- Interact menu -----------------------------------------------------------

    /// A new game standing beside a crew-deck door with the hand lamp
    /// from the gear locker, so there's light to see by, but no cutter: at
    /// (22, 6) with the cabin door at (22, 7) just south, and another door
    /// at (22, 4) two tiles north.
    fn by_a_door() -> (App, tempfile::TempDir) {
        let (mut app, dir) = new_app(Some(7));
        // Two steps north-west to (8, 5), beside the gear locker at
        // (7, 4); `e`, the cursor north-west onto it, `t` to take out.
        // Then drop the cutter, the first thing in the inventory, go down
        // past the screwdriver and switch on the lamp.
        press_all(&mut app, "yyeyt");
        press_all(&mut app, "idja");
        app.handle_key(key(KeyCode::Esc));
        let route = std::iter::repeat_n('k', 3)
            .chain(std::iter::repeat_n('l', 25))
            .chain(['>'])
            .chain(std::iter::repeat_n('j', 4))
            .chain(std::iter::repeat_n('h', 11));
        for c in route {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(world(&app).player(), Pos::new(22, 6, 1));
        (app, dir)
    }

    fn press_all(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn e_starts_the_interact_cursor_on_you() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "e");
        let cursor = app.cursor.unwrap();
        assert_eq!(cursor.at, Pos::new(22, 6, 1));
        assert_eq!(cursor.mode, CursorMode::Interact);
    }

    #[test]
    fn choosing_open_opens_the_door_and_puts_the_cursor_away() {
        let (mut app, _dir) = by_a_door();
        let before = world(&app).seconds();
        press_all(&mut app, "ejo");
        let door = Pos::new(22, 7, 1);
        assert_eq!(
            world(&app).structure_at(door).map(|s| s.id.as_str()),
            Some("door_open")
        );
        assert_eq!(app.cursor, None);
        assert!(world(&app).seconds() > before, "opening takes time");
        press_all(&mut app, "ejc");
        assert_eq!(
            world(&app).structure_at(door).map(|s| s.id.as_str()),
            Some("door")
        );
    }

    #[test]
    fn a_blocked_choice_says_why_and_keeps_the_cursor_up() {
        let (mut app, _dir) = by_a_door();
        let before = world(&app).seconds();
        press_all(&mut app, "ejx");
        assert_eq!(last_log(&app), "You need a tool with LASER 1 for that.");
        assert!(app.cursor.is_some());
        press_all(&mut app, "kkko"); // from the south door to the one two tiles north of you
        assert_eq!(
            last_log(&app),
            "That's too far away: you need to be right beside it."
        );
        assert_eq!(world(&app).seconds(), before, "nothing happened");
    }

    #[test]
    fn enter_does_the_first_action_that_can_be_done() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "ej");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            world(&app)
                .structure_at(Pos::new(22, 7, 1))
                .map(|s| s.id.as_str()),
            Some("door_open"),
            "Open, not the blocked Cut"
        );
    }

    #[test]
    fn enter_with_nothing_possible_explains_the_first_option() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "ekk");
        app.handle_key(key(KeyCode::Enter));
        assert!(last_log(&app).starts_with("That's too far away"));
    }

    #[test]
    fn choosing_on_open_floor_or_an_action_not_listed_says_so() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "el");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_log(&app), "There's nothing to do with that here.");
        press_all(&mut app, "hjc"); // the door is shut: no Close on the list
        assert_eq!(
            last_log(&app),
            "There's nothing to do with that here. (×2)",
            "said twice in a row, so counted"
        );
    }

    #[test]
    fn choosing_something_out_of_sight_says_you_cannot_see_it() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "ekkk"); // into the cabin behind the shut door
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_log(&app), "You can't see there.");
        assert!(app.cursor.is_some(), "so you can pick something else");
    }

    #[test]
    fn action_keys_do_nothing_in_look_mode() {
        let (mut app, _dir) = by_a_door();
        press_all(&mut app, "Vjo");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            world(&app)
                .structure_at(Pos::new(22, 7, 1))
                .map(|s| s.id.as_str()),
            Some("door")
        );
        assert_eq!(app.cursor.map(|c| c.mode), Some(CursorMode::Look));
    }

    // --- Log ------------------------------------------------------------------

    #[test]
    fn the_same_line_twice_in_a_row_is_counted_not_repeated() {
        let (mut app, _dir) = new_app(Some(7));
        let before = app.log.len();
        for _ in 0..3 {
            app.push_log("You wait.".into());
        }
        assert_eq!(app.log.len(), before + 1);
        assert_eq!(last_log(&app), "You wait. (×3)");
        app.push_log("You open the door.".into());
        app.push_log("You wait.".into());
        assert_eq!(last_log(&app), "You wait.", "only lines in a row count");
    }

    #[test]
    fn repeats_reads_its_own_counts() {
        assert_eq!(repeats("a", "a"), Some(1));
        assert_eq!(repeats("a (×4)", "a"), Some(4));
        assert_eq!(repeats("a (×x)", "a"), None);
        assert_eq!(repeats("b", "a"), None);
        assert_eq!(repeats("a (×4", "a"), None);
    }

    // --- Inventory --------------------------------------------------------------

    /// A new game having taken the cutter, lamp and screwdriver out of the
    /// gear locker at (7, 4), then picked up the four scraps at (9, 9). In
    /// the pockets: cutter, screwdriver; lamp and the four scraps.
    fn carrying_everything() -> (App, tempfile::TempDir) {
        let (mut app, dir) = new_app(Some(7));
        press_all(&mut app, "yyeyt"); // to (8, 5); take out of the locker
        press_all(&mut app, "jjjeng"); // to (8, 8); pick up the scrap at (9, 9)
        assert_eq!(world(&app).carried().len(), 7);
        (app, dir)
    }

    #[test]
    fn i_opens_the_inventory_on_the_first_item_and_takes_no_time() {
        // The lines: the jumpsuit, the seven things in its pockets, then
        // the deck shoes.
        let (mut app, _dir) = carrying_everything();
        let before = world(&app).seconds();
        press_all(&mut app, "i");
        assert_eq!(app.inventory, Some(1), "on the first thing carried");
        press_all(&mut app, "jjjjjjjjjj");
        assert_eq!(app.inventory, Some(8), "stops at the last line, the shoes");
        press_all(&mut app, "kkkkkkkkkk");
        assert_eq!(app.inventory, Some(0), "and at the first, the jumpsuit");
        press_all(&mut app, ".l");
        assert_eq!(world(&app).seconds(), before, "other keys do nothing");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.inventory, None);
    }

    #[test]
    fn d_drops_the_highlighted_item_where_you_stand() {
        let (mut app, _dir) = carrying_everything();
        let here = world(&app).player();
        press_all(&mut app, "ijjjjjjd"); // the last scrap
        assert_eq!(world(&app).carried().len(), 6);
        assert_eq!(world(&app).items_at(here).count(), 1);
        assert_eq!(app.inventory, Some(7), "the line below moves up: the shoes");
        assert_eq!(last_log(&app), "You drop the scrap (Steel).");
        // Up to the first thing carried, and drop the lot from the top.
        press_all(&mut app, "kkkkkkdddddd");
        assert!(world(&app).carried().is_empty());
        press_all(&mut app, "d");
        assert_eq!(
            last_log(&app),
            "You're wearing that. Take it off first (W).",
            "the shoes"
        );
    }

    #[test]
    fn p_puts_the_highlighted_item_in_the_only_container_within_reach() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "yyeyt"); // to (8, 5); take out of the locker
        let (before, carried) = (world(&app).seconds(), world(&app).carried().len());
        press_all(&mut app, "ip"); // the cutter, first in the list
        assert_eq!(
            last_log(&app),
            "You put the basic laser cutter (Aluminium Alloy) in the locker."
        );
        assert_eq!(world(&app).carried().len(), carried - 1);
        assert_eq!(world(&app).seconds(), before + nighthawk_sim::STORE_SECONDS);
        // The jumpsuit, the screwdriver, the lamp, the shoes: put the lamp
        // away too, and the highlight moves onto the shoes below it.
        press_all(&mut app, "jp");
        assert_eq!(world(&app).carried().len(), carried - 2);
        assert_eq!(app.inventory, Some(2), "the highlight stays on the list");
    }

    #[test]
    fn a_switches_the_lamp_and_u_and_r_take_its_cell_out_and_put_it_back() {
        let (mut app, _dir) = new_app(Some(7));
        // Take out the gear: the cutter and screwdriver in one pocket, the
        // lamp in the next.
        press_all(&mut app, "yyeyt");
        press_all(&mut app, "ijja");
        assert_eq!(
            last_said(&app),
            "You switch the hand lamp (Aluminium Alloy) on."
        );
        let before = world(&app).carried().len();
        press_all(&mut app, "u");
        assert_eq!(
            last_said(&app),
            "You take the cell out of the hand lamp (Aluminium Alloy)."
        );
        assert_eq!(world(&app).carried().len(), before + 1, "the cell");
        press_all(&mut app, "r");
        assert_eq!(
            last_said(&app),
            "You load a cell (119 kJ) into the hand lamp (Aluminium Alloy)."
        );
        assert_eq!(world(&app).carried().len(), before);
        press_all(&mut app, "ka");
        assert_eq!(
            last_said(&app),
            "You can't do that with it.",
            "a screwdriver"
        );
    }

    /// A new game with the gear taken out of the locker. The inventory's
    /// lines: 0 the jumpsuit, 1 the cutter, 2 the screwdriver (both in the
    /// left hip pocket), 3 the lamp (right hip), 4 the deck shoes.
    fn with_the_gear() -> (App, tempfile::TempDir) {
        let (mut app, dir) = new_app(Some(7));
        press_all(&mut app, "yyeyt");
        (app, dir)
    }

    #[test]
    fn e_takes_a_closer_look_where_the_same_keys_act_and_esc_goes_back() {
        let (mut app, _dir) = with_the_gear();
        press_all(&mut app, "ie");
        assert_eq!(app.inventory_mode, InventoryMode::Examine);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.inventory_mode, InventoryMode::List, "back to the list");
        assert_eq!(app.inventory, Some(1), "still open, on the cutter");
        press_all(&mut app, "jjea"); // the lamp
        assert_eq!(
            last_said(&app),
            "You switch the hand lamp (Aluminium Alloy) on."
        );
        assert_eq!(app.inventory_mode, InventoryMode::List, "acting goes back");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.inventory, None, "Esc on the list closes it");
    }

    #[test]
    fn i_moves_the_highlighted_item_to_the_pocket_you_choose() {
        let (mut app, _dir) = with_the_gear();
        press_all(&mut app, "ii"); // the cutter, in the left hip pocket
        let cutter = world(&app).carried()[0].0;
        let moving = |chosen| InventoryMode::Moving {
            item: cutter,
            chosen,
        };
        assert_eq!(app.inventory_mode, moving(0));
        // The other pockets: right hip, left leg, right leg.
        press_all(&mut app, "jjjk");
        assert_eq!(app.inventory_mode, moving(1));
        let before = world(&app).seconds();
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            last_said(&app),
            "You put the basic laser cutter (Aluminium Alloy) in your left leg pocket."
        );
        assert_eq!(app.inventory_mode, InventoryMode::List);
        let took = world(&app).seconds() - before;
        assert_eq!(took, nighthawk_sim::POCKET_SECONDS);
        assert_eq!(world(&app).worn()[0].pockets[2].len(), 1);
        press_all(&mut app, "i");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.inventory_mode, InventoryMode::List, "Esc cancels");
        app.show_title(None);
        app.on_pocket_choice(ListAction::Down); // no game: ignored
        assert_eq!(app.inventory_mode, InventoryMode::List);
    }

    #[test]
    fn capital_w_in_the_inventory_takes_off_or_puts_on_the_highlighted_line() {
        let (mut app, _dir) = with_the_gear();
        press_all(&mut app, "ijjjW"); // the deck shoes
        assert_eq!(last_said(&app), "You take off the deck shoes (Polyester).");
        // They're in a pocket now: the last line, under the lamp.
        assert_eq!(app.inventory, Some(4));
        press_all(&mut app, "W");
        assert_eq!(last_said(&app), "You put on the deck shoes (Polyester).");
        press_all(&mut app, "kkkkd"); // the jumpsuit
        assert_eq!(
            last_said(&app),
            "You're wearing that. Take it off first (W)."
        );
    }

    #[test]
    fn with_nothing_carried_the_inventory_opens_on_what_you_wear() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "ia");
        assert_eq!(app.inventory, Some(0), "the jumpsuit");
        assert_eq!(
            last_log(&app),
            "You're wearing that. Take it off first (W)."
        );
        assert_eq!(world(&app).seconds(), 0);
    }

    #[test]
    fn p_with_nowhere_to_put_it_says_so() {
        let (mut app, _dir) = carrying_everything(); // at (8, 8)
        press_all(&mut app, "ip");
        assert_eq!(
            last_log(&app),
            "There's nothing within reach to put things in."
        );
        assert_eq!(world(&app).carried().len(), 7);
    }

    /// An app on a small lit room with a locker on each side of you and
    /// two steel scraps at your feet:
    ///
    /// ```text
    /// ##E##
    /// #L@L#
    /// #####
    /// ```
    fn between_two_lockers() -> (App, tempfile::TempDir) {
        let rows = ["##E##", "#L@L#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "player_wears": [ {{ "item": "jumpsuit" }} ],
                 "legend": {{ "#": "bulkhead", "E": "emergency_light",
                     "L": {{ "structure": "deck_plating", "furniture": "locker" }},
                     "@": {{ "structure": "deck_plating",
                             "items": [ {{ "item": "scrap", "count": 2 }} ] }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let content = crate::content::load_content(&data).unwrap();
        let map = nighthawk_sim::parse_map(&json, &content).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let slot = Some(SaveSlot::in_dir(dir.path()));
        let mut app = App::new(map, slot, Some(1), fixed_seed);
        press_all(&mut app, "eg"); // pick up the scraps at your feet
        assert_eq!(world(&app).carried().len(), 2);
        (app, dir)
    }

    #[test]
    fn with_two_containers_in_reach_p_asks_which_way() {
        let (mut app, _dir) = between_two_lockers();
        press_all(&mut app, "ip");
        assert_eq!(
            last_log(&app),
            "Put it in which? (a direction key; anything else cancels)"
        );
        assert_eq!(world(&app).carried().len(), 2, "nothing yet");
        press_all(&mut app, "h");
        assert_eq!(last_log(&app), "You put the scrap (Steel) in the locker.");
        let west = world(&app).furniture_at(Pos::new(1, 1, 0)).unwrap();
        assert_eq!(west.contents.len(), 1);
        assert_eq!(app.inventory, Some(1), "the inventory stays open");
        press_all(&mut app, "px");
        assert_eq!(last_log(&app), "Never mind.");
        assert_eq!(world(&app).carried().len(), 1);
        press_all(&mut app, "pk");
        assert_eq!(last_log(&app), "There's nothing there to put things in.");
    }

    // --- Crafting ---------------------------------------------------------------

    #[test]
    fn crafting_before_you_know_anything_says_so_and_takes_no_time() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "C");
        assert_eq!(app.crafting, Some(0));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_log(&app), "You don't know how to make anything yet.");
        assert_eq!(world(&app).seconds(), 0);
        press_all(&mut app, "jkC");
        assert_eq!(app.crafting, None, "C closes it again");
    }

    #[test]
    fn picking_up_scrap_gives_you_the_idea_of_a_shiv_and_enter_makes_one() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "bejg"); // to (9, 8); pick up the scrap at (9, 9)
        press_all(&mut app, "C");
        let known = world(&app).known_crafts();
        let shiv = world(&app).content().crafts()[usize::from(known[0])]
            .id
            .clone();
        assert_eq!(shiv, "shiv");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_log(&app), "You make a shiv (Steel).");
        assert_eq!(app.crafting, Some(0), "still open, to make another");
        // Four crafts known (the shiv, rags, wire and plate): down past the
        // last stops on it, then up one.
        press_all(&mut app, "jjjjk");
        assert_eq!(app.crafting, Some(2), "between the first and the last");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.crafting, None);
    }

    #[test]
    fn capital_w_opens_the_wear_screen_where_enter_takes_off_and_puts_on() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "W");
        assert_eq!(app.wearing, Some(0));
        // The jumpsuit, then the deck shoes: take the shoes off. They go in
        // a pocket, and are next on the list, to put back on.
        press_all(&mut app, "j");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_said(&app), "You take off the deck shoes (Polyester).");
        assert_eq!(world(&app).seconds(), nighthawk_sim::TAKE_OFF_SECONDS);
        assert_eq!(app.wearing, Some(1), "still open, on the shoes");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_said(&app), "You put on the deck shoes (Polyester).");
        press_all(&mut app, "jjjkW");
        assert_eq!(app.wearing, None, "W closes it again");
    }

    #[test]
    fn with_nothing_on_and_nothing_to_hand_the_wear_screen_says_so() {
        let rows = ["###", "#@#", "###"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "bulkhead", "@": "deck_plating" }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let content = crate::content::load_content(&data).unwrap();
        let map = nighthawk_sim::parse_map(&json, &content).unwrap();
        let mut app = App::new(map, None, Some(1), fixed_seed);
        press_all(&mut app, "W");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(last_log(&app), "You have nothing to put on or take off.");
        press_all(&mut app, "Wie");
        assert_eq!(last_log(&app), "You have nothing on or with you.");
        assert_eq!(
            app.inventory_mode,
            InventoryMode::List,
            "nothing to look at"
        );
        app.handle_key(key(KeyCode::Esc));
        app.show_title(None);
        assert_eq!(app.wearing, None, "leaving the game closes it");
        app.on_wear(ListAction::Down); // no game: ignored
        assert_eq!(app.wearing, None);
    }

    #[test]
    fn leaving_the_game_closes_the_crafting_screen() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "C");
        app.show_title(None);
        assert_eq!(app.crafting, None);
        app.on_crafting(ListAction::Down); // no game: ignored
        assert_eq!(app.crafting, None);
    }

    #[test]
    fn leaving_the_game_closes_the_inventory() {
        let (mut app, _dir) = new_app(Some(7));
        press_all(&mut app, "i");
        app.show_title(None);
        assert_eq!(app.inventory, None);
        app.on_inventory(InventoryAction::Down); // no game: ignored
        assert_eq!(app.inventory, None);
    }
}
