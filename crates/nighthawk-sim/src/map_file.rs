//! Loading maps from JSON (decision D17: content is JSON).
//!
//! A map file draws each deck as rows of characters, and a legend says which
//! tile each character means. `@` is special: it marks where the player
//! starts, on a floor tile. For example:
//!
//! ```json
//! {
//!   "id": "closet",
//!   "name": "Supply closet",
//!   "legend": { "#": "wall", ".": "floor" },
//!   "decks": [ { "rows": ["#####", "#.@.#", "#####"] } ]
//! }
//! ```
//!
//! Decks are listed bottom to top: the first deck is `z = 0`. Every deck must
//! be the same size, because the [`Map`] is one 3D box.
//!
//! Furniture is drawn where it stands, with every tile it covers: if `"C"`
//! is a stasis chamber two tiles long, `CC` is one chamber and `CCCC` two.
//! The loader cuts the drawing into pieces in reading order, so the first
//! tile of a piece it meets is its top-left (see [`cut_into_pieces`]).
//!
//! This module does no file I/O. It turns JSON *text* into a map, so the sim
//! stays pure and the tests don't need files. Reading the file from disk is
//! the front-end's job.

use std::collections::{HashMap, HashSet};
use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::air::Mix;
use crate::content::Content;
use crate::furniture::{FurnitureType, Piece};
use crate::heat::{MAX_CELSIUS, MIN_CELSIUS, celsius_to_mk};
use crate::item::{self, FloorItem, Item, ItemContext, ItemType};
use crate::map::{Map, StairsError, Tile};
use crate::pos::Pos;

/// The glyph that marks the player's starting position.
const PLAYER_START: char = '@';

/// A map ready to play: the tiles plus everything else the file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedMap {
    pub id: String,
    pub name: String,
    /// One name per deck, bottom to top, like "Stasis bay".
    pub deck_names: Vec<String>,
    pub map: Map,
    pub player_start: Pos,
    /// All the content the map's tiles and items refer to (the world keeps
    /// its own copy, so a save never depends on data files).
    pub content: Content,
    /// The items lying on the floor, in reading order.
    pub floor_items: Vec<FloorItem>,
    /// What the player starts out wearing.
    pub worn: Vec<Item>,
    /// The lines the log opens with in a new game on this map.
    pub intro: Vec<String>,
    /// Every piece of furniture, in reading order of their top-left tiles.
    pub furniture: Vec<Piece>,
}

/// A mistake in a list of items in a map (on the floor, or worn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemSpecError {
    UnknownItem {
        item: String,
    },
    UnknownMaterial {
        item: String,
        material: String,
    },
    /// `"count": 0`.
    NoneOf {
        item: String,
    },
    /// A `charge_percent` over 100, or on something that holds no charge.
    Charge {
        item: String,
        problem: &'static str,
    },
    /// Something in `player_wears` that goes where something before it in
    /// the list already is (decision D61).
    SlotTaken {
        item: String,
        slot: String,
    },
    /// Something in `player_wears` that can't be worn.
    NotWearable {
        item: String,
    },
}

impl fmt::Display for ItemSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemSpecError::UnknownItem { item } => write!(f, "{item:?} isn't an item"),
            ItemSpecError::UnknownMaterial { item, material } => write!(
                f,
                "{item:?} is made of {material:?}, which isn't a material"
            ),
            ItemSpecError::NoneOf { item } => write!(f, "{item:?} has a count of 0"),
            ItemSpecError::Charge { item, problem } => write!(f, "{item:?} {problem}"),
            ItemSpecError::NotWearable { item } => write!(f, "{item:?} can't be worn"),
            ItemSpecError::SlotTaken { item, slot } => write!(
                f,
                "{item:?} is worn on the {slot}, where something else already is"
            ),
        }
    }
}

/// A mistake in a map's legend. Kept apart from [`MapError`] so each list
/// of messages stays short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegendError {
    /// A key that isn't exactly one character.
    BadKey { key: String },
    /// An entry naming a structure that isn't defined.
    UnknownStructure { glyph: char, structure: String },
    /// An entry naming a material that isn't defined.
    UnknownMaterial { glyph: char, material: String },

    /// `@` standing on something you can't stand on.
    StartInsideSomething { structure: String },
    /// A mistake in an entry's list of items.
    Item { glyph: char, error: ItemSpecError },
    /// Items placed on something solid.
    ItemsInSolid { glyph: char },
    /// An entry naming furniture that isn't defined.
    UnknownFurniture { glyph: char, furniture: String },
    /// Furniture fields that don't go together: furniture on something
    /// solid, or `turned` or `contents` with no furniture.
    Furniture { glyph: char, problem: &'static str },
    /// Contents bigger than the furniture holds.
    TooFull {
        glyph: char,
        furniture: String,
        holds_ml: u32,
        contents_ml: u64,
    },
}

impl fmt::Display for LegendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegendError::BadKey { key } => {
                write!(f, "legend key {key:?} must be exactly one character")
            }
            LegendError::UnknownStructure { glyph, structure } => write!(
                f,
                "legend entry {glyph:?} is {structure:?}, which isn't a structure"
            ),
            LegendError::UnknownMaterial { glyph, material } => write!(
                f,
                "legend entry {glyph:?} is made of {material:?}, which isn't a material"
            ),

            LegendError::Item { glyph, error } => write!(f, "legend entry {glyph:?}: {error}"),
            LegendError::ItemsInSolid { glyph } => write!(
                f,
                "legend entry {glyph:?} puts items on something solid; they need a floor"
            ),
            LegendError::StartInsideSomething { structure } => write!(
                f,
                "'{PLAYER_START}' marks where the player starts, so it must stand for something walkable, not {structure:?}"
            ),
            LegendError::UnknownFurniture { glyph, furniture } => write!(
                f,
                "legend entry {glyph:?} has furniture {furniture:?}, which isn't furniture"
            ),
            LegendError::Furniture { glyph, problem } => {
                write!(f, "legend entry {glyph:?} {problem}")
            }
            LegendError::TooFull {
                glyph,
                furniture,
                holds_ml,
                contents_ml,
            } => write!(
                f,
                "legend entry {glyph:?}: its contents take {contents_ml} ml, but a {furniture} holds {holds_ml} ml"
            ),
        }
    }
}

/// Everything that can be wrong with a map file. Each variant says exactly
/// where the problem is, so a content author can fix it without guessing.
#[derive(Debug)]
pub enum MapError {
    /// Not valid JSON, or the wrong shape: a missing field, a misspelled
    /// field name, or an unknown tile name in the legend.
    Json(serde_json::Error),
    /// A mistake in the legend.
    Legend(LegendError),
    /// A mistake in what the player starts out wearing.
    Wearing(ItemSpecError),
    NoDecks,
    EmptyDeck {
        deck: usize,
    },
    /// A row with a different length from the deck's first row.
    RaggedRow {
        deck: usize,
        row: usize,
        expected: usize,
        found: usize,
    },
    /// A deck with a different number of rows from deck 0.
    DeckHeightMismatch {
        deck: usize,
        expected: usize,
        found: usize,
    },
    /// A character that isn't in the legend.
    UnknownGlyph {
        glyph: char,
        deck: usize,
        row: usize,
        col: usize,
    },
    /// The map must contain exactly one `@`.
    PlayerStartCount {
        found: usize,
    },
    /// Stairs without a matching pair on the next deck.
    Stairs(StairsError),
    /// A deck's starting `pressure` is above the allowed maximum.
    BadPressure {
        deck: usize,
        percent: u32,
    },
    /// A deck's starting `temperature` is colder than deep space or too hot.
    BadTemperature {
        deck: usize,
        celsius: i32,
    },
    /// A piece of furniture drawn the wrong shape: starting at `at`, it
    /// also needs `needs` drawn with the same character (and not part of
    /// another piece).
    FurnitureShape {
        glyph: char,
        furniture: String,
        at: Pos,
        needs: Pos,
    },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Json(e) => write!(f, "invalid map JSON: {e}"),
            MapError::Legend(e) => write!(f, "{e}"),
            MapError::Wearing(e) => write!(f, "player_wears: {e}"),
            // Problems with the decks' size and shape get their own function,
            // so this `match` stays under the complexity limit.
            MapError::NoDecks
            | MapError::EmptyDeck { .. }
            | MapError::RaggedRow { .. }
            | MapError::DeckHeightMismatch { .. } => self.fmt_layout(f),
            MapError::UnknownGlyph {
                glyph,
                deck,
                row,
                col,
            } => write!(
                f,
                "deck {deck}, row {row}, column {col}: {glyph:?} is not in the legend"
            ),
            MapError::Stairs(e) => write!(f, "{e}"),
            MapError::BadPressure { deck, percent } => write!(
                f,
                "deck {deck} has pressure {percent}%; it must be 0 to {MAX_PRESSURE_PERCENT}"
            ),
            MapError::BadTemperature { deck, celsius } => write!(
                f,
                "deck {deck} has temperature {celsius} °C; it must be {MIN_CELSIUS} to {MAX_CELSIUS}"
            ),
            MapError::PlayerStartCount { found } => write!(
                f,
                "map must have exactly one '{PLAYER_START}' (player start); found {found}"
            ),
            MapError::FurnitureShape {
                glyph,
                furniture,
                at,
                needs,
            } => write!(
                f,
                "deck {}, row {}, column {}: the {furniture} {glyph:?} starting here also needs row {}, column {} drawn as {glyph:?}",
                at.z, at.y, at.x, needs.y, needs.x
            ),
        }
    }
}

impl MapError {
    /// Messages for decks of the wrong size or shape. `fmt` only sends
    /// those variants here, so the last arm is the one left: no decks.
    fn fmt_layout(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::EmptyDeck { deck } => write!(f, "deck {deck} has no rows"),
            MapError::RaggedRow {
                deck,
                row,
                expected,
                found,
            } => write!(
                f,
                "deck {deck}, row {row} is {found} characters wide; expected {expected}"
            ),
            MapError::DeckHeightMismatch {
                deck,
                expected,
                found,
            } => {
                write!(f, "deck {deck} has {found} rows; expected {expected}")
            }
            _ => write!(f, "map has no decks"),
        }
    }
}

impl std::error::Error for MapError {}

/// The JSON shape, exactly as written in the file. `deny_unknown_fields`
/// makes a misspelled field (`"legnd"`) an error instead of silently ignored.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/map.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    id: String,
    name: String,
    legend: HashMap<String, LegendEntry>,
    decks: Vec<DeckFile>,
    /// What the player starts out wearing.
    #[serde(default)]
    player_wears: Vec<ItemSpec>,
    /// Optional: the lines the log opens with when a new game starts on
    /// this map, like "Emergency power offline." (D66).
    #[serde(default)]
    intro: Vec<String>,
}

/// Some items, in a map: a type, optionally a material other than its own,
/// how many (1 if left out), and for power cells and things that run on
/// them, how charged (decision D59).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ItemSpec {
    item: String,
    #[serde(default)]
    material: Option<String>,
    #[serde(default = "one")]
    count: u32,
    /// For a cell, how full it is, 0 to 100 (full if left out). For
    /// something that takes a cell, it comes with one this full (and with
    /// none if left out).
    #[serde(default)]
    charge_percent: Option<u8>,
}

/// The default `count`. Serde's `default = "..."` names a function.
fn one() -> u32 {
    1
}

/// What a legend character stands for: just a structure's id
/// (`"bulkhead"`), made of the structure's own material; or a structure
/// with a different material, or items lying on it
/// (`{ "structure": "door", "material": "steel" }`). An open door is its
/// own structure (`"'": "open_door"`).
///
/// `untagged` means serde tries each form in turn and takes the first that
/// fits, so the JSON needs no extra label saying which form it is.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum LegendEntry {
    Id(String),
    Placed(Placed),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Placed {
    structure: String,
    #[serde(default)]
    material: Option<String>,
    /// Items lying on it.
    #[serde(default)]
    items: Vec<ItemSpec>,
    /// Furniture standing on it, by id. Draw every tile the furniture
    /// covers with this entry's character. Optional.
    #[serde(default)]
    furniture: Option<String>,
    /// The furniture is turned a quarter, swapping its width and height:
    /// a bed two tiles across becomes two tiles down. Optional.
    #[serde(default)]
    turned: bool,
    /// Items inside the furniture, if it's a container like a locker:
    /// every piece drawn with this character holds its own copies.
    /// Optional.
    #[serde(default)]
    contents: Vec<ItemSpec>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    /// Shown as the map panel's title while you're on this deck. Optional:
    /// without it the deck is called "Deck 0", "Deck 1" and so on.
    #[serde(default)]
    name: Option<String>,
    /// The deck's starting air pressure as a percentage of normal ship air
    /// (101.3 kPa), from 0 (vacuum) to 200. Optional: 100 if left out. The
    /// mix is always normal ship air, just thinner or denser.
    #[serde(default)]
    pressure: Option<u32>,
    /// The deck's starting air temperature in whole °C, from −270 (deep
    /// space) to 200. Optional: 20 °C if left out.
    #[serde(default)]
    temperature: Option<i32>,
    rows: Vec<String>,
}

/// The highest starting pressure a deck may ask for, in percent.
const MAX_PRESSURE_PERCENT: u32 = 200;

/// Parses and validates a map file's text. Its legend names structures from
/// `content`, made of its materials, with its items lying about.
pub fn parse_map(json: &str, content: &Content) -> Result<LoadedMap, MapError> {
    let file: MapFile = serde_json::from_str(json).map_err(MapError::Json)?;
    let legend = glyph_legend(&file.legend, content)?;
    let (width, height) = dimensions(&file.decks)?;

    let Built {
        mut map,
        starts,
        floor_items,
        furnished,
    } = build(&file.decks, &legend, (width, height))?;
    let furniture = cut_into_pieces(&furnished, content)?;

    map.check_stairs().map_err(MapError::Stairs)?;
    fill_air(&mut map, &file.decks)?;
    let deck_names = deck_names(&file.decks);
    let worn = worn_items(&file.player_wears, content)?;

    match starts[..] {
        [player_start] => Ok(LoadedMap {
            id: file.id,
            name: file.name,
            deck_names,
            map,
            player_start,
            content: content.clone(),
            floor_items,
            worn,
            intro: file.intro,
            furniture,
        }),
        _ => Err(MapError::PlayerStartCount {
            found: starts.len(),
        }),
    }
}

/// What drawing the decks builds: the map, where `@` appears, and the
/// items lying about.
struct Built {
    map: Map,
    starts: Vec<Pos>,
    floor_items: Vec<FloorItem>,
    /// Every tile drawn with furniture, in reading order, with the
    /// character it's drawn with and what it stands there.
    furnished: Vec<(Pos, char, Furnish)>,
}

/// Places every glyph of every deck on a new `width` × `height` map.
fn build(
    decks: &[DeckFile],
    legend: &HashMap<char, Placement>,
    (width, height): (usize, usize),
) -> Result<Built, MapError> {
    let depth = to_i32(decks.len());
    let mut built = Built {
        map: Map::filled(to_i32(width), to_i32(height), depth, Tile::default()),
        starts: Vec::new(),
        floor_items: Vec::new(),
        furnished: Vec::new(),
    };
    for (z, y, x, glyph) in cells(decks) {
        let at = Pos::new(to_i32(x), to_i32(y), to_i32(z));
        let placed = tile_for(glyph, legend, (z, y, x))?;
        built.map.place(at, placed.structure, placed.tile);
        built.map.set_material(at, placed.material);
        let here = placed.items.into_iter();
        built
            .floor_items
            .extend(here.map(|item| FloorItem { at, item }));
        if glyph == PLAYER_START {
            built.starts.push(at);
        }
        built
            .furnished
            .extend(placed.furniture.map(|f| (at, glyph, f)));
    }
    Ok(built)
}

/// Cuts the tiles drawn with furniture into pieces. They're visited in
/// reading order, so the first tile of a piece met is its top-left. A
/// piece then claims every tile its size covers from there, and each must
/// be drawn with the same character and not be part of a piece already;
/// otherwise the drawing is the wrong shape, and the error says where.
fn cut_into_pieces(
    furnished: &[(Pos, char, Furnish)],
    content: &Content,
) -> Result<Vec<Piece>, MapError> {
    let drawn: HashMap<Pos, char> = furnished.iter().map(|&(p, c, _)| (p, c)).collect();
    let mut claimed = HashSet::new();
    let mut pieces = Vec::new();
    for (at, glyph, furnish) in furnished {
        let (at, glyph) = (*at, *glyph);
        if claimed.contains(&at) {
            continue;
        }
        let kind = &content.furniture.list()[usize::from(furnish.kind)];
        let piece = Piece {
            kind: furnish.kind,
            material: furnish.material,
            at,
            turned: furnish.turned,
            contents: furnish.contents.clone(),
        };
        // `HashSet::insert` returns false if the tile was already there.
        if let Some(needs) = piece
            .tiles(kind)
            .find(|p| drawn.get(p) != Some(&glyph) || !claimed.insert(*p))
        {
            return Err(MapError::FurnitureShape {
                glyph,
                furniture: kind.id.clone(),
                at,
                needs,
            });
        }
        pieces.push(piece);
    }
    Ok(pieces)
}

/// Every glyph in every deck, as `(deck, row, column, glyph)`, in reading
/// order. Flattening the three nested loops into one iterator keeps
/// `parse_map` flat and easy to follow.
fn cells(decks: &[DeckFile]) -> impl Iterator<Item = (usize, usize, usize, char)> + '_ {
    decks.iter().enumerate().flat_map(|(z, deck)| {
        deck.rows.iter().enumerate().flat_map(move |(y, row)| {
            row.chars()
                .enumerate()
                .map(move |(x, glyph)| (z, y, x, glyph))
        })
    })
}

/// Gives every deck its starting air: normal ship air scaled to the
/// deck's `pressure` (100% unless the file says otherwise), at the deck's
/// `temperature` (20 °C unless it says otherwise).
fn fill_air(map: &mut Map, decks: &[DeckFile]) -> Result<(), MapError> {
    for (z, deck) in decks.iter().enumerate() {
        let percent = deck.pressure.unwrap_or(100);
        if percent > MAX_PRESSURE_PERCENT {
            return Err(MapError::BadPressure { deck: z, percent });
        }
        let celsius = deck.temperature.unwrap_or(DEFAULT_CELSIUS);
        if !(MIN_CELSIUS..=MAX_CELSIUS).contains(&celsius) {
            return Err(MapError::BadTemperature { deck: z, celsius });
        }
        map.fill_deck_air(to_i32(z), Mix::STANDARD.scaled(percent));
        map.set_deck_temperature(to_i32(z), celsius_to_mk(celsius));
    }
    Ok(())
}

/// The temperature a deck starts at if its map doesn't say: 20 °C.
const DEFAULT_CELSIUS: i32 = 20;

/// Each deck's name, or "Deck N" for decks without one.
fn deck_names(decks: &[DeckFile]) -> Vec<String> {
    decks
        .iter()
        .enumerate()
        .map(|(z, deck)| deck.name.clone().unwrap_or_else(|| format!("Deck {z}")))
        .collect()
}

/// What a legend character places: a tile, the indexes of its structure
/// and material, and any items lying on it.
#[derive(Clone)]
struct Placement {
    tile: Tile,
    structure: u16,
    material: u16,
    items: Vec<Item>,
    /// The furniture standing there, if any.
    furniture: Option<Furnish>,
}

/// The furniture a legend entry stands on its tiles.
#[derive(Debug, Clone)]
struct Furnish {
    kind: u16,
    material: u16,
    turned: bool,
    contents: Vec<Item>,
}

/// The items an item list in a map stands for: `count` new items each.
fn make_items(specs: &[ItemSpec], content: &Content) -> Result<Vec<Item>, ItemSpecError> {
    let mut items = Vec::new();
    for spec in specs {
        let kind =
            content
                .item_types
                .index_of(&spec.item)
                .ok_or_else(|| ItemSpecError::UnknownItem {
                    item: spec.item.clone(),
                })?;
        let types = content.item_types.list();
        let material_id = spec
            .material
            .as_ref()
            .unwrap_or(&types[usize::from(kind)].material);
        let material = content.materials.index_of(material_id).ok_or_else(|| {
            ItemSpecError::UnknownMaterial {
                item: spec.item.clone(),
                material: material_id.clone(),
            }
        })?;
        if spec.count == 0 {
            return Err(ItemSpecError::NoneOf {
                item: spec.item.clone(),
            });
        }
        let mut one = Item::new(kind, material, types);
        charge(&mut one, spec, content)?;
        items.extend(std::iter::repeat_n(one, spec.count as usize));
    }
    Ok(items)
}

/// Charges a new `item` as `spec` says: a cell to its `charge_percent`
/// (full without one); something that takes a cell, with a new cell that
/// full, if there's a `charge_percent`.
fn charge(item: &mut Item, spec: &ItemSpec, content: &Content) -> Result<(), ItemSpecError> {
    let problem = |problem| {
        Err(ItemSpecError::Charge {
            item: spec.item.clone(),
            problem,
        })
    };
    let types = content.item_types.list();
    let kind = &types[usize::from(item.kind)];
    let percent = spec.charge_percent.unwrap_or(100);
    if percent > 100 {
        return problem("has a charge_percent over 100");
    }
    let share = |capacity: u32| {
        let j = u64::from(capacity) * u64::from(percent) / 100;
        u32::try_from(j).unwrap_or(u32::MAX)
    };
    match (spec.charge_percent, &kind.cell, &kind.power) {
        (_, Some(cell), _) => item.charge_j = share(cell.capacity_j()),
        (Some(_), None, Some(power)) => {
            // Content checks guarantee the cell is a real cell type.
            let cell_kind = content.item_types.index_of(&power.cell).unwrap_or(0);
            let cell_type = &types[usize::from(cell_kind)];
            let material = content.materials.index_of(&cell_type.material).unwrap_or(0);
            let mut cell = Item::new(cell_kind, material, types);
            cell.charge_j = share(cell_type.cell.as_ref().map_or(0, |c| c.capacity_j()));
            item.cell = Some(Box::new(cell));
        }
        (Some(_), None, None) => return problem("has a charge_percent, but holds no charge"),
        (None, None, _) => {}
    }
    Ok(())
}

/// The items a legend entry lays on the floor, which needs to be walkable.
fn floor_items(
    glyph: char,
    specs: &[ItemSpec],
    tile: Tile,
    content: &Content,
) -> Result<Vec<Item>, LegendError> {
    let items = make_items(specs, content).map_err(|error| LegendError::Item { glyph, error })?;
    if !items.is_empty() && !tile.is_walkable() {
        return Err(LegendError::ItemsInSolid { glyph });
    }
    Ok(items)
}

/// What the player starts out wearing: items that can all be worn.
fn worn_items(specs: &[ItemSpec], content: &Content) -> Result<Vec<Item>, MapError> {
    let items = make_items(specs, content).map_err(MapError::Wearing)?;
    let types = content.item_types.list();
    match items
        .iter()
        .find(|i| types[usize::from(i.kind)].wear.is_none())
    {
        Some(item) => Err(MapError::Wearing(ItemSpecError::NotWearable {
            item: types[usize::from(item.kind)].id.clone(),
        })),
        None => slots_free(items, types),
    }
}

/// `items`, if no two of them are worn in the same slot.
fn slots_free(items: Vec<Item>, types: &[ItemType]) -> Result<Vec<Item>, MapError> {
    let Some(slot) = item::shared_slot(&items, types) else {
        return Ok(items);
    };
    // The second item in that slot is the one that doesn't fit.
    let in_slot = |i: &&Item| {
        types[usize::from(i.kind)]
            .wear
            .as_ref()
            .is_some_and(|w| w.slot == slot)
    };
    let second = items.iter().filter(in_slot).nth(1).map_or(0, |i| i.kind);
    Err(MapError::Wearing(ItemSpecError::SlotTaken {
        item: types[usize::from(second)].id.clone(),
        slot: slot.to_string(),
    }))
}

/// Turns the legend's one-character string keys into `char`s, and each
/// entry into what it places.
fn glyph_legend(
    legend: &HashMap<String, LegendEntry>,
    content: &Content,
) -> Result<HashMap<char, Placement>, MapError> {
    legend
        .iter()
        .map(|(key, entry)| {
            let mut chars = key.chars();
            let glyph = match (chars.next(), chars.next()) {
                (Some(c), None) => c,
                _ => return Err(LegendError::BadKey { key: key.clone() }),
            };
            Ok((glyph, place(glyph, entry, content)?))
        })
        .collect::<Result<_, LegendError>>()
        .map_err(MapError::Legend)
}

/// What a legend entry places.
fn place(glyph: char, entry: &LegendEntry, content: &Content) -> Result<Placement, LegendError> {
    let (id, material, specs) = match entry {
        LegendEntry::Id(id) => (id, None, &[][..]),
        LegendEntry::Placed(p) => (&p.structure, p.material.as_ref(), &p.items[..]),
    };
    let structure =
        content
            .structures
            .index_of(id)
            .ok_or_else(|| LegendError::UnknownStructure {
                glyph,
                structure: id.clone(),
            })?;
    let made = &content.structures.list()[usize::from(structure)];
    let tile = made.tile();
    if glyph == PLAYER_START && !tile.is_walkable() {
        return Err(LegendError::StartInsideSomething {
            structure: id.clone(),
        });
    }
    let material_id = material.unwrap_or(&made.material);
    let material =
        content
            .materials
            .index_of(material_id)
            .ok_or_else(|| LegendError::UnknownMaterial {
                glyph,
                material: material_id.clone(),
            })?;
    let items = floor_items(glyph, specs, tile, content)?;
    let furniture = match entry {
        LegendEntry::Placed(p) => furnish(glyph, p, tile, content)?,
        LegendEntry::Id(_) => None,
    };
    Ok(Placement {
        tile,
        structure,
        material,
        items,
        furniture,
    })
}

/// What's wrong with furniture on something solid.
const ON_SOLID: &str = "stands furniture on something solid; it needs a floor";
/// What's wrong with `"turned": true` and no furniture.
const NOTHING_TO_TURN: &str = "is turned, but has no furniture to turn";
/// What's wrong with `"contents"` and no furniture.
const NOTHING_TO_HOLD: &str = "has contents, but no furniture to hold them";

/// The furniture a legend entry stands on its tiles, if it names any. It
/// needs `tile` to be walkable, and the player can't start inside
/// furniture that's in the way.
fn furnish(
    glyph: char,
    placed: &Placed,
    tile: Tile,
    content: &Content,
) -> Result<Option<Furnish>, LegendError> {
    let Some(id) = &placed.furniture else {
        return match (placed.turned, placed.contents.is_empty()) {
            (true, _) => Err(LegendError::Furniture {
                glyph,
                problem: NOTHING_TO_TURN,
            }),
            (_, false) => Err(LegendError::Furniture {
                glyph,
                problem: NOTHING_TO_HOLD,
            }),
            _ => Ok(None),
        };
    };
    let kind = content
        .furniture
        .index_of(id)
        .ok_or_else(|| LegendError::UnknownFurniture {
            glyph,
            furniture: id.clone(),
        })?;
    let made = &content.furniture.list()[usize::from(kind)];
    if !tile.is_walkable() {
        return Err(LegendError::Furniture {
            glyph,
            problem: ON_SOLID,
        });
    }
    if glyph == PLAYER_START && !made.walkable {
        return Err(LegendError::StartInsideSomething {
            structure: id.clone(),
        });
    }
    let material = content
        .materials
        .index_of(&made.material)
        .expect("furniture materials were checked on load");
    let contents = contents_of(glyph, placed, made, content)?;
    Ok(Some(Furnish {
        kind,
        material,
        turned: placed.turned,
        contents,
    }))
}

/// The items a legend entry puts inside its furniture, which must fit:
/// all together, no bigger than what `made` holds.
fn contents_of(
    glyph: char,
    placed: &Placed,
    made: &FurnitureType,
    content: &Content,
) -> Result<Vec<Item>, LegendError> {
    let items = make_items(&placed.contents, content)
        .map_err(|error| LegendError::Item { glyph, error })?;
    let ctx = ItemContext {
        types: content.item_types.list(),
        materials: content.materials.list(),
    };
    let contents_ml: u64 = items.iter().map(|i| ctx.volume_ml(i)).sum();
    if contents_ml > u64::from(made.holds_ml) {
        return Err(LegendError::TooFull {
            glyph,
            furniture: made.id.clone(),
            holds_ml: made.holds_ml,
            contents_ml,
        });
    }
    Ok(items)
}

/// Checks every deck is the same rectangle and returns its `(width, height)`.
/// Width counts characters, not bytes, so non-ASCII glyphs like `═` work.
fn dimensions(decks: &[DeckFile]) -> Result<(usize, usize), MapError> {
    let first = decks.first().ok_or(MapError::NoDecks)?;
    let height = first.rows.len();
    let width = first
        .rows
        .first()
        .ok_or(MapError::EmptyDeck { deck: 0 })?
        .chars()
        .count();

    for (deck, d) in decks.iter().enumerate() {
        if d.rows.is_empty() {
            return Err(MapError::EmptyDeck { deck });
        }
        if d.rows.len() != height {
            return Err(MapError::DeckHeightMismatch {
                deck,
                expected: height,
                found: d.rows.len(),
            });
        }
        for (row, r) in d.rows.iter().enumerate() {
            let found = r.chars().count();
            if found != width {
                return Err(MapError::RaggedRow {
                    deck,
                    row,
                    expected: width,
                    found,
                });
            }
        }
    }
    Ok((width, height))
}

/// The tile a glyph stands for. `@` is the player start, on a floor tile.
fn tile_for(
    glyph: char,
    legend: &HashMap<char, Placement>,
    (deck, row, col): (usize, usize, usize),
) -> Result<Placement, MapError> {
    legend.get(&glyph).cloned().ok_or(MapError::UnknownGlyph {
        glyph,
        deck,
        row,
        col,
    })
}

/// Map files are small, so their sizes always fit in an `i32`. If one ever
/// doesn't, that's a bug worth crashing on, not a recoverable error.
fn to_i32(n: usize) -> i32 {
    i32::try_from(n).expect("map dimension fits in i32")
}

/// [`parse_map`] with a small set of test structures (named after their
/// behaviours: `"wall"`, `"floor"`, `"door"`…, all steel), so tests can
/// build maps from a few lines of JSON. If the legend doesn't say what `@`
/// stands on, it's `"floor"`. Other modules' tests import it *as*
/// `parse_map`.
#[cfg(test)]
pub(crate) fn parse_map_for_tests(json: &str) -> Result<LoadedMap, MapError> {
    let content = Content::for_tests();
    // Add `"@": "floor"` to the legend, if the JSON is valid enough to
    // have one and it doesn't say. Otherwise parse it as it is, so tests of
    // broken JSON see the real error.
    let mut value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(_) => return parse_map(json, &content),
    };
    if let Some(legend) = value.get_mut("legend").and_then(|l| l.as_object_mut()) {
        legend.entry("@").or_insert_with(|| "floor".into());
    }
    parse_map(&value.to_string(), &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    // This import beats the `parse_map` that `super::*` brings in: names
    // imported by name win over names imported with `*`.
    use super::parse_map_for_tests as parse_map;
    use crate::air::Mix;

    /// Builds a one-deck map file from rows, with the standard legend.
    fn one_deck(rows: &[&str]) -> String {
        let rows: Vec<String> = rows.iter().map(|r| format!("{r:?}")).collect();
        format!(
            r###"{{ "id": "t", "name": "Test", "legend": {{ "#": "wall", ".": "floor" }},
                  "decks": [ {{ "rows": [{}] }} ] }}"###,
            rows.join(", ")
        )
    }

    #[test]
    fn parses_a_valid_map() {
        let loaded = parse_map(&one_deck(&["####", "#.@#", "####"])).unwrap();

        assert_eq!(loaded.id, "t");
        assert_eq!(loaded.name, "Test");
        assert_eq!(
            (loaded.map.width(), loaded.map.height(), loaded.map.depth()),
            (4, 3, 1)
        );
        assert_eq!(loaded.player_start, Pos::new(2, 1, 0));
        assert_eq!(loaded.map.get(Pos::new(0, 0, 0)), Some(Tile::WALL));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 0)), Some(Tile::FLOOR));
    }

    #[test]
    fn player_start_is_floor() {
        let loaded = parse_map(&one_deck(&["###", "#@#", "###"])).unwrap();
        assert_eq!(loaded.map.get(loaded.player_start), Some(Tile::FLOOR));
    }

    #[test]
    fn decks_stack_bottom_to_top() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "#": "wall", ".": "floor" },
            "decks": [ { "rows": ["##", "##"] }, { "rows": ["@.", ".."] } ] }"###;
        let loaded = parse_map(json).unwrap();

        assert_eq!(loaded.map.depth(), 2);
        assert_eq!(loaded.player_start, Pos::new(0, 0, 1));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 0)), Some(Tile::WALL));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 1)), Some(Tile::FLOOR));
    }

    #[test]
    fn width_counts_characters_not_bytes() {
        // '═' is 3 bytes in UTF-8 but one tile.
        let json = r#"{ "id": "t", "name": "T", "legend": { "═": "wall", ".": "floor" },
            "decks": [ { "rows": ["═══", ".@."] } ] }"#;
        let loaded = parse_map(json).unwrap();
        assert_eq!(loaded.map.width(), 3);
        assert_eq!(loaded.map.get(Pos::new(1, 0, 0)), Some(Tile::WALL));
    }

    #[test]
    fn misspelled_field_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legnd": { "#": "wall" },
            "decks": [ { "rows": ["@"] } ] }"###;
        assert!(matches!(parse_map(json), Err(MapError::Json(_))));
    }

    #[test]
    fn unknown_structure_name_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "#": "wal" },
            "decks": [ { "rows": ["@"] } ] }"###;
        assert!(matches!(
            parse_map(json),
            Err(MapError::Legend(LegendError::UnknownStructure { .. }))
        ));
    }

    #[test]
    fn glyph_missing_from_legend_reports_its_location() {
        let err = parse_map(&one_deck(&["####", "#@x#", "####"])).unwrap_err();
        assert!(matches!(
            err,
            MapError::UnknownGlyph {
                glyph: 'x',
                deck: 0,
                row: 1,
                col: 2
            }
        ));
    }

    #[test]
    fn ragged_row_is_rejected() {
        let err = parse_map(&one_deck(&["####", "#@#", "####"])).unwrap_err();
        assert!(matches!(
            err,
            MapError::RaggedRow {
                deck: 0,
                row: 1,
                expected: 4,
                found: 3
            }
        ));
    }

    #[test]
    fn decks_of_different_heights_are_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { ".": "floor" },
            "decks": [ { "rows": ["@.", ".."] }, { "rows": [".."] } ] }"###;
        let err = parse_map(json).unwrap_err();
        assert!(matches!(
            err,
            MapError::DeckHeightMismatch {
                deck: 1,
                expected: 2,
                found: 1
            }
        ));
    }

    #[test]
    fn missing_player_start_is_rejected() {
        let err = parse_map(&one_deck(&["###", "#.#", "###"])).unwrap_err();
        assert!(matches!(err, MapError::PlayerStartCount { found: 0 }));
    }

    #[test]
    fn two_player_starts_are_rejected() {
        let err = parse_map(&one_deck(&["####", "#@@#", "####"])).unwrap_err();
        assert!(matches!(err, MapError::PlayerStartCount { found: 2 }));
    }

    #[test]
    fn multi_character_legend_key_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "##": "wall" },
            "decks": [ { "rows": ["@"] } ] }"###;
        let err = parse_map(json).unwrap_err();
        assert!(matches!(
            err,
            MapError::Legend(LegendError::BadKey { ref key }) if key == "##"
        ));
    }

    #[test]
    fn the_player_cannot_start_inside_a_wall() {
        let json = r#"{ "id": "t", "name": "T", "legend": { "@": "wall" },
            "decks": [ { "rows": ["@"] } ] }"#;
        assert_eq!(
            parse_map(json).unwrap_err().to_string(),
            "'@' marks where the player starts, so it must stand for something walkable, not \"wall\""
        );
    }

    #[test]
    fn map_without_decks_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {}, "decks": [] }"#;
        assert!(matches!(parse_map(json), Err(MapError::NoDecks)));
    }

    #[test]
    fn deck_without_rows_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {}, "decks": [ { "rows": [] } ] }"#;
        assert!(matches!(
            parse_map(json),
            Err(MapError::EmptyDeck { deck: 0 })
        ));
    }

    #[test]
    fn later_empty_deck_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {},
            "decks": [ { "rows": ["@"] }, { "rows": [] } ] }"#;
        assert!(matches!(
            parse_map(json),
            Err(MapError::EmptyDeck { deck: 1 })
        ));
    }

    #[test]
    fn every_error_message_says_where_the_problem_is() {
        let cases = [
            (
                MapError::Legend(LegendError::BadKey { key: "ab".into() }),
                "\"ab\"",
            ),
            (MapError::NoDecks, "no decks"),
            (MapError::EmptyDeck { deck: 3 }, "deck 3"),
            (
                MapError::RaggedRow {
                    deck: 0,
                    row: 4,
                    expected: 9,
                    found: 7,
                },
                "row 4 is 7 characters wide; expected 9",
            ),
            (
                MapError::DeckHeightMismatch {
                    deck: 2,
                    expected: 5,
                    found: 4,
                },
                "deck 2 has 4 rows; expected 5",
            ),
            (MapError::PlayerStartCount { found: 2 }, "found 2"),
        ];
        for (err, expected) in cases {
            let msg = err.to_string();
            assert!(
                msg.contains(expected),
                "{msg:?} should contain {expected:?}"
            );
        }
    }

    #[test]
    fn json_errors_say_what_went_wrong() {
        let err = parse_map("{ not json").unwrap_err();
        assert!(err.to_string().starts_with("invalid map JSON:"));
    }

    #[test]
    fn errors_have_readable_messages() {
        let err = parse_map(&one_deck(&["####", "#@x#", "####"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "deck 0, row 1, column 2: 'x' is not in the legend"
        );
    }

    #[test]
    fn decks_are_named_or_numbered() {
        let lower = ["#<#"];
        let upper = ["@>."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up", ">": "stairs_down" }},
                 "decks": [ {{ "rows": {lower:?} }}, {{ "name": "Upper", "rows": {upper:?} }} ] }}"#
        );
        let loaded = parse_map(&json).unwrap();
        assert_eq!(
            loaded.deck_names,
            vec!["Deck 0".to_string(), "Upper".to_string()]
        );
        assert_eq!(loaded.map.get(Pos::new(1, 0, 0)), Some(Tile::LADDER_UP));
        assert_eq!(loaded.map.get(Pos::new(1, 0, 1)), Some(Tile::LADDER_DOWN));
    }

    #[test]
    fn unpaired_stairs_are_rejected_with_their_location() {
        let rows = ["#@.<#"];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up" }}, "decks": [ {{ "rows": {rows:?} }} ] }}"#
        );
        let err = parse_map(&json).unwrap_err();
        assert!(matches!(err, MapError::Stairs(_)));
        assert_eq!(
            err.to_string(),
            "stairs up at deck 0, row 0, column 3 need stairs down at the same spot on deck 1"
        );
    }

    #[test]
    fn decks_start_with_normal_air_unless_the_file_says_otherwise() {
        let lower = ["#<#"];
        let upper = ["@>."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up", ">": "stairs_down" }},
                 "decks": [ {{ "rows": {lower:?} }},
                            {{ "pressure": 60, "rows": {upper:?} }} ] }}"#
        );
        let map = parse_map(&json).unwrap().map;
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix::STANDARD),
            "default 100%"
        );
        assert_eq!(
            map.air(Pos::new(0, 0, 0)),
            Some(Mix::VACUUM),
            "walls hold none"
        );
        assert_eq!(map.air(Pos::new(2, 0, 1)), Some(Mix::STANDARD.scaled(60)));
    }

    #[test]
    fn a_vacuum_deck_is_allowed() {
        let rows = ["@."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor" }},
                 "decks": [ {{ "pressure": 0, "rows": {rows:?} }} ] }}"#
        );
        let map = parse_map(&json).unwrap().map;
        assert_eq!(map.air(Pos::new(1, 0, 0)), Some(Mix::VACUUM));
    }

    #[test]
    fn too_high_a_pressure_is_rejected() {
        let rows = ["@."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor" }},
                 "decks": [ {{ "pressure": 201, "rows": {rows:?} }} ] }}"#
        );
        let err = parse_map(&json).unwrap_err();
        assert!(matches!(
            err,
            MapError::BadPressure {
                deck: 0,
                percent: 201
            }
        ));
        assert_eq!(
            err.to_string(),
            "deck 0 has pressure 201%; it must be 0 to 200"
        );
    }
}

#[cfg(test)]
mod temperature_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-row, one-deck map with `deck` as extra fields on the deck.
    fn map_with(deck: &str) -> Result<LoadedMap, MapError> {
        let rows = ["@.#"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", "#": "wall" }},
                 "decks": [ {{ {deck} "rows": {rows:?} }} ] }}"##
        );
        parse_map(&json)
    }

    #[test]
    fn decks_start_at_20_degrees_unless_the_file_says_otherwise() {
        let warm = map_with("").unwrap().map;
        assert_eq!(warm.temperature(Pos::new(1, 0, 0)), Some(celsius_to_mk(20)));
        let cold = map_with(r#""temperature": -20,"#).unwrap().map;
        assert_eq!(
            cold.temperature(Pos::new(1, 0, 0)),
            Some(celsius_to_mk(-20))
        );
        assert_eq!(
            cold.temperature(Pos::new(2, 0, 0)),
            None,
            "walls hold no air"
        );
    }

    #[test]
    fn a_vacuum_deck_has_no_temperature() {
        let map = map_with(r#""pressure": 0, "temperature": 5,"#).unwrap().map;
        assert_eq!(map.temperature(Pos::new(1, 0, 0)), None);
        assert_eq!(map.total_heat(), 0);
    }

    #[test]
    fn the_coldest_and_hottest_allowed_temperatures_load() {
        assert!(map_with(r#""temperature": -270,"#).is_ok());
        assert!(map_with(r#""temperature": 200,"#).is_ok());
    }

    #[test]
    fn temperatures_out_of_range_are_rejected_with_the_deck_named() {
        for celsius in [-271, 201] {
            let err = map_with(&format!(r#""temperature": {celsius},"#)).unwrap_err();
            assert!(
                matches!(err, MapError::BadTemperature { deck: 0, celsius: c } if c == celsius),
                "{err:?}"
            );
        }
        let err = map_with(r#""temperature": 201,"#).unwrap_err();
        assert_eq!(
            err.to_string(),
            "deck 0 has temperature 201 °C; it must be -270 to 200"
        );
    }
}

#[cfg(test)]
mod material_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-deck map whose legend has `extra` entries besides `.` floor.
    fn map_with_legend(extra: &str, row: &str) -> Result<LoadedMap, MapError> {
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", {extra} }},
                 "decks": [ {{ "rows": [{row:?}] }} ] }}"#
        );
        parse_map(&json)
    }

    #[test]
    fn a_structure_is_made_of_its_own_material_unless_the_legend_says() {
        let loaded = map_with_legend(r#""W": "wall""#, "@.W").unwrap();
        let wall = Pos::new(2, 0, 0);
        assert_eq!(loaded.map.structure_index(wall), Some(1));
        assert_eq!(loaded.map.material_index(wall), Some(0));
        assert_eq!(loaded.content.materials()[0].id, "steel");
        assert_eq!(
            loaded.content.materials().len(),
            2,
            "the whole list comes along"
        );
        assert_eq!(loaded.content.structures().len(), 14);
    }

    #[test]
    fn a_legend_entry_can_name_its_material() {
        let entry = r#""H": { "structure": "wall", "material": "hard_alloy" }"#;
        let loaded = map_with_legend(entry, "@.H").unwrap();
        assert_eq!(loaded.map.get(Pos::new(2, 0, 0)), Some(Tile::WALL));
        assert_eq!(loaded.map.material_index(Pos::new(2, 0, 0)), Some(1));
        assert_eq!(
            loaded.map.material_index(Pos::new(0, 0, 0)),
            Some(0),
            "the player starts on steel floor"
        );
    }

    #[test]
    fn an_open_door_is_its_own_structure() {
        let loaded = map_with_legend(r#""'": "open_door""#, "@.'").unwrap();
        assert_eq!(loaded.map.get(Pos::new(2, 0, 0)), Some(Tile::FLOOR));
        assert_eq!(loaded.map.structure_index(Pos::new(2, 0, 0)), Some(8));
    }

    #[test]
    fn open_is_no_longer_a_legend_field() {
        let entry = r#""'": { "structure": "door", "open": true }"#;
        assert!(map_with_legend(entry, "@.'").is_err());
    }

    #[test]
    fn unknown_structures_and_materials_are_errors_naming_the_glyph() {
        let err = map_with_legend(r#""H": "cheese_wall""#, "@.H").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"legend entry 'H' is "cheese_wall", which isn't a structure"#
        );
        let entry = r#""H": { "structure": "wall", "material": "cheese" }"#;
        let err = map_with_legend(entry, "@.H").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"legend entry 'H' is made of "cheese", which isn't a material"#
        );
    }

    #[test]
    fn a_legend_entry_with_a_misspelled_field_is_an_error() {
        let entry = r#""H": { "structure": "wall", "materal": "steel" }"#;
        let err = map_with_legend(entry, "@.H").unwrap_err();
        assert!(
            err.to_string()
                .contains("did not match any variant of untagged enum LegendEntry"),
            "{err}"
        );
    }

    #[test]
    fn the_test_helper_leaves_broken_json_for_the_real_parser() {
        assert!(matches!(parse_map("{ nope"), Err(MapError::Json(_))));
    }
}

#[cfg(test)]
mod item_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-row map: `@`, then `s` (floor with `items` on it), then a wall.
    /// `top` is extra top-level fields, like `player_wears`.
    fn map_with(items: &str, top: &str) -> Result<LoadedMap, MapError> {
        let json = format!(
            r##"{{ "id": "t", "name": "T", {top}
                 "legend": {{ "#": "wall",
                             "s": {{ "structure": "floor", "items": {items} }} }},
                 "decks": [ {{ "rows": ["@s#"] }} ] }}"##
        );
        parse_map(&json)
    }

    #[test]
    fn a_legend_entry_can_lay_items_on_the_floor() {
        let loaded = map_with(
            r#"[ { "item": "cutter" }, { "item": "scrap", "count": 3 } ]"#,
            "",
        )
        .unwrap();
        assert_eq!(loaded.floor_items.len(), 4);
        assert!(loaded.floor_items.iter().all(|f| f.at == Pos::new(1, 0, 0)));
        assert_eq!(loaded.floor_items[0].item.kind, 1);
        assert_eq!(loaded.floor_items[3].item.kind, 2);
        assert_eq!(loaded.content.item_types().len(), 8);
        assert!(loaded.worn.is_empty(), "nothing worn unless the map says");
    }

    #[test]
    fn items_can_be_made_of_another_material() {
        let loaded = map_with(r#"[ { "item": "scrap", "material": "hard_alloy" } ]"#, "").unwrap();
        assert_eq!(loaded.floor_items[0].item.material, 1);
    }

    #[test]
    fn the_player_starts_wearing_what_the_map_says() {
        let loaded = map_with("[]", r#""player_wears": [ { "item": "suit" } ],"#).unwrap();
        assert_eq!(loaded.worn.len(), 1);
        assert_eq!(loaded.worn[0].pockets.len(), 2, "with its pockets, empty");
    }

    fn error(items: &str, top: &str) -> String {
        map_with(items, top).unwrap_err().to_string()
    }

    #[test]
    fn mistakes_in_item_lists_name_the_glyph_and_the_item() {
        assert_eq!(
            error(r#"[ { "item": "spoon" } ]"#, ""),
            r#"legend entry 's': "spoon" isn't an item"#
        );
        assert_eq!(
            error(r#"[ { "item": "scrap", "material": "cheese" } ]"#, ""),
            r#"legend entry 's': "scrap" is made of "cheese", which isn't a material"#
        );
        assert_eq!(
            error(r#"[ { "item": "scrap", "count": 0 } ]"#, ""),
            r#"legend entry 's': "scrap" has a count of 0"#
        );
        assert_eq!(
            error(r#"[ { "item": "cell", "charge_percent": 101 } ]"#, ""),
            r#"legend entry 's': "cell" has a charge_percent over 100"#
        );
        assert_eq!(
            error(r#"[ { "item": "scrap", "charge_percent": 50 } ]"#, ""),
            r#"legend entry 's': "scrap" has a charge_percent, but holds no charge"#
        );
    }

    #[test]
    fn cells_come_full_unless_the_map_says_and_tools_come_with_one_if_it_does() {
        let loaded = map_with(
            r#"[ { "item": "cell" }, { "item": "cell", "charge_percent": 25 },
                 { "item": "saw" }, { "item": "saw", "charge_percent": 60 } ]"#,
            "",
        )
        .unwrap();
        let items: Vec<&Item> = loaded.floor_items.iter().map(|f| &f.item).collect();
        assert_eq!((items[0].charge_j, items[1].charge_j), (10_000, 2500));
        assert_eq!(items[2].cell, None, "a saw comes empty");
        let cell = items[3].cell.as_deref().unwrap();
        assert_eq!((cell.kind, cell.charge_j), (4, 6000), "or with a cell");
    }

    #[test]
    fn items_cannot_be_put_inside_a_wall() {
        let json = r##"{ "id": "t", "name": "T",
            "legend": { "W": { "structure": "wall", "items": [ { "item": "scrap" } ] } },
            "decks": [ { "rows": ["@W"] } ] }"##;
        assert_eq!(
            parse_map(json).unwrap_err().to_string(),
            "legend entry 'W' puts items on something solid; they need a floor"
        );
    }

    #[test]
    fn the_player_can_only_wear_wearable_things() {
        assert_eq!(
            error("[]", r#""player_wears": [ { "item": "scrap" } ],"#),
            r#"player_wears: "scrap" can't be worn"#
        );
        assert_eq!(
            error("[]", r#""player_wears": [ { "item": "toga" } ],"#),
            r#"player_wears: "toga" isn't an item"#
        );
    }

    #[test]
    fn the_player_wears_one_thing_in_each_slot() {
        let two = r#""player_wears": [ { "item": "suit" }, { "item": "boots" } ],"#;
        assert_eq!(map_with("[]", two).unwrap().worn.len(), 2, "body and feet");
        let three = r#""player_wears": [ { "item": "boots" }, { "item": "suit" },
                                           { "item": "boots" } ],"#;
        assert_eq!(
            error("[]", three),
            r#"player_wears: "boots" is worn on the feet, where something else already is"#
        );
    }
}

#[cfg(test)]
mod furniture_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-deck map drawn with `rows`, with floor `.`, wall `#`, and the
    /// test furniture: `b` a bench (two tiles across), `B` a bench turned
    /// to run down, `s` a stool you can walk through, `c` a crate in the
    /// way.
    fn map(rows: &[&str]) -> Result<LoadedMap, MapError> {
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ ".": "floor", "#": "wall",
                     "b": {{ "structure": "floor", "furniture": "bench" }},
                     "B": {{ "structure": "floor", "furniture": "bench", "turned": true }},
                     "s": {{ "structure": "floor", "furniture": "stool" }},
                     "c": {{ "structure": "floor", "furniture": "crate" }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        parse_map(&json)
    }

    fn error(rows: &[&str]) -> String {
        map(rows).unwrap_err().to_string()
    }

    /// Where each piece's top-left is, and whether it's turned.
    fn placed(loaded: &LoadedMap) -> Vec<(i32, i32, bool)> {
        let pieces = loaded.furniture.iter();
        pieces.map(|p| (p.at.x, p.at.y, p.turned)).collect()
    }

    #[test]
    fn furniture_is_drawn_where_it_stands_and_cut_into_pieces() {
        let loaded = map(&["@bbbb.", "c.B..s", "..B..."]).unwrap();
        assert_eq!(
            placed(&loaded),
            [
                (1, 0, false),
                (3, 0, false),
                (0, 1, false),
                (2, 1, true),
                (5, 1, false)
            ],
            "two benches side by side, a crate, a turned bench and a stool"
        );
        let bench = loaded.content.furniture_index("bench").unwrap();
        assert_eq!(loaded.furniture[0].kind, bench);
        let steel = loaded.content.material_index("steel").unwrap();
        assert_eq!(loaded.furniture[0].material, steel, "its own material");
        let floor = loaded.content.structure_index("floor").unwrap();
        assert_eq!(
            loaded.map.structure_index(Pos::new(1, 0, 0)),
            Some(floor),
            "standing on the floor"
        );
    }

    #[test]
    fn furniture_drawn_the_wrong_shape_says_where() {
        assert_eq!(
            error(&["@b."]),
            "deck 0, row 0, column 1: the bench 'b' starting here also needs row 0, column 2 drawn as 'b'"
        );
        assert_eq!(
            error(&["@bbb"]),
            "deck 0, row 0, column 3: the bench 'b' starting here also needs row 0, column 4 drawn as 'b'",
            "an odd one out at the end of the row (off the map)"
        );
        assert_eq!(
            error(&["@B.", "..."]),
            "deck 0, row 0, column 1: the bench 'B' starting here also needs row 1, column 1 drawn as 'B'"
        );
        assert!(map(&["@bb", "BB.", "BB."]).is_ok(), "pieces may touch");
    }

    #[test]
    fn legend_mistakes_with_furniture_are_errors() {
        let one = |entry: &str| {
            let json = format!(
                r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", "#": "wall", "x": {entry} }},
                     "decks": [ {{ "rows": ["@x"] }} ] }}"##
            );
            parse_map(&json).unwrap_err().to_string()
        };
        assert_eq!(
            one(r#"{ "structure": "floor", "furniture": "sofa" }"#),
            r#"legend entry 'x' has furniture "sofa", which isn't furniture"#
        );
        assert_eq!(
            one(r#"{ "structure": "wall", "furniture": "crate" }"#),
            "legend entry 'x' stands furniture on something solid; it needs a floor"
        );
        assert_eq!(
            one(r#"{ "structure": "floor", "turned": true }"#),
            "legend entry 'x' is turned, but has no furniture to turn"
        );
    }

    #[test]
    fn a_container_drawn_on_the_map_holds_what_its_entry_says() {
        let json = |contents: &str| {
            format!(
                r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor",
                     "c": {{ "structure": "floor", "furniture": "cabinet", "contents": {contents} }} }},
                     "decks": [ {{ "rows": ["@cc"] }} ] }}"##
            )
        };
        let loaded = parse_map(&json(r#"[ { "item": "scrap", "count": 2 } ]"#)).unwrap();
        assert_eq!(loaded.furniture.len(), 2);
        for piece in &loaded.furniture {
            assert_eq!(piece.contents.len(), 2, "each cabinet gets its own");
        }
        let error = |contents: &str| parse_map(&json(contents)).unwrap_err().to_string();
        assert_eq!(
            error(r#"[ { "item": "cutter", "count": 2 } ]"#),
            "legend entry 'c': its contents take 1600 ml, but a cabinet holds 1000 ml"
        );
        assert_eq!(
            error(r#"[ { "item": "toga" } ]"#),
            r#"legend entry 'c': "toga" isn't an item"#
        );
    }

    #[test]
    fn contents_need_furniture_and_only_containers_hold_any() {
        let one = |entry: &str| {
            let json = format!(
                r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", "x": {entry} }},
                     "decks": [ {{ "rows": ["@x"] }} ] }}"##
            );
            parse_map(&json).unwrap_err().to_string()
        };
        assert_eq!(
            one(r#"{ "structure": "floor", "contents": [ { "item": "scrap" } ] }"#),
            "legend entry 'x' has contents, but no furniture to hold them"
        );
        assert_eq!(
            one(
                r#"{ "structure": "floor", "furniture": "crate", "contents": [ { "item": "scrap" } ] }"#
            ),
            "legend entry 'x': its contents take 100 ml, but a crate holds 0 ml"
        );
    }

    #[test]
    fn the_player_can_start_on_a_stool_but_not_inside_a_crate() {
        let on = |furniture: &str| {
            let json = format!(
                r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor",
                     "@": {{ "structure": "floor", "furniture": "{furniture}" }} }},
                     "decks": [ {{ "rows": ["@."] }} ] }}"##
            );
            parse_map(&json)
        };
        assert_eq!(on("stool").unwrap().furniture.len(), 1);
        assert_eq!(
            on("crate").unwrap_err().to_string(),
            "'@' marks where the player starts, so it must stand for something walkable, not \"crate\""
        );
    }
}
