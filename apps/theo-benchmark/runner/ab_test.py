"""Phase 55 (prompt-ab-testing-plan) — A/B test orchestrator.

Executes the same N Terminal-Bench tasks across multiple prompt variants,
producing paired data for statistical comparison.

Usage on the droplet:
    python3 runner/ab_test.py \\
      --variants sota,sota-lean,sota-no-bench \\
      --n-tasks 20 \\
      --dataset terminal-bench-core==0.1.1 \\
      --output-dir reports/2026-04-24/ab

Output structure:
    <output-dir>/
      manifest.json                     # variants, task IDs, theo SHA, model
      <variant-1>/raw/                  # tb run --output-path
      <variant-2>/raw/
      ...

Determinism (D4): we sort all task IDs alphabetically and take the first N.
Same set across variants → paired comparison is valid.

Provenance pin (D3): manifest captures model + theo SHA + dataset + start
time so a future analyzer can verify the runs are comparable.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


def select_tasks_alphabetically(task_ids, n: int):
    """Return the first N task IDs sorted alphabetically (deterministic).

    `task_ids` may be a list or any iterable. N=0 returns empty list. N
    larger than available returns all available tasks (no error)."""
    sorted_ids = sorted(task_ids)
    if n < 0:
        raise ValueError(f"n must be >= 0, got {n}")
    return sorted_ids[:n]


def parse_dataset_spec(dataset_spec: str) -> tuple[str, str | None]:
    """Split `name==version` into (name, version). Returns (spec, None) when
    no `==` is present (caller may treat it as a path or raise)."""
    if "==" in dataset_spec:
        name, version = dataset_spec.split("==", 1)
        return name.strip(), version.strip()
    return dataset_spec.strip(), None


def list_dataset_tasks(dataset_spec: str) -> list[str]:
    """Enumerate task IDs in a tb dataset by spawning a small Python helper.

    Imports terminal_bench.dataset.Dataset on the droplet. Returns a sorted
    list of task IDs. Raises RuntimeError if terminal_bench isn't importable
    or the dataset spec cannot be resolved.
    """
    name, version = parse_dataset_spec(dataset_spec)
    if not version:
        raise ValueError(
            f"dataset spec must be 'name==version', got {dataset_spec!r}"
        )
    helper = (
        "from terminal_bench.dataset.dataset import Dataset; "
        f"d = Dataset(name={name!r}, version={version!r}); "
        "print('\\n'.join(sorted(d.task_ids)))"
    )
    try:
        out = subprocess.check_output(
            [sys.executable, "-c", helper],
            stderr=subprocess.STDOUT,
            timeout=60,
        )
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            f"failed to enumerate tasks for {dataset_spec!r}: {e.output.decode(errors='replace')}"
        ) from e
    return [line for line in out.decode().strip().splitlines() if line]


def write_manifest(
    out_dir: Path,
    *,
    variants: list[str],
    task_ids: list[str],
    theo_sha: str,
    model: str,
    dataset: str,
    started_at: str,
) -> Path:
    """Write manifest.json with full provenance pin."""
    manifest = {
        "schema": "ab.manifest.v1",
        "variants": list(variants),
        "task_ids": list(task_ids),
        "n_tasks": len(task_ids),
        "theo_sha": theo_sha,
        "model": model,
        "dataset": dataset,
        "started_at": started_at,
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n")
    return path


def build_tb_command(
    *,
    tb_bin: str,
    dataset: str,
    output_path: Path,
    task_ids: list[str],
    n_concurrent: int = 4,
) -> list[str]:
    """Construct the tb invocation. Pure function — easy to unit test."""
    cmd = [
        tb_bin,
        "run",
        "--dataset", dataset,
        "--agent-import-path", "tbench.agent:TheoAgent",
        "--n-concurrent", str(n_concurrent),
        "--output-path", str(output_path),
        "--no-upload-results",
    ]
    for tid in task_ids:
        cmd.extend(["--task-id", tid])
    return cmd


def _git_sha_short() -> str:
    try:
        out = subprocess.check_output(
            ["git", "-C", str(Path(__file__).resolve().parents[3]),
             "rev-parse", "--short", "HEAD"],
            stderr=subprocess.DEVNULL,
            timeout=5,
        )
        return out.decode().strip()
    except Exception:
        return "unknown"


def run_variant(
    variant: str,
    task_ids: list[str],
    output_dir: Path,
    *,
    tb_bin: str,
    dataset: str,
    n_concurrent: int,
    extra_env: dict | None = None,
) -> int:
    """Spawn tb for one variant. Returns process exit code."""
    raw_dir = output_dir / variant / "raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["THEO_PROMPT_VARIANT"] = variant
    if extra_env:
        env.update(extra_env)
    cmd = build_tb_command(
        tb_bin=tb_bin,
        dataset=dataset,
        output_path=raw_dir,
        task_ids=task_ids,
        n_concurrent=n_concurrent,
    )
    log_path = output_dir / variant / "run.log"
    print(f"[ab_test] variant={variant} tasks={len(task_ids)} → {raw_dir}", flush=True)
    with log_path.open("w") as log:
        return subprocess.call(cmd, env=env, stdout=log, stderr=subprocess.STDOUT)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Phase 55 — A/B prompt orchestrator")
    parser.add_argument("--variants", required=True,
                        help="comma-separated list, e.g. sota,sota-lean,sota-no-bench")
    parser.add_argument("--n-tasks", type=int, default=20,
                        help="number of tasks (alphabetical); default 20")
    parser.add_argument("--dataset", default="terminal-bench-core==0.1.1")
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--tb-bin", default="tb")
    parser.add_argument("--n-concurrent", type=int, default=4)
    parser.add_argument("--task-ids-file", type=Path,
                        help="optional: read task IDs from file (one per line) "
                             "instead of enumerating dataset")
    parser.add_argument("--dry-run", action="store_true",
                        help="write manifest + print commands, don't invoke tb")
    args = parser.parse_args(argv)

    variants = [v.strip() for v in args.variants.split(",") if v.strip()]
    if len(variants) < 2:
        parser.error("--variants needs at least 2 entries for an A/B run")

    if args.task_ids_file:
        all_ids = [line.strip() for line in args.task_ids_file.read_text().splitlines() if line.strip()]
    else:
        all_ids = list_dataset_tasks(args.dataset)

    selected = select_tasks_alphabetically(all_ids, args.n_tasks)
    if not selected:
        print(f"[ab_test] ERROR: no tasks selected (n_tasks={args.n_tasks}, available={len(all_ids)})",
              file=sys.stderr)
        return 2

    started_at = datetime.now(timezone.utc).isoformat()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    write_manifest(
        args.output_dir,
        variants=variants,
        task_ids=selected,
        theo_sha=_git_sha_short(),
        model=os.environ.get("THEO_MODEL", "gpt-5.4"),
        dataset=args.dataset,
        started_at=started_at,
    )

    if args.dry_run:
        for v in variants:
            cmd = build_tb_command(
                tb_bin=args.tb_bin,
                dataset=args.dataset,
                output_path=args.output_dir / v / "raw",
                task_ids=selected,
                n_concurrent=args.n_concurrent,
            )
            print(f"[dry-run] {v}: {' '.join(cmd)}")
        return 0

    rc_total = 0
    for v in variants:
        rc = run_variant(
            v, selected, args.output_dir,
            tb_bin=args.tb_bin,
            dataset=args.dataset,
            n_concurrent=args.n_concurrent,
        )
        if rc != 0:
            print(f"[ab_test] WARN: variant {v} returned exit code {rc}", file=sys.stderr)
            rc_total = rc

    return rc_total


if __name__ == "__main__":
    sys.exit(main())
