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
`qualities`, `materials`, `structures`, `items`, `recipes`.

Every tile on a map is a **structure** (what it is: a bulkhead, a door)
made of a **material** (what it's made of: steel, Nanocarbon Alloy).

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

## Materials (`materials/*.json`)

Each material has real physical properties, in whole numbers:

```json
{
  "materials": [
    { "id": "steel", "name": "Steel",
      "description": "Structural steel: deck plating, ladders and bulkheads.",
      "colour": "grey", "density": 7850, "melting_point": 1450,
      "specific_heat": 490, "thermal_conductivity": 45, "hardness": 150,
      "cut_by": { "LASER": 2 } }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | How structures and maps refer to it. |
| `name`, `description` | Shown in look mode (`V`). |
| `colour` | Walls made of it are drawn in this colour: `white`, `grey`, `dark_grey`, `red`, `yellow`, `green`, `cyan`, `blue`, `magenta`. |
| `density` | kg/m³ |
| `melting_point` | °C. Optional: leave it out for something that doesn't melt (cotton chars and burns). |
| `specific_heat` | J/(kg·K) |
| `thermal_conductivity` | **mW**/(m·K), thousandths of a W/(m·K), so insulators like cotton (40) aren't rounded to zero. Steel is 45000. |
| `hardness` | Vickers hardness (HV); 0 for soft things like fabric. |
| `cut_by` | Optional. Tool qualities that cut it, with the level each needs (1 to 10); any one is enough. Leave it out and nothing can cut it. Each must be a quality from `qualities/`. |

Mistakes are reported precisely: a duplicate id, or a level out of range.

## Structures (`structures/*.json`)

```json
{
  "structures": [
    { "id": "door", "name": "door",
      "description": "A sliding pressure door. Shut, it holds air; open, it doesn't.",
      "glyph": "+", "open_glyph": "'", "behaviour": "door",
      "material": "aluminium_alloy" }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | How map legends refer to it. |
| `name`, `description` | Shown in look mode and in messages ("You bump into the hull plating."). |
| `glyph` | The one character the map draws it with. |
| `open_glyph` | Doors only, and every door needs one: the character for the door when open. |
| `behaviour` | What it does in the simulation, one of: `floor` (walk on it; air passes), `wall` (solid, airtight), `stairs_up` / `stairs_down` (a ladder to the next deck; see [Stairs](#stairs)), `door` (opens and shuts; shut, it's airtight), `hull_crack` (solid, a 4 cm² leak to space), `hull_breach` (solid, a 400 cm² hole to space). |
| `material` | The material it's made of, unless a map legend says otherwise. |
| `colour` | Optional. Draw it in this colour (same names as materials) instead of the usual colour for its behaviour; a damaged door is red though it behaves like floor. |

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
| `action` | What the player does: `cut`, which needs a tool with whatever quality and level the target's **material** lists in `cut_by` (an aluminium door: `LASER 1`), from within 2 tiles along a clear line; or `repair`, by hand from right beside it, with nothing lying on the tile. |
| `from` | The structure it acts on. Only one recipe per action per structure. |
| `to` | The structure it becomes. |
| `seconds` | How long it takes (at least 1). If the player blacks out partway, nothing changes and nothing is used. |
| `uses` | Optional. Items it uses up from your pockets, as `{ "item": id, "count": n }` (n at least 1). The menu shows them ("Repair: 1 scrap") and says if you're short. |
| `material` | Optional. `kept` (the default): the tile keeps its material. `from_used`: it becomes the material of the first item used (steel scrap makes a steel patch); needs `uses`. |

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
| `legend` | One character → a structure's id, made of the structure's own material: `"#": "bulkhead"`. Or an object, to give it another material or place a door open: `"H": { "structure": "bulkhead", "material": "nanocarbon_alloy" }`, `"'": { "structure": "door", "open": true }`. Mistakes name the character: an unknown structure or material, `open` on something that isn't a door, or `@` standing on something solid. |
| `legend[].items` | Optional, in the object form: items lying on that tile, like `"s": { "structure": "deck_plating", "items": [ { "item": "scrap", "count": 4 } ] }`. Each entry is an `item`, optionally a `material` other than its own, and a `count` (default 1). Items need a walkable tile. |
| `player_wears` | Optional. What the player starts out wearing, as a list of items like `[ { "item": "jumpsuit" } ]`. Everything in it must be wearable. |
| `decks` | Listed bottom to top: the first is deck 0. All decks must be the same size. |
| `decks[].name` | Optional. Shown as the map panel's title on that deck. Without it the deck is "Deck 0", "Deck 1" and so on. |
| `decks[].pressure` | Optional. The deck's starting air pressure as a percentage of normal ship air (101.3 kPa: 21% O2, 79% N2, a trace of CO2), from 0 (vacuum) to 200. Default 100. Each second, air evens out across every connected open space (joined by open floor, open doors and ladders), so a deck at 60% next to a full one shares their air as soon as time passes. |
| `decks[].temperature` | Optional. The deck's starting air temperature in whole °C, from −270 (about as cold as deep space) to 200. Default 20. Vacuum has no temperature, so it doesn't matter on a deck at pressure 0. Warm and cold air mix where they meet, and heat slowly leaks out through the hull (the solid tiles around the map's border). |
| `decks[].rows` | The deck drawn as text, one string per row. |

### Stairs

Stairs come in pairs. Stairs up (`>` in the example) need stairs down (`<`) at
**exactly the same row and column on the deck above**, and stairs down need
stairs up below them. A staircase without its partner is an error naming
its spot, for example
`stairs up at deck 0, row 2, column 27 need stairs down at the same spot on deck 1`.
Climbing takes 20 seconds.

Mistakes are caught when the game loads, with the exact place named, for example
`deck 0, row 1, column 2: 'x' is not in the legend`. Misspelled field names are errors too.
`cargo test` loads every map in this folder, so a broken map also fails the tests.
It also checks that **every open tile can be reached from `@`**, across
decks by stairs and through doors (closed doors count, since the player can
open them), so a room sealed off by mistake fails too.
