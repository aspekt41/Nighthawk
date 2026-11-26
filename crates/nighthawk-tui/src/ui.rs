//! Screen layout and drawing.
//!
//! ```text
//! ┌ Stasis bay ─────────────────────────┐┌ Status ─────────┐
//! │                                     ││ Deck 0          │
//! │        the map, centred on @        ││ Turn 12         │
//! │                                     ││ ...             │
//! └─────────────────────────────────────┘└─────────────────┘
//! ┌ Log ──────────────────────────────────────────────────┐
//! │ You bump into the bulkhead.                           │
//! └───────────────────────────────────────────────────────┘
//! ```

use nighthawk_sim::{Pos, Tile, World};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::App;

const SIDEBAR_WIDTH: u16 = 24;
const LOG_HEIGHT: u16 = 6;

pub fn render(app: &App, frame: &mut Frame) {
    let [top, log_area] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(LOG_HEIGHT)]).areas(frame.area());
    let [map_area, side_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(SIDEBAR_WIDTH)]).areas(top);

    let map_block = Block::bordered().title(format!(" {} ", app.world.area_name()));
    let map_inner = map_block.inner(map_area);
    frame.render_widget(map_block, map_area);
    frame.render_widget(MapView { world: &app.world }, map_inner);

    frame.render_widget(sidebar(&app.world), side_area);
    frame.render_widget(log(&app.log, log_area.height.saturating_sub(2)), log_area);
}

/// Draws the map with the player in the middle of the view.
///
/// This is a custom widget: instead of using ratatui's text widgets, it writes
/// characters straight into the screen buffer, one cell per tile. The map
/// will always be drawn this way, because it's a grid, not text.
pub struct MapView<'a> {
    pub world: &'a World,
}

impl Widget for MapView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let player = self.world.player();
        // The map position that lands in the top-left cell of `area`.
        let origin_x = player.x - i32::from(area.width) / 2;
        let origin_y = player.y - i32::from(area.height) / 2;

        for row in 0..area.height {
            for col in 0..area.width {
                let pos = Pos::new(
                    origin_x + i32::from(col),
                    origin_y + i32::from(row),
                    player.z,
                );
                let (glyph, style) = glyph_at(self.world, pos);
                buf[(area.x + col, area.y + row)]
                    .set_symbol(glyph)
                    .set_style(style);
            }
        }
    }
}

/// The character and colour for one map cell. The player is drawn on top of
/// whatever tile they stand on. Off-map cells are blank.
fn glyph_at(world: &World, pos: Pos) -> (&'static str, Style) {
    if pos == world.player() {
        return (
            "@",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        );
    }
    match world.map().get(pos) {
        Some(Tile::Wall) => ("#", Style::new().fg(Color::Gray)),
        Some(Tile::Floor) => (".", Style::new().fg(Color::DarkGray)),
        None => (" ", Style::new()),
    }
}

fn sidebar(world: &World) -> Paragraph<'static> {
    let p = world.player();
    let dim = Style::new().fg(Color::DarkGray);
    Paragraph::new(vec![
        Line::styled("NIGHTHAWK", Style::new().add_modifier(Modifier::BOLD)),
        Line::raw(""),
        Line::raw(format!("Deck  {}", p.z)),
        Line::raw(format!("Pos   {}, {}", p.x, p.y)),
        Line::raw(format!("Turn  {}", world.turn())),
        Line::raw(""),
        Line::styled("move  arrows hjkl", dim),
        Line::styled("      yubn numpad", dim),
        Line::styled("wait  . or 5", dim),
        Line::styled("quit  q", dim),
    ])
    .block(Block::bordered().title(" Status "))
}

/// The newest `lines` log entries, oldest at the top.
fn log(entries: &[String], lines: u16) -> Paragraph<'static> {
    let start = entries.len().saturating_sub(usize::from(lines));
    let text: Vec<Line> = entries[start..]
        .iter()
        .map(|e| Line::raw(e.clone()))
        .collect();
    Paragraph::new(text).block(Block::bordered().title(" Log "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nighthawk_sim::{Command, Dir, parse_map};

    /// A 7×5 room with the player in the middle.
    fn test_world() -> World {
        let rows = ["#######", "#.....#", "#..@..#", "#.....#", "#######"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", ".": "floor" }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).unwrap())
    }

    /// Renders the map into an off-screen buffer and returns its rows as text.
    fn draw(world: &World, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        MapView { world }.render(area, &mut buf);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn player_is_drawn_in_the_centre() {
        let world = test_world();
        let rows = draw(&world, 21, 11);
        assert_eq!(rows[5].chars().nth(10), Some('@'));
    }

    #[test]
    fn walls_floor_and_empty_space_use_their_glyphs() {
        let world = test_world();
        let rows = draw(&world, 21, 11);
        // Row 5 is the player's row: wall, floor, @, floor, wall, then off-map.
        assert_eq!(&rows[5][7..14], "#..@..#");
        assert_eq!(rows[5].chars().next(), Some(' '));
    }

    /// Renders the whole screen, as the game would, into a fake 80×24 terminal.
    fn draw_screen(app: &App) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(app, frame)).unwrap();
        let buf = terminal.backend().buffer();
        (0..24)
            .map(|y| (0..80).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
            .collect()
    }

    #[test]
    fn whole_screen_shows_map_status_and_log() {
        let app = App::new(test_world());
        let screen = draw_screen(&app);
        assert!(
            screen.contains(" T "),
            "map panel is titled with the area name"
        );
        assert!(screen.contains("Turn  0"));
        assert!(screen.contains("Deck  0"));
        assert!(screen.contains("You wake in the stasis bay."));
    }

    #[test]
    fn log_panel_shows_the_newest_lines() {
        let mut app = App::new(test_world());
        for i in 0..20 {
            app.push_log(format!("entry {i}"));
        }
        let screen = draw_screen(&app);
        assert!(screen.contains("entry 19"));
        assert!(!screen.contains("entry 0 "), "old lines scroll off");
    }

    #[test]
    fn view_follows_the_player() {
        let mut world = test_world();
        let before = draw(&world, 21, 11);
        world.apply(Command::Move(Dir::East));
        let after = draw(&world, 21, 11);
        // The @ stays centred, so the room scrolls instead.
        assert_eq!(after[5].chars().nth(10), Some('@'));
        assert_ne!(before, after);
    }
}
