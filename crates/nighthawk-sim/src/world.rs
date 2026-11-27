//! The world state and the one function that changes it.

use serde::{Deserialize, Serialize};

use crate::action::Effect;
use crate::body::{self, BREATH_FULL, BreathLevel, Cause, WARMTH_FULL, WarmthLevel};
use crate::calendar::{DateTime, SHIP_START};
use crate::content::Content;
use crate::furniture::{Furnishings, FurnitureType, Piece};
use crate::hash::Fnv64;
use crate::interact::{self, Blocked};
use crate::item::{self, FloorItem, Item, ItemContext, ItemPath, ItemRef, ItemType};
use crate::light::Light;
use crate::map::{Map, Tile};
use crate::map_file::LoadedMap;
use crate::material::Material;
use crate::pos::{Dir, Pos, Vertical};
use crate::recipe::{Recipe, RecipeMaterial};
use crate::rng::{GameRng, Stream};
use crate::sight::{Remembered, Scene, View};
use crate::structure::Structure;

/// Something the player wants to do. Front-ends turn key presses into these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Move(Dir),
    Wait,
    /// Use the stairs you're standing on.
    Climb(Vertical),
    /// Do action number `action` (its place in the world's list of
    /// actions) to the tile at `at`, from the interact menu (D36) or a
    /// shortcut key. The same rules the menu shows are checked again here.
    Interact {
        at: Pos,
        action: u16,
    },
    /// Drop a carried item onto the floor where you stand.
    Drop(ItemPath),
}

/// Something that happened, for the front-end to show or log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Moved {
        to: Pos,
    },
    /// The player tried to walk into something solid. No time passes.
    Bumped {
        at: Pos,
        tile: Tile,
    },
    /// The player tried to walk off the edge of the map. No time passes.
    BlockedByEdge,
    /// The player used stairs and arrived at `to`, one deck up or down.
    Climbed {
        to: Pos,
        way: Vertical,
    },
    /// The player tried to climb without standing on the right stairs. No
    /// time passes.
    NoStairs {
        way: Vertical,
    },
    Waited,
    /// A background noise from the ship, heard while time passes.
    Ambient(Sound),
    /// The space the player is in lost air to vacuum during this action.
    AirEscaping,
    /// The player's breath fell to, or recovered past, a level worth a
    /// warning.
    Breath(BreathLevel),
    /// The player's warmth fell to, or recovered past, a level worth a
    /// warning.
    Warmth(WarmthLevel),
    /// The player's breath or warmth ran out: they've blacked out, and the
    /// world takes no more commands.
    BlackedOut(Cause),
    /// Action number `action` couldn't be done, and why. No time passes.
    Unable {
        action: u16,
        blocked: Blocked,
    },
    /// The player picked this item up and put it in a pocket.
    PickedUp(ItemRef),
    /// This item fits in none of the player's pockets, so it stays put.
    NoRoom(ItemRef),
    /// The player dropped this item where they stand.
    Dropped(ItemRef),
    /// There's no item where a drop asked for one. No time passes.
    NothingToDrop,
    /// The player followed a recipe for action number `action` on the tile
    /// at `at`, which was structure number `was` and is now what the
    /// recipe makes (it cut through a door, patched a crack…).
    Transformed {
        at: Pos,
        action: u16,
        was: u16,
    },
}

/// Noises the dead ship makes. Pure atmosphere for now. Later, sounds will
/// come from real things (a pump, a breach, something in the vents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    VentTick,
    HullCreak,
    DistantClang,
    CoolantDrip,
}

impl Sound {
    pub const ALL: [Sound; 4] = [
        Sound::VentTick,
        Sound::HullCreak,
        Sound::DistantClang,
        Sound::CoolantDrip,
    ];
}

// How long each action takes, in seconds (decision D29). The world is
// simulated one second at a time, so an action that takes 20 seconds lets
// 20 seconds of air, heat and everything else happen.

/// One step across flat floor: 1 m.
pub const STEP_SECONDS: u64 = 3;
/// One diagonal step: about 1.4 m, so about 1.4 times as long.
pub const DIAGONAL_STEP_SECONDS: u64 = 4;
/// Climbing a ladder to the next deck.
pub const CLIMB_SECONDS: u64 = 20;
/// Waiting (`.`): a short pause.
pub const WAIT_SECONDS: u64 = 5;
/// Dropping an item. (Actions on tiles, like opening a door or picking
/// something up, say how long they take in `data/actions/` and
/// `data/recipes/`.)
pub const DROP_SECONDS: u64 = 1;

/// Each second has a 1 in this many chance of an ambient sound, so about
/// one sound every 25 seconds.
const AMBIENT_ONE_IN: u32 = 25;

/// Everything about the game at one moment.
///
/// This struct *is* the save game: everything in it is written to the save
/// file, and nothing else is (see `save.rs`). So any new piece of game state
/// goes here, and into [`World::state_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    /// One name per deck, bottom to top. The map panel shows the name of
    /// the deck the player is on.
    deck_names: Vec<String>,
    map: Map,
    player: Pos,
    /// Seconds of game time since the game began.
    seconds: u64,
    /// The seed this game started from. Same seed + same commands = same game.
    seed: u64,
    ambient_rng: GameRng,
    /// The player's breath meter, 0 to [`BREATH_FULL`].
    breath: u32,
    /// The player's warmth meter, 0 to [`WARMTH_FULL`].
    warmth: u32,
    /// Every material, structure, item type and recipe, copied from
    /// `data/` when the game began and saved with it, so editing the data
    /// files never changes a game in progress (decisions D35 and D43).
    /// Tiles and items refer to entries by their place in its lists.
    content: Content,
    /// The items lying on the floor around the ship.
    floor_items: Vec<FloorItem>,
    /// Every piece of furniture standing in the ship (D53).
    furniture: Furnishings,
    /// What the player is wearing; carried items are in its pockets.
    worn: Vec<Item>,
    /// What the player has seen of the ship, and can see now (D49).
    view: View,
}

impl World {
    /// A new game on a loaded map, with the player at the map's `@`, the
    /// clock at 0 seconds, and all randomness derived from `seed`. The
    /// player starts out seeing what's around them.
    pub fn new(loaded: LoadedMap, seed: u64) -> Self {
        let view = View::new(loaded.map.tile_count());
        let kinds = loaded.content.furniture();
        let furniture = Furnishings::new(loaded.furniture, &loaded.map, kinds);
        let mut world = Self {
            deck_names: loaded.deck_names,
            map: loaded.map,
            player: loaded.player_start,
            seconds: 0,
            seed,
            ambient_rng: GameRng::new(seed, Stream::Ambient),
            breath: BREATH_FULL,
            warmth: WARMTH_FULL,
            content: loaded.content,
            floor_items: loaded.floor_items,
            furniture,
            worn: loaded.worn,
            view,
        };
        world.refresh_view();
        world
    }

    /// Works out what the player can see from where they stand now, and
    /// remembers it. Done after every command, since anything that changes
    /// (where you are, a door, an item) changes what you see.
    fn refresh_view(&mut self) {
        let lights = self.lights();
        // These are all different fields of `self` from `self.view`, so
        // they can be read while the view changes.
        let (map, furniture, items) = (&self.map, &self.furniture, &self.floor_items);
        let kinds = self.content.furniture();
        let kind_at = |p: Pos| furniture.at(map, p).map(|piece| piece.kind);
        let blocks = |p: Pos| {
            let tall = kind_at(p).and_then(|k| kinds.get(usize::from(k)));
            map.get(p).is_none_or(Tile::blocks_sight) || tall.is_some_and(|k| k.blocks_sight)
        };
        let scene = Scene {
            map,
            blocks: &blocks,
            lights: &lights,
            furniture: &kind_at,
            top_item: &|p| items.iter().find(|f| f.at == p).map(|f| f.item.kind),
        };
        self.view.refresh(&scene, self.player);
    }

    /// Every light on the player's deck (only those can light what the
    /// player sees): structures that give light, items lying there that
    /// do, and the brightest thing the player wears or carries.
    fn lights(&self) -> Vec<Light> {
        let z = self.player.z;
        let light_of = |kind: u16| {
            self.content
                .item_types()
                .get(usize::from(kind))
                .map_or(0, |t| t.light)
        };
        let built = self.map.positions().filter(|p| p.z == z).map(|at| Light {
            at,
            reach: self.map.get(at).map_or(0, |tile| tile.light),
        });
        let lying = self
            .floor_items
            .iter()
            .filter(|f| f.at.z == z)
            .map(|f| Light {
                at: f.at,
                reach: light_of(f.item.kind),
            });
        let held = self
            .worn
            .iter()
            .chain(self.carried().into_iter().map(|(_, item)| item));
        let carried = Light {
            at: self.player,
            reach: held.map(|item| light_of(item.kind)).max().unwrap_or(0),
        };
        built
            .chain(lying)
            .chain([carried])
            .filter(|light| light.reach > 0)
            .collect()
    }

    /// Whether the tile at `p` is lit right now. Only the player's deck is
    /// ever lit: light elsewhere can't be seen.
    pub fn is_lit(&self, p: Pos) -> bool {
        self.view.is_lit(&self.map, p)
    }

    /// Every piece of furniture in the ship.
    pub fn furniture(&self) -> &[Piece] {
        self.furniture.pieces()
    }

    /// The piece of furniture covering the tile at `p`, if any.
    pub fn furniture_at(&self, p: Pos) -> Option<&Piece> {
        self.furniture.at(&self.map, p)
    }

    /// The kind of furniture covering the tile at `p`, if any.
    pub fn furniture_kind_at(&self, p: Pos) -> Option<&FurnitureType> {
        let piece = self.furniture_at(p)?;
        self.content.furniture().get(usize::from(piece.kind))
    }

    /// Whether furniture at `p` is in the way of walking there.
    fn furniture_in_the_way(&self, p: Pos) -> bool {
        self.furniture_kind_at(p).is_some_and(|k| !k.walkable)
    }

    /// Whether the player can see the tile at `p` right now.
    pub fn is_visible(&self, p: Pos) -> bool {
        self.view.is_visible(&self.map, p)
    }

    /// What the player remembers of the tile at `p` (the last they saw of
    /// it), if they've ever seen it. For a tile in view, it's as it is now.
    pub fn remembered(&self, p: Pos) -> Option<Remembered> {
        self.view.remembered(&self.map, p)
    }

    /// The name of the deck the player is on, like "Stasis bay".
    pub fn area_name(&self) -> &str {
        self.deck_name(self.player.z)
    }

    /// The name of deck `z`, or `""` if there's no such deck.
    pub fn deck_name(&self, z: i32) -> &str {
        usize::try_from(z)
            .ok()
            .and_then(|z| self.deck_names.get(z))
            .map_or("", String::as_str)
    }

    pub fn map(&self) -> &Map {
        &self.map
    }

    /// What the tile at `p` is made of, or `None` off the map.
    pub fn material_at(&self, p: Pos) -> Option<&Material> {
        let index = self.map.material_index(p)?;
        self.materials().get(usize::from(index))
    }

    /// The items lying at `p`, in the order they were put there.
    pub fn items_at(&self, p: Pos) -> impl Iterator<Item = &Item> {
        self.floor_items
            .iter()
            .filter(move |f| f.at == p)
            .map(|f| &f.item)
    }

    /// Every item lying on the floor.
    pub fn floor_items(&self) -> &[FloorItem] {
        &self.floor_items
    }

    /// What the player is wearing (carried items are in its pockets).
    pub fn worn(&self) -> &[Item] {
        &self.worn
    }

    /// Every carried item and where it is: worn item by worn item, pocket
    /// by pocket, in order. What the inventory screen lists.
    pub fn carried(&self) -> Vec<(ItemPath, &Item)> {
        self.worn
            .iter()
            .enumerate()
            .flat_map(|(worn, wearing)| item::pocketed(worn, wearing))
            .collect()
    }

    /// How many items of type number `kind` the player carries.
    pub fn carried_count(&self, kind: u16) -> u32 {
        let n = self
            .carried()
            .iter()
            .filter(|(_, item)| item.kind == kind)
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    /// The number of the item type with this id, if there is one.
    pub fn item_type_index(&self, id: &str) -> Option<u16> {
        self.content.item_type_index(id)
    }

    /// Every kind of item in this game.
    pub fn item_types(&self) -> &[ItemType] {
        self.content.item_types()
    }

    /// Everything this game is made of.
    pub fn content(&self) -> &Content {
        &self.content
    }

    /// The item types and materials together, for working out an item's
    /// weight and whether it fits a pocket.
    pub fn item_context(&self) -> ItemContext<'_> {
        ItemContext {
            types: self.content.item_types(),
            materials: self.content.materials(),
        }
    }

    /// What the tile at `p` is, or `None` off the map.
    pub fn structure_at(&self, p: Pos) -> Option<&Structure> {
        let index = self.map.structure_index(p)?;
        self.structures().get(usize::from(index))
    }

    /// Every structure in this game, in the order tiles refer to them.
    pub fn structures(&self) -> &[Structure] {
        self.content.structures()
    }

    /// Every material in this game, in the order tiles refer to them.
    pub fn materials(&self) -> &[Material] {
        self.content.materials()
    }

    /// The materials, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn materials_mut(&mut self) -> &mut Vec<Material> {
        self.content.materials_mut()
    }

    pub fn player(&self) -> Pos {
        self.player
    }

    /// Seconds of game time since the game began.
    pub fn seconds(&self) -> u64 {
        self.seconds
    }

    /// The date and time on the ship's clock: [`SHIP_START`] plus the
    /// seconds that have passed.
    pub fn now(&self) -> DateTime {
        SHIP_START.plus_seconds(self.seconds)
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Carries out one command and reports what happened.
    ///
    /// This is the *only* place the world changes, and the only place time
    /// moves forward (decision D7). An action that fails, like bumping a
    /// wall, costs no time.
    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        // A blacked-out player can't do anything.
        if !self.is_conscious() {
            return Vec::new();
        }
        let events = match command {
            Command::Move(dir) => self.walk(dir),
            Command::Climb(way) => self.climb(way),
            Command::Interact { at, action } => self.interact(at, action),
            Command::Drop(path) => self.drop_item(path),
            Command::Wait => {
                let mut events = vec![Event::Waited];
                self.pass_time(WAIT_SECONDS, &mut events);
                events
            }
        };
        self.refresh_view();
        events
    }

    fn walk(&mut self, dir: Dir) -> Vec<Event> {
        let target = self.player.step(dir);
        match self.map.get(target) {
            None => vec![Event::BlockedByEdge],
            Some(tile) if !tile.is_walkable() || self.furniture_in_the_way(target) => {
                self.bump(target, tile)
            }
            Some(_) => {
                self.player = target;
                let mut events = vec![Event::Moved { to: target }];
                let seconds = if dir.is_diagonal() {
                    DIAGONAL_STEP_SECONDS
                } else {
                    STEP_SECONDS
                };
                self.pass_time(seconds, &mut events);
                events
            }
        }
    }

    /// Walking into the solid `tile` at `at`: does the first action marked
    /// `on_bump` that can be done there (walking into a shut door opens
    /// it), or else bumps into it, which costs no time.
    fn bump(&mut self, at: Pos, tile: Tile) -> Vec<Event> {
        let chosen = (0..)
            .zip(self.content.actions())
            .find(|&(number, action)| {
                action.on_bump && interact::blocked(self, at, number).is_none()
            })
            .map(|(number, _)| number);
        match chosen {
            Some(action) => self.interact(at, action),
            None => vec![Event::Bumped { at, tile }],
        }
    }

    /// Action number `action` on the tile at `at`. If anything blocks it,
    /// nothing happens and no time passes; the event says why.
    fn interact(&mut self, at: Pos, action: u16) -> Vec<Event> {
        if let Some(blocked) = interact::blocked(self, at, action) {
            return vec![Event::Unable { action, blocked }];
        }
        // `blocked` checked the action exists. Copy out what's needed, so
        // the borrow of `self.content` ends before `self` changes.
        let rule = &self.content.actions()[usize::from(action)];
        let (effect, seconds) = (rule.effect, u64::from(rule.seconds.unwrap_or(0)));
        match effect {
            Effect::Recipe => self.follow_recipe(at, action),
            Effect::PickUp => self.pick_up(at, seconds),
        }
    }

    /// Follows the recipe for action number `action` on the tile at `at`, which the caller has
    /// checked exists and can be done (decision D40). The recipe's time
    /// passes first; if the player blacks out before it's done, nothing
    /// changes and nothing is used up. Then the items it uses come out of
    /// the player's pockets, and the tile becomes the recipe's `to`: of the
    /// same material (an aluminium door cut open leaves an aluminium frame),
    /// or of the first item used (a crack patched with steel scrap).
    fn follow_recipe(&mut self, at: Pos, action: u16) -> Vec<Event> {
        let was = self.map.structure_index(at).unwrap_or(0);
        let id = &self.content.actions()[usize::from(action)].id;
        let recipe = self
            .recipe_for(id, at)
            .cloned()
            .expect("interact checked there's a recipe");
        let mut events = Vec::new();
        self.pass_time(u64::from(recipe.seconds), &mut events);
        if !self.is_conscious() {
            return events;
        }
        let used = self.use_up(&recipe);
        if recipe.material == RecipeMaterial::FromUsed
            && let Some(first) = used.first()
        {
            self.map.set_material(at, first.material);
        }
        let to = self.structure_index_of(&recipe.to);
        let tile = self.structures()[usize::from(to)].tile();
        self.map.place(at, to, tile);
        // Last, after everything heard while working, so it's the line the
        // log ends on.
        events.push(Event::Transformed { at, action, was });
        events
    }

    /// Takes the items `recipe` uses out of the player's pockets.
    fn use_up(&mut self, recipe: &Recipe) -> Vec<Item> {
        let mut used = Vec::new();
        for needed in &recipe.uses {
            let kind = self
                .item_type_index(&needed.item)
                .expect("recipes name real items");
            let count = usize::try_from(needed.count).unwrap_or(usize::MAX);
            used.extend(item::take_kind(&mut self.worn, kind, count));
        }
        used
    }

    /// The index of the structure with this id. Recipes are checked on load
    /// to name only real structures, so it's always there.
    fn structure_index_of(&self, id: &str) -> u16 {
        self.content
            .structure_index(id)
            .expect("recipes name real structures")
    }

    /// The recipe for the action with id `action` on whatever structure is
    /// at `at`, if there is one.
    pub fn recipe_for(&self, action: &str, at: Pos) -> Option<&Recipe> {
        let structure = self.structure_at(at)?;
        self.content
            .recipes()
            .iter()
            .find(|r| r.action == action && r.from == structure.id)
    }

    /// The best level of `quality` among everything the player wears or
    /// carries, or 0 with nothing that has it. A tool works from any
    /// pocket (decision D38).
    pub fn tool_level(&self, quality: &str) -> u8 {
        let carried = self.carried().into_iter().map(|(_, item)| item);
        self.worn
            .iter()
            .chain(carried)
            .filter_map(|item| self.item_types().get(usize::from(item.kind)))
            .filter_map(|kind| kind.qualities.get(quality).copied())
            .max()
            .unwrap_or(0)
    }

    /// Picks up everything at `at` that fits in a pocket, one item at a
    /// time, taking `seconds_each` per item picked up; anything that
    /// doesn't fit stays where it was.
    fn pick_up(&mut self, at: Pos, seconds_each: u64) -> Vec<Event> {
        let (here, elsewhere): (Vec<FloorItem>, Vec<FloorItem>) =
            std::mem::take(&mut self.floor_items)
                .into_iter()
                .partition(|f| f.at == at);
        self.floor_items = elsewhere;
        // Borrowing two fields to read while changing two others is fine:
        // Rust tracks borrows of separate fields of `self` separately.
        let ctx = ItemContext {
            types: self.content.item_types(),
            materials: self.content.materials(),
        };
        let mut events = Vec::new();
        let mut picked = 0;
        for FloorItem { item, .. } in here {
            let id = item.id();
            match item::stow(&mut self.worn, item, ctx) {
                Ok(()) => {
                    picked += 1;
                    events.push(Event::PickedUp(id));
                }
                Err(item) => {
                    self.floor_items.push(FloorItem { at, item });
                    events.push(Event::NoRoom(id));
                }
            }
        }
        self.pass_time(seconds_each * picked, &mut events);
        events
    }

    /// Drops the carried item at `path` where the player stands.
    fn drop_item(&mut self, path: ItemPath) -> Vec<Event> {
        let Some(item) = item::take(&mut self.worn, path) else {
            return vec![Event::NothingToDrop];
        };
        let mut events = vec![Event::Dropped(item.id())];
        self.floor_items.push(FloorItem {
            at: self.player,
            item,
        });
        self.pass_time(DROP_SECONDS, &mut events);
        events
    }

    /// Uses the ladder under the player. A ladder up only goes up and one
    /// down only goes down; anywhere else nothing happens and no time
    /// passes. Maps guarantee every ladder has a partner
    /// (`Map::check_stairs`), but this still checks the destination is
    /// walkable, to stay safe.
    fn climb(&mut self, way: Vertical) -> Vec<Event> {
        let target = self.player.climb(way);
        let on_stairs = self.map.get(self.player).and_then(|t| t.climb) == Some(way);
        if !on_stairs || !self.map.get(target).is_some_and(Tile::is_walkable) {
            return vec![Event::NoStairs { way }];
        }
        self.player = target;
        let mut events = vec![Event::Climbed { to: target, way }];
        self.pass_time(CLIMB_SECONDS, &mut events);
        events
    }

    /// Advances the clock one second at a time, letting the world react to
    /// each second. Every system that runs "while time passes" (air, heat,
    /// power, creatures) will hook in here, so it only ever runs because the
    /// player did something.
    ///
    /// If the player blacks out, time stops there: the rest of the action
    /// never happens.
    fn pass_time(&mut self, seconds: u64, events: &mut Vec<Event>) {
        let mut heard_escaping = false;
        for _ in 0..seconds {
            self.seconds += 1;
            let venting = self.map.spread_air();
            self.map.spread_heat();
            if !heard_escaping
                && self
                    .map
                    .index_of(self.player)
                    .is_some_and(|i| venting.contains(&i))
            {
                heard_escaping = true;
                events.push(Event::AirEscaping);
            }
            self.breathe(events);
            self.keep_warm(events);
            if !self.is_conscious() {
                return;
            }
            if self.ambient_rng.one_in(AMBIENT_ONE_IN)
                && let Some(&sound) = self.ambient_rng.pick(&Sound::ALL)
            {
                events.push(Event::Ambient(sound));
            }
        }
    }

    /// One second of breathing the air on the player's tile.
    fn breathe(&mut self, events: &mut Vec<Event>) {
        let o2 = self.map.air(self.player).map_or(0, |mix| mix.o2);
        let before = self.breath;
        self.breath = body::breath_after(before, o2);
        let levels = body::breath_levels(before, self.breath);
        events.extend(levels.into_iter().map(Event::Breath));
        if before > 0 && self.breath == 0 {
            events.push(Event::BlackedOut(Cause::Air));
        }
    }

    /// One second of keeping warm (or not) in the air on the player's tile.
    /// If breath has just run out, the player is already out cold, so this
    /// does nothing: there's only ever one cause of blacking out.
    fn keep_warm(&mut self, events: &mut Vec<Event>) {
        if !self.is_conscious() {
            return;
        }
        let before = self.warmth;
        self.warmth = body::warmth_after(before, self.map.temperature(self.player));
        let levels = body::warmth_levels(before, self.warmth);
        events.extend(levels.into_iter().map(Event::Warmth));
        if self.warmth == 0 {
            events.push(Event::BlackedOut(Cause::Cold));
        }
    }

    /// The player's breath meter, 0 (blacked out) to [`BREATH_FULL`].
    pub fn breath(&self) -> u32 {
        self.breath
    }

    /// The player's warmth meter, 0 (blacked out) to [`WARMTH_FULL`].
    pub fn warmth(&self) -> u32 {
        self.warmth
    }

    /// Whether the player is still awake. Once breath or warmth runs out
    /// they black out, and [`World::apply`] does nothing more.
    pub fn is_conscious(&self) -> bool {
        self.blackout_cause().is_none()
    }

    /// Why the player blacked out, or `None` while they're awake.
    pub fn blackout_cause(&self) -> Option<Cause> {
        if self.breath == 0 {
            Some(Cause::Air)
        } else if self.warmth == 0 {
            Some(Cause::Cold)
        } else {
            None
        }
    }

    /// Checks rules that a world built by [`World::new`] always follows, for
    /// worlds read back from a save file: the map is consistent and the
    /// player stands on a walkable tile inside it.
    pub(crate) fn check_consistent(&self) -> Result<(), String> {
        self.map.check_consistent()?;
        let (names, depth) = (self.deck_names.len(), self.map.depth());
        if i64::try_from(names).ok() != Some(i64::from(depth)) {
            return Err(format!("{names} deck names for {depth} decks"));
        }
        self.check_meters()?;

        self.check_items()?;
        let highest = usize::from(self.map.highest_material());
        if highest >= self.materials().len() {
            return Err(format!(
                "a tile is made of material {highest}, but there are only {}",
                self.materials().len()
            ));
        }
        let kinds = self.content.furniture();
        self.furniture
            .check(&self.map, kinds, self.materials().len())?;
        self.check_player()
    }

    /// Checks the player stands somewhere they could have walked to.
    fn check_player(&self) -> Result<(), String> {
        let inside = |name: &str| format!("player at {:?} is inside a {name}", self.player);
        match self.map.get(self.player) {
            None => Err(format!("player at {:?} is outside the map", self.player)),
            Some(tile) if !tile.is_walkable() => Err(inside(
                self.structure_at(self.player)
                    .map_or("?", |s| s.name.as_str()),
            )),
            Some(_) => match self.furniture_kind_at(self.player) {
                Some(kind) if !kind.walkable => Err(inside(&kind.name)),
                _ => Ok(()),
            },
        }
    }

    /// Gets a world read back from a save ready to play: fills in each
    /// tile's properties from its structure (saves don't store them), then
    /// checks everything is consistent.
    pub(crate) fn finish_loading(&mut self) -> Result<(), String> {
        // Borrowing `self.content` to read and `self.map` to change at once
        // is fine: they're separate fields.
        let structures = self.content.structures();
        self.map
            .rebuild_tiles(|i| structures.get(usize::from(i)).map(Structure::tile))?;
        self.furniture.rebuild(&self.map, self.content.furniture());
        self.check_consistent()?;
        let counts = [
            self.structures().len(),
            self.content.furniture().len(),
            self.item_types().len(),
        ];
        self.view.check(self.map.tile_count(), counts)?;
        self.refresh_view();
        Ok(())
    }

    /// Checks the breath and warmth meters aren't over full.
    fn check_meters(&self) -> Result<(), String> {
        if self.breath > BREATH_FULL {
            return Err(format!(
                "breath {} is over the full {BREATH_FULL}",
                self.breath
            ));
        }
        if self.warmth > WARMTH_FULL {
            return Err(format!(
                "warmth {} is over the full {WARMTH_FULL}",
                self.warmth
            ));
        }
        Ok(())
    }

    /// Checks every item makes sense, everything on the floor lies on a
    /// walkable tile, and everything worn can be worn.
    fn check_items(&self) -> Result<(), String> {
        let ctx = self.item_context();
        for floor in &self.floor_items {
            item::check_item(&floor.item, ctx)?;
            if !self.map.get(floor.at).is_some_and(Tile::is_walkable) {
                return Err(format!("an item lies inside something at {:?}", floor.at));
            }
        }
        for worn in &self.worn {
            item::check_item(worn, ctx)?;
            let kind = &self.item_types()[usize::from(worn.kind)];
            if kind.wear.is_none() {
                return Err(format!(
                    "the player is wearing a {}, which can't be worn",
                    kind.id
                ));
            }
        }
        Ok(())
    }

    /// A fingerprint of the entire game state.
    ///
    /// Two worlds with the same hash are, for all practical purposes, the same
    /// game. The determinism test plays a scripted game and compares this
    /// against a stored value, so it changes exactly when game behaviour
    /// changes. Every new piece of state must be added here.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        // A version tag: bump it if the hashing scheme itself changes.
        h.str("nighthawk-world-v2")
            .u64(self.deck_names.len() as u64);
        for name in &self.deck_names {
            h.str(name);
        }
        self.map.hash_into(&mut h);
        h.i32(self.player.x).i32(self.player.y).i32(self.player.z);
        h.u64(self.seconds)
            .u64(self.seed)
            .u64(self.ambient_rng.fingerprint())
            .u64(u64::from(self.breath))
            .u64(u64::from(self.warmth));
        self.content.hash_into(&mut h);
        h.u64(self.floor_items.len() as u64);
        for floor in &self.floor_items {
            h.i32(floor.at.x).i32(floor.at.y).i32(floor.at.z);
            floor.item.hash_into(&mut h);
        }
        h.u64(self.worn.len() as u64);
        for worn in &self.worn {
            worn.hash_into(&mut h);
        }
        self.furniture.hash_into(&mut h);
        self.view.hash_into(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::tests::{CLOSE, CUT, OPEN, PICK_UP, REPAIR};
    use crate::map_file::parse_map_for_tests as parse_map;

    const TEST_SEED: u64 = 1234;
    /// How long the test actions take to open or close a door, and to pick
    /// up one item (`data/actions/` says, in the real game).
    const DOOR_SECONDS: u64 = 2;
    const PICK_UP_SECONDS: u64 = 2;

    /// The events with ambient sounds removed, for tests about movement.
    fn without_ambient(events: Vec<Event>) -> Vec<Event> {
        events
            .into_iter()
            .filter(|e| !matches!(e, Event::Ambient(_)))
            .collect()
    }

    fn count_ambient(events: &[Event]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, Event::Ambient(_)))
            .count()
    }

    /// A small room: walls around the edge, one pillar, the player inside.
    ///
    /// ```text
    /// ######
    /// #.@..#
    /// #..#.#
    /// ######
    /// ```
    fn test_world() -> World {
        world_from_rows(&["######", "#.@..#", "#..#.#", "######"])
    }

    fn world_from_rows(rows: &[&str]) -> World {
        let rows: Vec<String> = rows.iter().map(|r| format!("{r:?}")).collect();
        let json = format!(
            r##"{{ "id": "t", "name": "Test room", "legend": {{ "#": "wall", ".": "floor" }},
                  "decks": [ {{ "rows": [{}] }} ] }}"##,
            rows.join(", ")
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    #[test]
    fn a_new_world_starts_at_zero_seconds_on_the_start_tile() {
        let world = test_world();
        assert_eq!(world.seconds(), 0);
        assert_eq!(world.player(), Pos::new(2, 1, 0));
        assert_eq!(world.area_name(), "Deck 0", "unnamed decks get a number");
        assert_eq!(world.map().width(), 6);
        assert_eq!(world.seed(), TEST_SEED);
    }

    #[test]
    fn a_step_moves_one_tile_and_takes_three_seconds() {
        let mut world = test_world();
        let start = world.player();

        let events = world.apply(Command::Move(Dir::East));

        let to = start.step(Dir::East);
        assert_eq!(world.player(), to);
        assert_eq!(world.seconds(), STEP_SECONDS);
        assert_eq!(STEP_SECONDS, 3);
        assert_eq!(without_ambient(events), vec![Event::Moved { to }]);
    }

    #[test]
    fn bumping_a_wall_costs_no_time() {
        let mut world = test_world();

        let events = world.apply(Command::Move(Dir::North));

        assert_eq!(world.player(), Pos::new(2, 1, 0));
        assert_eq!(world.seconds(), 0);
        assert_eq!(
            events,
            vec![Event::Bumped {
                at: Pos::new(2, 0, 0),
                tile: Tile::WALL
            }]
        );
    }

    #[test]
    fn diagonal_moves_are_blocked_by_walls_too() {
        let mut world = test_world();
        // South-east of the start is the pillar at (3, 2).
        let events = world.apply(Command::Move(Dir::SouthEast));
        assert!(matches!(
            events[..],
            [Event::Bumped {
                tile: Tile::WALL,
                ..
            }]
        ));
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn walking_off_the_map_edge_is_blocked_and_costs_no_time() {
        // No walls: the start tile is on the map's top edge.
        let mut world = world_from_rows(&[".@.", "..."]);

        let events = world.apply(Command::Move(Dir::North));

        assert_eq!(events, vec![Event::BlockedByEdge]);
        assert_eq!(world.player(), Pos::new(1, 0, 0));
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn waiting_passes_time_without_moving() {
        let mut world = test_world();
        let start = world.player();
        world.apply(Command::Wait);
        world.apply(Command::Wait);
        assert_eq!(world.player(), start);
        assert_eq!(world.seconds(), 2 * WAIT_SECONDS);
    }

    #[test]
    fn a_diagonal_step_takes_longer_than_a_straight_one() {
        // From (2, 1) south-west to (1, 2): open floor.
        let mut world = test_world();
        world.apply(Command::Move(Dir::SouthWest));
        assert_eq!(world.player(), Pos::new(1, 2, 0));
        assert_eq!(world.seconds(), DIAGONAL_STEP_SECONDS);
    }

    #[test]
    fn the_ship_clock_starts_at_0842_and_moves_with_the_seconds() {
        let mut world = test_world();
        assert_eq!(world.now(), SHIP_START);
        world.apply(Command::Wait);
        assert_eq!(world.now().to_string(), "25 Nov 2631 08:42:05");
    }

    #[test]
    fn the_time_costs_are_the_agreed_ones() {
        // Decision D29. Changing one is a design change: update the
        // decision log and the golden determinism test with it.
        assert_eq!(
            [
                STEP_SECONDS,
                DIAGONAL_STEP_SECONDS,
                CLIMB_SECONDS,
                WAIT_SECONDS,
                DROP_SECONDS
            ],
            [3, 4, 20, 5, 1]
        );
    }

    #[test]
    fn same_commands_give_the_same_world() {
        // The first determinism check. It becomes a seeded, hashed test in M0.3.
        let script = [
            Command::Move(Dir::East),
            Command::Move(Dir::SouthEast),
            Command::Wait,
            Command::Move(Dir::North),
            Command::Move(Dir::West),
        ];
        let mut a = test_world();
        let mut b = test_world();
        for c in script {
            a.apply(c);
            b.apply(c);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn failed_actions_make_no_sound_and_change_nothing() {
        let mut world = test_world();
        let before = world.state_hash();
        for _ in 0..200 {
            let events = world.apply(Command::Move(Dir::North)); // into the wall
            assert_eq!(count_ambient(&events), 0);
        }
        assert_eq!(world.state_hash(), before, "bumping must not touch the RNG");
    }

    #[test]
    fn ambient_sounds_happen_about_once_every_25_seconds() {
        let mut world = test_world();
        let heard: usize = (0..1000)
            .map(|_| count_ambient(&world.apply(Command::Wait)))
            .sum();
        // 1000 waits of 5 s = 5000 s, so expect about 200. Wide bounds: the
        // test is about the rate, not luck.
        assert!(
            (150..=250).contains(&heard),
            "heard {heard} sounds in 5000 seconds"
        );
    }

    #[test]
    fn every_sound_can_happen() {
        let mut world = test_world();
        let heard: Vec<Sound> = (0..2000)
            .flat_map(|_| world.apply(Command::Wait))
            .filter_map(|e| match e {
                Event::Ambient(sound) => Some(sound),
                _ => None,
            })
            .collect();
        for sound in Sound::ALL {
            assert!(heard.contains(&sound), "{sound:?} never happened");
        }
    }

    #[test]
    fn same_seed_gives_the_same_sounds_and_different_seeds_differ() {
        let script = |seed| {
            let mut world = test_world();
            world = World {
                ambient_rng: GameRng::new(seed, Stream::Ambient),
                seed,
                ..world
            };
            (0..100)
                .flat_map(|_| world.apply(Command::Wait))
                .collect::<Vec<_>>()
        };
        assert_eq!(script(5), script(5));
        assert_ne!(script(5), script(6));
    }

    #[test]
    fn state_hash_is_equal_for_equal_worlds() {
        assert_eq!(test_world().state_hash(), test_world().state_hash());
    }

    #[test]
    fn state_hash_changes_when_anything_changes() {
        let start = test_world().state_hash();

        let mut moved = test_world();
        moved.apply(Command::Move(Dir::East));
        assert_ne!(moved.state_hash(), start, "position and time");

        let mut waited = test_world();
        waited.apply(Command::Wait);
        assert_ne!(waited.state_hash(), start, "time and RNG");

        let other_seed = World::new(
            parse_map(
                r#"{ "id": "t", "name": "Test room", "legend": { ".": "floor" },
                     "decks": [ { "rows": ["@"] } ] }"#,
            )
            .unwrap(),
            TEST_SEED,
        );
        assert_ne!(other_seed.state_hash(), start, "map contents");
    }

    #[test]
    fn state_hash_includes_the_seed() {
        let a = test_world();
        let b = World {
            seed: TEST_SEED + 1,
            ..test_world()
        };
        assert_ne!(a.state_hash(), b.state_hash());
    }

    /// Two named decks joined by stairs at (3, 1):
    ///
    /// ```text
    /// deck 0 "Lower"   deck 1 "Upper"
    /// #####            #####
    /// #@.<#            #..>#
    /// #####            #####
    /// ```
    fn two_deck_world() -> World {
        let lower = ["#####", "#@.<#", "#####"];
        let upper = ["#####", "#..>#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "Test", "legend": {{ "#": "wall", ".": "floor",
                     "<": "stairs_up", ">": "stairs_down" }},
                  "decks": [ {{ "name": "Lower", "rows": {lower:?} }},
                             {{ "name": "Upper", "rows": {upper:?} }} ] }}"##
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    /// Walks the two-deck world's player onto the stairs up at (3, 1, 0).
    fn on_stairs_up() -> World {
        let mut world = two_deck_world();
        world.apply(Command::Move(Dir::East));
        world.apply(Command::Move(Dir::East));
        assert_eq!(world.player(), Pos::new(3, 1, 0));
        world
    }

    #[test]
    fn climbing_up_arrives_at_the_same_spot_one_deck_higher() {
        let mut world = on_stairs_up();
        let before = world.seconds();

        let events = world.apply(Command::Climb(Vertical::Up));

        assert_eq!(world.player(), Pos::new(3, 1, 1));
        assert_eq!(world.seconds(), before + CLIMB_SECONDS);
        assert_eq!(
            without_ambient(events),
            vec![Event::Climbed {
                to: Pos::new(3, 1, 1),
                way: Vertical::Up
            }]
        );
    }

    #[test]
    fn climbing_down_brings_you_back() {
        let mut world = on_stairs_up();
        world.apply(Command::Climb(Vertical::Up));
        let events = world.apply(Command::Climb(Vertical::Down));
        assert_eq!(world.player(), Pos::new(3, 1, 0));
        assert!(matches!(
            events[0],
            Event::Climbed {
                way: Vertical::Down,
                ..
            }
        ));
    }

    #[test]
    fn the_area_name_follows_the_deck() {
        let mut world = on_stairs_up();
        assert_eq!(world.area_name(), "Lower");
        world.apply(Command::Climb(Vertical::Up));
        assert_eq!(world.area_name(), "Upper");
        assert_eq!(world.deck_name(0), "Lower");
        assert_eq!(world.deck_name(5), "", "no such deck");
        assert_eq!(world.deck_name(-1), "", "no such deck");
    }

    #[test]
    fn climbing_without_stairs_does_nothing_and_costs_no_time() {
        let mut world = two_deck_world(); // standing on plain floor
        for way in [Vertical::Up, Vertical::Down] {
            assert_eq!(
                world.apply(Command::Climb(way)),
                vec![Event::NoStairs { way }]
            );
        }
        assert_eq!(world.player(), Pos::new(1, 1, 0));
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn stairs_only_go_their_own_way() {
        let mut world = on_stairs_up();
        let before = world.seconds();
        let events = world.apply(Command::Climb(Vertical::Down));
        assert_eq!(
            events,
            vec![Event::NoStairs {
                way: Vertical::Down
            }]
        );
        assert_eq!(world.seconds(), before);

        world.apply(Command::Climb(Vertical::Up)); // now on stairs down
        let events = world.apply(Command::Climb(Vertical::Up));
        assert_eq!(events, vec![Event::NoStairs { way: Vertical::Up }]);
    }

    #[test]
    fn blocked_stairs_are_refused_safely() {
        // Valid maps can't do this, but a world built another way might.
        let mut world = on_stairs_up();
        world.map.set(Pos::new(3, 1, 1), Tile::WALL);
        let events = world.apply(Command::Climb(Vertical::Up));
        assert_eq!(events, vec![Event::NoStairs { way: Vertical::Up }]);
        assert_eq!(world.player(), Pos::new(3, 1, 0));
    }

    #[test]
    fn deck_names_are_part_of_the_state_hash() {
        let a = two_deck_world();
        let mut b = two_deck_world();
        b.deck_names[1] = "Renamed".into();
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn a_world_with_the_wrong_number_of_deck_names_is_inconsistent() {
        let mut world = two_deck_world();
        assert_eq!(world.check_consistent(), Ok(()));
        world.deck_names.pop();
        assert_eq!(
            world.check_consistent(),
            Err("1 deck names for 2 decks".to_string())
        );
    }

    /// A corridor with a closed door: `#@+.#`, door at (2, 1), and a lamp
    /// in the wall above the door, lighting both sides.
    fn door_world() -> World {
        let rows = ["##*##", "#@+.#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", ".": "floor",
                     "+": "door", "*": "lamp" }},
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    const DOOR: Pos = Pos::new(2, 1, 0);
    /// The test structures' numbers for a shut and an open door.
    const SHUT: u16 = 4;
    const OPENED: u16 = 8;

    #[test]
    fn walking_into_a_closed_door_opens_it_and_takes_two_seconds() {
        let mut world = door_world();

        let events = world.apply(Command::Move(Dir::East));

        assert_eq!(
            without_ambient(events),
            vec![Event::Transformed {
                at: DOOR,
                action: OPEN,
                was: SHUT
            }]
        );
        assert_eq!(world.map().get(DOOR), Some(Tile::FLOOR));
        assert_eq!(world.structure_at(DOOR).unwrap().id, "open_door");
        assert_eq!(
            world.player(),
            Pos::new(1, 1, 0),
            "opening doesn't move you"
        );
        assert_eq!(world.seconds(), DOOR_SECONDS);
    }

    #[test]
    fn an_open_door_can_be_walked_through() {
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open
        world.apply(Command::Move(Dir::East)); // into the doorway
        world.apply(Command::Move(Dir::East)); // out the other side
        assert_eq!(world.player(), Pos::new(3, 1, 0));
    }

    #[test]
    fn closing_an_open_door_beside_you_takes_two_seconds() {
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open
        let before = world.seconds();

        let events = world.apply(close_toward(&world, Dir::East));

        assert_eq!(
            without_ambient(events),
            vec![Event::Transformed {
                at: DOOR,
                action: CLOSE,
                was: OPENED
            }]
        );
        assert_eq!(world.map().get(DOOR), Some(Tile::WALL));
        assert_eq!(world.structure_at(DOOR).unwrap().id, "door");
        assert_eq!(world.seconds(), before + DOOR_SECONDS);
    }

    #[test]
    fn closing_where_there_is_no_open_door_does_nothing_and_costs_no_time() {
        let mut world = door_world();
        let cases = [Dir::East, Dir::North, Dir::West]; // closed door, wall, wall
        for dir in cases {
            assert_eq!(
                world.apply(close_toward(&world, dir)),
                vec![Event::Unable {
                    action: CLOSE,
                    blocked: Blocked::NotPossible
                }]
            );
        }
        assert_eq!(world.seconds(), 0);
        assert_eq!(world.map().get(DOOR), Some(Tile::WALL));
    }

    #[test]
    fn you_cannot_close_a_door_you_are_standing_in() {
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open
        world.apply(Command::Move(Dir::East)); // stand in the doorway
        for dir in Dir::ALL {
            let events = world.apply(close_toward(&world, dir));
            assert!(matches!(events[..], [Event::Unable { .. }]), "{dir:?}");
        }
        assert_eq!(world.map().get(DOOR), Some(Tile::FLOOR));
        let own = Command::Interact {
            at: world.player(),
            action: CLOSE,
        };
        assert_eq!(
            world.apply(own),
            vec![Event::Unable {
                action: CLOSE,
                blocked: Blocked::StandingInIt
            }]
        );
    }

    /// Closing the tile next to the player in direction `dir`, as the `c`
    /// shortcut does.
    fn close_toward(world: &World, dir: Dir) -> Command {
        Command::Interact {
            at: world.player().step(dir),
            action: CLOSE,
        }
    }

    #[test]
    fn door_state_is_part_of_the_state_hash() {
        let closed = door_world();
        let mut opened = door_world();
        opened.apply(Command::Move(Dir::East));
        opened.seconds = closed.seconds; // so only the door differs
        opened.ambient_rng = closed.ambient_rng.clone();
        assert_ne!(closed.state_hash(), opened.state_hash());
    }

    #[test]
    fn air_moves_only_when_time_passes() {
        // Vacuum beyond a closed door: nothing moves while it's shut or
        // while no time passes. Opening takes 2 seconds and the door is
        // open only at the end of them, like every recipe (D46); the next
        // time time passes, the two sides share one even pressure.
        let mut world = door_world();
        let beyond = Pos::new(3, 1, 0);
        world.map.set_air(beyond, crate::air::Mix::VACUUM);
        world.apply(Command::Move(Dir::North)); // bump a wall: no time
        assert_eq!(world.map.air(beyond), Some(crate::air::Mix::VACUUM));

        world.apply(Command::Move(Dir::East)); // open the door: 2 seconds
        assert_eq!(
            world.map.air(beyond),
            Some(crate::air::Mix::VACUUM),
            "still shut while it was opening"
        );
        world.apply(Command::Wait);
        let here = world.map.air(world.player()).unwrap().total();
        let there = world.map.air(beyond).unwrap().total();
        assert!(there > 0, "air rushed through the open door");
        assert!(
            here.abs_diff(there) <= 3,
            "one even space: {here} vs {there}"
        );
    }

    // --- Breath --------------------------------------------------------------

    /// The test room, with its air replaced by `mix` everywhere.
    fn world_in(mix: crate::air::Mix) -> World {
        let mut world = test_world();
        let all: Vec<Pos> = world.map.positions().collect();
        for p in all {
            if world.map.get(p).is_some_and(Tile::holds_air) {
                world.map.set_air(p, mix);
            }
        }
        world
    }

    fn breath_events_of(events: &[Event]) -> Vec<Event> {
        events
            .iter()
            .copied()
            .filter(|e| matches!(e, Event::Breath(_) | Event::BlackedOut(_)))
            .collect()
    }

    #[test]
    fn breath_starts_full_and_stays_full_in_normal_air() {
        let mut world = test_world();
        assert_eq!(world.breath(), BREATH_FULL);
        for _ in 0..20 {
            world.apply(Command::Wait);
        }
        assert_eq!(world.breath(), BREATH_FULL);
        assert!(world.is_conscious());
    }

    #[test]
    fn in_vacuum_you_black_out_in_about_36_seconds_with_warnings_on_the_way() {
        let mut world = world_in(crate::air::Mix::VACUUM);
        let mut events = Vec::new();
        while world.is_conscious() {
            events.extend(world.apply(Command::Wait));
        }
        assert!(
            (35..=40).contains(&world.seconds()),
            "blacked out after {} s",
            world.seconds()
        );
        assert_eq!(
            breath_events_of(&events),
            vec![
                Event::Breath(BreathLevel::Short),
                Event::Breath(BreathLevel::Struggling),
                Event::Breath(BreathLevel::Fading),
                Event::BlackedOut(Cause::Air),
            ]
        );
    }

    #[test]
    fn blacking_out_stops_time_and_ignores_commands() {
        let mut world = world_in(crate::air::Mix::VACUUM);
        while world.is_conscious() {
            world.apply(Command::Wait);
        }
        let frozen = world.clone();
        assert_eq!(world.apply(Command::Wait), vec![]);
        assert_eq!(world.apply(Command::Move(Dir::East)), vec![]);
        assert_eq!(world, frozen);
        // Time stopped at the second breath ran out, not at the end of the
        // last action: a 5-second wait can end early.
        assert_eq!(world.breath(), 0);
    }

    #[test]
    fn thin_air_drains_breath_more_slowly_than_vacuum() {
        let thin = crate::air::Mix::STANDARD.scaled(50); // about 10.6 kPa of O2
        let mut thin_world = world_in(thin);
        let mut vacuum_world = world_in(crate::air::Mix::VACUUM);
        for _ in 0..2 {
            thin_world.apply(Command::Wait);
            vacuum_world.apply(Command::Wait);
        }
        assert!(thin_world.breath() > vacuum_world.breath());
        assert!(thin_world.breath() < BREATH_FULL);
    }

    #[test]
    fn breath_comes_back_in_good_air() {
        let mut world = world_in(crate::air::Mix::VACUUM);
        world.apply(Command::Wait);
        world.apply(Command::Wait); // 10 s of vacuum
        let low = world.breath();
        assert!(low < 750);
        let all: Vec<Pos> = world.map.positions().collect();
        for p in all {
            if world.map.get(p).is_some_and(Tile::holds_air) {
                world.map.set_air(p, crate::air::Mix::STANDARD);
            }
        }
        world.apply(Command::Wait);
        assert_eq!(world.breath(), low + 5 * body::BREATH_RECOVERY);
        world.apply(Command::Wait);
        world.apply(Command::Wait);
        assert_eq!(world.breath(), BREATH_FULL);
    }

    #[test]
    fn standing_in_a_venting_space_says_air_is_escaping_once_per_action() {
        // The test room with a breach in its north wall above the player.
        let mut world = test_world();
        world.map.set(Pos::new(2, 0, 0), Tile::BREACH);
        let events = world.apply(Command::Wait);
        let escaping = events.iter().filter(|e| **e == Event::AirEscaping).count();
        assert_eq!(escaping, 1, "{events:?}");
        assert_eq!(
            test_world()
                .apply(Command::Wait)
                .iter()
                .filter(|e| **e == Event::AirEscaping)
                .count(),
            0
        );
    }

    #[test]
    fn a_world_with_too_much_breath_is_inconsistent() {
        let mut world = test_world();
        world.breath = BREATH_FULL + 1;
        assert!(
            world
                .check_consistent()
                .unwrap_err()
                .contains("over the full")
        );
    }

    #[test]
    fn breath_is_part_of_the_state_hash() {
        let a = test_world();
        let b = World {
            breath: 999,
            ..test_world()
        };
        assert_ne!(a.state_hash(), b.state_hash());
    }

    // --- Warmth --------------------------------------------------------------

    /// The test room with all its air at `celsius`.
    fn world_at(celsius: i32) -> World {
        let mut world = test_world();
        let all: Vec<Pos> = world.map.positions().collect();
        for p in all {
            world
                .map
                .set_temperature(p, crate::heat::celsius_to_mk(celsius));
        }
        world
    }

    #[test]
    fn warmth_starts_full_and_stays_full_at_room_temperature() {
        let mut world = test_world();
        assert_eq!(world.warmth(), WARMTH_FULL);
        for _ in 0..20 {
            world.apply(Command::Wait);
        }
        assert_eq!(world.warmth(), WARMTH_FULL);
    }

    #[test]
    fn bitter_cold_chills_you_then_knocks_you_out() {
        let mut world = world_at(-80);
        let mut events = Vec::new();
        while world.is_conscious() {
            events.extend(world.apply(Command::Wait));
        }
        let warmth_events: Vec<Event> = events
            .into_iter()
            .filter(|e| matches!(e, Event::Warmth(_) | Event::BlackedOut(_)))
            .collect();
        assert_eq!(
            warmth_events,
            vec![
                Event::Warmth(WarmthLevel::Chilled),
                Event::Warmth(WarmthLevel::Numb),
                Event::Warmth(WarmthLevel::Freezing),
                Event::BlackedOut(Cause::Cold),
            ]
        );
        assert_eq!(world.blackout_cause(), Some(Cause::Cold));
        assert_eq!(world.breath(), BREATH_FULL, "the air was fine");
        // About 20 minutes at -80 °C (the room's hull makes it colder still).
        assert!(
            (900..=1300).contains(&world.seconds()),
            "{} s",
            world.seconds()
        );
        assert_eq!(world.apply(Command::Wait), vec![], "out cold");
    }

    #[test]
    fn running_out_of_air_and_warmth_at_once_blames_the_air() {
        let mut world = world_in(crate::air::Mix::VACUUM);
        world.breath = 1;
        world.warmth = 1;
        let events = world.apply(Command::Wait);
        assert_eq!(world.blackout_cause(), Some(Cause::Air));
        assert_eq!(world.warmth(), 1, "warmth stops once you're out");
        let blackouts = events
            .iter()
            .filter(|e| matches!(e, Event::BlackedOut(_)))
            .count();
        assert_eq!(blackouts, 1);
    }

    #[test]
    fn an_awake_player_has_no_blackout_cause() {
        assert_eq!(test_world().blackout_cause(), None);
        assert!(test_world().is_conscious());
    }

    #[test]
    fn heat_moves_only_when_time_passes() {
        let mut world = test_world();
        let cold = Pos::new(4, 2, 0);
        world
            .map
            .set_temperature(cold, crate::heat::celsius_to_mk(-50));
        world.apply(Command::Move(Dir::North)); // bump: no time
        assert_eq!(
            world.map.temperature(cold),
            Some(crate::heat::celsius_to_mk(-50))
        );
        world.apply(Command::Wait);
        let warmer = world.map.temperature(cold).unwrap();
        assert!(warmer > crate::heat::celsius_to_mk(-50), "{warmer}");
    }

    #[test]
    fn a_world_with_too_much_warmth_is_inconsistent() {
        let mut world = test_world();
        world.warmth = WARMTH_FULL + 1;
        assert!(
            world
                .check_consistent()
                .unwrap_err()
                .contains("warmth 1000001 is over")
        );
    }

    #[test]
    fn warmth_is_part_of_the_state_hash() {
        let b = World {
            warmth: 7,
            ..test_world()
        };
        assert_ne!(test_world().state_hash(), b.state_hash());
    }

    // --- Materials -----------------------------------------------------------

    #[test]
    fn every_tile_is_made_of_a_material_from_the_list() {
        let world = test_world();
        let wall = world.material_at(Pos::new(0, 0, 0)).unwrap();
        assert_eq!(wall.id, "steel");
        assert_eq!(world.materials().len(), 2);
        assert_eq!(world.material_at(Pos::new(-1, 0, 0)), None);
    }

    #[test]
    fn a_tile_made_of_a_missing_material_is_inconsistent() {
        let mut world = test_world();
        world.map.set_material(Pos::new(0, 0, 0), 2);
        assert_eq!(
            world.check_consistent(),
            Err("a tile is made of material 2, but there are only 2".to_string())
        );
    }

    #[test]
    fn the_world_keeps_all_its_content_in_one_bundle() {
        let world = test_world();
        let content = world.content();
        assert_eq!(content.materials(), world.materials());
        assert_eq!(content.structures(), world.structures());
        assert_eq!(content.item_types(), world.item_types());
        assert_eq!(
            content,
            &Content::for_tests(),
            "a copy of what it was made from"
        );
    }

    #[test]
    fn the_material_list_is_part_of_the_state_hash() {
        let mut changed = test_world();
        changed.content.materials.list_mut()[1].hardness += 1;
        assert_ne!(test_world().state_hash(), changed.state_hash());
    }

    // --- Interacting ----------------------------------------------------------

    #[test]
    fn interacting_opens_and_closes_a_door_beside_you() {
        let mut world = door_world();
        let door = Pos::new(2, 1, 0);
        let events = world.apply(Command::Interact {
            at: door,
            action: OPEN,
        });
        let opened = Event::Transformed {
            at: door,
            action: OPEN,
            was: SHUT,
        };
        assert_eq!(events.last(), Some(&opened));
        assert_eq!(world.map.get(door), Some(Tile::FLOOR));
        assert_eq!(world.seconds(), DOOR_SECONDS);
        let events = world.apply(Command::Interact {
            at: door,
            action: CLOSE,
        });
        let closed = Event::Transformed {
            at: door,
            action: CLOSE,
            was: OPENED,
        };
        assert_eq!(events.last(), Some(&closed));
        assert_eq!(world.map.get(door), Some(Tile::WALL));
    }

    #[test]
    fn a_blocked_interaction_says_why_and_costs_no_time() {
        let mut world = door_world();
        let unable = |world: &mut World, at| world.apply(Command::Interact { at, action: OPEN });
        let wall = unable(&mut world, Pos::new(1, 0, 0));
        assert_eq!(
            wall,
            vec![Event::Unable {
                action: OPEN,
                blocked: Blocked::NotPossible
            }]
        );
        let behind_the_door = unable(&mut world, Pos::new(3, 1, 0));
        assert_eq!(
            behind_the_door,
            vec![Event::Unable {
                action: OPEN,
                blocked: Blocked::CantSee
            }]
        );
        assert_eq!(world.seconds(), 0);
    }

    // --- Furniture -------------------------------------------------------------

    /// A lit room with a bench at (2, 1)–(3, 1), a cabinet (which hides
    /// what's behind it) at (5, 1) and a stool at (1, 2):
    ///
    /// ```text
    /// ###*####
    /// #@bb.c.#
    /// #s.....#
    /// ########
    /// ```
    fn furnished_world() -> World {
        let rows = ["###*####", "#@bb.c.#", "#s.....#", "########"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", ".": "floor", "*": "lamp",
                     "b": {{ "structure": "floor", "furniture": "bench" }},
                     "c": {{ "structure": "floor", "furniture": "cabinet" }},
                     "s": {{ "structure": "floor", "furniture": "stool" }} }},
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    #[test]
    fn a_piece_of_furniture_is_one_thing_on_every_tile_it_covers() {
        let world = furnished_world();
        assert_eq!(world.furniture().len(), 3);
        let left = world.furniture_at(Pos::new(2, 1, 0)).unwrap();
        assert_eq!(world.furniture_at(Pos::new(3, 1, 0)), Some(left));
        assert_eq!(
            world.furniture_kind_at(Pos::new(3, 1, 0)).unwrap().name,
            "bench"
        );
        assert_eq!(world.furniture_at(Pos::new(4, 1, 0)), None);
        assert_eq!(world.furniture_kind_at(Pos::new(-1, 0, 0)), None);
    }

    #[test]
    fn walking_into_furniture_bumps_it_and_costs_no_time() {
        let mut world = furnished_world();
        let events = world.apply(Command::Move(Dir::East));
        assert_eq!(
            events,
            vec![Event::Bumped {
                at: Pos::new(2, 1, 0),
                tile: Tile::FLOOR
            }]
        );
        assert_eq!((world.player(), world.seconds()), (Pos::new(1, 1, 0), 0));
    }

    #[test]
    fn you_can_walk_through_a_stool() {
        let mut world = furnished_world();
        world.apply(Command::Move(Dir::South));
        assert_eq!(world.player(), Pos::new(1, 2, 0));
        assert_eq!(world.seconds(), STEP_SECONDS);
    }

    #[test]
    fn tall_furniture_hides_what_is_behind_it_and_furniture_is_remembered() {
        let world = furnished_world();
        assert!(world.is_visible(Pos::new(5, 1, 0)), "the cabinet itself");
        assert!(!world.is_visible(Pos::new(6, 1, 0)), "behind it");
        assert!(world.is_visible(Pos::new(4, 1, 0)), "past the low bench");
        let bench = world.content().furniture_index("bench");
        assert_eq!(
            world.remembered(Pos::new(3, 1, 0)).unwrap().furniture,
            bench
        );
    }

    #[test]
    fn furniture_counts_as_clutter_for_work_that_needs_the_tile_clear() {
        // A crate stands on a doorframe beside you: the frame can't be
        // repaired into a door with it there.
        let rows = ["#*##", "#@f#", "####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", "*": "lamp",
                     "f": {{ "structure": "frame", "furniture": "crate" }} }},
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let world = World::new(parse_map(&json).unwrap(), TEST_SEED);
        assert_eq!(
            interact::blocked(&world, Pos::new(2, 1, 0), REPAIR),
            Some(Blocked::Cluttered)
        );
    }

    #[test]
    fn furniture_survives_saving_and_is_part_of_the_state_hash() {
        let world = furnished_world();
        let loaded = crate::save::load_from_str(&crate::save::save_to_string(&world)).unwrap();
        assert_eq!(loaded, world);
        assert!(
            loaded.furniture_at(Pos::new(3, 1, 0)).is_some(),
            "found again"
        );
        let mut bare = world.clone();
        bare.furniture = Furnishings::default();
        assert_ne!(bare.state_hash(), world.state_hash());
    }

    #[test]
    fn a_loaded_world_must_not_put_the_player_or_furniture_anywhere_odd() {
        let mut inside = furnished_world();
        inside.player = Pos::new(2, 1, 0);
        assert_eq!(
            inside.check_consistent(),
            Err("player at Pos { x: 2, y: 1, z: 0 } is inside a bench".into())
        );
        let mut on_the_stool = furnished_world();
        on_the_stool.player = Pos::new(1, 2, 0);
        assert_eq!(on_the_stool.check_consistent(), Ok(()));
        let mut in_the_wall = furnished_world();
        let mut pieces = in_the_wall.furniture().to_vec();
        pieces[0].at = Pos::new(0, 0, 0);
        let kinds = in_the_wall.content.furniture().to_vec();
        in_the_wall.furniture = Furnishings::new(pieces, &in_the_wall.map, &kinds);
        assert!(
            in_the_wall
                .check_consistent()
                .unwrap_err()
                .contains("isn't on the floor")
        );
    }

    // --- Sight -----------------------------------------------------------------

    #[test]
    fn you_see_your_surroundings_from_the_start_and_remember_them() {
        let world = door_world(); // #@+.#, door at (2, 1)
        assert!(world.is_visible(world.player()));
        assert!(world.is_visible(DOOR), "the shut door");
        assert!(!world.is_visible(Pos::new(3, 1, 0)), "behind it");
        assert_eq!(world.remembered(DOOR).unwrap().structure, SHUT);
        assert_eq!(world.remembered(Pos::new(3, 1, 0)), None);
    }

    #[test]
    fn opening_a_door_shows_what_is_beyond_and_memory_keeps_it_after() {
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open
        let beyond = Pos::new(3, 1, 0);
        assert!(world.is_visible(beyond));
        world.apply(close_toward(&world, Dir::East));
        assert!(!world.is_visible(beyond), "shut out of sight again");
        let floor = world.content().structure_index("floor").unwrap();
        assert_eq!(world.remembered(beyond).unwrap().structure, floor);
        assert_eq!(world.remembered(DOOR).unwrap().structure, SHUT);
    }

    #[test]
    fn memory_keeps_the_door_as_it_was_last_seen() {
        // Open the door, walk away round a corner so it's out of sight, and
        // it's remembered open; it is open, as it happens, but memory never
        // looks: change it behind the player's back and memory stays open.
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open
        assert_eq!(world.remembered(DOOR).unwrap().structure, OPENED);
        world.map.place(DOOR, SHUT, Tile::WALL); // shut, unseen...
        world.view = {
            // ...as if the player had looked away: keep memory, see nothing
            let mut blind = world.view.clone();
            let scene = Scene {
                map: &world.map,
                blocks: &|_| true,
                lights: &[],
                furniture: &|_| None,
                top_item: &|_| None,
            };
            blind.refresh(&scene, Pos::new(99, 99, 0));
            blind
        };
        assert_eq!(world.remembered(DOOR).unwrap().structure, OPENED);
    }

    /// A dark corridor, with no light but a lamp (light 2) lying at the far
    /// end, at (7, 1). The player wears the test suit, to carry it in:
    ///
    /// ```text
    /// #########
    /// #@.....!#
    /// #########
    /// ```
    fn dark_corridor() -> World {
        let rows = ["#########", "#@.....!#", "#########"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "player_wears": [ {{ "item": "suit" }} ],
                  "legend": {{ "#": "wall", ".": "floor",
                     "!": {{ "structure": "floor", "items": [ {{ "item": "lamp" }} ] }} }},
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    #[test]
    fn in_the_dark_you_see_only_beside_you_and_what_a_lamp_lights() {
        let world = dark_corridor();
        let at = |x| Pos::new(x, 1, 0);
        assert!(world.is_visible(at(2)), "beside you");
        assert!(!world.is_visible(at(3)) && !world.is_visible(at(4)), "dark");
        assert!(!world.is_lit(at(4)));
        // The lamp lights two tiles round it, and you see that from afar.
        for x in 5..=8 {
            assert!(world.is_lit(at(x)) && world.is_visible(at(x)), "{x}");
        }
        assert_eq!(world.remembered(at(3)), None, "never seen");
    }

    #[test]
    fn a_lamp_you_carry_lights_the_way_around_you() {
        let mut world = dark_corridor();
        let at = |x| Pos::new(x, 1, 0);
        for _ in 0..5 {
            world.apply(Command::Move(Dir::East));
        }
        world.apply(Command::Interact {
            at: at(7),
            action: PICK_UP,
        });
        assert!(
            world.is_lit(at(4)) && world.is_lit(at(8)),
            "around you at 6"
        );
        for _ in 0..5 {
            world.apply(Command::Move(Dir::West));
        }
        assert_eq!(world.player(), at(1));
        assert!(world.is_lit(at(3)) && world.is_visible(at(3)));
        assert!(!world.is_lit(at(8)) && !world.is_visible(at(8)), "dark now");
        assert!(world.remembered(at(8)).is_some(), "but remembered");
    }

    #[test]
    fn only_lights_on_your_own_deck_are_worked_out() {
        let lower = ["#####", "#@.<#", "#####"];
        let upper = ["##*##", "#..>#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "Test", "legend": {{ "#": "wall", ".": "floor",
                     "*": "lamp", "<": "stairs_up", ">": "stairs_down" }},
                  "decks": [ {{ "rows": {lower:?} }}, {{ "rows": {upper:?} }} ] }}"##
        );
        let mut world = World::new(parse_map(&json).unwrap(), TEST_SEED);
        let upstairs = Pos::new(1, 1, 1);
        assert!(!world.is_lit(upstairs), "you're downstairs");
        world.apply(Command::Move(Dir::East));
        world.apply(Command::Move(Dir::East));
        world.apply(Command::Climb(Vertical::Up));
        assert!(world.is_lit(upstairs) && world.is_visible(upstairs));
        assert!(!world.is_lit(Pos::new(1, 1, 0)));
    }

    #[test]
    fn items_seen_lying_about_are_remembered_by_kind() {
        let mut world = world_with_items(); // cutter and scraps at (3, 1)
        let pile = Pos::new(3, 1, 0);
        let first = world.items_at(pile).next().unwrap().kind;
        assert_eq!(world.remembered(pile).unwrap().item, Some(first));
        world.apply(Command::Interact {
            at: pile,
            action: PICK_UP,
        });
        let left = world.items_at(pile).next().map(|i| i.kind);
        assert_eq!(world.remembered(pile).unwrap().item, left, "seen again");
    }

    #[test]
    fn what_you_remember_is_part_of_the_state_hash_and_survives_loading() {
        let mut world = door_world();
        world.apply(Command::Move(Dir::East)); // open: see beyond...
        world.apply(close_toward(&world, Dir::East)); // ...and shut it
        let mut forgetful = world.clone();
        forgetful.view = View::new(world.map.tile_count());
        forgetful.refresh_view();
        assert_ne!(world.state_hash(), forgetful.state_hash());
        let loaded = crate::save::load_from_str(&crate::save::save_to_string(&world)).unwrap();
        assert_eq!(loaded, world);
        assert!(loaded.is_visible(DOOR), "worked out again on load");
        assert!(loaded.remembered(Pos::new(3, 1, 0)).is_some());
    }

    // --- Structures -----------------------------------------------------------

    #[test]
    fn every_tile_has_a_structure_from_the_list() {
        let world = test_world();
        assert_eq!(world.structure_at(Pos::new(0, 0, 0)).unwrap().id, "wall");
        assert_eq!(world.structure_at(Pos::new(1, 1, 0)).unwrap().id, "floor");
        assert_eq!(world.structure_at(Pos::new(-1, 0, 0)), None);
        assert_eq!(world.structures().len(), 10);
    }

    #[test]
    fn loading_rebuilds_each_tile_from_its_structure() {
        // A tile that somehow doesn't match its structure is put right:
        // the structure is what's saved.
        let mut world = test_world();
        world.map.set(Pos::new(0, 0, 0), Tile::FLOOR); // still a "wall"
        world.finish_loading().unwrap();
        assert_eq!(world.map.get(Pos::new(0, 0, 0)), Some(Tile::WALL));
        assert_eq!(world, test_world());
    }

    #[test]
    fn a_tile_with_a_missing_structure_is_inconsistent() {
        let mut world = test_world();
        world.map.place(Pos::new(0, 0, 0), 99, Tile::WALL);
        assert_eq!(
            world.finish_loading(),
            Err("the tile at Pos { x: 0, y: 0, z: 0 } has no structure".to_string())
        );
    }

    #[test]
    fn the_structure_list_is_part_of_the_state_hash() {
        let mut changed = test_world();
        changed.content.structures.list_mut()[0].name.push('!');
        assert_ne!(test_world().state_hash(), changed.state_hash());
    }

    // --- Items ----------------------------------------------------------------

    /// The test room wearing the test suit (two 1 L pockets), with a cutter
    /// and three scraps on the floor beside the player at (3, 1).
    fn world_with_items() -> World {
        let rows = ["######", "#.@s.#", "#..#.#", "######"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "wall", ".": "floor",
                    "s": {{ "structure": "floor",
                           "items": [ {{ "item": "cutter" }}, {{ "item": "scrap", "count": 3 }} ] }} }},
                 "player_wears": [ {{ "item": "suit" }} ],
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).unwrap(), TEST_SEED)
    }

    const PILE: Pos = Pos::new(3, 1, 0);

    fn pick_up_pile(world: &mut World) -> Vec<Event> {
        world.apply(Command::Interact {
            at: PILE,
            action: PICK_UP,
        })
    }

    fn carried(world: &World) -> usize {
        world.worn()[0].pockets.iter().map(Vec::len).sum()
    }

    #[test]
    fn items_lie_where_the_map_put_them() {
        let world = world_with_items();
        assert_eq!(world.items_at(PILE).count(), 4);
        assert_eq!(world.items_at(world.player()).count(), 0);
        assert_eq!(world.floor_items().len(), 4);
        assert_eq!(world.item_types().len(), 4);
        assert_eq!(world.worn().len(), 1);
    }

    #[test]
    fn picking_up_puts_everything_that_fits_in_pockets_two_seconds_each() {
        let mut world = world_with_items();
        let events = without_ambient(pick_up_pile(&mut world));
        let picked = events
            .iter()
            .filter(|e| matches!(e, Event::PickedUp(_)))
            .count();
        assert_eq!(picked, 4);
        assert_eq!(world.items_at(PILE).count(), 0);
        assert_eq!(carried(&world), 4);
        assert_eq!(world.seconds(), 4 * PICK_UP_SECONDS);
    }

    #[test]
    fn what_does_not_fit_stays_on_the_floor() {
        let mut world = world_with_items();
        // Fill both pockets with cutters (800 ml each), leaving 200 ml free.
        for p in 0..2 {
            let cutter = Item::new(1, 0, world.item_types());
            world.worn[0].pockets[p].push(cutter);
        }
        let events = without_ambient(pick_up_pile(&mut world));
        // Each pocket has 200 ml left: room for two scraps each, so all
        // three scraps fit, and only the cutter stays on the floor.
        let no_room: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::NoRoom(_)))
            .collect();
        assert_eq!(
            no_room,
            vec![&Event::NoRoom(ItemRef {
                kind: 1,
                material: 0
            })]
        );
        assert_eq!(world.items_at(PILE).count(), 1);
        assert_eq!(world.seconds(), 3 * PICK_UP_SECONDS);
    }

    #[test]
    fn picking_up_nothing_that_fits_costs_no_time() {
        let mut world = world_with_items();
        world.worn.clear();
        let events = pick_up_pile(&mut world);
        assert!(events.iter().all(|e| matches!(e, Event::NoRoom(_))));
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn dropping_puts_an_item_where_you_stand() {
        let mut world = world_with_items();
        pick_up_pile(&mut world);
        let before = world.seconds();
        let path = ItemPath {
            worn: 0,
            pocket: 0,
            index: 0,
        };
        let events = world.apply(Command::Drop(path));
        assert!(matches!(events[0], Event::Dropped(ItemRef { kind: 1, .. })));
        assert_eq!(world.items_at(world.player()).count(), 1);
        assert_eq!(carried(&world), 3);
        assert_eq!(world.seconds(), before + DROP_SECONDS);
    }

    #[test]
    fn dropping_from_an_empty_pocket_does_nothing() {
        let mut world = world_with_items();
        let path = ItemPath {
            worn: 0,
            pocket: 1,
            index: 0,
        };
        assert_eq!(world.apply(Command::Drop(path)), vec![Event::NothingToDrop]);
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn carried_lists_every_pocketed_item_with_where_it_is() {
        let mut world = world_with_items();
        assert!(world.carried().is_empty());
        pick_up_pile(&mut world);
        let paths: Vec<(usize, usize)> = world
            .carried()
            .iter()
            .map(|(path, _)| (path.pocket, path.index))
            .collect();
        // Cutter and two scraps fill the first pocket; the last scrap goes
        // in the second.
        assert_eq!(paths, vec![(0, 0), (0, 1), (0, 2), (1, 0)]);
        assert_eq!(world.carried()[0].1.kind, 1);
    }

    #[test]
    fn the_item_context_weighs_what_you_carry() {
        let mut world = world_with_items();
        pick_up_pile(&mut world);
        // Suit 600 g + cutter 400 g + three scraps 100 g each.
        assert_eq!(world.item_context().mass_g(&world.worn()[0]), 1_300);
    }

    #[test]
    fn broken_items_in_a_save_are_inconsistent() {
        let mut world = world_with_items();
        world.floor_items[0].item.kind = 99;
        assert!(world.check_consistent().unwrap_err().contains("type 99"));

        let mut world = world_with_items();
        world.floor_items[0].at = Pos::new(0, 0, 0);
        assert!(
            world
                .check_consistent()
                .unwrap_err()
                .contains("lies inside")
        );

        let mut world = world_with_items();
        world.worn[0].pockets.clear();
        assert!(world.check_consistent().unwrap_err().contains("pockets"));

        let mut world = world_with_items();
        world.worn = vec![Item::new(2, 0, world.item_types())];
        assert_eq!(
            world.check_consistent(),
            Err("the player is wearing a scrap, which can't be worn".to_string())
        );
    }

    #[test]
    fn items_are_part_of_the_state_hash() {
        let base = world_with_items().state_hash();
        let mut moved = world_with_items();
        moved.floor_items[0].at = Pos::new(4, 1, 0);
        let mut carrying = world_with_items();
        let scrap = Item::new(2, 0, carrying.item_types());
        carrying.worn[0].pockets[0].push(scrap);
        let mut retyped = world_with_items();
        retyped.content.item_types.list_mut()[2].glyph = '%';
        for other in [moved, carrying, retyped] {
            assert_ne!(base, other.state_hash());
        }
    }

    // --- Cutting --------------------------------------------------------------

    /// The door room (`#@+.#`), wearing the test suit with the test cutter
    /// in a pocket, its LASER level set to `level`. The test door is steel,
    /// which needs LASER 2.
    fn cutting_world(level: u8) -> World {
        let rows = ["#####", "#@+.#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", ".": "floor", "+": "door" }},
                  "player_wears": [ {{ "item": "suit" }} ],
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let mut world = World::new(parse_map(&json).unwrap(), TEST_SEED);
        world.content.item_types.list_mut()[1]
            .qualities
            .insert("LASER".into(), level);
        let cutter = Item::new(1, 0, world.item_types());
        world.worn[0].pockets[0].push(cutter);
        world
    }

    fn cut(world: &mut World, at: Pos) -> Vec<Event> {
        world.apply(Command::Interact { at, action: CUT })
    }

    #[test]
    fn the_best_tool_you_carry_sets_your_level() {
        let mut world = cutting_world(3);
        assert_eq!(world.tool_level("LASER"), 3);
        world.worn[0].pockets[0].clear();
        assert_eq!(world.tool_level("LASER"), 0, "nothing carried");
        world.worn.clear();
        assert_eq!(world.tool_level("LASER"), 0, "nothing at all");
    }

    #[test]
    fn a_good_enough_tool_cuts_the_door_into_what_the_recipe_makes() {
        let mut world = cutting_world(2);
        let door = Pos::new(2, 1, 0);
        let events = without_ambient(cut(&mut world, door));
        assert_eq!(
            events.last(),
            Some(&Event::Transformed {
                at: door,
                action: CUT,
                was: 4
            })
        );
        assert_eq!(world.seconds(), 60, "the recipe's time");
        assert_eq!(world.map().get(door), Some(Tile::FLOOR));
        assert_eq!(world.structure_at(door).unwrap().id, "floor");
        assert_eq!(
            world.material_at(door).unwrap().id,
            "steel",
            "it keeps its material"
        );
    }

    #[test]
    fn a_tool_too_weak_for_the_material_cuts_nothing() {
        let mut world = cutting_world(1);
        let events = cut(&mut world, Pos::new(2, 1, 0));
        assert_eq!(
            events,
            vec![Event::Unable {
                action: CUT,
                blocked: Blocked::Needs {
                    quality: 0, // LASER, the first test quality
                    level: 2
                }
            }]
        );
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn blacking_out_partway_leaves_the_door_uncut() {
        let mut world = cutting_world(2);
        world.map.set_air(world.player(), crate::air::Mix::VACUUM);
        world.breath = 30;
        let door = Pos::new(2, 1, 0);
        let events = cut(&mut world, door);
        assert!(events.contains(&Event::BlackedOut(Cause::Air)));
        assert_eq!(world.map().get(door), Some(Tile::WALL));
    }

    #[test]
    fn a_cut_door_lets_air_through_from_then_on() {
        let mut world = cutting_world(2);
        let beyond = Pos::new(3, 1, 0);
        world.map.set_air(beyond, crate::air::Mix::VACUUM);
        cut(&mut world, Pos::new(2, 1, 0));
        world.apply(Command::Wait);
        assert!(world.map().air(beyond).unwrap().total() > 0);
    }

    #[test]
    fn recipes_are_part_of_the_state_hash() {
        let mut changed = cutting_world(2);
        changed.content.recipes.list_mut()[0].seconds += 1;
        assert_ne!(cutting_world(2).state_hash(), changed.state_hash());
    }

    // --- Repairing --------------------------------------------------------------

    /// A room with cracked hull above the player and a door frame to the
    /// east, wearing the test suit with `scraps` pieces of scrap in a
    /// pocket: the first of hard alloy, the rest steel.
    ///
    /// ```text
    /// #x###
    /// #@/.#
    /// #####
    /// ```
    fn repair_world(scraps: usize) -> World {
        let rows = ["#x###", "#@/.#", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                  "legend": {{ "#": "wall", ".": "floor", "x": "hull_crack", "/": "frame" }},
                  "player_wears": [ {{ "item": "suit" }} ],
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let mut world = World::new(parse_map(&json).unwrap(), TEST_SEED);
        for n in 0..scraps {
            let material = u16::from(n == 0);
            let scrap = Item::new(2, material, world.item_types());
            world.worn[0].pockets[0].push(scrap);
        }
        world
    }

    const CRACK: Pos = Pos::new(1, 0, 0);
    const FRAME: Pos = Pos::new(2, 1, 0);

    fn repair(world: &mut World, at: Pos) -> Vec<Event> {
        world.apply(Command::Interact { at, action: REPAIR })
    }

    #[test]
    fn patching_a_crack_uses_the_scrap_and_is_made_of_it() {
        let mut world = repair_world(2);
        let events = without_ambient(repair(&mut world, CRACK));
        assert_eq!(
            events.last(),
            Some(&Event::Transformed {
                at: CRACK,
                action: REPAIR,
                was: 5
            })
        );
        assert_eq!(world.seconds(), 30, "the recipe's time");
        assert_eq!(world.map().get(CRACK), Some(Tile::WALL));
        assert_eq!(world.structure_at(CRACK).unwrap().id, "wall");
        assert_eq!(
            world.material_at(CRACK).unwrap().id,
            "hard_alloy",
            "the first scrap used"
        );
        assert_eq!(world.carried_count(2), 0, "both scraps used up");
    }

    #[test]
    fn a_patched_crack_stops_the_leak() {
        let mut world = repair_world(2);
        repair(&mut world, CRACK);
        let vented = world.map().vented();
        world.apply(Command::Wait);
        assert_eq!(world.map().vented(), vented);
    }

    #[test]
    fn without_enough_scrap_nothing_happens() {
        let mut world = repair_world(1);
        assert_eq!(
            repair(&mut world, CRACK),
            vec![Event::Unable {
                action: REPAIR,
                blocked: Blocked::Missing {
                    item: 2,
                    need: 2,
                    have: 1
                }
            }]
        );
        assert_eq!(world.seconds(), 0);
        assert_eq!(world.carried_count(2), 1);
    }

    #[test]
    fn a_repaired_frame_keeps_its_own_material_and_needs_you_off_it_and_it_clear() {
        let mut world = repair_world(1); // one hard alloy scrap
        world.apply(Command::Move(Dir::East)); // onto the frame
        assert_eq!(
            interact::blocked(&world, FRAME, REPAIR),
            Some(Blocked::StandingInIt)
        );
        world.apply(Command::Move(Dir::West));
        let lying = Item::new(2, 0, world.item_types());
        world.floor_items.push(FloorItem {
            at: FRAME,
            item: lying,
        });
        assert_eq!(
            interact::blocked(&world, FRAME, REPAIR),
            Some(Blocked::Cluttered)
        );
        world.floor_items.clear();
        repair(&mut world, FRAME);
        assert_eq!(world.map().get(FRAME), Some(Tile::WALL));
        assert_eq!(world.material_at(FRAME).unwrap().id, "steel", "kept");
    }

    #[test]
    fn blacking_out_partway_keeps_the_scrap_and_the_crack() {
        let mut world = repair_world(2);
        for x in 1..4 {
            world
                .map
                .set_air(Pos::new(x, 1, 0), crate::air::Mix::VACUUM);
        }
        world.breath = 10;
        let events = repair(&mut world, CRACK);
        assert!(events.contains(&Event::BlackedOut(Cause::Air)));
        assert_eq!(world.map().get(CRACK), Some(Tile::CRACK));
        assert_eq!(world.carried_count(2), 2);
    }

    #[test]
    fn counting_and_finding_item_types() {
        let world = repair_world(2);
        assert_eq!(world.item_type_index("scrap"), Some(2));
        assert_eq!(world.item_type_index("cheese"), None);
        assert_eq!(world.carried_count(1), 0);
    }
}
