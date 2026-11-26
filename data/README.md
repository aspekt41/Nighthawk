# Game data

Everything in this folder is content: plain JSON the game loads at startup
(decision D17). Changing a file here needs no Rust and no recompiling.

The game looks for this folder in three places, in order:
`$NIGHTHAWK_DATA`, then `./data`, then the repository's `data/` as it was when
the game was built.

## Maps (`maps/*.json`)

Each deck is drawn as rows of characters. The `legend` says which tile each
character is. `@` marks the player start, on a floor tile, and must appear
exactly once.

```json
{
  "id": "closet",
  "name": "Supply closet",
  "legend": { "#": "wall", ".": "floor" },
  "decks": [
    { "rows": ["#####",
               "#.@.#",
               "#####"] }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Must match the file name (`closet.json` → `"closet"`) |
| `name` | Shown as the map panel's title |
| `legend` | One character → tile name. Tiles today: `wall`, `floor`. `@` is reserved. |
| `decks` | Listed bottom to top: the first is deck 0. All decks must be the same size. |

Mistakes are caught when the game loads, with the exact place named, for example
`deck 0, row 1, column 2: 'x' is not in the legend`. Misspelled field names are errors too.
`cargo test` loads every map in this folder, so a broken map also fails the tests.
