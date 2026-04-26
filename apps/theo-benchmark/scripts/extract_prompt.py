#!/usr/bin/env python3
"""Phase 53 (prompt-ab-testing-plan) — extract the SOTA prompt literals from
the Rust source so the 3 markdown variants stay in lockstep with the binary.

Reads:
  crates/theo-agent-runtime/src/config.rs  → `default_system_prompt()` body
  apps/theo-cli/src/main.rs                → `BENCHMARK_CONTEXT_NOTE` literal

Writes:
  apps/theo-benchmark/prompts/sota.md          (default + bench addendum)
  apps/theo-benchmark/prompts/sota-no-bench.md (default only)
  apps/theo-benchmark/prompts/sota-lean.md     (NOT generated — hand-trimmed)

Run:
  python3 apps/theo-benchmark/scripts/extract_prompt.py

Idempotent. Re-run after every prompt edit in the Rust code; commit the diff.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CONFIG_RS = ROOT / "crates" / "theo-agent-runtime" / "src" / "config.rs"
MAIN_RS = ROOT / "apps" / "theo-cli" / "src" / "main.rs"
OUT_DIR = ROOT / "apps" / "theo-benchmark" / "prompts"


def extract_default_prompt(rust_src: str) -> str:
    """Pull the body of the r#"..."# literal returned by default_system_prompt()."""
    # Match the function body up to the closing `}` (one literal only).
    m = re.search(
        r'fn default_system_prompt\(\)[^{]*\{[^"]*r#"(.*?)"#\s*\n?\}',
        rust_src,
        re.DOTALL,
    )
    if not m:
        raise SystemExit("could not locate default_system_prompt() body")
    return m.group(1)


def extract_benchmark_addendum(main_src: str) -> str:
    """Pull the BENCHMARK_CONTEXT_NOTE string literal."""
    m = re.search(
        r'const BENCHMARK_CONTEXT_NOTE: &str = "(.*?)";',
        main_src,
        re.DOTALL,
    )
    if not m:
        raise SystemExit("could not locate BENCHMARK_CONTEXT_NOTE literal")
    raw = m.group(1)
    # Rust string literal: \n → newline, \" → ", \\ → \
    return (
        raw.replace("\\n", "\n")
        .replace('\\"', '"')
        .replace("\\\\", "\\")
    )


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    config_src = CONFIG_RS.read_text()
    main_src = MAIN_RS.read_text()

    default_prompt = extract_default_prompt(config_src)
    addendum = extract_benchmark_addendum(main_src)

    # Strip backslash continuations that Rust uses for line wrapping
    # (within `"..."` strings, `\<newline>whitespace` is elided).
    addendum = re.sub(r"\\\n\s*", "", addendum)

    sota_no_bench = default_prompt
    sota = default_prompt + addendum

    (OUT_DIR / "sota.md").write_text(sota)
    (OUT_DIR / "sota-no-bench.md").write_text(sota_no_bench)

    print(f"[extract_prompt] sota.md           {len(sota):>5} chars (~{len(sota)//4} tokens)")
    print(f"[extract_prompt] sota-no-bench.md  {len(sota_no_bench):>5} chars (~{len(sota_no_bench)//4} tokens)")

    if not (OUT_DIR / "sota-lean.md").is_file():
        print("[extract_prompt] WARN: sota-lean.md not present — hand-write it")
    else:
        lean = (OUT_DIR / "sota-lean.md").read_text()
        print(f"[extract_prompt] sota-lean.md     {len(lean):>5} chars (~{len(lean)//4} tokens) [hand-trimmed]")

    return 0


if __name__ == "__main__":
    sys.exit(main())
