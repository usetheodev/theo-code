#!/usr/bin/env python3
"""Extract `#[cfg(test)] mod tests { ... }` to a sibling `<file>_tests.rs` file.

Usage:
    scripts/extract-tests-to-sibling.py <path/to/file.rs>

The script:
  1. Finds the FIRST `#[cfg(test)]` (or `#[cfg(all(test, ...))]`) attribute
     followed by a `mod tests {` block.
  2. Extracts the entire balanced-brace body of that mod into a sibling
     file `<basename>_tests.rs`.
  3. Replaces the inline mod in the original file with:
       #[cfg(test)]
       #[path = "<basename>_tests.rs"]
       mod tests;

  4. Idempotent: re-running on an already-extracted file is a no-op
     (detects the `#[path = ".._tests.rs"]` form and exits 0).

T0.2 of docs/plans/god-files-2026-07-23-plan.md.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


CFG_TEST_LINE = re.compile(r'^\s*#\[cfg\((all\([^)]*\b)?test\b')
PATH_FORM = re.compile(r'^\s*#\[path\s*=\s*"[^"]+_tests\.rs"\]\s*$')
MOD_TESTS_OPEN = re.compile(r'^\s*(pub\s+)?mod\s+tests\b\s*\{?\s*$')


def line_inside_raw_string(lines: list[str], idx: int) -> bool:
    """Return True if the start of `lines[idx]` falls inside an unterminated raw string.

    Walks every line before `idx`, tracking open `r#"..."#` (or `r"..."`)
    raw strings. If the open count never returns to 0 by the start of `idx`,
    `idx` is inside a raw string.
    """
    inside_hash_count = -1  # -1 means "not inside"; otherwise the # count of the open
    for k in range(idx):
        line = lines[k]
        i = 0
        n = len(line)
        while i < n:
            if inside_hash_count >= 0:
                # Look for the closing `"` followed by hash_count `#`s.
                terminator = '"' + '#' * inside_hash_count
                end = line.find(terminator, i)
                if end == -1:
                    break  # still inside; next line continues
                i = end + len(terminator)
                inside_hash_count = -1
                continue
            # Not inside a raw string: scan for `r#`/`r"` start.
            c = line[i]
            if c == 'r' and i + 1 < n and (line[i + 1] == '"' or line[i + 1] == '#'):
                j = i + 1
                hash_count = 0
                while j < n and line[j] == '#':
                    hash_count += 1
                    j += 1
                if j < n and line[j] == '"':
                    # Open raw string. Find single-line termination first.
                    terminator = '"' + '#' * hash_count
                    end = line.find(terminator, j + 1)
                    if end == -1:
                        inside_hash_count = hash_count
                        break  # multi-line raw string
                    i = end + len(terminator)
                    continue
            # Skip line comments
            if c == '/' and i + 1 < n and line[i + 1] == '/':
                break
            # Skip regular string literals
            if c == '"':
                j = i + 1
                while j < n:
                    if line[j] == '\\' and j + 1 < n:
                        j += 2
                        continue
                    if line[j] == '"':
                        break
                    j += 1
                i = j + 1
                continue
            i += 1
    return inside_hash_count >= 0


def find_extraction_target(lines: list[str]) -> tuple[int, int] | None:
    """Return (start_idx, end_idx) of the inline `#[cfg(test)] mod tests { ... }`.

    `start_idx` points at the `#[cfg(test)]` line; `end_idx` is the
    matching closing-brace line index (inclusive). Returns None if
    the file doesn't contain an inline test mod or already uses the
    `#[path = "..."]` form.
    """
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        if CFG_TEST_LINE.match(line) and not line_inside_raw_string(lines, i):
            # Check next 2 lines for either the #[path = "..._tests.rs"] form
            # (already extracted) or `mod tests {` opening (inline).
            j = i + 1
            # Skip nested attributes between cfg(test) and mod
            while j < n and lines[j].strip().startswith('#['):
                if PATH_FORM.match(lines[j]):
                    return None  # already extracted
                j += 1
            if j < n and MOD_TESTS_OPEN.match(lines[j]):
                # Find matching close brace (balanced).
                start_idx = i
                depth = 0
                k = j
                # Account for the possibility of `mod tests {` ending the line.
                if '{' in lines[j]:
                    depth = 1
                    k = j + 1
                # Skip-aware brace counter that ignores braces inside string
                # literals (regular and raw).
                while k < n and depth > 0:
                    depth = update_depth_for_line(lines[k], depth)
                    if depth == 0:
                        return (start_idx, k)
                    k += 1
                return None
        i += 1
    return None


def update_depth_for_line(line: str, depth: int) -> int:
    """Return the new depth after consuming `line`, ignoring braces in string literals."""
    i = 0
    n = len(line)
    while i < n:
        c = line[i]
        # Raw string `r#"..."#`, `r##"..."##`, `r"..."`
        if c == 'r' and i + 1 < n and (line[i + 1] == '"' or line[i + 1] == '#'):
            j = i + 1
            hash_count = 0
            while j < n and line[j] == '#':
                hash_count += 1
                j += 1
            if j < n and line[j] == '"':
                # Find closing `"` followed by `hash_count` `#`s.
                terminator = '"' + '#' * hash_count
                end = line.find(terminator, j + 1)
                if end == -1:
                    # Multi-line raw string; treat the rest of the line as inside the string.
                    return depth
                i = end + len(terminator)
                continue
        # Regular string literal "..."
        if c == '"':
            j = i + 1
            while j < n:
                if line[j] == '\\' and j + 1 < n:
                    j += 2
                    continue
                if line[j] == '"':
                    break
                j += 1
            i = j + 1
            continue
        # Line comment // — consume to EOL
        if c == '/' and i + 1 < n and line[i + 1] == '/':
            return depth
        if c == '{':
            depth += 1
        elif c == '}':
            depth -= 1
            if depth == 0:
                return 0
        i += 1
    return depth


def extract(path: Path) -> bool:
    """Returns True if extraction happened, False if no-op (already extracted or no test mod)."""
    if not path.exists():
        print(f"file not found: {path}", file=sys.stderr)
        sys.exit(2)
    if path.suffix != '.rs':
        print(f"not a Rust file: {path}", file=sys.stderr)
        sys.exit(2)

    text = path.read_text()
    lines = text.splitlines(keepends=True)

    target = find_extraction_target(lines)
    if target is None:
        print(f"{path}: no extraction needed (already extracted or no inline test mod)")
        return False

    start, end = target

    # Carve out the test mod body (from `#[cfg(test)]` through the closing brace).
    sibling = path.with_name(path.stem + '_tests.rs')
    if sibling.exists():
        print(f"sibling file already exists: {sibling} — refusing to overwrite", file=sys.stderr)
        sys.exit(2)

    # Find where the actual `mod tests {` body starts after the cfg(test) attribute.
    # We want to write the *contents* of the mod into the sibling — header + body.
    # Approach: copy lines [start..end] verbatim into the sibling file, prefixed
    # with a generated header explaining the source.
    block = ''.join(lines[start : end + 1])

    # Strip the outer `#[cfg(test)] mod tests {` and the closing `}` so the
    # sibling file has a flat `use ...` + `#[test] fn ...` body.
    # The sibling is included via #[path = "..."] mod tests; — so the body
    # at file-scope IS the contents of mod tests, NOT wrapped in `mod tests`.
    body_lines = lines[start : end + 1]

    # Find the line with `mod tests {` and replace the prefix (cfg attr + mod
    # tests { line) with nothing. Replace the trailing `}` line with nothing.
    mod_line_idx = None
    for off, l in enumerate(body_lines):
        if MOD_TESTS_OPEN.match(l):
            mod_line_idx = off
            break
    if mod_line_idx is None:
        print(f"could not find `mod tests` opener in extraction window", file=sys.stderr)
        sys.exit(2)

    # Body is everything STRICTLY INSIDE the mod tests { … } block.
    # If `mod tests {` is on its own line, body starts at mod_line_idx + 1.
    inner_start = mod_line_idx + 1
    inner_end = len(body_lines) - 1  # the closing `}` line
    inner_body = ''.join(body_lines[inner_start:inner_end])

    # Write the sibling file.
    sibling_header = (
        f"//! Sibling test body of `{path.name}`.\n"
        f"//! Extracted by `scripts/extract-tests-to-sibling.py` "
        f"(T0.2 of docs/plans/god-files-2026-07-23-plan.md).\n"
        f"//! Included from `{path.name}` via `#[path = \"{sibling.name}\"] mod tests;`.\n"
        f"//!\n"
        f"//! Do not edit the path attribute — it is what keeps this file linked.\n\n"
    )
    sibling.write_text(sibling_header + inner_body)

    # Rewrite the original: replace lines[start:end+1] with the path-form import.
    indent_match = re.match(r'^(\s*)', lines[start])
    indent = indent_match.group(1) if indent_match else ''
    replacement = (
        f"{indent}#[cfg(test)]\n"
        f"{indent}#[path = \"{sibling.name}\"]\n"
        f"{indent}mod tests;\n"
    )
    new_lines = lines[:start] + [replacement] + lines[end + 1 :]
    path.write_text(''.join(new_lines))

    print(f"{path}: extracted {(end - start + 1)} lines to {sibling}")
    return True


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    target = Path(sys.argv[1])
    extract(target)
    return 0


if __name__ == '__main__':
    sys.exit(main())
