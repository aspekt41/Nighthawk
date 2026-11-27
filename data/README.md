# Game data

Everything in this folder is content: plain JSON the game loads at startup
(decision D17). Changing a file here needs no Rust and no recompiling.

The game looks for this folder in three places, in order:
`$NIGHTHAWK_DATA`, then `./data`, then the repository's `data/` as it was when
the game was built.

## Folders of content

Each kind of content has a folder, and **every `.json` file in it is
loaded**, in name order, so content can be grouped into as many files as
reads best (`metals.json`, `fabrics.json`…). An id must be unique across all
the files in its folder. Mistakes name the folder and the file, for example
`data/structures: invalid structures file doors.json: missing field "name"`.

The folders load in this order, and each may refer to the ones before it:
`qualities`, `actions`, `materials`, `structures`, `items`, `recipes`.

Every tile on a map is a **structure** (what it is: a bulkhead, a door)
made of a **material** (what it's made of: steel, Nanocarbon Alloy).

**Everything about a thing is here, not in Rust** (decision D43): which
structures, materials, items, tool qualities, actions and recipes exist,
what they're called, what keys do what, how long things take and what the
log says. The game's code only knows what each *field* means. See [How to
add things](#how-to-add-things) for worked examples.

### Schemas: your editor can check these files

Each file starts with a line like `"$schema": "../schema/materials.schema.json"`,
pointing at a [JSON Schema](https://json-schema.org/) in `schema/`. Editors
that understand schemas (VS Code, JetBrains IDEs, Zed, Neovim with a JSON
language server) then autocomplete field names, show each field's meaning
on hover, and underline a misspelt or missing field as you type. The schemas
are generated from the Rust types that load the files, so they always match
what the game accepts; a test fails if one is out of date. To regenerate
them after changing a content type:

```sh
NIGHTHAWK_WRITE_SCHEMAS=1 cargo test -p nighthawk-sim --test schemas
```

## Qualities (`qualities/*.json`)

What tools can do (decision D9). A tool has a quality at a level from 1 to
10 (`LASER 1` is a small laser cutter), and actions ask for a quality at a
level, never for a particular tool, so there can be many ways to get one.
Materials and items may only name qualities listed here. There may be none
at all.

```json
{
  "qualities": [
    { "id": "LASER", "name": "laser cutting",
      "description": "Cutting with a beam of light: the tool melts a slot through it." }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Unique. How materials and items name it, and how the game shows it next to a level: "LASER 2". Capitals by convention. |
| `name` | What it is in words. |
| `description` | A sentence or two about it. |

## Actions (`actions/*.json`)

What the player can do to a tile, in the interact menu (`e`). Each action
is a verb with its rules; what it *does* is one of a few built-in effects,
usually following a recipe (below). The menu lists actions in file order.

```json
{
  "actions": [
    { "id": "cut", "name": "Cut", "key": "x", "reach": 2, "clear_line": true,
      "tool_from_material": true, "done": "You cut through the {was}." },
    { "id": "pick_up", "name": "Pick up", "key": "g", "reach": 1, "on_own_tile": true,
      "effect": "pick_up", "seconds": 2 }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Unique. How recipes and materials name it. |
| `name` | How the menu shows it ("Cut"). Lower-cased, it's also the verb in messages: "There's nothing to cut there." |
| `key` | The key that does it in the menu. Unique, and not a movement key (`hjkl yubn`, arrows, numpad, `.`), nor `V`, `e` or `q`, which put the cursor away. |
| `reach` | How many steps away it works, diagonals counting as one; at least 1. |
| `clear_line` | Optional. Something solid on the straight line to the tile blocks it (a laser). |
| `on_own_tile` | Optional. It can be done to the tile you stand on (picking up). Otherwise you're told to step out first. |
| `clear_of_items` | Optional. Items lying on the tile block it (they'd end up inside a repair). |
| `tool_from_material` | Optional. It needs a tool at the level the tile's **material** lists for this action in `worked_by` (an aluminium door: `LASER 1` to cut). A material that lists none can't have it done at all. |
| `effect` | Optional. `recipe` (the default): follow this action's recipe for the structure there. Or the built-in `pick_up`. (Opening and closing doors are recipes: `recipes/doors.json`.) |
| `seconds` | Only for built-in effects: how long it takes (for picking up, per item). A recipe action takes its time from the recipe. |
| `done` | Only for recipe actions, and required: the log line when it's done. `{was}` is the structure's name before, `{now}` after: "You cut through the {was}." |
| `on_bump` | Optional. Walking into a solid tile does this action, if it can be done there (walking into a shut door opens it). |
| `shortcut` | Optional. Its key also works while playing, followed by a direction (`c` then `l` closes the door to the east). It may not be a key the game already uses while playing (`a t V e i S q Q`). |

Before anything is done the rules are checked in this order, and the first
that fails is the reason shown: nothing could do it (`tool_from_material`
and no level listed), your own tile, reach, the line, items lying there,
then tools (the material's level, then the recipe's `needs`) and items
(the recipe's `uses`).

## Materials (`materials/*.json`)

Each material has real physical properties, in whole numbers:

```json
{
  "materials": [
    { "id": "steel", "name": "Steel",
      "description": "Structural steel: deck plating, ladders and bulkheads.",
      "colour": "grey", "density": 7850, "melting_point": 1450,
      "specific_heat": 490, "thermal_conductivity": 45, "hardness": 150,
      "worked_by": { "cut": { "LASER": 2 } } }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | How structures and maps refer to it. |
| `name`, `description` | Shown in look mode (`V`). |
| `colour` | Walls made of it are drawn in this colour: `white`, `grey`, `dark_grey`, `red`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `light_red`. |
| `density` | kg/m³ |
| `melting_point` | °C. Optional: leave it out for something that doesn't melt (cotton chars and burns). |
| `specific_heat` | J/(kg·K) |
| `thermal_conductivity` | **mW**/(m·K), thousandths of a W/(m·K), so insulators like cotton (40) aren't rounded to zero. Steel is 45000. |
| `hardness` | Vickers hardness (HV); 0 for soft things like fabric. |
| `worked_by` | Optional. For each action with `tool_from_material`, the tool qualities that can do it to this material and the level each needs (1 to 10); any one is enough. `{ "cut": { "LASER": 2 } }`: cutting it needs `LASER 2`. Leave an action out and nothing can do it to this material. Actions must be from `actions/`, qualities from `qualities/`. |

Mistakes are reported precisely: a duplicate id, or a level out of range.

## Structures (`structures/*.json`)

What each tile is. A shut door and an open one are two structures: a door
opens and shuts by recipe (`recipes/doors.json`: the `open` action turns
`door` into `door_open` in 2 seconds, `close` turns it back), like any
other change.

```json
{
  "structures": [
    { "id": "door", "name": "door",
      "description": "A sliding pressure door, shut. It holds air.",
      "glyph": "+", "material": "aluminium_alloy", "colour": "light_red" },
    { "id": "door_open", "name": "open door",
      "description": "A sliding pressure door, open. Air passes through it.",
      "glyph": "'", "material": "aluminium_alloy", "colour": "light_red",
      "walkable": true }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | How map legends refer to it. |
| `name`, `description` | Shown in look mode and in messages ("You bump into the hull plating."). |
| `glyph` | The one character the map draws it with. |

| `walkable` | Optional. You can walk on it, and air fills it and moves through it (floor, ladders, an open door). Left out, it's solid and airtight (a wall, a shut door). |
| `leak_cm2` | Optional. A hole to space of this many cm² (a hull crack is 4, a breach 400): the space beside it leaks, faster the bigger the hole. Solid structures only. |
| `climb` | Optional. A ladder: `"up"` to the deck above or `"down"` to the one below (see [Stairs](#stairs)). Walkable structures only. |
| `see_through` | Optional. You can see through it though it's solid, like a window. Left out, solid structures block sight. Walkable structures never block it, so it's for solid ones only. |
| `material` | The material it's made of, unless a map legend says otherwise. |
| `colour` | Optional. Draw it in this colour (same names as materials). Otherwise ladders are cyan, leaks red (bold from 100 cm²), floor dark grey, and solid structures their material's colour. Doors are `light_red`; a damaged door is `red`. |

## Recipes (`recipes/*.json`)

How one structure becomes another. The interact menu offers an action on a
tile only when a recipe for it exists.

```json
{
  "recipes": [
    { "id": "cut_door", "action": "cut", "from": "door", "to": "damaged_door", "seconds": 120 },
    { "id": "patch_hull_crack", "action": "repair", "from": "hull_crack", "to": "hull_plating",
      "seconds": 60, "uses": [ { "item": "scrap", "count": 1 } ], "material": "from_used" }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Unique across all recipe files. |
| `action` | The action that follows it, from `actions/`; its effect must be `recipe`. The action's rules (reach, line, tools from the material…) apply. Today: `cut` and `repair`. |
| `from` | The structure it acts on. Only one recipe per action per structure. |
| `to` | The structure it becomes. |
| `seconds` | How long it takes (at least 1). If the player blacks out partway, nothing changes and nothing is used. |
| `uses` | Optional. Items it uses up from your pockets, as `{ "item": id, "count": n }` (n at least 1). The menu shows them ("Repair: 1 scrap") and says if you're short. |
| `material` | Optional. `kept` (the default): the tile keeps its material. `from_used`: it becomes the material of the first item used (steel scrap makes a steel patch); needs `uses`. |
| `needs` | Optional. Tool levels it needs, all of them, like `{ "WELD": 1 }`, whatever the material. Qualities from `qualities/`. |

## Items (`items/*.json`)

Things you can pick up, carry and wear, grouped by kind (`clothing.json`,
`tools.json`, `salvage.json`). An item is an item type made of a material;
its weight is worked out: volume × the material's density × how solid it
is, plus whatever is in its pockets.

```json
{
  "items": [
    { "id": "basic_laser_cutter", "name": "basic laser cutter",
      "description": "A handheld fibre-laser cutter, about a kilowatt.",
      "glyph": "(", "material": "aluminium_alloy",
      "volume_ml": 800, "solid_percent": 50, "qualities": { "LASER": 1 } },
    { "id": "jumpsuit", "name": "jumpsuit", "description": "A crew jumpsuit.",
      "glyph": "[", "material": "cotton_fabric", "volume_ml": 2000, "solid_percent": 30,
      "wear": { "pockets": [
        { "name": "left hip pocket", "volume_ml": 1000, "max_mass_g": 2500 } ] } }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | How maps refer to it. |
| `name`, `description` | Shown in messages, look mode and the inventory. Items are named with their material: "scrap (Steel)". |
| `glyph` | The character it's drawn with lying on the floor, in its material's colour. |
| `material` | What it's made of, unless the map says otherwise. |
| `volume_ml` | How much room it takes, in millilitres (above 0). |
| `solid_percent` | How much of that volume is solid material, 1 to 100: 100 for a lump of scrap, 50 for a tool full of air and wiring. |
| `qualities` | Optional. Tool qualities it has, levels 1 to 10, like `{ "LASER": 1 }`. Each must be a quality from `qualities/`. |
| `wear` | Optional. Makes it wearable; `pockets` lists what it gives you to carry things in, each with a `name`, a `volume_ml` and a `max_mass_g`. |

## Maps (`maps/*.json`)

Each deck is drawn as rows of characters. The `legend` says which structure
each character is. `@` marks where the player starts: it must appear exactly
once, and the legend says what it stands on (something walkable).

```json
{
  "id": "closet",
  "name": "Supply closet",
  "legend": { "#": "bulkhead", ".": "deck_plating", "@": "deck_plating",
              ">": "ladder_up", "<": "ladder_down" },
  "decks": [
    { "name": "Lower closet",
      "rows": ["#####",
               "#.@>#",
               "#####"] },
    { "name": "Upper closet",
      "rows": ["#####",
               "#..<#",
               "#####"] }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Must match the file name (`closet.json` → `"closet"`) |
| `name` | Shown as the map panel's title |
| `legend` | One character → a structure's id, made of the structure's own material: `"#": "bulkhead"`. Or an object, to give it another material or lay items on it: `"H": { "structure": "bulkhead", "material": "nanocarbon_alloy" }`. An open door is its own structure: `"'": "door_open"`. Mistakes name the character: an unknown structure or material, or `@` standing on something solid. |
| `legend[].items` | Optional, in the object form: items lying on that tile, like `"s": { "structure": "deck_plating", "items": [ { "item": "scrap", "count": 4 } ] }`. Each entry is an `item`, optionally a `material` other than its own, and a `count` (default 1). Items need a walkable tile. |
| `player_wears` | Optional. What the player starts out wearing, as a list of items like `[ { "item": "jumpsuit" } ]`. Everything in it must be wearable. |
| `decks` | Listed bottom to top: the first is deck 0. All decks must be the same size. |
| `decks[].name` | Optional. Shown as the map panel's title on that deck. Without it the deck is "Deck 0", "Deck 1" and so on. |
| `decks[].pressure` | Optional. The deck's starting air pressure as a percentage of normal ship air (101.3 kPa: 21% O2, 79% N2, a trace of CO2), from 0 (vacuum) to 200. Default 100. Each second, air evens out across every connected open space (joined by open floor, open doors and ladders), so a deck at 60% next to a full one shares their air as soon as time passes. |
| `decks[].temperature` | Optional. The deck's starting air temperature in whole °C, from −270 (about as cold as deep space) to 200. Default 20. Vacuum has no temperature, so it doesn't matter on a deck at pressure 0. Warm and cold air mix where they meet, and heat slowly leaks out through the hull (the solid tiles around the map's border). |
| `decks[].rows` | The deck drawn as text, one string per row. |

### Stairs

Stairs come in pairs. A ladder up (a structure with `"climb": "up"`, `>` in
the example) needs a ladder down (`"climb": "down"`, `<`) at **exactly the
same row and column on the deck above**, and a ladder down needs one up
below it. A ladder without its partner is an error naming its spot, for
example
`stairs up at deck 0, row 2, column 27 need stairs down at the same spot on deck 1`.
Climbing takes 20 seconds.

Mistakes are caught when the game loads, with the exact place named, for example
`deck 0, row 1, column 2: 'x' is not in the legend`. Misspelled field names are errors too.
`cargo test` loads every map in this folder, so a broken map also fails the tests.
It also checks that **every open tile can be reached from `@`**, across
decks by stairs and through doors (closed doors count, since the player can
open them), so a room sealed off by mistake fails too.

## How to add things

None of these needs Rust or recompiling: add the JSON, restart the game.
Put new entries in a new file if you like (every `.json` file in a folder
is loaded); `cargo test` then checks them along with everything else.

### A new kind of door

A heavy steel hatch that's slow to open: two structures (shut and open)
and three recipes. In `structures/hatches.json`:

```json
{ "$schema": "../schema/structures.schema.json", "structures": [
  { "id": "hatch", "name": "hatch", "description": "A heavy steel hatch, shut.",
    "glyph": "=", "material": "steel", "colour": "light_red" },
  { "id": "hatch_open", "name": "open hatch", "description": "A heavy steel hatch, open.",
    "glyph": "_", "material": "steel", "colour": "light_red", "walkable": true } ] }
```

And in `recipes/hatches.json`:

```json
{ "$schema": "../schema/recipes.schema.json", "recipes": [
  { "id": "open_hatch", "action": "open", "from": "hatch", "to": "hatch_open", "seconds": 6 },
  { "id": "close_hatch", "action": "close", "from": "hatch_open", "to": "hatch", "seconds": 6 },
  { "id": "cut_hatch", "action": "cut", "from": "hatch", "to": "damaged_door", "seconds": 300 } ] }
```

Use it in a map's legend (`"=": "hatch"`). Walking into it opens it,
because the `open` action is `on_bump`; `c` closes it; and it can be cut
open, needing `LASER 2`, because the `cut` action takes its level from the
material, steel.

### A new tool, verb and recipe: welding

What the proof test `crates/nighthawk-sim/tests/new_content.rs` adds: one
entry in each of four folders. A quality, what a tool can do (in
`qualities/`):

```json
{ "id": "WELD", "name": "welding", "description": "Joining metal with heat." }
```

A tool that does it (in `items/`):

```json
{ "id": "welding_torch", "name": "welding torch", "description": "A small gas torch.",
  "glyph": "!", "material": "steel", "volume_ml": 300, "solid_percent": 50,
  "qualities": { "WELD": 1 } }
```

The verb, with its key and rules (in `actions/`):

```json
{ "id": "weld", "name": "Weld", "key": "w", "reach": 1, "clear_of_items": true,
  "done": "You weld the {was} shut." }
```

And what it does to what (in `recipes/`):

```json
{ "id": "weld_hull_crack", "action": "weld", "from": "hull_crack", "to": "hull_plating",
  "seconds": 90, "needs": { "WELD": 1 }, "uses": [ { "item": "scrap", "count": 1 } ],
  "material": "from_used" }
```

The interact menu then offers `w  Weld: 1 scrap` on every hull crack,
greyed with "needs WELD 1" until you carry the torch.

### A new kind of tile

A floor grate is a walkable structure; a weak, leaky panel is a solid one
with a `leak_cm2`. Properties are all there is to a tile's simulation:
`walkable`, `leak_cm2` and `climb` (see [Structures](#structures-structuresjson)).
