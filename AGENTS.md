# AGENTS.md

Instructions for AI coding agents working on Nighthawk. People should read
[CONTRIBUTING.md](CONTRIBUTING.md) and [TOOLING.md](TOOLING.md). This file
condenses both and adds what an agent specifically needs.

## Project in one paragraph

Nighthawk is a turn-based terminal survival and base-building game in Rust: you
reclaim a dead starship one sealed room at a time. It's a Cargo workspace with
two crates. `crates/nighthawk-sim` holds the pure game rules (no I/O).
`crates/nighthawk-tui` is the terminal front-end, built on ratatui. Content is
JSON in `data/`. Design history and every decision are in `docs/`.

## Before you change anything

1. Read [`docs/scoping/03-decisions.md`](docs/scoping/03-decisions.md). Don't
   contradict a decision. If the task needs a different choice, stop and ask
   the owner, then record the new decision there.
2. Find the current milestone step in
   [`docs/scoping/02-scope-and-roadmap.md`](docs/scoping/02-scope-and-roadmap.md)
   (the M0–M6 table).
3. Run `git status` and `scripts/check.sh` so you start from a known-good state.

## Commands

All from the repository root. The toolchain is pinned in `rust-toolchain.toml`.

```sh
scripts/check.sh                                  # ALL gates: fmt, clippy, tests, docs, coverage, complexity
cargo run -p nighthawk-tui                        # run the game (needs a real terminal; see below)
cargo test --workspace                            # all tests (run from root, not a crate dir)
cargo test -p nighthawk-sim <name_fragment>       # a subset of tests
cargo fmt --all                                   # format
cargo clippy --workspace --all-targets -- -D warnings
cargo llvm-cov --workspace --show-missing-lines   # which lines no test reaches
lizard -l rust -C 10 -w crates/                   # functions over cyclomatic complexity 10
```

`scripts/check.sh` must pass before you commit. If cargo-llvm-cov or lizard
is missing, install them (`cargo install cargo-llvm-cov`, `pip install lizard`)
rather than accepting a skipped step.

## Architecture rules (do not break)

1. **The sim is pure.** No terminal, file I/O or clock in `nighthawk-sim`.
   Front-ends read files and pass text in (`parse_map(&str)`), and pass the
   seed in (`World::new(map, seed)`).
2. **The sim is deterministic (D22).** Randomness only via `GameRng`
   (`crates/nighthawk-sim/src/rng.rs`): add a new `Stream` variant with a
   new number for each new system; never renumber or reuse one; never use
   `rand`'s helpers, `rand::rng()` or the clock. New state goes into
   `World::state_hash`. New tile types get a new, never-reused `hash_id`.
   New state must also survive save/load (D23): it lives in `World`, so it's
   saved automatically. If it changes the save layout, bump `SAVE_VERSION` in
   `crates/nighthawk-sim/src/save.rs` and update the save-format test.
3. **`World::apply(Command) -> Vec<Event>` is the only way the world changes**,
   and the only place `turn` advances. Failed actions, like bumping a wall,
   cost no time. Never add real-time ticking (D7).
4. **Positions are always 3D** (`Pos { x, y, z }`) (D8).
5. **Content is strict JSON:** `#[serde(deny_unknown_fields)]`, validated on
   load, with errors that name the exact location (D17).
6. **Default keys are plain keys**, with no kitty keyboard protocol required (D16).
7. **No `unsafe`** (forbidden in the workspace lints). **Single-player only** (D3).

## How to work

- **Small steps.** Do the one roadmap step asked for, not the next three.
- **Tests come with the code.** Each piece of logic gets tests for its normal
  case, edge cases and every error path. Name tests as behaviour sentences.
  The sim crate is at 100% line coverage, so keep it there.
- **Quality gates are fixed.** Never raise thresholds in `clippy.toml` or
  `scripts/check.sh`, add `#[allow(...)]`, or delete, skip or weaken a test to
  get green. Fix the code instead: split functions, flatten nesting with
  helpers or iterators, and replace long `match` blocks with tables. If you
  truly believe a gate is wrong, ask the owner.
- **Comments are for a Rust learner.** The owner is learning Rust, so explain
  *why* in plain English and explain idioms the first time they appear.
- **Dependencies:** only maintained crates, pinned to exact versions with
  `cargo add name@x.y.z`. Use `ratatui::crossterm`, never a separate
  crossterm dependency.
- **Verify, don't assume.** After changing behaviour, run the game in tmux
  (recipe below) and look at the screen. Report real results, including
  failures.

## The golden determinism test

`crates/nighthawk-sim/tests/determinism.rs` compares a scripted game's
`state_hash` with a stored value. If your change *shouldn't* alter gameplay
and the test fails, you introduced a bug or non-determinism: fix it. If it
*should* (a new rule, roll or map change), update `EXPECTED_STATE_HASH` and
`EXPECTED_AMBIENT_COUNT` from the failure message, and explain why in the
commit message. Never "fix" it by loosening the test.

CI (`.github/workflows/ci.yml`) runs `scripts/check.sh` on Linux and the tests
on Linux and macOS, including this test in release mode. With `CI` set, the
check script fails on missing tools instead of skipping them.

## Running the TUI headlessly (tmux)

The game needs a terminal. Use a private tmux socket so you never touch the
owner's sessions, and never run `tmux kill-server`:

```sh
cargo build -p nighthawk-tui
tmux -L nhtest new-session -d -s nh -x 100 -y 30 \
  "NIGHTHAWK_SAVE_DIR=/tmp/nh-saves ./target/debug/nighthawk-tui --seed 42"
sleep 1
tmux -L nhtest send-keys -t nh l l k .   # keys: right, right, up, wait
tmux -L nhtest capture-pane -t nh -p     # read the screen
tmux -L nhtest send-keys -t nh Q q       # leave without saving, then quit from the menu
```

**Always set `NIGHTHAWK_SAVE_DIR` to a scratch folder** when running the
game. Without it, quitting saves over the owner's real game. Tests must use
temporary folders too (`tempfile::tempdir()` with `SaveSlot::in_dir`), never
the default save folder.

With `--seed` the game skips the title menu and starts playing at once,
which is what scripted runs want. Without it, the title menu shows first:
press `n` for a new game. In a game, `q` saves and returns to the menu, and
`q` on the menu exits, so `q q` ends a session.

Pass `--seed` so the screen is reproducible: the same seed and keys give a
byte-identical `capture-pane`, which is an easy end-to-end determinism check.

To check error handling without a terminal, point the game at a bad data
folder: `NIGHTHAWK_DATA=/some/dir ./target/debug/nighthawk-tui`. It prints a
precise error and exits with code 1 before entering raw mode.

## Gotchas

- **Raw strings and `#`.** Map JSON is full of `#`. In Rust, `r#"..."#` ends at
  the first `"#`, and `r##"..."##` at the first `"##`. A row like `"#######"`
  contains both, so no delimiter is safe. In tests, build the JSON with
  `format!` from a `[&str]` of rows (see `test_world()` in
  `crates/nighthawk-sim/src/world.rs` and `crates/nighthawk-tui/src/ui.rs`).
- **`cargo test` inside a crate folder** only tests that crate. Run it from the root.
- **Rust 2024 edition.** Clippy prefers let-chains
  (`if let A = x && let B = y { }`) over nested `if let`.
- **`std::env::set_var` is `unsafe` in Rust 2024**, and `unsafe` is forbidden.
  Write functions that take paths or values as parameters instead of reading
  environment variables in tests (see `find_data_dir` in
  `crates/nighthawk-tui/src/content.rs`).
- **Editing map rows with scripts:** use exact string replacement. In `sed` and
  regex patterns `.` matches *any* character, and `.` is also the floor glyph,
  so a pattern meant for one row can silently rewrite others (it once punched
  holes in the outer walls). The content tests catch this, but don't rely on it.
- **`set -e` and `&&` in shell scripts.** Bash's stop-on-error mode ignores a
  failure on the left of `&&`, so `tool && echo ok` never fails a script. In
  `scripts/check.sh`, put each gate command on its own line. (This once let a
  complexity violation pass silently.)
- **The data folder** is found via `$NIGHTHAWK_DATA`, then `./data`, then the
  build-time repo path.

## Keeping docs in sync

Update these in the same commit as the code:

| Change | Update |
|---|---|
| Finished a roadmap step | Tick it in `docs/scoping/02-scope-and-roadmap.md` (M0 row) |
| Made a design, tech or process choice | Add a `D<n>` row to `docs/scoping/03-decisions.md` |
| Changed how to run, play or check | `README.md`, `TOOLING.md`, and this file if agents are affected |
| Changed a content format | `data/README.md` |

The markdown in this repository is the source of truth. The owner also
reviews the research and scoping docs in a commentable Claude Docs copy.
When your tools can reach it, mirror doc changes there too. When they can't,
say that the copy needs updating.

## Commits

- Code, tests and docs for one step go in one commit.
- Imperative summary line under about 72 characters, then a body listing what
  changed and why.
- Commit or push only to the branch you were told to use. Never force-push.
  Never commit `target/`.
- Before you report a step as done, `scripts/check.sh` must pass with no
  skipped steps, and you've seen the game run.
