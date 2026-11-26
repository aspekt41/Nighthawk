# Scoping 03: Decision Log

*Started 2026-11-26 from the first review pass on the research and scoping docs.*

This file records decisions as they are made. Newest decisions go at the bottom of each
table. If a decision changes, strike it through and add the new one; don't delete history.

## Direction

| # | Decision | Why | Affects |
|---|---|---|---|
| D1 | **Concept C, *Nighthawk*** (derelict ship salvage), as described in Scoping 01 | Building the base and exploring the world are the same mechanic | Everything |
| D2 | **Rust** for the game | We want to learn Rust, safety helps avoid nasty bugs, and it's fast. Go + Bubble Tea + Wish isn't being pursued. | Tech stack |
| D3 | **Single-player only.** Multiplayer is dropped from the vision. | A better single-player simulation beats a multiplayer compromise. "Playing over SSH" means SSHing into your own machine and running the game in tmux, which needs no special support. | Non-goals, architecture |
| D4 | **Tone: gritty realism.** Writing (logs, item text) can shift the mood when needed. | — | Writing, content |
| D5 | **Death: respawn at base.** Difficulty decides whether you keep your items or drop them where you died. | Keeps exploration risky without throwing away hours of base building | Saves, difficulty settings |
| D6 | **Crew management is not planned.** It might become a very late addition. | Not wanted now | Roadmap |

## Simulation and design rules

| # | Decision | Why | Affects |
|---|---|---|---|
| D7 | **Time moves only when you act.** No real-time ticking anywhere, including environment systems. Every action has a time cost, and the world simulates exactly that much time. | You can leave the game sitting in tmux for days and nothing happens | Turn scheduler, all systems |
| D8 | **Fully 3D from the first line of code.** Positions are always (x, y, z). Decks are z-levels, and air, heat, fluids and creatures move between levels through shafts, hatches and breaches. | The 3D nature of DF and CDDA sets them apart (RimWorld's flatness is the biggest gripe) | Map, every grid system |
| D9 | **Tool qualities, not specific tools.** Recipes ask for `CUT 1`, `HAMMER 2`, and so on. | There should be a thousand ways to get `CUT 1` | Crafting, content |
| D10 | **Materials carry physical properties** (hardness, melting point, conductivity…) that drive tiers, damage, fire and construction. | DF's core lesson | Content model |
| D11 | **Inventory is physical containers with pockets**, like CDDA but with smoother UX. What you wear matters: a medical pouch on your belt survives dropping your backpack. | Containers make choices about what you carry, as in Project Zomboid | Items, UI |
| D12 | **Logistics is a problem the player solves, not one the game hides.** Early on you craft from what's on you and in containers within reach. Networked storage and item transport are machines you build later, in the style of Applied Energistics 2 and Mekanism. | Solving logistics is itself the fun | Crafting UI, automation, roadmap |
| D13 | **Automation must produce something that matters for survival or exploration**, such as ore-processing multipliers, O2 production or air scrubbing. It isn't automation for its own sake, which is the difference between Mekanism and Satisfactory. | — | Late-game design |
| D14 | **Life support is the needs model:** air, food, water, waste disposal, power, fatigue, radiation and lighting. MVP: O2, body heat, hunger, thirst and fatigue. Radiation and waste come right after the MVP. | All the systems people need, ONI-style | Body, base systems |
| D15 | **The stasis pod is a lifepod-style safety net.** It gives minimal emergency O2, heat and a basic fabricator, and it's your first respawn point. It's enough to scrape by on, not enough to thrive. | Subnautica's opening: scramble to survive while bootstrapping a real base | MVP opening |

## Technology

| # | Decision | Why | Affects |
|---|---|---|---|
| D16 | **Primary platform: Debian, inside tmux, over SSH from a macOS terminal (zsh).** Native macOS and Linux are expected to work. Windows is best-effort. | That's how the game will actually be played | Testing, input, colour |
| D17 | **Content is JSON**, validated in three ways: strict typed loading (`serde` with `deny_unknown_fields`, so a typo is a load error), a content-validation test (every reference resolves), and a **JSON Schema generated from the Rust types** (`schemars`) for editor autocompletion and checking. | Everyone knows JSON. RON has no schema advantage, and JSON's schema tooling is better. | Content pipeline |
| D18 | **The simulation stays separate from every UI.** Commands and events are plain serialisable data, so a future web front-end could stream state over WebSockets or HTTP, or run the sim in WASM. | Keeps the web option open | Architecture |
| D19 | **ECS choice deferred.** Plain structs until entity counts force a decision (bevy_ecs or hecs). | Simpler code while learning Rust; no lock-in | Architecture |

## Engineering practice

| # | Decision | Why | Affects |
|---|---|---|---|
| D20 | **Quality gates on every change** (`scripts/check.sh`): rustfmt; clippy with warnings as errors and complexity lints on; all tests; line coverage of at least 90% with cargo-llvm-cov; no function over cyclomatic complexity 10, measured by lizard. Unit tests are written alongside each piece of critical logic. | Catch bugs early, keep functions small enough to understand, and make sure the thing actually compiles | All code |
| D21 | **One set of instructions for people and agents.** `CONTRIBUTING.md` (rules and workflow), `TOOLING.md` (how to use each tool) and `AGENTS.md` (agent instructions; `CLAUDE.md` imports it rather than duplicating it). The Rust toolchain is pinned in `rust-toolchain.toml`, and the check script also builds the docs. | Anyone, human or agent, can build, test and contribute the same way with the same compiler | All contributors |
| D22 | **Determinism contract.** All randomness in the sim comes from `GameRng`: PCG32 (`rand_pcg`), one independent stream per game system, all derived from the world seed. We use only its raw output (pinned to the PCG reference values by a test) plus our own unbiased `below`/`one_in`/`pick`, never library helpers that may change between versions. `World::state_hash` (FNV-1a over explicit bytes) fingerprints the whole state. A golden test plays a fixed 500-command script and must match the stored hash; CI runs it on Linux and macOS, in debug and release. Stream numbers and tile hash IDs are never changed or reused. | Same seed + same moves = same game on every machine and version, which makes bugs reproducible and accidental changes visible | Sim, saves, CI |
| D23 | **Save and load come before more content (M0.4).** A save is the whole `World` as pretty JSON inside a `{format, version}` envelope; strict parsing (`deny_unknown_fields`), then consistency checks (map size, player position). Other versions are rejected with a clear message (no migrations before 1.0). One save slot, in the platform data folder (`~/.local/share/nighthawk/` on Linux, `~/Library/Application Support/nighthawk/` on macOS), overridable with `NIGHTHAWK_SAVE_DIR`. Binary saves come later, when saves get large. Golden tests save and reload mid-game and must still match the stored fingerprint. | Cheapest while state is small; every later system is built saveable; determinism turns save/load into a very strong test; JSON is readable while learning | Sim, TUI, all future state |
| D24 | **Title menu and leaving the game.** The game opens on a menu (New game / Continue / Quit; Continue is greyed out with no save). In a game, `q` saves and returns to the menu, `Q` returns without saving, `S` saves, Ctrl-C saves and exits; a failed save never leaves the game silently. `--seed N` skips the menu and starts that ship. A damaged save is reported on the menu, never blocking a new game. | Familiar flow; nothing is ever lost by accident; scripted runs and replays stay one command | TUI |
| D25 | **Decks and stairs.** Stairs come in pairs: stairs up (`<`) at a spot need stairs down (`>`) at the same x, y on the deck above, checked when a map or save loads. Climbing takes 2 turns; trying to climb without the right stairs takes none. Decks have optional names (shown as the map panel title; default "Deck N"). The starting map is now `aft_ring` ("Aft habitation ring": Stasis bay below, Crew quarters above). A content test requires every walkable tile to be reachable from the start, across decks. Save layout went to version 2; version-1 saves are rejected with a clear message. | Simple, predictable vertical movement that keeps the 3D promise (D8) and can't strand the player | Maps, sim, saves, content |
