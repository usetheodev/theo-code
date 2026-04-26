"""
Continuous bench-run telemetry monitor — Phase 47.

Polls a tb run output directory + Docker state at fixed intervals and
writes one JSONL line per snapshot. Used to characterize:
  - completion rate (tasks/min over time)
  - container parallelism over time
  - per-task wall-clock distribution
  - host pressure (CPU, RAM, disk) during the run
  - Docker layer of the harness (when does each task start/finish)

Output: <output-jsonl> (append-only)

Run on the droplet:
  python3 runner/monitor.py \\
    --raw-dir /opt/theo-code/apps/theo-benchmark/reports/<date>/tbench-core/raw \\
    --interval-sec 30 \\
    --output /opt/theo-code/apps/theo-benchmark/reports/<date>/tbench-core/monitor.jsonl

Stop with Ctrl-C; final line is written on exit.
"""

from __future__ import annotations

import argparse
import json
import shutil
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def docker_ps() -> list[dict]:
    """Return list of running containers (name, image, status, started)."""
    try:
        out = subprocess.check_output(
            ["docker", "ps", "--format",
             "{{.Names}}|{{.Image}}|{{.Status}}|{{.RunningFor}}"],
            timeout=10,
        ).decode()
    except Exception as e:
        return [{"_error": f"docker_ps_failed: {e}"}]
    rows: list[dict] = []
    for line in out.splitlines():
        parts = line.split("|", 3)
        if len(parts) == 4:
            rows.append({
                "name": parts[0],
                "image": parts[1],
                "status": parts[2],
                "running_for": parts[3],
            })
    return rows


def docker_stats() -> list[dict]:
    """Return per-container CPU/memory snapshot."""
    try:
        out = subprocess.check_output(
            ["docker", "stats", "--no-stream", "--format",
             "{{.Name}}|{{.CPUPerc}}|{{.MemUsage}}|{{.MemPerc}}"],
            timeout=15,
        ).decode()
    except Exception as e:
        return [{"_error": f"docker_stats_failed: {e}"}]
    rows: list[dict] = []
    for line in out.splitlines():
        parts = line.split("|")
        if len(parts) == 4:
            rows.append({
                "name": parts[0],
                "cpu_pct": parts[1],
                "mem_usage": parts[2],
                "mem_pct": parts[3],
            })
    return rows


def host_pressure() -> dict:
    """Capture host-level CPU + memory + disk usage."""
    out = {}
    # /proc/loadavg
    try:
        la = Path("/proc/loadavg").read_text().split()[:3]
        out["loadavg"] = [float(x) for x in la]
    except Exception:
        pass
    # /proc/meminfo
    try:
        info = {}
        for line in Path("/proc/meminfo").read_text().splitlines():
            k, _, v = line.partition(":")
            v = v.strip().split()
            if v:
                info[k.strip()] = int(v[0])  # in kB
        out["mem_total_mb"] = info.get("MemTotal", 0) // 1024
        out["mem_available_mb"] = info.get("MemAvailable", 0) // 1024
    except Exception:
        pass
    # disk usage of /
    try:
        s = shutil.disk_usage("/")
        out["disk_total_gb"] = round(s.total / (1024**3), 1)
        out["disk_used_gb"] = round((s.total - s.free) / (1024**3), 1)
        out["disk_pct"] = round((s.total - s.free) * 100 / s.total, 1)
    except Exception:
        pass
    return out


def scan_results(raw_dir: Path) -> dict:
    """Walk the raw_dir and collect per-task verdicts.

    tb writes one results.json per trial AND an aggregate at the run root.
    We count completed tasks + failure modes from per-trial files (more
    granular than the master).
    """
    completed: list[dict] = []
    if not raw_dir.exists():
        return {"completed_count": 0, "tasks": []}
    for results_file in raw_dir.rglob("results.json"):
        # Skip the master aggregate (lives at <raw>/<run>/results.json)
        if results_file.parent.parent == raw_dir:
            continue
        try:
            d = json.loads(results_file.read_text())
        except Exception:
            continue
        # Per-trial schema
        completed.append({
            "task_id": d.get("task_id", "?"),
            "trial": d.get("trial_name", "?"),
            "resolved": bool(d.get("is_resolved")),
            "failure_mode": d.get("failure_mode", "unknown"),
            "agent_started_at": d.get("agent_started_at"),
            "agent_ended_at": d.get("agent_ended_at"),
            "trial_started_at": d.get("trial_started_at"),
            "trial_ended_at": d.get("trial_ended_at"),
            "input_tokens": int(d.get("total_input_tokens", 0) or 0),
            "output_tokens": int(d.get("total_output_tokens", 0) or 0),
        })
    completed.sort(key=lambda r: r.get("trial_ended_at") or "")
    return {
        "completed_count": len(completed),
        "resolved_count": sum(1 for r in completed if r["resolved"]),
        "tasks": completed,
    }


_running = True


def _graceful_stop(signum, frame) -> None:
    global _running
    _running = False
    print(f"\n[monitor] received signal {signum} — stopping after current poll", file=sys.stderr)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw-dir", required=True, type=Path,
                    help="tb output --output-path target")
    ap.add_argument("--output", required=True, type=Path,
                    help="JSONL file for snapshots (append-only)")
    ap.add_argument("--interval-sec", type=int, default=30)
    args = ap.parse_args(argv)

    args.output.parent.mkdir(parents=True, exist_ok=True)

    signal.signal(signal.SIGTERM, _graceful_stop)
    signal.signal(signal.SIGINT, _graceful_stop)

    print(f"[monitor] starting; interval={args.interval_sec}s; output={args.output}",
          file=sys.stderr)

    snapshot_n = 0
    while _running:
        snapshot_n += 1
        snap = {
            "snapshot": snapshot_n,
            "ts": _now(),
            "host": host_pressure(),
            "containers": docker_ps(),
            "stats": docker_stats(),
            "results": scan_results(args.raw_dir),
        }
        with args.output.open("a") as fp:
            fp.write(json.dumps(snap) + "\n")
        rs = snap["results"]
        c = len(snap["containers"])
        print(
            f"[monitor] snap#{snapshot_n} {snap['ts']} "
            f"completed={rs.get('completed_count', 0)} "
            f"resolved={rs.get('resolved_count', 0)} "
            f"containers={c} "
            f"load={snap['host'].get('loadavg', [0])[0]:.2f}",
            file=sys.stderr,
            flush=True,
        )
        # Sleep but be responsive to signals
        for _ in range(args.interval_sec):
            if not _running:
                break
            time.sleep(1)

    print(f"[monitor] stopped after {snapshot_n} snapshots", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
