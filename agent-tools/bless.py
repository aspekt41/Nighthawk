#!/usr/bin/env python3
"""Update a snapshot constant from a failing test: "bless" the new output.

Some tests compare text with a stored copy, like the save-format test,
which checks `save_to_string` against `const EXPECTED_SAVE: &str = r#"..."#`.
When the format changes on purpose, this runs the test, takes the actual
text from its `left:` line, and writes it into the constant, so it doesn't
have to be copied by hand.

    agent-tools/bless.py the_save_format_is_stable crates/nighthawk-sim/src/save.rs EXPECTED_SAVE
    agent-tools/bless.py ... --dry-run    # show what would be written, change nothing
    agent-tools/bless.py ... -p nighthawk-sim   # test one crate (when another won't build yet)

The test must compare with `assert_eq!(actual, CONST)`, actual first: the
`left:` value is what gets written. Afterwards the test is run again to
check it now passes. Read the diff before committing: blessing a snapshot
says the new output is right.

Exit status: 0 if the constant was updated and the test passes, 1 if not.
Only the Python standard library is used.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

from show import format_rust, repo_root

ESCAPES = {"n": "\n", "t": "\t", "r": "\r", '"': '"', "'": "'", "\\": "\\", "0": "\0"}


def debug_unescape(quoted):
    """The string a Rust `{:?}` string literal (with its quotes) stands for."""
    if len(quoted) < 2 or quoted[0] != '"' or quoted[-1] != '"':
        raise ValueError(f"not a quoted string: {quoted[:40]!r}")
    body, out, i = quoted[1:-1], [], 0
    while i < len(body):
        ch = body[i]
        if ch != "\\":
            out.append(ch)
            i += 1
            continue
        code = body[i + 1 : i + 2]
        if code == "u":
            close = body.index("}", i)
            out.append(chr(int(body[i + 3 : close], 16)))
            i = close + 1
        elif code in ESCAPES:
            out.append(ESCAPES[code])
            i += 2
        else:
            raise ValueError(f"unknown escape \\{code}")
    return "".join(out)


def left_value(output):
    """The actual value from a failed `assert_eq!` in test output."""
    for line in output.split("\n"):
        stripped = line.strip()
        if stripped.startswith("left: "):
            return debug_unescape(stripped[len("left: ") :])
    if "could not compile" in output:
        raise ValueError("the tests didn't compile:\n" + compile_errors(output))
    raise ValueError("no `left:` line in the test output (did the test fail on an assert_eq?)")


def compile_errors(output):
    """The `error` lines from cargo's output, and where each one is."""
    lines = output.split("\n")
    picked = [
        line
        for i, line in enumerate(lines)
        if line.startswith("error") or (i and lines[i - 1].startswith("error") and "-->" in line)
    ]
    return "\n".join(picked[:20])


def replace_const(text, name, value):
    """`text` with the raw string constant `name` holding `value`."""
    start = re.search(rf"const {re.escape(name)}: &str = r(#*)\"", text)
    if not start:
        raise ValueError(f"no `const {name}: &str = r#\"...\"#` in the file")
    hashes = start.group(1)
    close = '"' + hashes
    end = text.find(close, start.end())
    if end == -1:
        raise ValueError(f"{name}'s raw string never closes")
    if close in value:
        raise ValueError(f"the new value contains {close!r}: use more # in the delimiter")
    return text[: start.end()] + value + text[end:]


def run_test(root, test, package=None):
    """Runs the tests matching `test`, in `package` or the whole
    workspace; returns (passed, output)."""
    scope = ["-p", package] if package else ["--workspace"]
    result = subprocess.run(
        ["cargo", "test", *scope, "--quiet", test],
        cwd=root,
        capture_output=True,
        text=True,
    )
    return result.returncode == 0, result.stdout + result.stderr


def main(argv=None, runner=run_test):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("test", help="the test's name (a filter for cargo test)")
    parser.add_argument("file", type=Path, help="the file holding the constant")
    parser.add_argument("const", help="the constant's name, like EXPECTED_SAVE")
    parser.add_argument("--dry-run", action="store_true", help="print the new value only")
    parser.add_argument("-p", "--package", help="test only this crate")
    args = parser.parse_args(argv)
    root = repo_root(args.file.resolve().parent)
    if runner is run_test:
        runner = lambda root, test: run_test(root, test, args.package)  # noqa: E731

    passed, output = runner(root, args.test)
    if passed:
        print(f"{args.test} already passes; nothing to bless")
        return 0
    try:
        value = left_value(output)
        updated = replace_const(args.file.read_text(), args.const, value)
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    if args.dry_run:
        print(value)
        return 0
    args.file.write_text(updated)
    format_rust(args.file)
    passed, output = runner(root, args.test)
    if not passed:
        print(output[-2000:] + f"\nerror: {args.test} still fails after blessing", file=sys.stderr)
        return 1
    print(f"blessed {args.const} in {args.file}; {args.test} passes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
