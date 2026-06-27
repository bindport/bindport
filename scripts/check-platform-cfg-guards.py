#!/usr/bin/env python3
"""Catch Linux-only cfg patterns that Linux clippy cannot see."""

from pathlib import Path
import re
import sys

LET_RE = re.compile(r"^\s*let\s+[A-Za-z_][A-Za-z0-9_]*\b")
LINUX_CFG_RE = re.compile(r'^\s*#\[cfg\(\s*target_os\s*=\s*"linux"\s*\)\]')


def rust_files() -> list[Path]:
    return sorted(Path("crates").glob("**/*.rs"))


def next_significant_line(lines: list[str], index: int) -> int | None:
    for current in range(index, len(lines)):
        stripped = lines[current].strip()
        if stripped and not stripped.startswith("//"):
            return current
    return None


def statement_end(lines: list[str], index: int) -> int:
    current = index
    while current < len(lines):
        if ";" in lines[current]:
            return current
        current += 1
    return index


def main() -> int:
    failures: list[str] = []

    for path in rust_files():
        lines = path.read_text().splitlines()
        for index, line in enumerate(lines):
            if not LET_RE.match(line):
                continue

            cfg_index = next_significant_line(lines, statement_end(lines, index) + 1)
            if cfg_index is None or not LINUX_CFG_RE.match(lines[cfg_index]):
                continue

            failures.append(f"{path}:{index + 1}")

    if not failures:
        return 0

    print(
        "Linux-only cfg guard found declarations immediately before "
        "#[cfg(target_os = \"linux\")] blocks.",
        file=sys.stderr,
    )
    print(
        "Move declarations used only by Linux cfg blocks inside the cfg block. "
        "Linux clippy treats them as used, but macOS clippy reports them as unused.",
        file=sys.stderr,
    )
    for failure in failures:
        print(f"  {failure}", file=sys.stderr)

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
