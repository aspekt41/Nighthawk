# CLAUDE.md

All project instructions for agents live in AGENTS.md, so that every agent
tool reads the same rules. Claude Code loads it through the import below.

@AGENTS.md

## Claude Code notes

- Use the tmux recipe in AGENTS.md (private `-L nhtest` socket) to check the
  TUI. Don't run `tmux kill-server` or touch other tmux sessions.
- When you finish a step, tell the owner in plain language what changed, the
  test count, coverage and complexity results from `scripts/check.sh`, and
  what you saw when you ran the game.
