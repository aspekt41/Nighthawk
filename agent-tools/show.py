#!/usr/bin/env python3
"""Show a file the way it really is: formatted first, then searched.

Rust files are run through `cargo fmt` before anything is shown, so the text
you read is exactly the text a later edit will have to match. Then it prints
the matching lines with line numbers and context, a range of lines, or the
whole file.

    agent-tools/show.py FILE PATTERN          # lines matching a regex, with context
    agent-tools/show.py FILE PATTERN -F       # PATTERN is plain text, not a regex
    agent-tools/show.py FILE PATTERN -C 8     # 8 lines of context (default 3)
    agent-tools/show.py FILE "fn cut" -A 30   # 30 lines after each match (-B: before)
    agent-tools/show.py FILE --lines 120:160  # a range of lines
    agent-tools/show.py FILE                  # the whole file (first 300 lines; --all for more)
    agent-tools/show.py FILE ... --no-format  # skip cargo fmt

Exit status: 0 if anything was shown, 1 if the pattern matched nothing.
Only the Python standard library is used.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

WHOLE_FILE_LIMIT = 300


def repo_root(start: Path) -> Path:
    """The nearest folder at or above `start` with a Cargo.toml, else `start`."""
    for folder in [start, *start.parents]:
        if (folder / "Cargo.toml").is_file():
            return folder
    return start


def format_rust(path: Path) -> str:
    """Runs `cargo fmt --all` for a Rust file. Returns a warning to show, or ""."""
    if path.suffix != ".rs":
        return ""
    root = repo_root(path.resolve().parent)
    result = subprocess.run(
        ["cargo", "fmt", "--all"], cwd=root, capture_output=True, text=True
    )
    if result.returncode != 0:
        return "warning: cargo fmt failed, so this is the unformatted text:\n" + result.stderr
    return ""


def matching_ranges(lines, pattern, literal, before, after):
    """The 0-based (first, last) line ranges around each match, from
    `before` lines above it to `after` lines below, overlaps merged."""
    regex = re.compile(re.escape(pattern) if literal else pattern)
    ranges = []
    for number, line in enumerate(lines):
        if not regex.search(line):
            continue
        first, last = max(0, number - before), min(len(lines) - 1, number + after)
        if ranges and first <= ranges[-1][1] + 1:
            ranges[-1] = (ranges[-1][0], max(ranges[-1][1], last))
        else:
            ranges.append((first, last))
    return ranges


def numbered(lines, first, last):
    """Lines `first`..=`last` (0-based), each with its 1-based line number."""
    return [f"{n + 1:>5}  {lines[n]}" for n in range(first, last + 1)]


def parse_line_range(text, count):
    """'120:160' as a 0-based inclusive (first, last), clamped to the file."""
    start, _, end = text.partition(":")
    first = max(1, int(start or 1)) - 1
    last = min(count, int(end or count)) - 1
    return first, last


def render(lines, args):
    """The output lines for the request in `args`, and whether anything matched."""
    if args.lines:
        first, last = parse_line_range(args.lines, len(lines))
        return numbered(lines, first, last), True
    if args.pattern is None:
        last = len(lines) - 1 if args.all else min(len(lines), WHOLE_FILE_LIMIT) - 1
        out = numbered(lines, 0, last)
        if last < len(lines) - 1:
            out.append(f"... {len(lines) - last - 1} more lines (use --all or --lines)")
        return out, True
    before = args.context if args.before is None else args.before
    after = args.context if args.after is None else args.after
    ranges = matching_ranges(lines, args.pattern, args.fixed, before, after)
    out = []
    for i, (first, last) in enumerate(ranges):
        if i:
            out.append("   --")
        out.extend(numbered(lines, first, last))
    return out, bool(ranges)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("file", type=Path)
    parser.add_argument("pattern", nargs="?")
    parser.add_argument("-F", "--fixed", action="store_true", help="plain text, not a regex")
    parser.add_argument("-C", "--context", type=int, default=3, help="lines around a match")
    parser.add_argument("-A", "--after", type=int, help="lines after a match (instead of -C)")
    parser.add_argument("-B", "--before", type=int, help="lines before a match (instead of -C)")
    parser.add_argument("--lines", help="a range like 120:160")
    parser.add_argument("--all", action="store_true", help="the whole file, however long")
    parser.add_argument("--no-format", action="store_true", help="don't run cargo fmt")
    args = parser.parse_args(argv)

    if not args.no_format:
        warning = format_rust(args.file)
        if warning:
            print(warning, file=sys.stderr)
    lines = args.file.read_text().split("\n")
    out, found = render(lines, args)
    print("\n".join(out))
    if not found:
        print(f"no line matches {args.pattern!r}", file=sys.stderr)
    return 0 if found else 1


if __name__ == "__main__":
    sys.exit(main())
