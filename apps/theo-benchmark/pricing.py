"""
Pricing helper — Phase 46 (benchmark-validation-plan).

Loads model price table from pricing.toml and computes cost_usd
per (input_tokens, output_tokens, model_id). Used by post_run analysis
to attach a cost dimension to every benchmark report row.

Contract:
  compute_cost(tokens_in: int, tokens_out: int, model: str) -> float
    returns USD as float, 0.0 when model unknown (with stderr warning).

  load_table(path: str | None = None) -> dict
    returns the full table; useful for tests and dashboards.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Optional

try:
    import tomllib  # py 3.11+
except ImportError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


_DEFAULT_TABLE_PATH = Path(__file__).parent / "pricing.toml"
_FALLBACK_KEY = "__fallback__"


def load_table(path: Optional[Path] = None) -> dict:
    """Load the pricing table from disk."""
    src = path or _DEFAULT_TABLE_PATH
    with open(src, "rb") as f:
        data = tomllib.load(f)
    return data.get("models", {})


def compute_cost(tokens_in: int, tokens_out: int, model: str,
                 table: Optional[dict] = None) -> float:
    """Compute USD cost from input/output tokens for `model`.

    Returns 0.0 when model is not in the table (a stderr warning is emitted
    once per unknown model so test runs surface missing entries).
    """
    if tokens_in < 0 or tokens_out < 0:
        raise ValueError("token counts must be non-negative")
    t = table if table is not None else load_table()
    entry = t.get(model)
    if entry is None:
        # Warn-once via a module-level set
        _warn_unknown(model)
        entry = t.get(_FALLBACK_KEY, {"input_per_mtok": 0.0, "output_per_mtok": 0.0})
    input_cost = (tokens_in / 1_000_000.0) * float(entry["input_per_mtok"])
    output_cost = (tokens_out / 1_000_000.0) * float(entry["output_per_mtok"])
    return round(input_cost + output_cost, 6)


_WARNED: set[str] = set()


def _warn_unknown(model: str) -> None:
    if model in _WARNED:
        return
    _WARNED.add(model)
    print(
        f"[pricing] WARN: model '{model}' not in pricing.toml — cost reported as 0",
        file=sys.stderr,
    )
