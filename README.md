# Nighthawk

A terminal-native survival and base-building game where the systems run deep. It follows
the Dwarf Fortress / Cataclysm: DDA tradition and is built around the explore → find →
craft → build loop of Subnautica, Abiotic Factor and Minecraft.

**Status:** pre-production. Research and scoping only, with no code yet.

## Documents

### Research
- [01 — Genre landscape](docs/research/01-genre-landscape.md): what the reference games do well, what to take from each, and the gap Nighthawk fills
- [02 — Progression loop design](docs/research/02-progression-loop-design.md): how the explore/craft/build loop works, gating types, making the base matter, which simulation layers to build
- [03 — Terminal tech stack](docs/research/03-terminal-tech-stack.md): language and framework comparison, architecture, library versions as of Nov 2026, the terminal as a platform

### Scoping
- [01 — Game concepts](docs/scoping/01-game-concepts.md): four candidate concepts, a scored comparison, and the recommendation
- [02 — Scope and roadmap](docs/scoping/02-scope-and-roadmap.md): vision, pillars, non-goals, the MVP vertical slice, milestones, risks and open questions

## Current direction (proposed)

- **Concept:** *Nighthawk*. You wake alone on a drifting, dead starship and take it back one sealed, powered and pressurised room at a time. Base expansion and world progression are the same mechanic.
- **Stack:** Rust, with ratatui + crossterm for the terminal UI. The simulation core is a deterministic library crate with no I/O, and it can also target the browser (Ratzilla/WASM) and SSH later.
- **Play model:** a single avatar, turn-based with action costs. Air, heat and power are simulated per tile.
