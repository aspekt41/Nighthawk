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

use nighthawk_sim::{
    ActionOption, BREATH_FULL, Colour, Material, Mix, Pos, Quality, Tile, WARMTH_FULL, World,
    celsius_to_mk, mk_to_tenths_celsius, options_at,
};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};

use crate::app::{App, Screen};
use crate::title::{MenuItem, TitleMenu};
use crate::{keys, text};

const SIDEBAR_WIDTH: u16 = 24;
const LOG_HEIGHT: u16 = 6;
const TITLE_WIDTH: u16 = 60;
/// Tall enough for the menu plus a few lines of wrapped message, and still
/// inside the 80×24 minimum terminal.
const TITLE_HEIGHT: u16 = 20;

/// A way of shading the map to show one part of the simulation (design
/// pillar 4: if a system matters, it has an overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Floor shaded by air pressure (`a`).
    Air,
    /// Floor shaded by air temperature (`t`).
    Heat,
}

impl Overlay {
    /// The note added to the map panel's title while this overlay is on.
    fn label(self) -> &'static str {
        match self {
            Overlay::Air => " [air overlay]",
            Overlay::Heat => " [heat overlay]",
        }
    }
}

/// What the cursor is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMode {
    /// Look mode (`V`): describe the tile.
    Look,
    /// The interact menu (`e`): list what you can do to the tile.
    Interact,
}

/// The cursor for look mode and the interact menu: where it is, and which
/// of the two it's for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub at: Pos,
    pub mode: CursorMode,
}

/// How the player has asked to see the game: view settings that change
/// nothing in the world. `Default` gives the plain map, no cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct View {
    pub overlay: Option<Overlay>,
    pub cursor: Option<Cursor>,
}

/// Draws whichever screen the app is on.
pub fn render(app: &App, frame: &mut Frame) {
    let view = View {
        overlay: app.overlay,
        cursor: app.cursor,
    };
    match &app.screen {
        Screen::Title(menu) => render_title(menu, frame),
        Screen::Playing(world) => render_game(world, &app.log, view, frame),
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
///
/// With the cursor up, the map centres on it and the side panel is about
/// the tile under it: a description in look mode (with the material's
/// description in place of the log), or what you can do in the interact
/// menu (keeping the log, which says why an action can't be done).
fn render_game(world: &World, log_lines: &[String], view: View, frame: &mut Frame) {
    let [top, log_area] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(LOG_HEIGHT)]).areas(frame.area());
    let [map_area, side_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(SIDEBAR_WIDTH)]).areas(top);

    let overlay_note = view.overlay.map_or("", Overlay::label);
    let cursor_note = match view.cursor.map(|c| c.mode) {
        Some(CursorMode::Look) => " [looking]",
        Some(CursorMode::Interact) => " [interact]",
        None => "",
    };
    let title = format!(" {}{overlay_note}{cursor_note} ", world.area_name());
    let map_block = Block::bordered().title(title);
    let map_inner = map_block.inner(map_area);
    frame.render_widget(map_block, map_area);
    let map_view = MapView {
        world,
        overlay: view.overlay,
        cursor: view.cursor.map(|c| c.at),
    };
    frame.render_widget(map_view, map_inner);

    let log_lines_shown = log_area.height.saturating_sub(2);
    match view.cursor {
        Some(Cursor {
            at,
            mode: CursorMode::Look,
        }) => {
            frame.render_widget(look_panel(world, at), side_area);
            frame.render_widget(description(world, at), log_area);
        }
        Some(Cursor {
            at,
            mode: CursorMode::Interact,
        }) => {
            frame.render_widget(interact_panel(world, at), side_area);
            frame.render_widget(log(log_lines, log_lines_shown), log_area);
        }
        None => {
            frame.render_widget(sidebar(world), side_area);
            frame.render_widget(log(log_lines, log_lines_shown), log_area);
        }
    }
}

/// Draws the map with the player in the middle of the view.
///
/// This is a custom widget: instead of using ratatui's text widgets, it writes
/// characters straight into the screen buffer, one cell per tile. The map
/// will always be drawn this way, because it's a grid, not text.
pub struct MapView<'a> {
    pub world: &'a World,
    /// Shade floor tiles by air pressure or temperature instead of drawing
    /// them as `.`.
    pub overlay: Option<Overlay>,
    /// The look cursor, if looking: the view centres on it and highlights
    /// it. Otherwise the view centres on the player.
    pub cursor: Option<Pos>,
}

impl Widget for MapView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let centre = self.cursor.unwrap_or(self.world.player());
        // The map position that lands in the top-left cell of `area`.
        let origin_x = centre.x - i32::from(area.width) / 2;
        let origin_y = centre.y - i32::from(area.height) / 2;

        for row in 0..area.height {
            for col in 0..area.width {
                let pos = Pos::new(
                    origin_x + i32::from(col),
                    origin_y + i32::from(row),
                    centre.z,
                );
                let (glyph, style) = self.cell(pos);
                buf[(area.x + col, area.y + row)]
                    .set_symbol(glyph)
                    .set_style(style);
            }
        }
    }
}

impl MapView<'_> {
    /// The character and style for the map cell at `pos`, highlighted
    /// (drawn in reverse) if it's under the look cursor.
    fn cell(&self, pos: Pos) -> (&'static str, Style) {
        let (glyph, style) = glyph_at(self.world, pos, self.overlay);
        if Some(pos) == self.cursor {
            return (glyph, style.add_modifier(Modifier::REVERSED));
        }
        (glyph, style)
    }
}

/// The character and colour for one map cell. The player is drawn on top of
/// whatever tile they stand on. Off-map cells are blank.
fn glyph_at(world: &World, pos: Pos, overlay: Option<Overlay>) -> (&'static str, Style) {
    if pos == world.player() {
        return (
            "@",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        );
    }
    let map = world.map();
    match (map.get(pos), overlay) {
        (Some(Tile::Floor), Some(Overlay::Air)) => air_glyph(map.air(pos).unwrap_or(Mix::VACUUM)),
        (Some(Tile::Floor), Some(Overlay::Heat)) => heat_glyph(map.temperature(pos)),
        // Walls take their material's colour, so the hull stands out from
        // the bulkheads inside.
        (Some(Tile::Wall), _) => {
            let colour = world
                .material_at(pos)
                .map_or(Color::Gray, |m| colour(m.colour));
            ("#", Style::new().fg(colour))
        }
        (tile, _) => tile.map_or((" ", Style::new()), tile_glyph),
    }
}

/// The terminal colour for a material's colour.
fn colour(c: Colour) -> Color {
    match c {
        Colour::White => Color::White,
        Colour::Grey => Color::Gray,
        Colour::DarkGrey => Color::DarkGray,
        Colour::Red => Color::Red,
        Colour::Yellow => Color::Yellow,
        Colour::Green => Color::Green,
        Colour::Cyan => Color::Cyan,
        Colour::Blue => Color::Blue,
        Colour::Magenta => Color::Magenta,
    }
}

/// How warm a floor tile looks on the heat overlay. The further from
/// comfortable, the denser the block: green `░` is comfortable (10 to
/// 30 °C), cyan `▒` chilly, light blue `▓` cold (below 0 °C), blue `█`
/// freezing (below −15 °C), red `█` hot. Vacuum has no temperature and
/// shows a dim `.`.
fn heat_glyph(mk: Option<u32>) -> (&'static str, Style) {
    let Some(mk) = mk else {
        return (".", Style::new().fg(Color::DarkGray));
    };
    let (_, glyph, colour) = HEAT_BANDS
        .iter()
        .find(|&&(below, _, _)| mk < below)
        .copied()
        .unwrap_or(HOT);
    (glyph, Style::new().fg(colour))
}

/// The heat overlay's bands, coldest first: each is (below this
/// temperature in mK, glyph, colour). A table rather than a `match` so the
/// bands are easy to read and change. Anything warmer than the last band
/// is [`HOT`].
const HEAT_BANDS: [(u32, &str, Color); 4] = [
    (celsius_to_mk(-15), "█", Color::Blue),
    (celsius_to_mk(0), "▓", Color::LightBlue),
    (celsius_to_mk(10), "▒", Color::Cyan),
    (celsius_to_mk(30), "░", Color::Green),
];
const HOT: (u32, &str, Color) = (u32::MAX, "█", Color::Red);

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
        // Drawn with the key that uses them (D31): `>` up, `<` down.
        Tile::StairsUp => (
            ">",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Tile::StairsDown => (
            "<",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Tile::DoorClosed => ("+", Style::new().fg(Color::LightRed)),
        Tile::DoorOpen => ("'", Style::new().fg(Color::LightRed)),
        Tile::HullCrack => ("x", Style::new().fg(Color::Red)),
        Tile::HullBreach => (
            "X",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
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
        Line::raw(format!(
            "Deck {}/{}  Pos {}, {}",
            p.z,
            world.map().depth(),
            p.x,
            p.y
        )),
        Line::raw(format!("Date  {}", world.now().date_text())),
        Line::raw(format!("Time  {}", world.now().time_text())),
        Line::raw(format!("Seed  {}", world.seed())),
        Line::raw(""),
        meter_line("Breath", world.breath(), BREATH_FULL),
        meter_line("Warmth", world.warmth(), WARMTH_FULL),
    ];
    lines.extend(air_lines(world.map().air(p).unwrap_or(Mix::VACUUM)));
    lines.push(temperature_line(world.map().temperature(p)));
    lines.extend([
        Line::raw(""),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("wait .  climb < >", dim),
        Line::styled("close c  air a  heat t", dim),
        Line::styled("save S  menu q/Q", dim),
    ]);
    lines
}

/// One of the player's meters (breath, warmth): a percentage and a
/// five-block gauge, green when it's fine, yellow when it's dropping, red
/// when it's nearly gone.
fn meter_line(label: &str, value: u32, full: u32) -> Line<'static> {
    // Worked in u64: warmth counts in millionths, and five times a million
    // is fine, but a percentage of it is safer with room to spare.
    let (value, full) = (u64::from(value), u64::from(full));
    let percent = value * 100 / full;
    let blocks = (value * 5).div_ceil(full) as usize; // any amount shows a block
    let gauge = format!("{}{}", "█".repeat(blocks), "░".repeat(5 - blocks));
    let colour = match percent {
        75.. => Color::Green,
        25..=74 => Color::Yellow,
        _ => Color::Red,
    };
    Line::styled(
        format!("{label} {percent:>3}% {gauge}"),
        Style::new().fg(colour),
    )
}

/// The temperature where the player stands, in the colour the heat
/// overlay uses for it.
fn temperature_line(mk: Option<u32>) -> Line<'static> {
    let Some(kelvin) = mk else {
        return Line::raw("Temp  --");
    };
    let style = heat_glyph(mk).1;
    Line::styled(format!("Temp  {}", celsius(kelvin)), style)
}

/// A temperature in mK as °C with one decimal place: 253_150 → "-20.0 °C".
fn celsius(mk: u32) -> String {
    let tenths = mk_to_tenths_celsius(mk);
    let sign = if tenths < 0 { "-" } else { "" };
    let tenths = tenths.unsigned_abs();
    format!("{sign}{}.{} °C", tenths / 10, tenths % 10)
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

/// The look panel: what's at `at`, what it's made of, and the air there.
/// Like the status panel, it must fit in 16 lines.
fn look_panel(world: &World, at: Pos) -> Paragraph<'static> {
    Paragraph::new(look_lines(world, at)).block(Block::bordered().title(" Look "))
}

fn look_lines(world: &World, at: Pos) -> Vec<Line<'static>> {
    let dim = Style::new().fg(Color::DarkGray);
    let here = if at == world.player() {
        Line::styled("You are here.", Style::new().fg(Color::Yellow))
    } else {
        Line::raw("")
    };
    let mut lines = vec![
        Line::raw(format!("Look  {}, {}  deck {}", at.x, at.y, at.z)),
        here,
    ];
    let tile = world.map().get(at).unwrap_or(Tile::Wall);
    lines.push(Line::raw(tile_name(world, at, tile)));
    if let Some(material) = world.material_at(at) {
        lines.extend(material_lines(material));
    }
    lines.push(Line::raw(""));
    lines.extend(air_here(world, at, tile));
    lines.extend([
        Line::raw(""),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("Esc   stop looking", dim),
    ]);
    lines
}

/// A material's name, in its colour, and its properties.
fn material_lines(m: &Material) -> Vec<Line<'static>> {
    vec![
        Line::styled(m.name.clone(), Style::new().fg(colour(m.colour))),
        Line::raw(format!("Density  {} kg/m³", m.density)),
        Line::raw(format!("Melts    {} °C", m.melting_point)),
        Line::raw(format!("Hardness {} HV", m.hardness)),
        Line::raw(format!("Heat cap {} J/kg·K", m.specific_heat)),
        Line::raw(format!("Conducts {} W/m·K", m.thermal_conductivity)),
        Line::raw(format!("Cut by   {}", cut_by(m))),
    ]
}

/// The qualities that cut a material, like "LASER 2", or "nothing".
fn cut_by(m: &Material) -> String {
    if m.cut_by.is_empty() {
        return "nothing".into();
    }
    let each: Vec<String> = m
        .cut_by
        .iter()
        .map(|(q, level): (&Quality, &u8)| format!("{} {level}", q.name()))
        .collect();
    each.join(", ")
}

/// The air at a looked-at tile: its pressure and temperature, or that
/// it's solid.
fn air_here(world: &World, at: Pos, tile: Tile) -> Vec<Line<'static>> {
    if !tile.holds_air() {
        return vec![Line::raw("Solid: no air")];
    }
    let mix = world.map().air(at).unwrap_or(Mix::VACUUM);
    let [pressure, ..] = air_lines(mix);
    vec![pressure, temperature_line(world.map().temperature(at))]
}

/// What to call the tile at `at`: "Outer hull" for a wall on the hull,
/// otherwise the shape's own name.
fn tile_name(world: &World, at: Pos, tile: Tile) -> String {
    if tile == Tile::Wall && world.map().is_outer_hull(at) {
        return "Outer hull".into();
    }
    capitalised(tile.name())
}

/// "ladder up" → "Ladder up".
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The interact menu's panel: the tile, then each action with its key,
/// greyed with a short reason when it can't be done.
fn interact_panel(world: &World, at: Pos) -> Paragraph<'static> {
    Paragraph::new(interact_lines(world, at)).block(Block::bordered().title(" Interact "))
}

fn interact_lines(world: &World, at: Pos) -> Vec<Line<'static>> {
    let dim = Style::new().fg(Color::DarkGray);
    let tile = world.map().get(at).unwrap_or(Tile::Wall);
    let mut lines = vec![
        Line::raw(format!("At    {}, {}  deck {}", at.x, at.y, at.z)),
        Line::raw(tile_name(world, at, tile)),
    ];
    if let Some(material) = world.material_at(at) {
        lines.push(Line::styled(
            material.name.clone(),
            Style::new().fg(colour(material.colour)),
        ));
    }
    lines.push(Line::raw(""));
    let options = options_at(world, at);
    if options.is_empty() {
        lines.push(Line::styled("Nothing to do here.", dim));
    }
    for option in options {
        lines.extend(option_lines(option));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("Enter first option", dim),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("Esc   cancel", dim),
    ]);
    lines
}

/// One option: "o  Open", and if it can't be done, greyed with the reason
/// on the next line.
fn option_lines(option: ActionOption) -> Vec<Line<'static>> {
    let key = keys::action_key(option.action);
    let label = format!("{key}  {}", text::action_name(option.action));
    match option.blocked {
        None => vec![Line::styled(label, Style::new().fg(Color::Yellow))],
        Some(blocked) => {
            let dim = Style::new().fg(Color::DarkGray);
            vec![
                Line::styled(label, dim),
                Line::styled(format!("   {}", text::blocked_short(blocked)), dim),
            ]
        }
    }
}

/// The bottom panel in look mode: the material's description, wrapped.
fn description(world: &World, at: Pos) -> Paragraph<'static> {
    let text = world
        .material_at(at)
        .map_or(String::new(), |m| m.description.clone());
    Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .block(Block::bordered().title(" Made of "))
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
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let materials = crate::content::load_materials(&data).unwrap();
        World::new(parse_map(&json, &materials).unwrap(), 1)
    }

    /// Renders the map into an off-screen buffer and returns its rows as text.
    fn draw(world: &World, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        MapView {
            world,
            overlay: None,
            cursor: None,
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
        draw_screen(80, 24, |frame| {
            render_game(world, log, View::default(), frame)
        })
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
        assert!(screen.contains("Date  25 Nov 2631"));
        assert!(screen.contains("Time  08:42:00"));
        assert!(screen.contains("Breath 100%"));
        assert!(screen.contains("Warmth 100%"));
        assert!(screen.contains("Temp  20.0 °C"));
        assert!(screen.contains("Deck 0/1  Pos 3, 2"));
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
            Tile::HullCrack,
            Tile::HullBreach,
        ];
        let glyphs: Vec<&str> = tiles.iter().map(|&t| tile_glyph(t).0).collect();
        assert_eq!(glyphs, vec![".", "#", ">", "<", "+", "'", "x", "X"]);
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
        assert!(
            screen.contains("@>"),
            "the ladder up is right of the player"
        );
        assert!(screen.contains("Deck 0/2  Pos 26, 2"));
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
    fn the_breath_gauge_shows_percent_blocks_and_colour() {
        let line = |b| {
            let l = meter_line("Breath", b, BREATH_FULL);
            (l.to_string(), l.style.fg)
        };
        assert_eq!(line(1000), ("Breath 100% █████".into(), Some(Color::Green)));
        assert_eq!(line(750), ("Breath  75% ████░".into(), Some(Color::Green)));
        assert_eq!(line(500), ("Breath  50% ███░░".into(), Some(Color::Yellow)));
        assert_eq!(line(100), ("Breath  10% █░░░░".into(), Some(Color::Red)));
        assert_eq!(
            line(5),
            ("Breath   0% █░░░░".into(), Some(Color::Red)),
            "a sliver still shows"
        );
        assert_eq!(line(0), ("Breath   0% ░░░░░".into(), Some(Color::Red)));
    }

    #[test]
    fn the_warmth_gauge_counts_in_millionths() {
        let line = |w| meter_line("Warmth", w, WARMTH_FULL).to_string();
        assert_eq!(line(WARMTH_FULL), "Warmth 100% █████");
        assert_eq!(line(499_999), "Warmth  49% ███░░");
        assert_eq!(line(1), "Warmth   0% █░░░░");
    }

    // --- Heat -----------------------------------------------------------------

    #[test]
    fn temperatures_show_in_celsius_with_one_decimal() {
        assert_eq!(celsius(celsius_to_mk(20)), "20.0 °C");
        assert_eq!(celsius(celsius_to_mk(-20)), "-20.0 °C");
        assert_eq!(celsius(celsius_to_mk(0) - 450), "-0.5 °C");
        assert_eq!(celsius(celsius_to_mk(0) + 40), "0.0 °C");
    }

    #[test]
    fn the_sidebar_reads_out_the_temperature_in_the_overlay_colour() {
        let warm = temperature_line(Some(celsius_to_mk(20)));
        assert_eq!(warm.to_string(), "Temp  20.0 °C");
        assert_eq!(warm.style.fg, Some(Color::Green));
        let cold = temperature_line(Some(celsius_to_mk(-5)));
        assert_eq!(cold.style.fg, Some(Color::LightBlue));
        assert_eq!(temperature_line(None).to_string(), "Temp  --");
    }

    #[test]
    fn the_heat_overlay_gets_denser_the_further_from_comfortable() {
        let band = |c| {
            let (glyph, style) = heat_glyph(Some(celsius_to_mk(c)));
            (glyph, style.fg.unwrap())
        };
        assert_eq!(band(-60), ("█", Color::Blue));
        assert_eq!(band(-16), ("█", Color::Blue));
        assert_eq!(band(-15), ("▓", Color::LightBlue), "-15 is the edge");
        assert_eq!(band(-1), ("▓", Color::LightBlue));
        assert_eq!(band(5), ("▒", Color::Cyan));
        assert_eq!(band(20), ("░", Color::Green));
        assert_eq!(band(30), ("█", Color::Red), "30 is hot");
        assert_eq!(heat_glyph(None).0, ".", "vacuum has no temperature");
    }

    #[test]
    fn the_heat_overlay_shows_the_cold_crew_deck_and_is_labelled() {
        let map = crate::content::load_start_map().unwrap();
        let mut world = World::new(map, 1);
        let heat = View {
            overlay: Some(Overlay::Heat),
            cursor: None,
        };
        let bay = draw_screen(80, 24, |f| render_game(&world, &[], heat, f));
        assert!(bay.contains("[heat overlay]"));
        assert!(bay.contains("░░░"), "the stasis bay is comfortable, just");

        use nighthawk_sim::Command::{Climb, Move};
        let route = std::iter::repeat_n(Move(Dir::North), 5)
            .chain(std::iter::repeat_n(Move(Dir::East), 23))
            .chain([Climb(nighthawk_sim::Vertical::Up)]);
        for command in route {
            world.apply(command);
        }
        let upstairs = draw_screen(80, 24, |f| render_game(&world, &[], heat, f));
        assert!(
            upstairs.contains("▓▓▓"),
            "the corridor is cold:\n{upstairs}"
        );
        assert!(
            upstairs.contains("███"),
            "the shut cabins are freezing:\n{upstairs}"
        );
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

    fn air() -> View {
        View {
            overlay: Some(Overlay::Air),
            cursor: None,
        }
    }

    #[test]
    fn the_air_overlay_shows_thin_air_upstairs_and_is_labelled() {
        let map = crate::content::load_start_map().unwrap();
        let mut world = World::new(map, 1);
        let full = draw_screen(80, 24, |f| render_game(&world, &[], air(), f));
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
        let upstairs = draw_screen(80, 24, |f| render_game(&world, &[], air(), f));
        assert!(upstairs.contains("▒▒▒"), "thin air upstairs:\n{upstairs}");
        let plain = draw_screen(80, 24, |f| render_game(&world, &[], View::default(), f));
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

    // --- Look mode and materials -----------------------------------------------

    fn real_world() -> World {
        World::new(crate::content::load_start_map().unwrap(), 1)
    }

    fn look_texts(world: &World, at: Pos) -> Vec<String> {
        look_lines(world, at)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn looking_at_the_hull_shows_its_material_and_laser_level() {
        let world = real_world();
        let lines = look_texts(&world, Pos::new(0, 7, 0));
        assert_eq!(lines[0], "Look  0, 7  deck 0");
        assert_eq!(lines[2], "Outer hull");
        assert_eq!(lines[3], "Nanocarbon Alloy");
        assert!(lines.contains(&"Cut by   LASER 5".to_string()));
        assert!(lines.contains(&"Solid: no air".to_string()));
    }

    #[test]
    fn looking_at_your_own_tile_shows_the_air_and_says_you_are_here() {
        let world = real_world();
        let lines = look_texts(&world, world.player());
        assert_eq!(lines[1], "You are here.");
        assert_eq!(lines[2], "Floor");
        assert_eq!(lines[3], "Steel");
        assert!(lines.contains(&"Air   101.3 kPa".to_string()));
        assert!(lines.contains(&"Temp  14.0 °C".to_string()));
    }

    #[test]
    fn the_look_panel_fits_a_minimum_terminal_wherever_it_points() {
        let world = real_world();
        for at in world.map().positions() {
            let lines = look_lines(&world, at);
            assert!(lines.len() <= 16, "{at:?}: {} lines", lines.len());
            for line in &lines {
                assert!(line.width() <= usize::from(SIDEBAR_WIDTH - 2), "{line}");
            }
        }
    }

    #[test]
    fn a_material_nothing_can_cut_says_so() {
        let mut material = real_world().materials()[0].clone();
        assert_eq!(cut_by(&material), "LASER 2");
        material.cut_by.clear();
        assert_eq!(cut_by(&material), "nothing");
    }

    #[test]
    fn the_look_screen_is_labelled_and_shows_the_description() {
        let world = real_world();
        let view = View {
            overlay: None,
            cursor: Some(Cursor {
                at: Pos::new(0, 7, 0),
                mode: CursorMode::Look,
            }),
        };
        let screen = draw_screen(80, 24, |f| render_game(&world, &[], view, f));
        assert!(screen.contains("[looking]"));
        assert!(screen.contains(" Made of "));
        assert!(screen.contains("The outer hull"));
        assert!(!screen.contains(" Status "), "the look panel replaces it");
    }

    #[test]
    fn the_cursor_is_highlighted_and_the_view_centres_on_it() {
        let world = real_world();
        let at = Pos::new(1, 1, 0);
        let area = Rect::new(0, 0, 21, 11);
        let mut buf = Buffer::empty(area);
        let view = MapView {
            world: &world,
            overlay: None,
            cursor: Some(at),
        };
        view.render(area, &mut buf);
        let centre = &buf[(10, 5)];
        assert_eq!(centre.symbol(), ".");
        assert!(centre.modifier.contains(Modifier::REVERSED));
        assert!(!buf[(9, 5)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn walls_are_drawn_in_their_materials_colour() {
        let world = real_world();
        let hull = glyph_at(&world, Pos::new(0, 7, 0), None);
        let pillar = glyph_at(&world, Pos::new(9, 5, 0), None);
        assert_eq!(hull, ("#", Style::new().fg(Color::DarkGray)));
        assert_eq!(pillar, ("#", Style::new().fg(Color::Gray)));
    }

    #[test]
    fn every_material_colour_has_a_terminal_colour() {
        let all = [
            Colour::White,
            Colour::Grey,
            Colour::DarkGrey,
            Colour::Red,
            Colour::Yellow,
            Colour::Green,
            Colour::Cyan,
            Colour::Blue,
            Colour::Magenta,
        ];
        let colours: Vec<Color> = all.iter().map(|&c| colour(c)).collect();
        for (i, c) in colours.iter().enumerate() {
            assert!(!colours[i + 1..].contains(c), "{c:?} used twice");
        }
    }

    #[test]
    fn names_are_capitalised_for_the_panel() {
        assert_eq!(capitalised("ladder up"), "Ladder up");
        assert_eq!(capitalised(""), "");
    }

    // --- Interact menu -----------------------------------------------------------

    fn interact_texts(world: &World, at: Pos) -> Vec<String> {
        interact_lines(world, at)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn the_interact_panel_lists_options_with_keys_and_reasons() {
        let world = real_world();
        // From the start (4, 7), the hull at (0, 7) is four tiles west.
        let lines = interact_texts(&world, Pos::new(0, 7, 0));
        assert_eq!(lines[0], "At    0, 7  deck 0");
        assert_eq!(lines[1], "Outer hull");
        assert_eq!(lines[2], "Nanocarbon Alloy");
        assert_eq!(lines[4], "x  Cut");
        assert_eq!(lines[5], "   too far: reach 2");
    }

    #[test]
    fn open_floor_has_nothing_to_do() {
        let world = real_world();
        let lines = interact_texts(&world, world.player());
        assert!(lines.contains(&"Nothing to do here.".to_string()));
    }

    #[test]
    fn options_that_can_be_done_are_highlighted() {
        let open = option_lines(ActionOption {
            action: nighthawk_sim::Action::Open,
            blocked: None,
        });
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].to_string(), "o  Open");
        assert_eq!(open[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn the_interact_panel_fits_a_minimum_terminal_wherever_it_points() {
        let world = real_world();
        for at in world.map().positions() {
            let lines = interact_lines(&world, at);
            assert!(lines.len() <= 16, "{at:?}: {} lines", lines.len());
            for line in &lines {
                assert!(line.width() <= usize::from(SIDEBAR_WIDTH - 2), "{line}");
            }
        }
    }

    #[test]
    fn the_interact_screen_is_labelled_and_keeps_the_log() {
        let world = real_world();
        let view = View {
            overlay: None,
            cursor: Some(Cursor {
                at: world.player(),
                mode: CursorMode::Interact,
            }),
        };
        let log = ["You need a tool with LASER 1 for that.".to_string()];
        let screen = draw_screen(80, 24, |f| render_game(&world, &log, view, f));
        assert!(screen.contains("[interact]"));
        assert!(screen.contains(" Interact "));
        assert!(screen.contains("LASER 1 for that"), "the log stays visible");
    }
}
