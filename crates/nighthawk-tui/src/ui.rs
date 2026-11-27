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
    ActionOption, BREATH_FULL, Colour, CraftMaterial, Item, ItemRef, Levels, Material, Mix, Piece,
    PocketType, Pos, Tile, Use, WARMTH_FULL, World, celsius_to_mk, mk_to_tenths_celsius,
    options_at,
};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};

use crate::app::{App, Screen};
use crate::text;
use crate::title::{MenuItem, TitleMenu};

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
    /// The highlighted item, while the inventory screen is open.
    pub inventory: Option<usize>,
    /// The highlighted craft, while the crafting screen is open.
    pub crafting: Option<usize>,
}

/// Draws whichever screen the app is on.
pub fn render(app: &App, frame: &mut Frame) {
    let view = View {
        overlay: app.overlay,
        cursor: app.cursor,
        inventory: app.inventory,
        crafting: app.crafting,
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
    // The inventory or crafting screen, if open, takes the map's place.
    match (view.inventory, view.crafting) {
        (Some(selected), _) => frame.render_widget(inventory_panel(world, selected), map_area),
        (None, Some(selected)) => frame.render_widget(crafting_panel(world, selected), map_area),
        (None, None) => frame.render_widget(map_view, map_inner),
    }

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
                    .set_symbol(&glyph)
                    .set_style(style);
            }
        }
    }
}

impl MapView<'_> {
    /// The character and style for the map cell at `pos`, highlighted
    /// (drawn in reverse) if it's under the look cursor.
    fn cell(&self, pos: Pos) -> (String, Style) {
        let (glyph, style) = glyph_at(self.world, pos, self.overlay);
        if Some(pos) == self.cursor {
            return (glyph, style.add_modifier(Modifier::REVERSED));
        }
        (glyph, style)
    }
}

/// The character and colour for one map cell. The player is drawn on top of
/// whatever tile they stand on. Tiles out of sight are drawn as
/// remembered; off-map cells are blank.
fn glyph_at(world: &World, pos: Pos, overlay: Option<Overlay>) -> (String, Style) {
    if pos == world.player() {
        let style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        return ("@".into(), style);
    }
    // Off the map is never in sight, so it goes the way of the unseen.
    match world.map().get(pos).filter(|_| world.is_visible(pos)) {
        Some(tile) => visible_glyph(world, pos, tile, overlay),
        None => remembered_glyph(world, pos),
    }
}

/// How a tile in sight looks: the top item lying on it, an overlay's
/// shading, or what stands there.
fn visible_glyph(world: &World, pos: Pos, tile: Tile, overlay: Option<Overlay>) -> (String, Style) {
    let map = world.map();
    // Items lying on the floor show on top, unless an overlay is shading it.
    if overlay.is_none()
        && let Some(item) = world.items_at(pos).next()
    {
        return item_glyph(world, item);
    }
    // Overlays shade the open spaces: walkable tiles with nothing in the
    // way, except ladders, which stay visible so you can find your way.
    let in_the_way = world.furniture_kind_at(pos).is_some_and(|k| !k.walkable);
    let open_space = tile.walkable && tile.climb.is_none() && !in_the_way;
    let shaded = match overlay {
        Some(Overlay::Air) if open_space => air_glyph(map.air(pos).unwrap_or(Mix::VACUUM)),
        Some(Overlay::Heat) if open_space => heat_glyph(map.temperature(pos)),
        _ => return standing_glyph(world, pos, tile),
    };
    (shaded.0.to_string(), shaded.1)
}

/// How what stands on a tile looks: furniture if there is any, else the
/// structure.
fn standing_glyph(world: &World, pos: Pos, tile: Tile) -> (String, Style) {
    match world.furniture_at(pos) {
        Some(piece) => furniture_glyph(world, piece),
        None => structure_glyph(world, pos, tile),
    }
}

/// How a piece of furniture looks: its kind's glyph on every tile it
/// covers, in its kind's own colour if it has one, else its material's.
fn furniture_glyph(world: &World, piece: &Piece) -> (String, Style) {
    let kind = world.content().furniture().get(usize::from(piece.kind));
    let made_of = world.materials().get(usize::from(piece.material));
    let shade = kind.and_then(|k| k.colour).or(made_of.map(|m| m.colour));
    let style = shade.map_or(Style::new(), |c| Style::new().fg(colour(c)));
    (kind.map_or('?', |k| k.glyph).to_string(), style)
}

/// How a tile out of sight looks: what was last seen there (the top item
/// if there was one, else the furniture, else the structure), all in one
/// dim colour so it's clearly memory, not sight. Never seen: blank.
fn remembered_glyph(world: &World, pos: Pos) -> (String, Style) {
    let Some(seen) = world.remembered(pos) else {
        return (" ".into(), Style::new());
    };
    let item = seen
        .item
        .and_then(|kind| world.item_types().get(usize::from(kind)))
        .map(|t| t.glyph);
    let furniture = seen
        .furniture
        .and_then(|kind| world.content().furniture().get(usize::from(kind)))
        .map(|f| f.glyph);
    let structure = world.structures().get(usize::from(seen.structure));
    let glyph = item
        .or(furniture)
        .or(structure.map(|s| s.glyph))
        .unwrap_or('?');
    (glyph.to_string(), REMEMBERED)
}

/// The colour of remembered tiles: dark grey, dimmed where the terminal
/// can.
const REMEMBERED: Style = Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM);

/// How an item lying on the floor looks: its type's glyph, in its
/// material's colour, bold so it stands out from the floor.
fn item_glyph(world: &World, item: &Item) -> (String, Style) {
    let glyph = world
        .item_types()
        .get(usize::from(item.kind))
        .map_or('?', |t| t.glyph);
    let colour = world
        .materials()
        .get(usize::from(item.material))
        .map_or(Color::White, |m| colour(m.colour));
    let style = Style::new().fg(colour).add_modifier(Modifier::BOLD);
    (glyph.to_string(), style)
}

/// How the structure at `pos` looks: its own glyph, in its own colour if
/// it has one (doors are light red), else the colour for its properties.
/// Plain solid tiles take their material's colour, so the hull stands out
/// from the bulkheads inside.
fn structure_glyph(world: &World, pos: Pos, tile: Tile) -> (String, Style) {
    let glyph = world.structure_at(pos).map_or('?', |s| s.glyph);
    let own = world.structure_at(pos).and_then(|s| s.colour);
    let plain_solid = !tile.walkable && tile.leak_cm2 == 0;
    let style = match (own, world.material_at(pos)) {
        (Some(c), _) => tile_style(tile).fg(colour(c)),
        (None, Some(material)) if plain_solid => Style::new().fg(colour(material.colour)),
        _ => tile_style(tile),
    };
    (glyph.to_string(), style)
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
        Colour::LightRed => Color::LightRed,
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

/// A hole to space at least this big (cm²) is drawn bold: a breach, not a
/// crack.
const BIG_HOLE_CM2: u32 = 100;

/// The colour for a tile's properties: ladders cyan, leaks red, open floor
/// dark grey, anything else solid grey. Ladders and big holes are bold, so
/// the way on and the danger catch the eye.
fn tile_style(tile: Tile) -> Style {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    if tile.climb.is_some() {
        bold.fg(Color::Cyan)
    } else if tile.leak_cm2 >= BIG_HOLE_CM2 {
        bold.fg(Color::Red)
    } else if tile.leak_cm2 > 0 {
        Style::new().fg(Color::Red)
    } else if tile.walkable {
        Style::new().fg(Color::DarkGray)
    } else {
        Style::new().fg(Color::Gray)
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
        Line::raw(format!(
            "{}  {}",
            world.now().date_text(),
            world.now().time_text()
        )),
        Line::raw(format!("Seed  {}", world.seed())),
        Line::raw(""),
        meter_line("Breath", world.breath(), BREATH_FULL),
        meter_line("Warmth", world.warmth(), WARMTH_FULL),
    ];
    lines.extend(air_lines(world.map().air(p).unwrap_or(Mix::VACUUM)));
    lines.push(temperature_line(world.map().temperature(p)));
    lines.extend([
        Line::raw(""),
        Line::styled("move hjkl yubn  wait .", dim),
        Line::styled(format!("climb < >{}", shortcut_hints(world)), dim),
        Line::styled("look V  use e  items i", dim),
        Line::styled("air a  heat t  save S", dim),
        Line::styled("craft C  menu q/Q", dim),
    ]);
    lines
}

/// The shortcut actions' keys (from `data/actions/`), for the key hints:
/// "  close c".
fn shortcut_hints(world: &World) -> String {
    world
        .content()
        .actions()
        .iter()
        .filter(|a| a.shortcut)
        .map(|a| format!("  {} {}", a.name.to_lowercase(), a.key))
        .collect()
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
    if world.is_visible(at) {
        lines.extend(seen_lines(world, at));
    } else {
        lines.extend(memory_lines(world, at));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("Esc   stop looking", dim),
    ]);
    lines
}

/// What the look panel says about a tile in sight: its name, its
/// material, and the air there.
fn seen_lines(world: &World, at: Pos) -> Vec<Line<'static>> {
    let tile = world.map().get(at).unwrap_or_default();
    let mut lines = vec![Line::raw(tile_name(world, at, tile))];
    if let Some(material) = material_here(world, at) {
        lines.extend(material_lines(world, material));
    }
    lines.push(Line::raw(""));
    lines.extend(air_here(world, at, tile));
    lines
}

/// What the look panel says about a tile out of sight: what's remembered
/// of it (only what it was and what lay there: you can't read its air
/// from here), or that it hasn't been seen.
fn memory_lines(world: &World, at: Pos) -> Vec<Line<'static>> {
    let dim = Style::new().fg(Color::DarkGray);
    let Some(seen) = world.remembered(at) else {
        return vec![Line::styled("You haven't seen this.", dim)];
    };
    let furniture = seen
        .furniture
        .and_then(|k| world.content().furniture().get(usize::from(k)))
        .map(|f| f.name.as_str());
    let structure = world
        .structures()
        .get(usize::from(seen.structure))
        .map(|s| s.name.as_str());
    let name = furniture
        .or(structure)
        .map_or_else(|| "Something".into(), capitalised);
    let mut lines = vec![Line::raw(name)];
    if let Some(t) = seen
        .item
        .and_then(|k| world.item_types().get(usize::from(k)))
    {
        lines.extend([
            Line::raw("Last seen here:"),
            Line::raw(capitalised(&t.name)),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("Out of sight:", dim),
        Line::styled("as you remember it", dim),
    ]);
    lines
}

/// A material's name, in its colour, and its properties, then for each
/// action that needs a tool at the material's level, what does it ("Cut
/// by   LASER 2").
fn material_lines(world: &World, m: &Material) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(m.name.clone(), Style::new().fg(colour(m.colour))),
        Line::raw(format!("Density  {} kg/m³", m.density)),
        Line::raw(format!("Melts    {}", melts(m.melting_point))),
        Line::raw(format!("Hardness {} HV", m.hardness)),
        Line::raw(format!("Heat cap {} J/kg·K", m.specific_heat)),
        Line::raw(format!("Conducts {} W/m·K", watts(m.thermal_conductivity))),
    ];
    let tool_actions = world
        .content()
        .actions()
        .iter()
        .filter(|a| a.tool_from_material);
    lines.extend(tool_actions.map(|action| {
        let label = format!("{} by", action.name);
        Line::raw(format!("{label:<9}{}", worked_by(m, &action.id)))
    }));
    lines
}

/// A melting point for the look panel: "1450 °C", or "never" for things
/// that burn instead.
fn melts(celsius: Option<i32>) -> String {
    celsius.map_or_else(|| "never".into(), |c| format!("{c} °C"))
}

/// Milliwatts as watts, without needless zeros: 45000 → "45", 40 → "0.04".
fn watts(milliwatts: u32) -> String {
    let (whole, thousandths) = (milliwatts / 1000, milliwatts % 1000);
    if thousandths == 0 {
        return whole.to_string();
    }
    let fraction = format!("{thousandths:03}");
    format!("{whole}.{}", fraction.trim_end_matches('0'))
}

/// The tool levels that do action `action` to a material, like "LASER 2",
/// or "nothing".
fn worked_by(m: &Material, action: &str) -> String {
    let Some(levels) = m.worked_by.get(action) else {
        return "nothing".into();
    };
    let each: Vec<String> = levels
        .iter()
        .map(|(quality, level)| format!("{quality} {level}"))
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

/// What to call the tile at `at`: the furniture standing there, or its
/// structure's name ("Open door" is a structure of its own).
fn tile_name(world: &World, at: Pos, tile: Tile) -> String {
    if let Some(kind) = world.furniture_kind_at(at) {
        return capitalised(&kind.name);
    }
    world
        .structure_at(at)
        .map_or_else(|| solid_or_open(tile).into(), |s| capitalised(&s.name))
}

/// What the thing at `at` is made of: the furniture standing there, or
/// else the tile itself.
fn material_here(world: &World, at: Pos) -> Option<&Material> {
    let piece = world.furniture_at(at);
    piece
        .and_then(|p| world.materials().get(usize::from(p.material)))
        .or_else(|| world.material_at(at))
}

/// What to call a tile with no structure to name it (which a map never
/// has, but the panel shouldn't fail): what it's like.
fn solid_or_open(tile: Tile) -> &'static str {
    if tile.walkable {
        "Open space"
    } else {
        "Something solid"
    }
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
    let mut lines = vec![Line::raw(format!(
        "At    {}, {}  deck {}",
        at.x, at.y, at.z
    ))];
    if world.is_visible(at) {
        lines.extend(option_list(world, at));
    } else {
        lines.push(Line::styled("You can't see there.", dim));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("Enter first option", dim),
        Line::styled("move  hjkl yubn", dim),
        Line::styled("Esc   cancel", dim),
    ]);
    lines
}

/// The interact panel for a tile in sight: what it is, then what can be
/// done to it.
fn option_list(world: &World, at: Pos) -> Vec<Line<'static>> {
    let tile = world.map().get(at).unwrap_or_default();
    let mut lines = vec![Line::raw(tile_name(world, at, tile))];
    if let Some(material) = material_here(world, at) {
        lines.push(Line::styled(
            material.name.clone(),
            Style::new().fg(colour(material.colour)),
        ));
    }
    lines.push(Line::raw(""));
    let options = options_at(world, at);
    if options.is_empty() {
        let dim = Style::new().fg(Color::DarkGray);
        lines.push(Line::styled("Nothing to do here.", dim));
    }
    for option in options {
        lines.extend(option_lines(world, at, option));
    }
    lines
}

/// One option: "o  Open", or for a recipe that uses items, what it uses
/// ("r  Repair: 2 scrap"); and if it can't be done, greyed with the reason
/// on the next line.
fn option_lines(world: &World, at: Pos, option: ActionOption) -> Vec<Line<'static>> {
    let action = &world.content().actions()[usize::from(option.action)];
    let label = format!(
        "{}  {}{}",
        action.key,
        action.name,
        uses(world, at, &action.id)
    );
    match option.blocked {
        None => vec![Line::styled(label, Style::new().fg(Color::Yellow))],
        Some(blocked) => {
            let dim = Style::new().fg(Color::DarkGray);
            vec![
                Line::styled(label, dim),
                Line::styled(
                    format!(
                        "   {}",
                        text::blocked_short(blocked, option.action, Some(world))
                    ),
                    dim,
                ),
            ]
        }
    }
}

/// What the recipe for the action with id `action` at `at` uses up, like
/// ": 2 metal scrap", or "" if it uses nothing or there's no recipe.
fn uses(world: &World, at: Pos, action: &str) -> String {
    let recipe = world.recipe_for(action, at);
    let parts: Vec<String> = recipe
        .map(|r| r.uses.iter().map(|u| use_text(world, u)).collect())
        .unwrap_or_default();
    if parts.is_empty() {
        return String::new();
    }
    format!(": {}", parts.join(", "))
}

/// Some items a recipe or craft uses: "2 metal scrap".
fn use_text(world: &World, used: &Use) -> String {
    let tag = used.tag.as_ref().map_or(String::new(), |t| format!("{t} "));
    format!("{} {tag}{}", used.count, item_name(world, &used.item))
}

/// The crafting screen: every craft you know, with what it uses and how
/// long it takes, greyed with the reason if you can't make it now; then
/// what the highlighted one makes. It takes the map's place, like the
/// inventory.
fn crafting_panel(world: &World, selected: usize) -> Paragraph<'static> {
    Paragraph::new(crafting_lines(world, selected))
        .wrap(Wrap { trim: false })
        .block(Block::bordered().title(" Crafting "))
}

fn crafting_lines(world: &World, selected: usize) -> Vec<Line<'static>> {
    let dim = Style::new().fg(Color::DarkGray);
    let known = world.known_crafts();
    let mut lines = Vec::new();
    if known.is_empty() {
        lines.extend([
            Line::raw("You don't know how to make anything yet."),
            Line::styled("Pick things up, and ideas will come.", dim),
        ]);
    }
    for (i, &number) in known.iter().enumerate() {
        lines.extend(craft_lines(world, number, i == selected));
    }
    if let Some(&number) = known.get(selected) {
        lines.push(Line::raw(""));
        lines.extend(made_lines(world, number));
    }
    lines.extend([
        Line::raw(""),
        Line::styled("j/k choose   Enter make   Esc close", dim),
    ]);
    lines
}

/// One known craft: "shiv  1 rigid scrap  5 min", yellow if you can make
/// it now, grey with the reason on the next line if not, and reversed if
/// it's the highlighted one.
fn craft_lines(world: &World, number: u16, chosen: bool) -> Vec<Line<'static>> {
    let craft = &world.content().crafts()[usize::from(number)];
    let blocked = world.craft_blocked(number);
    let dim = Style::new().fg(Color::DarkGray);
    let style = blocked.map_or(Style::new().fg(Color::Yellow), |_| dim);
    let style = match chosen {
        true => style.add_modifier(Modifier::REVERSED),
        false => style,
    };
    let uses: Vec<String> = craft.uses.iter().map(|u| use_text(world, u)).collect();
    let label = format!(
        "{}  {}  {}",
        made_name(world, &craft.makes, craft.count),
        uses.join(", "),
        duration(craft.seconds)
    );
    let mut lines = vec![Line::styled(label, style)];
    if let Some(blocked) = blocked {
        let why = text::blocked_short(blocked, u16::MAX, Some(world));
        lines.push(Line::styled(format!("    {why}"), dim));
    }
    lines
}

/// What craft number `number` makes, for under the list: how many and of
/// what, the tools it needs, the qualities the made item has, and its
/// description.
fn made_lines(world: &World, number: u16) -> Vec<Line<'static>> {
    let craft = &world.content().crafts()[usize::from(number)];
    let of = match craft.material {
        CraftMaterial::FromUsed => ", of the material used",
        CraftMaterial::Own => "",
    };
    let made = made_name(world, &craft.makes, craft.count);
    let mut lines = vec![Line::raw(format!("Makes {made}{of}."))];
    if !craft.needs.is_empty() {
        lines.push(Line::raw(format!(
            "Needs a tool with {}.",
            levels_text(&craft.needs)
        )));
    }
    let kind = world
        .item_type_index(&craft.makes)
        .and_then(|k| world.item_types().get(usize::from(k)));
    if let Some(kind) = kind {
        if !kind.qualities.is_empty() {
            lines.push(Line::raw(format!(
                "It has {}.",
                levels_text(&kind.qualities)
            )));
        }
        lines.push(Line::styled(
            kind.description.clone(),
            Style::new().fg(Color::Gray),
        ));
    }
    lines
}

/// Tool levels, like "CUT 1, SCREW 1".
fn levels_text(levels: &Levels) -> String {
    let each: Vec<String> = levels.iter().map(|(q, l)| format!("{q} {l}")).collect();
    each.join(", ")
}

/// "shiv", or "2 × rag": the name of the item type with id `id`, after
/// how many if more than one.
fn made_name(world: &World, id: &str, count: u32) -> String {
    let name = item_name(world, id);
    match count {
        1 => name,
        _ => format!("{count} × {name}"),
    }
}

/// A time in seconds as a person would say it: "45 s", "5 min", "1 min
/// 30 s".
fn duration(seconds: u32) -> String {
    match (seconds / 60, seconds % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

/// The display name of the item type with id `id` ("scrap" → "metal
/// scrap"), or the id itself if there's no such type.
fn item_name(world: &World, id: &str) -> String {
    world
        .item_type_index(id)
        .and_then(|k| world.item_types().get(usize::from(k)))
        .map_or_else(|| id.to_string(), |t| t.name.clone())
}

/// The bottom panel in look mode: what's lying there, what the structure
/// is, then what it's made of, wrapped. Out of sight, only what's
/// remembered.
fn description(world: &World, at: Pos) -> Paragraph<'static> {
    let text = if world.is_visible(at) {
        seen_description(world, at)
    } else {
        remembered_description(world, at)
    };
    Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .block(Block::bordered().title(" About "))
}

/// The About text for a tile in sight: what lies there, then what stands
/// there (furniture, or else the structure) and what that's made of.
fn seen_description(world: &World, at: Pos) -> String {
    let furniture = world.furniture_kind_at(at).map(|k| k.description.as_str());
    let structure = world.structure_at(at).map(|s| s.description.as_str());
    let standing = furniture.or(structure).unwrap_or("");
    let material = material_here(world, at)
        .map_or(String::new(), |m| format!("{}: {}", m.name, m.description));
    let items = items_here(world, at);
    let inside = inside_here(world, at);
    format!("{items}{standing} {inside}{material}")
}

/// For a container standing at `at`: "Inside: basic laser cutter
/// (Aluminium Alloy), hand lamp (Aluminium Alloy). ", or "It's empty. ".
/// Nothing for anything else.
fn inside_here(world: &World, at: Pos) -> String {
    let container = world.furniture_kind_at(at).is_some_and(|k| k.holds_ml > 0);
    let contents = world.furniture_at(at).map_or(&[][..], |p| &p.contents[..]);
    match (container, contents) {
        (false, _) => String::new(),
        (true, []) => "It's empty. ".into(),
        (true, items) => format!(
            "Inside: {}. ",
            counted_names(world, items.iter()).join(", ")
        ),
    }
}

/// The About text for a tile out of sight: the description of the
/// furniture or structure remembered there, or nothing.
fn remembered_description(world: &World, at: Pos) -> String {
    let Some(seen) = world.remembered(at) else {
        return String::new();
    };
    let furniture = seen
        .furniture
        .and_then(|k| world.content().furniture().get(usize::from(k)))
        .map(|f| f.description.as_str());
    let structure = world
        .structures()
        .get(usize::from(seen.structure))
        .map(|s| s.description.as_str());
    furniture.or(structure).unwrap_or("").to_string()
}

/// "On the floor: basic laser cutter (Aluminium Alloy), 4 × scrap
/// (Steel). ", or nothing if the floor is bare. Identical items are
/// counted together, in the order they first appear.
fn items_here(world: &World, at: Pos) -> String {
    let names = counted_names(world, world.items_at(at));
    if names.is_empty() {
        return String::new();
    }
    format!("On the floor: {}. ", names.join(", "))
}

/// The names of `items`, with identical ones counted together in the
/// order they first appear: "basic laser cutter (Aluminium Alloy)",
/// "4 × scrap (Steel)".
///
/// `'a` is a lifetime: it says the items are borrowed from somewhere that
/// lives at least as long as the iterator does.
fn counted_names<'a>(world: &World, items: impl Iterator<Item = &'a Item>) -> Vec<String> {
    let mut counted: Vec<(ItemRef, usize)> = Vec::new();
    for item in items {
        match counted.iter_mut().find(|(id, _)| *id == item.id()) {
            Some((_, n)) => *n += 1,
            None => counted.push((item.id(), 1)),
        }
    }
    counted
        .iter()
        .map(|&(id, n)| match n {
            1 => text::item_name(world, id),
            _ => format!("{n} × {}", text::item_name(world, id)),
        })
        .collect()
}

/// The inventory screen: what you're wearing, each of its pockets with how
/// full it is, and what's in them, with the highlighted item in reverse.
fn inventory_panel(world: &World, selected: usize) -> Paragraph<'static> {
    Paragraph::new(inventory_lines(world, selected)).block(Block::bordered().title(" Inventory "))
}

fn inventory_lines(world: &World, selected: usize) -> Vec<Line<'static>> {
    let ctx = world.item_context();
    let dim = Style::new().fg(Color::DarkGray);
    let mut lines = Vec::new();
    // `next` counts carried items in the same order as `World::carried`,
    // so it matches the highlighted index.
    let mut next = 0;
    for wearing in world.worn() {
        let name = text::item_name(world, wearing.id());
        lines.push(Line::raw(format!(
            "Wearing {name}  {}",
            kilograms(ctx.mass_g(wearing))
        )));
        let pocket_types = world.item_types()[usize::from(wearing.kind)].pockets();
        for (pocket, held) in pocket_types.iter().zip(&wearing.pockets) {
            lines.push(Line::styled(pocket_line(world, pocket, held), dim));
            for item in held {
                lines.push(carried_line(world, item, next == selected));
                next += 1;
            }
        }
    }
    let total: u64 = world.worn().iter().map(|w| ctx.mass_g(w)).sum();
    lines.extend([
        Line::raw(format!("Carrying {} in all", kilograms(total))),
        Line::raw(""),
        Line::styled("j/k choose   d drop   Esc close", dim),
    ]);
    lines
}

/// A pocket's heading: its name and how full it is, by volume and weight.
fn pocket_line(world: &World, pocket: &PocketType, held: &[Item]) -> String {
    let ctx = world.item_context();
    let volume: u64 = held.iter().map(|i| ctx.volume_ml(i)).sum();
    let mass: u64 = held.iter().map(|i| ctx.mass_g(i)).sum();
    let empty = if held.is_empty() { "  (empty)" } else { "" };
    format!(
        "  {}  {}/{} L  {}/{}{empty}",
        capitalised(&pocket.name),
        litres(volume),
        litres(u64::from(pocket.volume_ml)),
        kilograms(mass).trim_end_matches(" kg"),
        kilograms(u64::from(pocket.max_mass_g)),
    )
}

/// One carried item, highlighted in reverse if it's the chosen one.
fn carried_line(world: &World, item: &Item, chosen: bool) -> Line<'static> {
    let mass = kilograms(world.item_context().mass_g(item));
    let text = format!("    {}  {mass}", text::item_name(world, item.id()));
    let style = if chosen {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    };
    Line::styled(text, style)
}

/// Grams as kilograms with one decimal place, rounded: 785 → "0.8 kg".
fn kilograms(grams: u64) -> String {
    let tenths = (grams + 50) / 100;
    format!("{}.{} kg", tenths / 10, tenths % 10)
}

/// Millilitres as litres with one decimal place, rounded: 800 → "0.8".
fn litres(ml: u64) -> String {
    let tenths = (ml + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
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

    /// A 7×5 room with the player in the middle, lit by an emergency
    /// light in the top wall.
    fn test_world() -> World {
        let rows = ["###E###", "#.....#", "#..@..#", "#.....#", "#######"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "bulkhead", ".": "deck_plating", "@": "deck_plating",
                             "E": "emergency_light" }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let content = crate::content::load_content(&data).unwrap();
        World::new(parse_map(&json, &content).unwrap(), 1)
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
        assert!(screen.contains("25 Nov 2631  08:42:00"));
        assert!(screen.contains("Breath 100%"));
        assert!(screen.contains("Warmth 100%"));
        assert!(screen.contains("Temp  20.0 °C"));
        assert!(screen.contains("Deck 0/1  Pos 3, 2"));
        assert!(screen.contains("Seed  1"));
        assert!(screen.contains("look V  use e  items i"));
        assert!(screen.contains("menu q/Q"));
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
    fn every_structure_on_the_real_map_draws_its_own_glyph() {
        // As it looks in sight, wherever it is (most of it is out of sight
        // from the start).
        let world = real_world();
        let glyph = |x, y, z| {
            let pos = Pos::new(x, y, z);
            match world.map().get(pos) {
                Some(tile) => structure_glyph(&world, pos, tile).0,
                None => glyph_at(&world, pos, None).0,
            }
        };
        assert_eq!(glyph(5, 7, 0), ".", "deck plating");
        assert_eq!(glyph(0, 7, 0), "#", "hull plating");
        assert_eq!(glyph(33, 2, 0), ">", "ladder up");
        assert_eq!(glyph(33, 2, 1), "<", "ladder down");
        assert_eq!(glyph(11, 4, 1), "+", "a shut door");
        assert_eq!(glyph(6, 7, 0), "+", "the side room's door");
        assert_eq!(glyph(23, 0, 1), "x", "the crack");
        assert_eq!(glyph(11, 13, 1), "X", "the breach");
        assert_eq!(glyph(15, 6, 0), "¤", "an emergency light");
        assert_eq!(glyph(-5, 0, 0), " ", "off the map");
    }

    #[test]
    fn an_open_door_draws_its_open_glyph() {
        let mut world = real_world();
        // Up the ladder and along to the cabin door at (5, 4) on deck 1.
        use nighthawk_sim::Command::{Climb, Move};
        let route = std::iter::repeat_n(Move(Dir::North), 5)
            .chain(std::iter::repeat_n(Move(Dir::East), 23))
            .chain([Climb(nighthawk_sim::Vertical::Up)])
            .chain(std::iter::repeat_n(Move(Dir::South), 3))
            .chain(std::iter::repeat_n(Move(Dir::West), 22))
            .chain([Move(Dir::North)]);
        for command in route {
            world.apply(command);
        }
        assert_eq!(world.player(), Pos::new(11, 5, 1));
        assert_eq!(glyph_at(&world, Pos::new(11, 4, 1), None).0, "'");
    }

    #[test]
    fn tiles_never_seen_are_blank_and_remembered_ones_are_dim() {
        let mut world = real_world();
        let (inside, hull) = (Pos::new(5, 7, 0), Pos::new(0, 7, 0));
        assert_eq!(glyph_at(&world, inside, None), (" ".into(), Style::new()));
        // Walk to the side room's door, open it, step into the doorway to
        // peer into the dark, then step back and shut it.
        for _ in 0..3 {
            world.apply(Command::Move(Dir::West));
        }
        world.apply(Command::Move(Dir::West)); // opens it
        world.apply(Command::Move(Dir::West)); // into the doorway
        let floor = Style::new().fg(Color::DarkGray);
        assert_eq!(glyph_at(&world, inside, None), (".".into(), floor));
        world.apply(Command::Move(Dir::East));
        world.apply(Command::Interact {
            at: Pos::new(6, 7, 0),
            action: number(&world, "close"),
        });
        assert_eq!(glyph_at(&world, inside, None), (".".into(), REMEMBERED));
        assert_eq!(
            glyph_at(&world, hull, None),
            (" ".into(), Style::new()),
            "too dark in there to see the far wall"
        );
        let overlaid = glyph_at(&world, inside, Some(Overlay::Air));
        assert_eq!(
            overlaid.1, REMEMBERED,
            "overlays only shade what's in sight"
        );
    }

    /// The real world, after climbing to the crew deck: everything on the
    /// stasis deck is out of sight, but remembered.
    fn upstairs_world() -> World {
        let mut world = real_world();
        use nighthawk_sim::Command::{Climb, Move};
        let route = std::iter::repeat_n(Move(Dir::North), 5)
            .chain(std::iter::repeat_n(Move(Dir::East), 23))
            .chain([Climb(nighthawk_sim::Vertical::Up)]);
        for command in route {
            world.apply(command);
        }
        assert_eq!(world.player().z, 1);
        world
    }

    #[test]
    fn furniture_draws_on_every_tile_it_covers_and_look_mode_names_it() {
        let world = real_world();
        let chamber = Pos::new(11, 6, 0);
        let cyan = Style::new().fg(Color::Cyan);
        assert_eq!(glyph_at(&world, chamber, None), ("0".into(), cyan));
        assert_eq!(glyph_at(&world, Pos::new(12, 6, 0), None).0, "0");
        assert_eq!(
            glyph_at(&world, chamber, Some(Overlay::Air)).0,
            "0",
            "overlays shade the floor around it, not it"
        );
        let lines = look_texts(&world, chamber);
        assert_eq!(
            (lines[2].as_str(), lines[3].as_str()),
            ("Stasis chamber", "Steel")
        );
        assert!(seen_description(&world, chamber).starts_with("A coffin-sized pod"));
        let options = interact_texts(&world, chamber);
        assert_eq!(options[1], "Stasis chamber");
        let locker = Pos::new(7, 4, 0);
        assert_eq!(
            glyph_at(&world, locker, None),
            ("{".into(), Style::new().fg(Color::Gray))
        );
    }

    #[test]
    fn look_mode_says_what_is_inside_a_container() {
        let mut world = real_world();
        let locker = Pos::new(7, 4, 0);
        let about = seen_description(&world, locker);
        assert!(
            about.contains(
                "Inside: basic laser cutter (Aluminium Alloy), hand lamp (Aluminium Alloy), \
                 screwdriver (Steel). Steel:"
            ),
            "{about}"
        );
        world.apply(Command::Move(Dir::NorthWest));
        world.apply(Command::Move(Dir::NorthWest));
        world.apply(Command::Interact {
            at: locker,
            action: number(&world, "take_out"),
        });
        assert!(seen_description(&world, locker).contains("It's empty. "));
        assert_eq!(
            inside_here(&world, Pos::new(11, 6, 0)),
            "",
            "a chamber holds nothing"
        );
        let options = interact_texts(&world, Pos::new(12, 6, 0));
        assert_eq!(options[1], "Stasis chamber");
    }

    #[test]
    fn remembered_furniture_is_drawn_and_described_from_memory() {
        let world = upstairs_world();
        let chamber = Pos::new(11, 6, 0);
        assert_eq!(glyph_at(&world, chamber, None), ("0".into(), REMEMBERED));
        assert_eq!(look_texts(&world, chamber)[2], "Stasis chamber");
        assert!(remembered_description(&world, chamber).starts_with("A coffin-sized pod"));
    }

    #[test]
    fn remembered_items_show_where_they_were_last_seen() {
        let world = upstairs_world();
        let scraps = Pos::new(9, 9, 0);
        let kind = world.items_at(scraps).next().unwrap().kind;
        let glyph = world.item_types()[usize::from(kind)].glyph;
        assert_eq!(glyph_at(&world, scraps, None), (glyph.into(), REMEMBERED));
        let lines = look_texts(&world, scraps);
        let name = &world.item_types()[usize::from(kind)].name;
        assert_eq!(lines[2], "Deck plating");
        assert_eq!(lines[3], "Last seen here:");
        assert_eq!(lines[4], capitalised(name));
        assert!(lines.contains(&"Out of sight:".to_string()));
        assert!(
            !lines.iter().any(|l| l.starts_with("Air")),
            "no air readings"
        );
    }

    #[test]
    fn looking_out_of_sight_shows_only_memory() {
        let world = upstairs_world();
        let hull = Pos::new(10, 0, 0);
        let lines = look_texts(&world, hull);
        assert_eq!(lines[2], "Hull plating");
        assert!(
            !lines.contains(&"Nanocarbon Alloy".to_string()),
            "{lines:?}"
        );
        let about = |at| {
            let area = Rect::new(0, 0, 60, 5);
            let mut buf = Buffer::empty(area);
            description(&world, at).render(area, &mut buf);
            (0..5)
                .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<String>()
        };
        assert!(about(hull).contains("The outer hull."));
        assert!(!about(hull).contains("Nanocarbon"));
        let never = Pos::new(5, 7, 0);
        assert_eq!(look_texts(&world, never)[2], "You haven't seen this.");
        assert!(!about(never).contains('.'), "nothing to say");
    }

    #[test]
    fn tiles_are_coloured_by_their_properties() {
        let solid = Tile::default();
        let floor = Tile {
            walkable: true,
            ..solid
        };
        let ladder = Tile {
            climb: Some(nighthawk_sim::Vertical::Up),
            ..floor
        };
        let crack = Tile {
            leak_cm2: 4,
            ..solid
        };
        let breach = Tile {
            leak_cm2: 400,
            ..solid
        };
        assert_eq!(tile_style(floor).fg, Some(Color::DarkGray));
        assert_eq!(tile_style(solid).fg, Some(Color::Gray));
        assert_eq!(tile_style(ladder).fg, Some(Color::Cyan));
        assert!(tile_style(ladder).add_modifier.contains(Modifier::BOLD));
        assert_eq!(tile_style(crack).fg, Some(Color::Red));
        assert!(!tile_style(crack).add_modifier.contains(Modifier::BOLD));
        assert!(tile_style(breach).add_modifier.contains(Modifier::BOLD));
        assert_eq!(solid_or_open(floor), "Open space");
        assert_eq!(solid_or_open(solid), "Something solid");
    }

    #[test]
    fn doors_keep_their_light_red_through_their_colour() {
        // The real side-room door: solid, coloured light red in the data.
        let world = real_world();
        let (glyph, style) = glyph_at(&world, Pos::new(6, 7, 0), None);
        assert_eq!(glyph, "+");
        assert_eq!(style.fg, Some(Color::LightRed));
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
        assert!(screen.contains("Deck 0/2  Pos 32, 2"));
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
            inventory: None,
            crafting: None,
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
            inventory: None,
            crafting: None,
        }
    }

    #[test]
    fn the_air_overlay_shows_thin_air_upstairs_and_is_labelled() {
        let mut world = with_the_gear();
        let full = draw_screen(80, 24, |f| render_game(&world, &[], air(), f));
        assert!(full.contains("[air overlay]"));
        assert!(full.contains("███"), "the stasis bay is at full pressure");

        // Climb to the crew deck, which starts at 60% pressure.
        use nighthawk_sim::Command::{Climb, Move};
        // The corridor soon fills from the shaft, so go along it and open
        // a cabin door to look in by the lamp's light.
        let route = std::iter::repeat_n(Move(Dir::North), 3)
            .chain(std::iter::repeat_n(Move(Dir::East), 25))
            .chain([Climb(nighthawk_sim::Vertical::Up)])
            .chain(std::iter::repeat_n(Move(Dir::South), 3))
            .chain(std::iter::repeat_n(Move(Dir::West), 11))
            .chain([Move(Dir::North)]);
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

    /// The real world, after taking the gear (the cutter and the lamp)
    /// out of the gear locker at (7, 4): the player stands beside it at
    /// (8, 5), with light to see by anywhere.
    fn with_the_gear() -> World {
        let mut world = real_world();
        world.apply(Command::Move(Dir::NorthWest));
        world.apply(Command::Move(Dir::NorthWest));
        world.apply(Command::Interact {
            at: Pos::new(7, 4, 0),
            action: number(&world, "take_out"),
        });
        let lamp = world.item_type_index("hand_lamp").unwrap();
        assert_eq!(world.carried_count(lamp), 1);
        world
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
        let lines = look_texts(&world, Pos::new(10, 0, 0));
        assert_eq!(lines[0], "Look  10, 0  deck 0");
        assert_eq!(lines[2], "Hull plating");
        assert_eq!(lines[3], "Nanocarbon Alloy");
        assert!(lines.contains(&"Cut by   LASER 5".to_string()));
        assert!(lines.contains(&"Solid: no air".to_string()));
    }

    #[test]
    fn looking_at_your_own_tile_shows_the_air_and_says_you_are_here() {
        let world = real_world();
        let lines = look_texts(&world, world.player());
        assert_eq!(lines[1], "You are here.");
        assert_eq!(lines[2], "Deck plating");
        assert_eq!(lines[3], "Steel");
        assert!(lines.contains(&"Air   101.3 kPa".to_string()));
        assert!(lines.contains(&"Temp  14.0 °C".to_string()));
    }

    #[test]
    fn the_look_panel_fits_a_minimum_terminal_wherever_it_points() {
        // From the start, most tiles are unseen; from upstairs, all of
        // the stasis deck is remembered.
        for world in [real_world(), upstairs_world()] {
            look_panel_fits(&world);
        }
    }

    fn look_panel_fits(world: &World) {
        for at in world.map().positions() {
            let lines = look_lines(world, at);
            assert!(lines.len() <= 16, "{at:?}: {} lines", lines.len());
            for line in &lines {
                assert!(line.width() <= usize::from(SIDEBAR_WIDTH - 2), "{line}");
            }
        }
    }

    #[test]
    fn a_material_nothing_can_cut_says_so() {
        let world = real_world();
        let steel = world.materials().iter().find(|m| m.id == "steel").unwrap();
        let mut material = steel.clone();
        assert_eq!(worked_by(&material, "cut"), "LASER 2");
        material.worked_by.clear();
        assert_eq!(worked_by(&material, "cut"), "nothing");
        let texts: Vec<String> = material_lines(&world, &material)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(texts.last().map(String::as_str), Some("Cut by   nothing"));
    }

    /// The number of the shipped action with this id.
    fn number(world: &World, id: &str) -> u16 {
        world.content().action_index(id).unwrap()
    }

    #[test]
    fn the_look_screen_is_labelled_and_shows_the_description() {
        let world = real_world();
        let view = View {
            overlay: None,
            cursor: Some(Cursor {
                at: Pos::new(10, 0, 0),
                mode: CursorMode::Look,
            }),
            inventory: None,
            crafting: None,
        };
        let screen = draw_screen(80, 24, |f| render_game(&world, &[], view, f));
        assert!(screen.contains("[looking]"));
        assert!(screen.contains(" About "));
        assert!(screen.contains("The outer hull."), "the structure");
        assert!(
            screen.contains("Nanocarbon Alloy: Layered carbon"),
            "and its material"
        );
        assert!(!screen.contains(" Status "), "the look panel replaces it");
    }

    #[test]
    fn the_cursor_is_highlighted_and_the_view_centres_on_it() {
        let world = real_world();
        let at = Pos::new(7, 1, 0);
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
        let hull = glyph_at(&world, Pos::new(10, 0, 0), None);
        let pillar = glyph_at(&world, Pos::new(15, 5, 0), None);
        assert_eq!(hull, ("#".to_string(), Style::new().fg(Color::DarkGray)));
        assert_eq!(pillar, ("#".to_string(), Style::new().fg(Color::Gray)));
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
        // From the start (10, 7), the side room's door at (6, 7) is four
        // tiles west: too far to open or to cut.
        let lines = interact_texts(&world, Pos::new(6, 7, 0));
        assert_eq!(lines[0], "At    6, 7  deck 0");
        assert_eq!(lines[1], "Door");
        assert_eq!(lines[2], "Aluminium Alloy");
        assert_eq!(lines[4], "o  Open");
        assert_eq!(lines[5], "   too far: reach 1");
        assert_eq!(lines[6], "x  Cut");
        assert_eq!(lines[7], "   too far: reach 2");
    }

    #[test]
    fn the_hull_offers_nothing_to_do_until_there_is_a_recipe_for_it() {
        let world = real_world();
        let lines = interact_texts(&world, Pos::new(10, 0, 0));
        assert_eq!(lines[1], "Hull plating");
        assert_eq!(lines[4], "Nothing to do here.");
    }

    #[test]
    fn a_hull_crack_offers_a_repair_that_names_the_scrap_it_uses() {
        let mut world = real_world();
        // Up to the crew deck and into the room with the crack, without
        // picking up any scrap on the way.
        use nighthawk_sim::Command::{Climb, Move};
        let route = std::iter::repeat_n(Move(Dir::North), 5)
            .chain(std::iter::repeat_n(Move(Dir::East), 23))
            .chain([Climb(nighthawk_sim::Vertical::Up)])
            .chain(std::iter::repeat_n(Move(Dir::South), 3))
            .chain(std::iter::repeat_n(Move(Dir::West), 11))
            .chain(std::iter::repeat_n(Move(Dir::North), 3))
            .chain([Move(Dir::NorthEast), Move(Dir::North)]);
        for command in route {
            world.apply(command);
        }
        assert_eq!(world.player(), Pos::new(23, 1, 1));
        let lines = interact_texts(&world, Pos::new(23, 0, 1));
        assert_eq!(lines[4], "r  Repair: 1 metal scrap");
        assert_eq!(lines[5], "   needs 1 metal scrap");
    }

    #[test]
    fn what_you_cannot_see_offers_nothing() {
        let world = real_world();
        let lines = interact_texts(&world, Pos::new(23, 0, 1));
        assert_eq!(lines[0], "At    23, 0  deck 1");
        assert_eq!(lines[1], "You can't see there.");
        assert!(!lines.iter().any(|l| l.contains("Repair")));
    }

    #[test]
    fn open_floor_has_nothing_to_do() {
        let world = real_world();
        let lines = interact_texts(&world, world.player());
        assert!(lines.contains(&"Nothing to do here.".to_string()));
    }

    #[test]
    fn options_that_can_be_done_are_highlighted() {
        let world = real_world();
        let open = option_lines(
            &world,
            world.player(),
            ActionOption {
                action: number(&world, "open"),
                blocked: None,
            },
        );
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
            inventory: None,
            crafting: None,
        };
        let log = ["You need a tool with LASER 1 for that.".to_string()];
        let screen = draw_screen(80, 24, |f| render_game(&world, &log, view, f));
        assert!(screen.contains("[interact]"));
        assert!(screen.contains(" Interact "));
        assert!(screen.contains("LASER 1 for that"), "the log stays visible");
    }

    // --- Items and the inventory ----------------------------------------------

    #[test]
    fn items_on_the_floor_draw_over_it_in_their_materials_colour() {
        let world = real_world();
        let (scrap, style) = glyph_at(&world, Pos::new(9, 9, 0), None);
        assert_eq!(scrap, "*");
        assert_eq!(style.fg, Some(Color::Gray), "steel is grey");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        let (bones, style) = glyph_at(&world, Pos::new(22, 11, 0), None);
        assert_eq!(
            (bones.as_str(), style.fg),
            ("*", Some(Color::White)),
            "bone is white"
        );
        assert_eq!(
            glyph_at(&world, Pos::new(9, 9, 0), Some(Overlay::Air)).0,
            "█",
            "an overlay shades the floor instead"
        );
    }

    #[test]
    fn look_mode_says_what_lies_on_the_floor_counting_alike_items() {
        let world = real_world();
        assert_eq!(
            items_here(&world, Pos::new(9, 9, 0)),
            "On the floor: 4 × scrap (Steel). "
        );
        assert_eq!(
            items_here(&world, Pos::new(14, 2, 0)),
            "On the floor: 2 × scrap (Bone). "
        );
        assert_eq!(items_here(&world, Pos::new(5, 5, 0)), "");
    }

    fn inventory_texts(world: &World, selected: usize) -> Vec<String> {
        inventory_lines(world, selected)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn the_inventory_lists_what_you_wear_and_each_pocket() {
        let world = real_world();
        let lines = inventory_texts(&world, 0);
        assert_eq!(lines[0], "Wearing jumpsuit (Cotton Fabric)  0.9 kg");
        assert_eq!(
            lines[1],
            "  Left hip pocket  0.0/1.0 L  0.0/2.5 kg  (empty)"
        );
        assert_eq!(lines.len(), 8, "four pockets, total, blank, help");
        assert_eq!(lines[5], "Carrying 0.9 kg in all");
    }

    #[test]
    fn carried_items_show_under_their_pocket_with_the_chosen_one_reversed() {
        let world = with_the_gear();
        let lines = inventory_lines(&world, 0);
        assert_eq!(
            lines[1].to_string(),
            "  Left hip pocket  0.9/1.0 L  1.6/2.5 kg"
        );
        assert_eq!(
            lines[2].to_string(),
            "    basic laser cutter (Aluminium Alloy)  1.1 kg"
        );
        assert!(lines[2].style.add_modifier.contains(Modifier::REVERSED));
        let unchosen = inventory_lines(&world, 5);
        assert!(!unchosen[2].style.add_modifier.contains(Modifier::REVERSED));
    }

    fn crafting_texts(world: &World, selected: usize) -> Vec<String> {
        crafting_lines(world, selected)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The real world after picking up the four steel scraps at (9, 9).
    fn with_scrap() -> World {
        let mut world = real_world();
        world.apply(Command::Move(Dir::SouthWest));
        world.apply(Command::Interact {
            at: Pos::new(9, 9, 0),
            action: number(&world, "pick_up"),
        });
        world
    }

    #[test]
    fn the_crafting_screen_lists_what_you_know_and_why_you_cannot_make_it() {
        assert_eq!(
            crafting_texts(&real_world(), 0)[0],
            "You don't know how to make anything yet."
        );
        let world = with_scrap();
        let lines = crafting_lines(&world, 0);
        let texts = crafting_texts(&world, 0);
        assert_eq!(texts[0], "shiv  1 rigid scrap  5 min");
        assert_eq!(lines[0].style.fg, Some(Color::Yellow), "can be made");
        assert!(lines[0].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(texts[1], "2 × rag  1 fabric scrap  1 min");
        assert_eq!(lines[1].style.fg, Some(Color::DarkGray));
        assert_eq!(texts[2], "    needs CUT 1", "the reason, under it");
        assert_eq!(texts[4], "Makes shiv, of the material used.");
        assert_eq!(texts[5], "It has CUT 1.");
        assert!(texts[6].starts_with("A shard of scrap"));
        let rags = crafting_texts(&world, 1);
        assert_eq!(rags[4], "Makes 2 × rag, of the material used.");
        assert_eq!(rags[5], "Needs a tool with CUT 1.");
    }

    #[test]
    fn the_crafting_screen_replaces_the_map() {
        let world = with_scrap();
        let view = View {
            overlay: None,
            cursor: None,
            inventory: None,
            crafting: Some(0),
        };
        let screen = draw_screen(80, 24, |f| render_game(&world, &[], view, f));
        assert!(screen.contains(" Crafting ") && screen.contains("Enter make"));
        assert!(!screen.contains(" Stasis bay "), "no map while it's open");
    }

    #[test]
    fn times_read_as_a_person_would_say_them() {
        assert_eq!(duration(45), "45 s");
        assert_eq!(duration(300), "5 min");
        assert_eq!(duration(90), "1 min 30 s");
    }

    #[test]
    fn the_inventory_screen_replaces_the_map_and_fits_80_by_24() {
        let world = real_world();
        let view = View {
            overlay: None,
            cursor: None,
            inventory: Some(0),
            crafting: None,
        };
        let screen = draw_screen(80, 24, |f| render_game(&world, &[], view, f));
        assert!(screen.contains(" Inventory "));
        assert!(screen.contains("Right leg pocket"));
        assert!(!screen.contains(" Stasis bay "), "no map while it's open");
    }

    #[test]
    fn weights_and_volumes_round_to_one_decimal() {
        assert_eq!(kilograms(785), "0.8 kg");
        assert_eq!(kilograms(1_080), "1.1 kg");
        assert_eq!(kilograms(49), "0.0 kg");
        assert_eq!(kilograms(2_500), "2.5 kg");
        assert_eq!(litres(800), "0.8");
        assert_eq!(litres(1_000), "1.0");
    }

    #[test]
    fn materials_that_burn_never_melt_and_insulators_keep_their_decimals() {
        assert_eq!(melts(Some(1450)), "1450 °C");
        assert_eq!(melts(None), "never");
        assert_eq!(watts(45_000), "45");
        assert_eq!(watts(40), "0.04");
        assert_eq!(watts(167_500), "167.5");
    }

    #[test]
    fn a_cut_door_draws_as_a_red_frame() {
        let mut world = with_the_gear();
        world.apply(Command::Move(Dir::South));
        world.apply(Command::Move(Dir::South)); // to (8, 7), two from the door
        world.apply(Command::Interact {
            at: Pos::new(6, 7, 0),
            action: number(&world, "cut"),
        });
        let (glyph, style) = glyph_at(&world, Pos::new(6, 7, 0), None);
        assert_eq!(glyph, "/");
        assert_eq!(
            style.fg,
            Some(Color::Red),
            "its own colour, not the floor's"
        );
        assert_eq!(
            look_texts(&world, Pos::new(6, 7, 0))[2],
            "Damaged door",
            "named for what it is now"
        );
    }
}
