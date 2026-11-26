//! Screen layout and drawing.
//!
//! Two screens. The title menu is a small centred box. While playing:
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

use nighthawk_sim::{Mix, Pos, Tile, World};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};

use crate::app::{App, Screen};
use crate::title::{MenuItem, TitleMenu};

const SIDEBAR_WIDTH: u16 = 24;
const LOG_HEIGHT: u16 = 6;
const TITLE_WIDTH: u16 = 60;
/// Tall enough for the menu plus a few lines of wrapped message, and still
/// inside the 80×24 minimum terminal.
const TITLE_HEIGHT: u16 = 20;

/// Draws whichever screen the app is on.
pub fn render(app: &App, frame: &mut Frame) {
    match &app.screen {
        Screen::Title(menu) => render_title(menu, frame),
        Screen::Playing(world) => render_game(world, &app.log, app.show_air, frame),
    }
}

/// The title menu, centred on the screen.
fn render_title(menu: &TitleMenu, frame: &mut Frame) {
    let area = centred(frame.area(), TITLE_WIDTH, TITLE_HEIGHT);
    let dim = Style::new().fg(Color::DarkGray);
    let mut lines = vec![
        Line::styled(
            "N I G H T H A W K",
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Line::styled("A dead ship. One survivor. Take it back.", dim),
        Line::raw(""),
    ];
    lines.extend(MenuItem::ALL.into_iter().map(|item| menu_line(menu, item)));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "j/k or arrows: move   Enter: choose   n/c/q: pick",
        dim,
    ));
    if menu.can_continue() && menu.selected() == MenuItem::NewGame {
        lines.push(Line::styled(
            "A new game replaces your save the next time you save.",
            dim,
        ));
    }
    if let Some(message) = menu.message() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            message.to_string(),
            Style::new().fg(Color::Yellow),
        ));
    }
    // Wrap long lines (a load error names a file path) instead of cutting them off.
    let menu_box = Paragraph::new(lines)
        .centered()
        .wrap(Wrap { trim: true })
        .block(Block::bordered());
    frame.render_widget(menu_box, area);
}

/// One menu line: highlighted with `>` when selected, greyed out when off.
fn menu_line(menu: &TitleMenu, item: MenuItem) -> Line<'static> {
    if !menu.is_enabled(item) {
        let label = format!("  {} (no save)  ", item.label());
        return Line::styled(label, Style::new().fg(Color::DarkGray));
    }
    if menu.selected() == item {
        let style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        return Line::styled(format!("> {} <", item.label()), style);
    }
    Line::raw(format!("  {}  ", item.label()))
}

/// A `width` × `height` box in the middle of `area`, shrunk to fit if the
/// terminal is smaller than that.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let [_, row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, cell, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(row);
    cell
}

/// The game screen: map, status sidebar and message log.
fn render_game(world: &World, log_lines: &[String], show_air: bool, frame: &mut Frame) {
    let [top, log_area] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(LOG_HEIGHT)]).areas(frame.area());
    let [map_area, side_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(SIDEBAR_WIDTH)]).areas(top);

    let overlay_note = if show_air { " [air overlay]" } else { "" };
    let map_block = Block::bordered().title(format!(" {}{overlay_note} ", world.area_name()));
    let map_inner = map_block.inner(map_area);
    frame.render_widget(map_block, map_area);
    frame.render_widget(MapView { world, show_air }, map_inner);

    frame.render_widget(sidebar(world), side_area);
    frame.render_widget(log(log_lines, log_area.height.saturating_sub(2)), log_area);
}

/// Draws the map with the player in the middle of the view.
///
/// This is a custom widget: instead of using ratatui's text widgets, it writes
/// characters straight into the screen buffer, one cell per tile. The map
/// will always be drawn this way, because it's a grid, not text.
pub struct MapView<'a> {
    pub world: &'a World,
    /// Shade floor tiles by air pressure instead of drawing them as `.`.
    pub show_air: bool,
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
                let (glyph, style) = glyph_at(self.world, pos, self.show_air);
                buf[(area.x + col, area.y + row)]
                    .set_symbol(glyph)
                    .set_style(style);
            }
        }
    }
}

/// The character and colour for one map cell. The player is drawn on top of
/// whatever tile they stand on. Off-map cells are blank.
fn glyph_at(world: &World, pos: Pos, show_air: bool) -> (&'static str, Style) {
    if pos == world.player() {
        return (
            "@",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        );
    }
    match (world.map().get(pos), world.map().air(pos)) {
        (Some(Tile::Floor), Some(mix)) if show_air => air_glyph(mix),
        (tile, _) => tile.map_or((" ", Style::new()), tile_glyph),
    }
}

/// How full of air a floor tile looks on the air overlay: solid for normal
/// pressure, fading through lighter shades to `.` for near vacuum. The
/// shape alone tells the story; colour (cyan for air, red for vacuum) helps.
fn air_glyph(mix: Mix) -> (&'static str, Style) {
    // Pressure in eighths of normal ship air, so 8 is normal.
    let eighths = mix.total() * 8 / Mix::STANDARD.total();
    let air = Style::new().fg(Color::Cyan);
    match eighths {
        7.. => ("█", air),
        5..=6 => ("▓", air),
        3..=4 => ("▒", air),
        1..=2 => ("░", air),
        0 => (".", Style::new().fg(Color::Red)),
    }
}

/// How each kind of tile looks. `<` and `>` are the classic roguelike
/// glyphs for ways up and down.
fn tile_glyph(tile: Tile) -> (&'static str, Style) {
    match tile {
        Tile::Wall => ("#", Style::new().fg(Color::Gray)),
        Tile::Floor => (".", Style::new().fg(Color::DarkGray)),
        Tile::StairsUp => (
            "<",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Tile::StairsDown => (
            ">",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Tile::DoorClosed => ("+", Style::new().fg(Color::LightRed)),
        Tile::DoorOpen => ("'", Style::new().fg(Color::LightRed)),
    }
}

/// The status panel.
fn sidebar(world: &World) -> Paragraph<'static> {
    Paragraph::new(sidebar_lines(world)).block(Block::bordered().title(" Status "))
}

/// The status panel's lines. They must fit in 16, the space a minimum
/// 80×24 terminal leaves the panel.
fn sidebar_lines(world: &World) -> Vec<Line<'static>> {
    let p = world.player();
    let dim = Style::new().fg(Color::DarkGray);
    let mut lines = vec![
        Line::raw(format!("Deck  {} of {}", p.z, world.map().depth())),
        Line::raw(format!("Pos   {}, {}", p.x, p.y)),
        Line::raw(format!("Turn  {}", world.turn())),
        Line::raw(format!("Seed  {}", world.seed())),
        Line::raw(""),
    ];
    lines.extend(air_lines(world.map().air(p).unwrap_or(Mix::VACUUM)));
    lines.extend([
        Line::raw(""),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("wait .  climb < >", dim),
        Line::styled("close c  air a", dim),
        Line::styled("save S  menu q/Q", dim),
    ]);
    lines
}

/// The air where the player stands: pressure, then each gas's share.
/// The shares always add up to exactly 100.0% (see `Mix::per_mille`).
fn air_lines(mix: Mix) -> [Line<'static>; 3] {
    let Some([o2, n2, co2]) = mix.per_mille() else {
        return [
            Line::styled("Air   vacuum", Style::new().fg(Color::Red)),
            Line::raw("O2 --    N2 --"),
            Line::raw("CO2 --"),
        ];
    };
    [
        Line::raw(format!("Air   {} kPa", kilopascals(mix.total()))),
        Line::raw(format!("O2 {}  N2 {}", percent(o2), percent(n2))),
        Line::raw(format!("CO2 {}", percent(co2))),
    ]
}

/// Pascals as kilopascals with one decimal place, rounded: 101325 → "101.3".
fn kilopascals(pa: u64) -> String {
    let tenths = (pa + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Tenths of a percent as a percentage: 210 → "21.0%".
fn percent(per_mille: u32) -> String {
    format!("{}.{}%", per_mille / 10, per_mille % 10)
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
        World::new(parse_map(&json).unwrap(), 1)
    }

    /// Renders the map into an off-screen buffer and returns its rows as text.
    fn draw(world: &World, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        MapView {
            world,
            show_air: false,
        }
        .render(area, &mut buf);
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

    /// Renders the whole screen, as the game would, into a fake terminal.
    fn draw_screen(width: u16, height: u16, render: impl FnOnce(&mut Frame)) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(render).unwrap();
        let buf = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
            .collect()
    }

    fn game_screen(world: &World, log: &[String]) -> String {
        draw_screen(80, 24, |frame| render_game(world, log, false, frame))
    }

    fn title_screen(menu: &TitleMenu) -> String {
        draw_screen(80, 24, |frame| render_title(menu, frame))
    }

    #[test]
    fn whole_screen_shows_map_status_and_log() {
        let screen = game_screen(&test_world(), &["You wake in the stasis bay.".to_string()]);
        assert!(
            screen.contains(" Deck 0 "),
            "map panel is titled with the deck's name (a number when it has none)"
        );
        assert!(screen.contains("Turn  0"));
        assert!(screen.contains("Deck  0 of 1"));
        assert!(screen.contains("Seed  1"));
        assert!(screen.contains("save S  menu q/Q"));
        assert!(screen.contains("You wake in the stasis bay."));
    }

    #[test]
    fn log_panel_shows_the_newest_lines() {
        let log: Vec<String> = (0..20).map(|i| format!("entry {i}")).collect();
        let screen = game_screen(&test_world(), &log);
        assert!(screen.contains("entry 19"));
        assert!(!screen.contains("entry 0 "), "old lines scroll off");
    }

    #[test]
    fn title_shows_the_name_and_every_item() {
        let screen = title_screen(&TitleMenu::new(true));
        assert!(screen.contains("N I G H T H A W K"));
        assert!(screen.contains("New game"));
        assert!(
            screen.contains("> Continue <"),
            "Continue is highlighted when there's a save"
        );
        assert!(screen.contains("Quit"));
    }

    #[test]
    fn without_a_save_continue_is_greyed_out() {
        let screen = title_screen(&TitleMenu::new(false));
        assert!(screen.contains("Continue (no save)"));
        assert!(screen.contains("> New game <"));
    }

    #[test]
    fn choosing_new_game_over_a_save_warns_about_replacing_it() {
        let mut menu = TitleMenu::new(true);
        menu.up(); // Continue → New game
        assert!(title_screen(&menu).contains("replaces your save"));
        assert!(!title_screen(&TitleMenu::new(false)).contains("replaces your save"));
    }

    #[test]
    fn title_messages_are_shown() {
        let menu = TitleMenu::new(false).with_message(Some("Could not load: damaged".into()));
        assert!(title_screen(&menu).contains("Could not load: damaged"));
    }

    #[test]
    fn long_title_messages_wrap_instead_of_being_cut_off() {
        let message = "Could not load: /home/someone/.local/share/nighthawk/save.json: \
                       save file is damaged: key must be a string at line 1 column 3";
        let menu = TitleMenu::new(true).with_message(Some(message.into()));
        // Join the screen's rows back up and squash the spaces, then the
        // whole message must be there, end to end.
        let squashed: String = title_screen(&menu).split_whitespace().collect();
        let expected: String = message.split_whitespace().collect();
        let inside: String = squashed.replace('│', "");
        assert!(
            inside.contains(&expected),
            "message was cut off:\n{}",
            title_screen(&menu)
        );
    }

    #[test]
    fn the_title_fits_an_80_by_24_terminal_and_shrinks_below_it() {
        // 80×24 is the minimum supported size; smaller must not panic.
        assert!(title_screen(&TitleMenu::new(true)).contains("Continue"));
        let tiny = draw_screen(20, 6, |frame| render_title(&TitleMenu::new(true), frame));
        assert_eq!(tiny.lines().count(), 6);
    }

    #[test]
    fn render_draws_whichever_screen_the_app_is_on() {
        let map = crate::content::load_start_map().unwrap();
        let on_title = crate::app::App::new(map.clone(), None, None, || 1);
        assert!(draw_screen(80, 24, |f| render(&on_title, f)).contains("N I G H T H A W K"));
        let playing = crate::app::App::new(map, None, Some(5), || 1);
        assert!(draw_screen(80, 24, |f| render(&playing, f)).contains("Seed  5"));
    }

    #[test]
    fn every_tile_kind_has_a_distinct_glyph() {
        let tiles = [
            Tile::Floor,
            Tile::Wall,
            Tile::StairsUp,
            Tile::StairsDown,
            Tile::DoorClosed,
            Tile::DoorOpen,
        ];
        let glyphs: Vec<&str> = tiles.iter().map(|&t| tile_glyph(t).0).collect();
        assert_eq!(glyphs, vec![".", "#", "<", ">", "+", "'"]);
    }

    #[test]
    fn the_real_map_shows_its_ladder_and_deck_name() {
        let map = crate::content::load_start_map().unwrap();
        let mut world = World::new(map, 1);
        // Walk to the ladder at (27, 2) and look at the screen there.
        for _ in 0..5 {
            world.apply(nighthawk_sim::Command::Move(Dir::North));
        }
        for _ in 0..22 {
            world.apply(nighthawk_sim::Command::Move(Dir::East));
        }
        let screen = game_screen(&world, &[]);
        assert!(
            screen.contains(" Stasis bay "),
            "deck name is the panel title"
        );
        assert!(screen.contains("@<"), "the ladder is right of the player");
        assert!(screen.contains("Deck  0 of 2"));
    }

    // --- Air ------------------------------------------------------------------

    #[test]
    fn pressures_and_shares_are_formatted_with_one_decimal() {
        assert_eq!(kilopascals(101_325), "101.3");
        assert_eq!(kilopascals(60_795), "60.8");
        assert_eq!(kilopascals(49), "0.0");
        assert_eq!(kilopascals(50), "0.1");
        assert_eq!(percent(210), "21.0%");
        assert_eq!(percent(1000), "100.0%");
        assert_eq!(percent(4), "0.4%");
    }

    fn texts(lines: [Line<'static>; 3]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_sidebar_reads_out_the_air() {
        assert_eq!(
            texts(air_lines(Mix::STANDARD)),
            vec!["Air   101.3 kPa", "O2 21.0%  N2 79.0%", "CO2 0.0%"]
        );
        assert_eq!(texts(air_lines(Mix::VACUUM))[0], "Air   vacuum");
    }

    #[test]
    fn the_overlay_shades_floor_by_pressure() {
        let shade = |percent| air_glyph(Mix::STANDARD.scaled(percent)).0;
        assert_eq!(shade(100), "█");
        assert_eq!(shade(150), "█", "over-pressure is still full");
        assert_eq!(shade(70), "▓");
        assert_eq!(shade(50), "▒");
        assert_eq!(shade(20), "░");
        assert_eq!(shade(5), ".");
        assert_eq!(shade(0), ".");
    }

    #[test]
    fn the_sidebar_fits_a_minimum_terminal() {
        // 80×24 leaves 24 - 6 (log) - 2 (borders) = 16 lines.
        let lines = sidebar_lines(&test_world());
        assert!(lines.len() <= 16, "{} lines", lines.len());
        for line in &lines {
            assert!(
                line.width() <= usize::from(SIDEBAR_WIDTH - 2),
                "too wide: {line}"
            );
        }
    }

    #[test]
    fn the_air_overlay_shows_thin_air_upstairs_and_is_labelled() {
        let map = crate::content::load_start_map().unwrap();
        let mut world = World::new(map, 1);
        let full = draw_screen(80, 24, |f| render_game(&world, &[], true, f));
        assert!(full.contains("[air overlay]"));
        assert!(full.contains("███"), "the stasis bay is at full pressure");

        // Climb to the crew deck, which starts at 60% pressure.
        use nighthawk_sim::Command::{Climb, Move};
        let route = std::iter::repeat_n(Move(Dir::North), 5)
            .chain(std::iter::repeat_n(Move(Dir::East), 23))
            .chain([Climb(nighthawk_sim::Vertical::Up)]);
        for command in route {
            world.apply(command);
        }
        let upstairs = draw_screen(80, 24, |f| render_game(&world, &[], true, f));
        assert!(upstairs.contains("▒▒▒"), "thin air upstairs:\n{upstairs}");
        let plain = draw_screen(80, 24, |f| render_game(&world, &[], false, f));
        assert!(!plain.contains("▒") && !plain.contains("[air overlay]"));
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
