#!/usr/bin/env python3
"""Play the game in tmux and print the screen, in one command.

Builds the game, starts it in its own session on the private `nhtest` tmux
socket (never touching anyone else's tmux), sends keys, waits for the screen
to settle after each batch, prints the screen, and kills only its own
session. Saves always go to a fresh temporary folder, so nothing ever lands
in the real save slot.

    agent-tools/play.py 'llk.'                       # type l, l, k, . then show the screen
    agent-tools/play.py '{k*5}{l*23}>' '{show}' jjj  # repeat keys; show the screen midway
    agent-tools/play.py 'V{h*4}' '{Esc}' l           # named keys: {Esc} {Enter} {Up} {Down} ...
    agent-tools/play.py 'e{h*4}' --grep 'Cut|reach'  # print only matching screen lines
    agent-tools/play.py lll --lines 19:23            # print only screen rows 19 to 23
    agent-tools/play.py lll --twice                  # play twice; say if the screens match
    agent-tools/play.py --seed 7 --size 100x30 ...   # another ship, another terminal size
    agent-tools/play.py --data /tmp/bad              # NIGHTHAWK_DATA, to test load errors

Key script: arguments are joined into one script. Plain characters are typed
as they are. In braces:
  {x*N}      the keys x (one or more characters), N times
  {Esc} {Enter} {Tab} {Up} {Down} {Left} {Right} {Space} {BSpace} {C-c}
             a named key, sent on its own with a pause either side (so Esc
             can't merge with the next key into Alt-key)
  {show}     print the screen as it is now
  {wait:MS}  wait MS milliseconds

Exit status: 0 normally; 1 if the game had already exited by the end, or if
--twice found the screens differ. Only the Python standard library is used.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from show import repo_root

SOCKET = "nhtest"
NAMED = {
    "Esc": "Escape",
    "Enter": "Enter",
    "Tab": "Tab",
    "Up": "Up",
    "Down": "Down",
    "Left": "Left",
    "Right": "Right",
    "Space": "Space",
    "BSpace": "BSpace",
    "C-c": "C-c",
}
EXITED = "[game exited:"


def parse_script(script):
    """The key script as a list of steps: ("type", text), ("key", name),
    ("show", None) or ("wait", seconds). Raises ValueError on bad braces."""
    steps, i = [], 0
    while i < len(script):
        if script[i] != "{":
            end = script.find("{", i)
            end = len(script) if end == -1 else end
            add_text(steps, script[i:end])
            i = end
            continue
        close = script.find("}", i)
        if close == -1:
            raise ValueError(f"unclosed brace at {script[i:]!r}")
        steps.extend(parse_brace(script[i + 1 : close]))
        i = close + 1
    return merged(steps)


def add_text(steps, text):
    """Adds typed text, joining it onto typed text just before it."""
    if steps and steps[-1][0] == "type":
        steps[-1] = ("type", steps[-1][1] + text)
    elif text:
        steps.append(("type", text))


def parse_brace(inner):
    """The steps for one `{...}`."""
    if inner == "show":
        return [("show", None)]
    if inner.startswith("wait:"):
        return [("wait", int(inner[5:]) / 1000)]
    if inner in NAMED:
        return [("key", NAMED[inner])]
    repeat = re.fullmatch(r"(.+)\*(\d+)", inner)
    if repeat:
        return [("type", repeat.group(1) * int(repeat.group(2)))]
    raise ValueError(f"unknown {{{inner}}}: use {{x*N}}, {{show}}, {{wait:MS}} or a named key")


def merged(steps):
    """Steps with neighbouring typed text joined, so it's sent in one go."""
    out = []
    for kind, value in steps:
        if kind == "type":
            add_text(out, value)
        else:
            out.append((kind, value))
    return out


def tmux(*args, check=True):
    """Runs a tmux command on the private socket and returns its output."""
    result = subprocess.run(
        ["tmux", "-L", SOCKET, *args], capture_output=True, text=True, check=False
    )
    if check and result.returncode != 0:
        raise RuntimeError(f"tmux {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def settle(capture, timeout, quiet=0.15, poll=0.05, clock=time.monotonic, sleep=time.sleep):
    """Waits until `capture()` returns the same non-empty screen for `quiet`
    seconds, or `timeout` passes. Returns the last screen."""
    deadline = clock() + timeout
    last, steady_since = None, clock()
    while True:
        screen = capture()
        now = clock()
        if screen != last or not screen.strip():
            last, steady_since = screen, now
        elif now - steady_since >= quiet:
            return screen
        if now >= deadline:
            return screen
        sleep(poll)


def game_command(root, args, save_dir):
    """The shell command tmux runs: the game, then a note of how it exited,
    then a pause so the screen stays up to be read."""
    env = f"NIGHTHAWK_SAVE_DIR={sh_quote(save_dir)}"
    if args.data:
        env += f" NIGHTHAWK_DATA={sh_quote(args.data)}"
    binary = sh_quote(str(root / "target" / "debug" / "nighthawk-tui"))
    seed = "" if args.no_seed else f" --seed {args.seed}"
    return f'{env} {binary}{seed}; echo "{EXITED} $?]"; sleep 600'


def sh_quote(text):
    """`text` in single quotes, safe to paste into a shell command."""
    return "'" + text.replace("'", "'\\''") + "'"


def filtered(screen, args):
    """The screen, cut down to `--lines` rows and `--grep` matches."""
    rows = screen.rstrip("\n").split("\n")
    if args.lines:
        start, _, end = args.lines.partition(":")
        rows = rows[int(start or 1) - 1 : int(end or len(rows))]
    if args.grep:
        rows = [r for r in rows if re.search(args.grep, r)]
    return "\n".join(rows)


def play_once(root, args, steps):
    """Plays the steps in a fresh session; returns the screens to print as
    (label, screen) pairs, the last always being the final screen."""
    session = f"nh-play-{os.getpid()}-{time.monotonic_ns()}"
    width, _, height = args.size.partition("x")
    with tempfile.TemporaryDirectory() as scratch:
        save_dir = args.save_dir or scratch
        command = game_command(root, args, save_dir)
        tmux("new-session", "-d", "-s", session, "-x", width, "-y", height, command)
        try:
            return run_steps(session, steps)
        finally:
            tmux("kill-session", "-t", session, check=False)


def run_steps(session, steps):
    """Sends each step to `session`, collecting the screens asked for."""
    def capture():
        return tmux("capture-pane", "-t", session, "-p")

    screens = []
    settle(capture, timeout=10)
    for kind, value in steps:
        if kind == "type":
            tmux("send-keys", "-t", session, "-l", value)
        elif kind == "key":
            settle(capture, timeout=5)
            tmux("send-keys", "-t", session, value)
        elif kind == "wait":
            time.sleep(value)
        if kind == "show":
            screens.append(("screen", settle(capture, timeout=10)))
        else:
            settle(capture, timeout=10)
    screens.append(("final screen", settle(capture, timeout=10)))
    return screens


def build(root):
    """Builds the game; returns an error message, or "" if it built."""
    result = subprocess.run(
        ["cargo", "build", "--quiet", "-p", "nighthawk-tui"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    return "" if result.returncode == 0 else result.stderr


def make_parser():
    """The command line's options."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("keys", nargs="*", help="the key script (see --help)")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--no-seed", action="store_true", help="start on the title menu")
    parser.add_argument("--size", default="80x24", help="terminal WIDTHxHEIGHT")
    parser.add_argument("--save-dir", help="keep saves here instead of a scratch folder")
    parser.add_argument("--data", help="use this data folder (NIGHTHAWK_DATA)")
    parser.add_argument("--lines", help="print only screen rows A:B (1-based)")
    parser.add_argument("--grep", help="print only screen lines matching this regex")
    parser.add_argument("--twice", action="store_true", help="play twice and compare")
    parser.add_argument("--no-build", action="store_true", help="don't run cargo build")
    return parser


def main(argv=None):
    args = make_parser().parse_args(argv)

    try:
        steps = parse_script("".join(args.keys))
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    root = repo_root(Path(__file__).resolve().parent)
    if not args.no_build and (error := build(root)):
        print(f"error: the game didn't build:\n{error}", file=sys.stderr)
        return 2

    screens = play_once(root, args, steps)
    for label, screen in screens:
        print(f"--- {label} ---\n{filtered(screen, args)}")
    status = 1 if EXITED in screens[-1][1] else 0
    if args.twice:
        again = play_once(root, args, steps)
        same = [s for _, s in again] == [s for _, s in screens]
        print("--- twice: the screens are " + ("identical" if same else "DIFFERENT") + " ---")
        status = status if same else 1
    return status


if __name__ == "__main__":
    sys.exit(main())
