#!/usr/bin/env python3
"""Measures how much larger each compiled example is than its source.

Usage:
    python3 scripts/measure_examples.py [src_dir] [out_dir]

Directories default to `examples/` and `build/` at the repository root, so a
bare invocation works after `luaux build`.

Comments and blank lines are stripped from both sides before counting, so the
numbers describe the code you type. Comments pass through the compiler
byte-for-byte and would only pull every ratio toward zero. Indentation and
spacing are kept: they are part of what is written and read, and both sides
are measured the same way. Newlines count as one character regardless of the
platform's line endings.

Stripping is regex-based, not a real lexer: a `--` or `<!--` inside a string
literal would be over-stripped. The examples contain no such strings, and both
sides of each pair are stripped identically, so the comparison stays fair.
"""

import re
import sys
from pathlib import Path

BLOCK_COMMENT = re.compile(r"--\[(=*)\[.*?\]\1\]", re.S)
MARKUP_COMMENT = re.compile(r"<!--.*?-->", re.S)
LINE_COMMENT = re.compile(r"^[ \t]*--.*$", re.M)
BLANK_LINES = re.compile(r"\n\s*\n+")


def code_size(text: str) -> int:
    """Characters of code once comments and blank lines are gone.

    Block comments go first: `--[[` would otherwise satisfy the line-comment
    pattern and strip only its opening line. Collapsing blank lines afterwards
    keeps a stripped comment from leaving its blank line behind to be counted.
    """
    text = BLOCK_COMMENT.sub("", text)
    text = MARKUP_COMMENT.sub("", text)
    text = LINE_COMMENT.sub("", text)
    return len(BLANK_LINES.sub("\n", text).strip())


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    src_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "examples"
    out_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "build"

    sources = sorted(src_dir.glob("*.luaux"))
    if not sources:
        print(f"no .luaux files in {src_dir}", file=sys.stderr)
        return 1

    rows = []
    for source in sources:
        compiled = out_dir / source.with_suffix(".luau").name
        if not compiled.is_file():
            print(f"skipped {source.name}: no {compiled} (run `luaux build`)", file=sys.stderr)
            continue

        src = code_size(source.read_text(encoding="utf-8"))
        out = code_size(compiled.read_text(encoding="utf-8"))
        rows.append((source.stem, src, out))

    if not rows:
        return 1

    rows.append(("total", sum(r[1] for r in rows), sum(r[2] for r in rows)))

    width = max(len(name) for name, _, _ in rows)
    print(f"{'':{width}}  {'source':>8}  {'compiled':>8}  {'change':>7}")

    for name, src, out in rows:
        change = (out - src) * 100.0 / src
        print(f"{name:{width}}  {src:8}  {out:8}  {change:+6.1f}%")

    return 0


if __name__ == "__main__":
    sys.exit(main())
