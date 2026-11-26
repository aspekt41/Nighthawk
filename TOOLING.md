# Tooling guide

How to install and use every tool this project relies on, written for someone
new to Rust. For *how we work* (workflow, rules, definition of done), see
[CONTRIBUTING.md](CONTRIBUTING.md).

Every command here is run from the repository root.

---

## 1. One-time setup

### Rust itself (rustup)

[rustup](https://rustup.rs) installs and updates Rust. On Debian or macOS:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Debian you also need a C linker, which Rust uses to produce the final binary:

```sh
sudo apt install build-essential
```

You don't pick a Rust version yourself. [`rust-toolchain.toml`](rust-toolchain.toml)
pins it (currently 1.97), and rustup downloads that exact version, with
`rustfmt`, `clippy` and `llvm-tools-preview`, the first time you run `cargo`
in this folder. Check it worked:

```sh
cargo --version              # cargo 1.97.x
rustup show active-toolchain # ...overridden by '.../rust-toolchain.toml'
```

### The two extra quality tools

```sh
cargo install cargo-llvm-cov      # test coverage (needs llvm-tools-preview, installed above)
pipx install lizard               # cyclomatic complexity (or: pip install lizard)
```

On Debian, `sudo apt install pipx` first if you don't have it.

### tmux (the main way we play)

```sh
sudo apt install tmux
```

Add these to `~/.tmux.conf`, then run `tmux source ~/.tmux.conf`:

```tmux
set -sg escape-time 10                              # Esc responds immediately
set -as terminal-features ',xterm-256color:RGB'     # truecolor passthrough
set -as terminal-features ',xterm*:sync'            # synchronized output (tmux 3.4+)
```

### Editor support (recommended)

Install **rust-analyzer** for your editor (VS Code, Neovim, Helix, Zed and
others all support it). It gives you inline type hints, go-to-definition,
errors as you type and format-on-save. It reads the same toolchain file, so
it uses the same Rust as everyone else.

---

## 2. The tools at a glance

| Tool | Command | What it's for |
|---|---|---|
| rustup | `rustup ...` | Installs and updates Rust and its components |
| cargo | `cargo ...` | Rust's build tool and package manager. Almost everything goes through it. |
| rustfmt | `cargo fmt` | Formats code to the standard Rust style |
| clippy | `cargo clippy` | Linter: catches bugs, awkward code and overly complex functions |
| rustdoc | `cargo doc` | Builds HTML docs from `///` and `//!` comments |
| test runner | `cargo test` | Runs every unit and integration test |
| cargo-llvm-cov | `cargo llvm-cov` | Measures which lines the tests actually run |
| lizard | `lizard ...` | Measures cyclomatic complexity per function |
| check script | `scripts/check.sh` | Runs all of the above in order, as one gate |

---

## 3. Building and running

```sh
cargo run -p nighthawk-tui        # build and run (debug build); opens the title menu
cargo run -p nighthawk-tui -- --seed 42   # skip the menu, play a specific seed (args go after --)
cargo run -p nighthawk-tui --release   # optimised build, slower to compile
cargo build --workspace           # build everything without running
cargo check --workspace           # type-check only: the fastest way to see errors
```

- `-p <name>` picks one package (crate) in the workspace. The crates are
  `nighthawk-sim` (the game rules) and `nighthawk-tui` (the terminal front-end).
- Build output goes in `target/`. It's ignored by git and safe to delete
  (`cargo clean`).
- The game reads content from `data/`. To point it somewhere else, set
  `NIGHTHAWK_DATA=/path/to/data`.
- Saves go to `~/.local/share/nighthawk/save.json` (Linux) or
  `~/Library/Application Support/nighthawk/save.json` (macOS). To keep test
  runs away from your real save, set `NIGHTHAWK_SAVE_DIR=/tmp/nh-saves`.
  Save files are readable JSON, so you can open one to see what was saved.
- If the game ever crashes and leaves your terminal in a strange state, type
  `reset` and press Enter.

---

## 4. Tests (`cargo test`)

```sh
cargo test --workspace                  # every test in every crate
cargo test -p nighthawk-sim             # one crate
cargo test map_file                     # only tests whose name contains "map_file"
cargo test ragged_row_is_rejected       # one specific test
cargo test -- --nocapture               # show println! output from tests
```

**Run tests from the repository root.** Running `cargo test` inside a crate's
folder only tests that one crate.

Where tests live:

- **Unit tests** sit at the bottom of the file they test, in a
  `#[cfg(test)] mod tests { ... }` block. They can see private functions.
- **Integration tests** live in a crate's `tests/` folder, for example
  `crates/nighthawk-sim/tests/content.rs`. They use the crate only through its
  public API, the way other code does. The content test there loads every
  file in `data/maps/`, so a broken map fails `cargo test`.
- **Screen tests** draw the UI into ratatui's `TestBackend`, a fake
  terminal held in memory, and check the text on it. See the tests in
  `crates/nighthawk-tui/src/ui.rs`.

A test is any function marked `#[test]`. It passes if it returns without
panicking, so `assert!`, `assert_eq!` and `.unwrap()` are how tests fail.

---

## 5. Formatting (`cargo fmt`)

```sh
cargo fmt --all               # format everything in place
cargo fmt --all -- --check    # only check; fails if anything needs formatting
```

We use the default Rust style with no custom config. Easiest of all: turn on
format-on-save in your editor (rust-analyzer does this).

---

## 6. Linting (`cargo clippy`)

```sh
cargo clippy --workspace --all-targets -- -D warnings    # what the check script runs
cargo clippy --fix --allow-dirty                          # apply the safe automatic fixes
cargo clippy --explain excessive_nesting                  # what a lint means and why
```

- `--all-targets` lints tests too, not just the game code.
- `-D warnings` turns every warning into an error, so nothing slips through.
- Project-wide lint settings live in the root [`Cargo.toml`](Cargo.toml)
  (`[workspace.lints]`), and their thresholds live in [`clippy.toml`](clippy.toml):

| Lint | Limit | Why |
|---|---|---|
| `too_many_lines` | 60 lines per function | Long functions are hard to test |
| `excessive_nesting` | 4 levels of blocks | Deep nesting hides bugs. Pull the inner part into a helper function. |
| `cognitive_complexity` | 15 | A rough guard only. lizard (below) is the real measure. |
| `unsafe_code` | forbidden | We don't need it, and it removes Rust's safety guarantees |

When clippy flags something, fix the code. Silencing a lint with
`#[allow(...)]` needs a comment saying why, and should be rare.

---

## 7. API docs (`cargo doc`)

```sh
cargo doc --workspace --no-deps --open    # build and open the docs in a browser
```

`///` comments document the item below them. `//!` comments document the
whole file or module. The check script builds the docs with warnings as
errors, so a broken link such as `` [`World`] `` pointing at nothing fails the check.

---

## 8. Test coverage (`cargo llvm-cov`)

Coverage says which lines of code the tests actually run. Untested lines are
where bugs hide.

```sh
cargo llvm-cov --workspace --summary-only            # table of % per file
cargo llvm-cov --workspace --show-missing-lines      # ...plus which line numbers are missed
cargo llvm-cov --workspace --html                    # full report: target/llvm-cov/html/index.html
cargo llvm-cov --workspace --open                    # same, opened in your browser (local machine only)
```

Over SSH, use `--show-missing-lines`, or build the HTML and copy
`target/llvm-cov/html/` back to your Mac.

How to read the summary table: **Lines** is the column to watch. **Functions**
shows functions no test ever calls. **Regions** is finer-grained, down to
parts of a line, such as one branch of an `if`.

The gate is **at least 90% of lines** across the workspace. In practice:

- `nighthawk-sim` (the game rules) should stay at or near 100%. It's pure
  logic and easy to test.
- `nighthawk-tui` misses a little: `main()` and the input loop need a real
  terminal. Everything else in it is tested.

---

## 9. Cyclomatic complexity (`lizard`)

Cyclomatic complexity (CCN) counts the independent paths through a function.
Each `if`, `match` arm, loop, `&&` and `||` adds one. A function with CCN 8
needs about 8 tests to try every path, so keeping CCN low keeps functions
testable.

```sh
lizard -l rust crates/                 # table of every function
lizard -l rust -C 10 -w crates/        # only functions over 10 (fails if any; what the check runs)
```

How to read a row:

```
  NLOC    CCN   token  PARAM  length  location
    26      8     248      1      29  parse_map@147-175@crates/nighthawk-sim/src/map_file.rs
```

**NLOC** is lines of code, **CCN** is the complexity, **PARAM** is how many
arguments it takes, and the location is `function@start-end@file`.

The gate is **no function over 10**. When something goes over, split it into
smaller functions, or replace a long `match` with a lookup table, as the
keymap in `nighthawk-tui/src/main.rs` does.

---

## 10. Determinism and the golden test

The same seed and the same moves must always give the same game (decision
D22). `crates/nighthawk-sim/tests/determinism.rs` plays a fixed 500-command
script and compares the world's fingerprint (`World::state_hash`) against a
stored value.

```sh
cargo test -p nighthawk-sim --test determinism            # run just this test
cargo test --release -p nighthawk-sim --test determinism  # and in an optimised build
```

If it fails, the message shows the new hash and ambient-sound count.
**Didn't mean to change gameplay?** Find what changed; don't update the
value. **Did mean to** (a new rule, a new random roll, a map edit)? Paste the
new values into `EXPECTED_STATE_HASH` / `EXPECTED_AMBIENT_COUNT` and say why
in the commit message.

To reproduce a game by hand, run with the seed from its sidebar:
`cargo run -p nighthawk-tui -- --seed <n>`.

## 11. All checks at once

```sh
scripts/check.sh
```

This runs formatting → clippy → tests → docs → coverage → complexity → the
agent tools' own tests (Python), and stops at the first failure. Run it
before every commit. If cargo-llvm-cov, lizard or python3 isn't installed,
that step is skipped with an install hint, and everything else still runs.

The helper scripts in `agent-tools/` (reading a file after formatting it,
and making exact edits that survive rustfmt) work for people too; see
`agent-tools/tool-descriptions.md`.

---

## 12. Continuous integration (GitHub Actions)

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every push and
pull request:

| Job | Runs on | Does |
|---|---|---|
| All checks | Linux | `scripts/check.sh`, the same gate you run locally, with every tool installed |
| Tests and determinism | Linux and macOS | `cargo test --workspace`, then the golden test in release mode. Matching on both proves the game is identical across machines. |

When `CI` is set, `scripts/check.sh` fails instead of skipping a missing tool.
Results show on the repository's **Actions** tab and next to each commit.

## 13. Testing the game in tmux without touching your session

The quick way is `agent-tools/play.py`: it plays a key script and prints
the screen in one command, with saves going to a scratch folder, for
example `agent-tools/play.py '{k*5}{l*23}>' --grep Pos`. See
`agent-tools/tool-descriptions.md`. The manual way:

Handy for checking a change in a real terminal, or for scripted checks. A
private tmux socket (`-L nhtest`) keeps it separate from your own sessions:

```sh
cargo build -p nighthawk-tui
tmux -L nhtest new-session -d -s nh -x 100 -y 30 "NIGHTHAWK_SAVE_DIR=/tmp/nh-saves ./target/debug/nighthawk-tui --seed 42"
tmux -L nhtest send-keys -t nh l l k .      # press keys: right, right, up, wait
tmux -L nhtest capture-pane -t nh -p        # print what's on screen
tmux -L nhtest send-keys -t nh Q q          # leave without saving, then quit from the menu
```

---

## 14. Dependencies

```sh
cargo add serde@1.0.229 --features derive -p nighthawk-sim   # add, pinned to an exact version
cargo add --dev tempfile@3.27.0 -p nighthawk-tui             # test-only dependency
cargo tree -p nighthawk-tui                                   # see what depends on what
cargo update                                                  # update within allowed versions
```

Before adding a crate, check on [crates.io](https://crates.io) that it's
maintained (a recent release, real download numbers). Pin the exact current
version. `Cargo.lock` records the full dependency tree and is committed.

ratatui re-exports crossterm as `ratatui::crossterm`. Use that rather than
adding crossterm separately, so the two versions can never disagree.

---

## 15. Troubleshooting

| Symptom | Fix |
|---|---|
| `linker 'cc' not found` | `sudo apt install build-essential` |
| Terminal is garbled after a crash | Type `reset` and press Enter |
| Esc feels slow in tmux | `set -sg escape-time 10` in `~/.tmux.conf` |
| Colours look washed out in tmux | Add the `RGB` terminal-features line from setup |
| `could not find the data folder` | Run from the repo root, or set `NIGHTHAWK_DATA` |
| A test string full of `#` won't compile | See the raw-string note in [AGENTS.md](AGENTS.md#gotchas) |
| `cargo llvm-cov` says llvm-tools are missing | `rustup component add llvm-tools-preview` |
| Coverage shows well-tested code as missed | Stale data from earlier builds: `cargo llvm-cov clean --workspace` (the check script does this itself) |
| The determinism test fails | See section 10: fix the accident, or update the stored value on purpose |
