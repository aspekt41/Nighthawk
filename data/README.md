# Game data

Everything in this folder is content: plain JSON the game loads at startup
(decision D17). Changing a file here needs no Rust and no recompiling.

The game looks for this folder in three places, in order:
`$NIGHTHAWK_DATA`, then `./data`, then the repository's `data/` as it was when
the game was built.

## Materials (`materials.json`)

Every tile is a *shape* (from the map) made of a *material* (from here).
Each material has real physical properties, in whole numbers:

```json
{
  "materials": [
    { "id": "steel", "name": "Steel",
      "description": "Structural steel: deck plating, ladders and bulkheads.",
      "colour": "grey", "density": 7850, "melting_point": 1450,
      "specific_heat": 490, "thermal_conductivity": 45, "hardness": 150,
      "cut_by": { "LASER": 2 } }
  ],
  "defaults": { "floor": "steel", "wall": "steel", "...": "one per shape" }
}
```

| Field | Meaning |
|---|---|
| `id` | How maps refer to it. Must be unique. |
| `name`, `description` | Shown in look mode (`V`). |
| `colour` | Walls made of it are drawn in this colour: `white`, `grey`, `dark_grey`, `red`, `yellow`, `green`, `cyan`, `blue`, `magenta`. |
| `density` | kg/m³ |
| `melting_point` | °C |
| `specific_heat` | J/(kg·K) |
| `thermal_conductivity` | W/(m·K) |
| `hardness` | Vickers hardness (HV) |
| `cut_by` | Optional. Tool qualities that cut it, with the level each needs (1 to 10); any one is enough. Leave it out and nothing can cut it. Qualities today: `LASER`. |
| `defaults` | The material each shape is made of unless a map's legend says otherwise. Every shape needs one. |

Mistakes are reported precisely: a duplicate id, a level out of range, a
shape with no default, or a default naming a material that doesn't exist.

## Maps (`maps/*.json`)

Each deck is drawn as rows of characters. The `legend` says which tile each
character is. `@` marks the player start, on a floor tile, and must appear
exactly once.

```json
{
  "id": "closet",
  "name": "Supply closet",
  "legend": { "#": "wall", ".": "floor", ">": "stairs_up", "<": "stairs_down" },
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
| `legend` | One character → a tile shape, made of that shape's default material (see [Materials](#materials-materialsjson)): `"#": "wall"`. Or a shape and a material: `"H": { "tile": "wall", "material": "nanocarbon_alloy" }`. Shapes today: `wall`, `floor`, `stairs_up`, `stairs_down`, `door_closed`, `door_open`, `hull_crack` (a 4 cm² leak to space), `hull_breach` (a 400 cm² hole to space). `@` is reserved; the player starts on default floor. |
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
