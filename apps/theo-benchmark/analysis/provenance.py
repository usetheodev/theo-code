"""
Provenance Collection — benchmark-sota-metrics-plan.

Captures environment metadata for reproducibility: git SHA, model,
provider, pricing table hash, hostname, Python version, timestamps.

Every benchmark report should include provenance so results can be
traced back to the exact code + config that produced them.
"""

from __future__ import annotations

import hashlib
import socket
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def collect_provenance(
    theo_dir: str | Path,
    model: str = "",
    provider: str = "",
    temperature: float = 0.0,
    max_iter: int = 30,
) -> dict:
    """Collect environment provenance for a benchmark run.

    Args:
        theo_dir: path to the theo-code repository root (used for git SHA).
        model: model identifier used for the run.
        provider: LLM provider used for the run.
        temperature: sampling temperature.
        max_iter: maximum iterations per task.

    Returns:
        Plain dict with all provenance fields.
    """
    theo_path = Path(theo_dir)

    return {
        "theo_sha": _git_sha(theo_path),
        "theo_version": _theo_version(),
        "model": model,
        "provider": provider,
        "temperature": temperature,
        "max_iter": max_iter,
        "pricing_toml_sha": _pricing_toml_sha(theo_path),
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "hostname": socket.gethostname(),
        "python_version": sys.version,
    }


def _git_sha(repo_dir: Path) -> str:
    """Get the current git HEAD SHA for the repository."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=str(repo_dir),
            capture_output=True,
            text=True,
            timeout=10,
        )
        if result.returncode == 0:
            return result.stdout.strip()
    except Exception:
        pass
    return "unknown"


def _theo_version() -> str:
    """Read theo version from environment or return unknown."""
    import os

    return os.environ.get("THEO_VERSION", "unknown")


def _pricing_toml_sha(repo_dir: Path) -> str:
    """Compute SHA-256 of the pricing.toml file for traceability."""
    # Try common locations
    candidates = [
        repo_dir / "apps" / "theo-benchmark" / "pricing.toml",
        repo_dir / "pricing.toml",
        Path(__file__).resolve().parent.parent / "pricing.toml",
    ]
    for path in candidates:
        if path.exists():
            try:
                content = path.read_bytes()
                return hashlib.sha256(content).hexdigest()
            except Exception:
                continue
    return "not_found"
