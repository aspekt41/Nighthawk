# Nighthawk

A terminal-native survival and base-building game where the systems run deep. It follows
the Dwarf Fortress / Cataclysm: DDA tradition and is built around the explore → find →
craft → build loop of Subnautica, Abiotic Factor and Minecraft.

**Status:** M0.1. You can walk an `@` around one room. See [Running it](#running-it).

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
- **Play model:** a single avatar, turn-based with action costs. Air, heat and power are simulated per tile.

## Reviewing

The same documents are also published as a commentable Claude doc, *Nighthawk: Research & Scoping*, with one tab per file. Feedback can be left there as comments. The markdown files in this repo stay the source of truth, and any changes made through review are copied back here.

## Running it

You need a Rust toolchain (`rustup`, Rust 1.85+ for the 2024 edition).

```sh
cargo run -p nighthawk-tui     # play
cargo test                     # run all tests
```

Controls: arrows, `hjkl` + `yubn` (vi keys) or the numpad to move; `.` or `5` to wait; `q` to quit.
Time only passes when you act, so it's safe to leave running in tmux.

Recommended `~/.tmux.conf` lines for the primary setup (macOS terminal → SSH → Debian → tmux):

```tmux
set -sg escape-time 10                              # Esc responds immediately
set -as terminal-features ',xterm-256color:RGB'     # truecolor passthrough
set -as terminal-features ',xterm*:sync'            # synchronized output (tmux 3.4+)
```

## Layout

```
crates/
  nighthawk-sim/   the game: world state, rules, no I/O (deterministic, testable)
  nighthawk-tui/   the terminal front-end: input, drawing, message log
docs/              research, scoping and the decision log
```
