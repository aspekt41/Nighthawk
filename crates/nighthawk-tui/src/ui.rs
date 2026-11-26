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

    let map_block = Block::bordered().title(" Stasis bay ");
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
                let (glyph, style) = if pos == player {
                    (
                        "@",
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                } else {
                    match self.world.map().get(pos) {
                        Some(Tile::Wall) => ("#", Style::new().fg(Color::Gray)),
                        Some(Tile::Floor) => (".", Style::new().fg(Color::DarkGray)),
                        None => (" ", Style::new()),
                    }
                };
                buf[(area.x + col, area.y + row)]
                    .set_symbol(glyph)
                    .set_style(style);
            }
        }
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
    use nighthawk_sim::{Command, Dir};

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
        let world = World::stasis_bay();
        let rows = draw(&world, 21, 11);
        assert_eq!(rows[5].chars().nth(10), Some('@'));
    }

    #[test]
    fn view_follows_the_player() {
        let mut world = World::stasis_bay();
        let before = draw(&world, 21, 11);
        world.apply(Command::Move(Dir::East));
        let after = draw(&world, 21, 11);
        // The @ stays centred, so the room scrolls instead.
        assert_eq!(after[5].chars().nth(10), Some('@'));
        assert_ne!(before, after);
    }
}
