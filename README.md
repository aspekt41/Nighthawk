# Nighthawk

A terminal-native survival and base-building game where the systems run deep. It follows
the Dwarf Fortress / Cataclysm: DDA tradition and is built around the explore → find →
craft → build loop of Subnautica, Abiotic Factor and Minecraft.

**Status:** M1.2. A title menu, save and load, a clock in real seconds, air (O2, N2 and CO2) that evens out across every connected space each second, and two decks to explore, the Stasis bay and the Crew quarters above it (with doors to open and close), joined by a ladder (loaded from [`data/maps/aft_ring.json`](data/maps/aft_ring.json)), while the ship creaks around you. Every game comes from a seed, so the same seed and moves replay exactly. See [Running it](#running-it).

## Documents

### Research
- [01 — Genre landscape](docs/research/01-genre-landscape.md): what the reference games do well, what to take from each, and the gap Nighthawk fills
- [02 — Progression loop design](docs/research/02-progression-loop-design.md): how the explore/craft/build loop works, gating types, making the base matter, which simulation layers to build
- [03 — Terminal tech stack](docs/research/03-terminal-tech-stack.md): language and framework comparison, architecture, library versions as of Nov 2026, the terminal as a platform

### Scoping
- [01 — Game concepts](docs/scoping/01-game-concepts.md): four candidate concepts, a scored comparison, and the recommendation
- [02 — Scope and roadmap](docs/scoping/02-scope-and-roadmap.md): vision, pillars, non-goals, the MVP vertical slice, milestones and risks
- [03 — Decision log](docs/scoping/03-decisions.md): every decision made so far, with the reasoning

## Current direction (decided)

- **Concept:** *Nighthawk*. You wake alone on a drifting, dead starship and take it back one sealed, powered and pressurised room at a time. Base expansion and world progression are the same mechanic.
- **Stack:** Rust, with ratatui + crossterm for the terminal UI. The simulation core is a deterministic library crate with no I/O, and it can also target the browser (Ratzilla/WASM) and SSH later.
- **Play model:** a single avatar, turn-based: each action takes a set number of seconds (a step 3 s, a ladder 20 s), and the world is simulated one second at a time on 1 m tiles. Air, heat and power are simulated per tile.

## Reviewing

The same documents are also published as a commentable Claude doc, *Nighthawk: Research & Scoping*, with one tab per file. Feedback can be left there as comments. The markdown files in this repo stay the source of truth, and any changes made through review are copied back here.

## Running it

You need [rustup](https://rustup.rs). The Rust version is pinned in `rust-toolchain.toml` and installs itself on first use. Full setup, including Debian packages and tmux settings, is in [TOOLING.md](TOOLING.md).

```sh
cargo run -p nighthawk-tui                  # opens the title menu
cargo run -p nighthawk-tui -- --seed 42     # skip the menu and play a specific ship
cargo test                                  # run all tests
```

**Title menu:** `j`/`k` or arrows to move, Enter to choose, or `n` (New game),
`c` (Continue), `q` (Quit). Continue is greyed out until you've saved.

**In a game:** arrows, `hjkl` + `yubn` (vi keys) or the numpad to move; `.` or `5`
to wait; `<` and `>` to climb a ladder up or down; walk into a door to open it,
`c` then a direction to close one; `a` to show or hide the air overlay; `S` to save; `q` to save and return to the menu; `Q` to return without
saving; Ctrl-C to save and exit.

The game saves to one slot: `~/.local/share/nighthawk/save.json` on Linux,
`~/Library/Application Support/nighthawk/save.json` on macOS, or the folder in
`$NIGHTHAWK_SAVE_DIR`.
Time only passes when you act, so it's safe to leave running in tmux. The seed is shown in the sidebar and in the first log line.

Recommended `~/.tmux.conf` lines for the primary setup (macOS terminal → SSH → Debian → tmux):

```tmux
set -sg escape-time 10                              # Esc responds immediately
set -as terminal-features ',xterm-256color:RGB'     # truecolor passthrough
set -as terminal-features ',xterm*:sync'            # synchronized output (tmux 3.4+)
```

## Quality checks

One command runs every check (formatting, lints, tests, docs, coverage of at
least 90%, and cyclomatic complexity of at most 10) and stops at the first failure:

```sh
scripts/check.sh
```

How to install and use each tool is in [TOOLING.md](TOOLING.md).

## Contributing

- [CONTRIBUTING.md](CONTRIBUTING.md): the rules, workflow and definition of done, for everyone
- [TOOLING.md](TOOLING.md): installing and using cargo, rustfmt, clippy, tests, coverage and complexity tools
- [AGENTS.md](AGENTS.md): instructions for AI coding agents ([CLAUDE.md](CLAUDE.md) imports it)

## Layout

```
crates/
  nighthawk-sim/   the game: world state, rules, no I/O (deterministic, testable)
  nighthawk-tui/   the terminal front-end: input, drawing, message log, file loading
data/              game content as JSON (see data/README.md)
docs/              research, scoping and the decision log
scripts/check.sh   every quality check in one command
rust-toolchain.toml  the pinned Rust version
```
