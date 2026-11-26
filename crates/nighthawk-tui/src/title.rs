//! The title menu: New game / Continue / Quit.
//!
//! This is only the menu's *state* (what's highlighted, what's available)
//! and how it moves. Drawing it is in `ui.rs`, and what each choice does is
//! in `app.rs`.

/// One line of the title menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    NewGame,
    Continue,
    Quit,
}

impl MenuItem {
    /// The items in the order they're shown.
    pub const ALL: [MenuItem; 3] = [MenuItem::NewGame, MenuItem::Continue, MenuItem::Quit];

    pub fn label(self) -> &'static str {
        match self {
            MenuItem::NewGame => "New game",
            MenuItem::Continue => "Continue",
            MenuItem::Quit => "Quit",
        }
    }

    /// The key that picks this item directly: the first letter of its label.
    pub fn hotkey(self) -> char {
        match self {
            MenuItem::NewGame => 'n',
            MenuItem::Continue => 'c',
            MenuItem::Quit => 'q',
        }
    }
}

/// The menu's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleMenu {
    /// Index into [`MenuItem::ALL`] of the highlighted item.
    selected: usize,
    /// Whether there's a save to continue. Without one, Continue is greyed
    /// out and can't be highlighted.
    can_continue: bool,
    /// A note shown under the menu, for example why loading failed.
    message: Option<String>,
}

impl TitleMenu {
    /// A fresh menu. If there's a save, Continue starts highlighted, since
    /// that's what you usually want; otherwise New game does.
    pub fn new(can_continue: bool) -> Self {
        let first = if can_continue {
            MenuItem::Continue
        } else {
            MenuItem::NewGame
        };
        Self {
            selected: index_of(first),
            can_continue,
            message: None,
        }
    }

    /// The same menu with a message shown under it.
    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }

    pub fn selected(&self) -> MenuItem {
        MenuItem::ALL[self.selected]
    }

    pub fn can_continue(&self) -> bool {
        self.can_continue
    }

    pub fn is_enabled(&self, item: MenuItem) -> bool {
        item != MenuItem::Continue || self.can_continue
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Highlight the next enabled item, wrapping from the bottom to the top.
    pub fn down(&mut self) {
        self.step(1);
    }

    /// Highlight the previous enabled item, wrapping from the top to the bottom.
    pub fn up(&mut self) {
        // Stepping forward by len - 1 is the same as stepping back by one.
        self.step(MenuItem::ALL.len() - 1);
    }

    /// Moves `by` places forward until it lands on an enabled item. At least
    /// one item (Quit) is always enabled, so this always stops.
    fn step(&mut self, by: usize) {
        let len = MenuItem::ALL.len();
        for _ in 0..len {
            self.selected = (self.selected + by) % len;
            if self.is_enabled(self.selected()) {
                return;
            }
        }
    }
}

fn index_of(item: MenuItem) -> usize {
    MenuItem::ALL
        .iter()
        .position(|&i| i == item)
        .expect("every item is in ALL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_a_save_continue_is_highlighted_first() {
        let menu = TitleMenu::new(true);
        assert_eq!(menu.selected(), MenuItem::Continue);
        assert!(menu.is_enabled(MenuItem::Continue));
    }

    #[test]
    fn without_a_save_new_game_is_highlighted_and_continue_is_off() {
        let menu = TitleMenu::new(false);
        assert_eq!(menu.selected(), MenuItem::NewGame);
        assert!(!menu.is_enabled(MenuItem::Continue));
        assert!(menu.is_enabled(MenuItem::NewGame) && menu.is_enabled(MenuItem::Quit));
    }

    #[test]
    fn down_and_up_move_and_wrap() {
        let mut menu = TitleMenu::new(true); // on Continue
        menu.down();
        assert_eq!(menu.selected(), MenuItem::Quit);
        menu.down();
        assert_eq!(menu.selected(), MenuItem::NewGame, "wraps to the top");
        menu.up();
        assert_eq!(menu.selected(), MenuItem::Quit, "wraps to the bottom");
        menu.up();
        assert_eq!(menu.selected(), MenuItem::Continue);
    }

    #[test]
    fn a_disabled_continue_is_skipped() {
        let mut menu = TitleMenu::new(false); // on New game
        menu.down();
        assert_eq!(menu.selected(), MenuItem::Quit);
        menu.up();
        assert_eq!(menu.selected(), MenuItem::NewGame);
    }

    #[test]
    fn messages_are_shown_until_replaced() {
        let menu = TitleMenu::new(false);
        assert_eq!(menu.message(), None);
        let menu = menu.with_message(Some("No save yet.".into()));
        assert_eq!(menu.message(), Some("No save yet."));
        assert!(!menu.can_continue());
    }

    #[test]
    fn labels_and_hotkeys_match() {
        for item in MenuItem::ALL {
            let first = item.label().chars().next().unwrap().to_ascii_lowercase();
            assert_eq!(first, item.hotkey(), "{item:?}");
        }
    }
}
