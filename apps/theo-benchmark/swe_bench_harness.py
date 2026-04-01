#!/usr/bin/env python3
"""
SWE-bench Lite Evaluation Harness for Theo Code Agent.

Downloads the SWE-bench Lite dataset (300 tasks) from HuggingFace and evaluates
the Theo Code autonomous agent on each task. Each task is run in an isolated
subprocess with its own cloned repo, ensuring clean state.

Usage:
    # Run all 300 tasks
    python3 swe_bench_harness.py

    # Run first 5 tasks (for testing)
    python3 swe_bench_harness.py --limit 5

    # Run only Django tasks
    python3 swe_bench_harness.py --filter django

    # Resume a previous run (skip completed tasks)
    python3 swe_bench_harness.py --resume

    # Custom timeout per task
    python3 swe_bench_harness.py --timeout 900

Environment:
    VLLM_URL=http://localhost:8000       (vLLM server)
    MODEL_NAME=...                        (model to use)
    THEO_CODE_BIN=./theo-code            (binary for context engineering)
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

VLLM_URL = os.environ.get("VLLM_URL", "http://localhost:8000")
MODEL_NAME = os.environ.get(
    "MODEL_NAME", "Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8"
)
DEFAULT_TIMEOUT = 600  # seconds per task
RESULTS_DIR = Path(__file__).parent / "swe_bench_results"
RESULTS_FILE = RESULTS_DIR / "results.json"
WORKDIR = Path(tempfile.gettempdir()) / "swe_bench_theo"
AGENT_SCRIPT = Path(__file__).parent / "theo_agent.py"

HF_DATASET = "princeton-nlp/SWE-bench_Lite"

# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------


@dataclass
class TaskResult:
    instance_id: str
    repo: str
    success: bool
    time_seconds: float
    error: Optional[str] = None
    agent_exit_code: int = -1
    tests_passed: int = 0
    tests_failed: int = 0
    tests_error: int = 0
    patch_applied: bool = False


@dataclass
class AggregateResults:
    total: int
    passed: int
    failed: int
    errored: int
    pass_rate: float
    avg_time: float
    total_time: float


# ---------------------------------------------------------------------------
# Dataset loading
# ---------------------------------------------------------------------------


def load_dataset() -> list[dict]:
    """Load SWE-bench Lite from HuggingFace using the datasets library."""
    try:
        from datasets import load_dataset as hf_load
    except ImportError:
        print("ERROR: 'datasets' package not installed.")
        print("Install with: pip install datasets")
        sys.exit(1)

    print(f"Loading dataset: {HF_DATASET}")
    ds = hf_load(HF_DATASET, split="test")
    tasks = list(ds)
    print(f"Loaded {len(tasks)} tasks from SWE-bench Lite")
    return tasks


def filter_tasks(
    tasks: list[dict],
    repo_filter: Optional[str] = None,
    limit: Optional[int] = None,
    completed_ids: Optional[set] = None,
) -> list[dict]:
    """Apply filters to the task list."""
    if repo_filter:
        tasks = [t for t in tasks if repo_filter.lower() in t["repo"].lower()]
        print(f"After repo filter '{repo_filter}': {len(tasks)} tasks")

    if completed_ids:
        tasks = [t for t in tasks if t["instance_id"] not in completed_ids]
        print(f"After skipping completed: {len(tasks)} tasks remaining")

    if limit is not None and limit > 0:
        tasks = tasks[:limit]
        print(f"After limit: {len(tasks)} tasks")

    return tasks


# ---------------------------------------------------------------------------
# Repo management
# ---------------------------------------------------------------------------


def clone_repo(repo: str, base_commit: str, work_dir: Path) -> Path:
    """Clone a repo at a specific commit into work_dir.

    Uses a shallow clone strategy: clone at HEAD then checkout the target commit.
    This is slower than a full clone for repeated use but keeps disk usage low.
    """
    repo_url = f"https://github.com/{repo}.git"
    repo_dir = work_dir / repo.replace("/", "__")

    if repo_dir.exists():
        subprocess.run(["rm", "-rf", str(repo_dir)], capture_output=True, timeout=60)

    print(f"  Cloning {repo} at {base_commit[:10]}...")

    # Reuse cached clone: copy + checkout target commit
    cache_dir = work_dir / f".cache_{repo.replace('/', '__')}"
    if cache_dir.exists():
        print(f"  Reusing cached clone...")
        subprocess.run(["cp", "-a", str(cache_dir), str(repo_dir)],
                       check=True, capture_output=True, timeout=300)
        # Fetch if needed (commit might not be in cache)
        try:
            subprocess.run(
                ["git", "checkout", "-f", base_commit],
                cwd=repo_dir, check=True, capture_output=True, text=True, timeout=60,
            )
        except subprocess.CalledProcessError:
            # Commit not in cache — fetch it
            print(f"  Fetching missing commit {base_commit[:10]}...")
            subprocess.run(
                ["git", "fetch", "--quiet", "origin", base_commit],
                cwd=repo_dir, capture_output=True, text=True, timeout=300,
            )
            subprocess.run(
                ["git", "checkout", "-f", base_commit],
                cwd=repo_dir, check=True, capture_output=True, text=True, timeout=60,
            )
        subprocess.run(
            ["git", "clean", "-fdx"],
            cwd=repo_dir, check=True, capture_output=True, text=True, timeout=60,
        )
    else:
        # First time: full clone, then cache it
        print(f"  Full clone (first time, will be cached)...")
        subprocess.run(
            ["git", "clone", "--quiet", repo_url, str(repo_dir)],
            check=True, capture_output=True, text=True, timeout=600,
        )
        subprocess.run(
            ["git", "checkout", base_commit],
            cwd=repo_dir, check=True, capture_output=True, text=True, timeout=120,
        )
        # Cache for future tasks from same repo
        print(f"  Caching clone for reuse...")
        subprocess.run(["cp", "-a", str(repo_dir), str(cache_dir)],
                       check=True, capture_output=True, timeout=300)

    # Pre-build GRAPHCTX cache: build once on cache_dir, copy to each checkout
    theo_bin = os.environ.get("THEO_CODE_BIN", "theo-code")
    if Path(theo_bin).exists() or shutil.which(theo_bin):
        # Check if the shared cache already has a built index
        shared_cache = cache_dir / ".theo-cache" if cache_dir.exists() else None
        local_cache = repo_dir / ".theo-cache"

        if shared_cache and (shared_cache / "graph.bin").exists():
            # Copy pre-built index from shared cache
            if not local_cache.exists():
                subprocess.run(["cp", "-a", str(shared_cache), str(local_cache)],
                               capture_output=True, timeout=30)
            print(f"  GRAPHCTX index copied from cache.")
        elif not (local_cache / "graph.bin").exists():
            # Build fresh on this checkout, then save to shared cache
            print(f"  Building GRAPHCTX index (one-time, ~2min)...")
            try:
                subprocess.run(
                    [theo_bin, "context", str(repo_dir), "warmup"],
                    capture_output=True, text=True, timeout=300,
                )
                # Copy to shared cache for future checkouts
                if cache_dir.exists() and local_cache.exists():
                    subprocess.run(["cp", "-a", str(local_cache), str(shared_cache)],
                                   capture_output=True, timeout=30)
                print(f"  GRAPHCTX index built and cached.")
            except subprocess.TimeoutExpired:
                print(f"  GRAPHCTX build timed out.")
            except Exception as e:
                print(f"  GRAPHCTX build failed: {e}")
        else:
            print(f"  GRAPHCTX index already present.")

    return repo_dir


def cleanup_repo(repo_dir: Path) -> None:
    """Remove cloned repo to free disk space."""
    if repo_dir.exists():
        shutil.rmtree(repo_dir, ignore_errors=True)


# ---------------------------------------------------------------------------
# Agent execution
# ---------------------------------------------------------------------------


def run_agent(repo_dir: Path, problem_statement: str, timeout: int) -> tuple[int, str]:
    """Run theo_agent.py as a subprocess against the cloned repo.

    Returns (exit_code, stdout+stderr).
    """
    env = os.environ.copy()
    env["VLLM_URL"] = VLLM_URL
    env["MODEL_NAME"] = MODEL_NAME
    env["THEO_CODE_BIN"] = os.environ.get("THEO_CODE_BIN", "theo-code")
    # GRAPHCTX always enabled — it's our core differentiator
    # Allow enough iterations for the fix
    env["THEO_MAX_ITERATIONS"] = "12"

    cmd = [
        sys.executable,
        str(AGENT_SCRIPT),
        "--repo", str(repo_dir),
        "--task", problem_statement,
        "--quiet",
    ]

    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=str(repo_dir),
        )
        output = proc.stdout + "\n" + proc.stderr
        return proc.returncode, output
    except subprocess.TimeoutExpired:
        return -1, f"TIMEOUT after {timeout}s"
    except Exception as e:
        return -1, f"AGENT ERROR: {e}"


# ---------------------------------------------------------------------------
# Test evaluation
# ---------------------------------------------------------------------------


def apply_test_patch(repo_dir: Path, test_patch: str) -> bool:
    """Apply the gold test patch to the repo.

    Returns True if the patch was applied successfully.
    """
    if not test_patch or not test_patch.strip():
        return False

    patch_file = repo_dir / "_swe_bench_test.patch"
    patch_file.write_text(test_patch)

    try:
        subprocess.run(
            ["git", "apply", "--allow-empty", str(patch_file)],
            cwd=repo_dir,
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
        return True
    except subprocess.CalledProcessError:
        # Try with more lenient options
        try:
            subprocess.run(
                ["git", "apply", "--3way", str(patch_file)],
                cwd=repo_dir,
                check=True,
                capture_output=True,
                text=True,
                timeout=60,
            )
            return True
        except subprocess.CalledProcessError as e:
            print(f"  WARNING: Could not apply test patch: {e.stderr[:200]}")
            return False
    finally:
        if patch_file.exists():
            patch_file.unlink()


def extract_test_files(test_patch: str) -> list[str]:
    """Extract test file paths from a git diff patch."""
    files = []
    for line in test_patch.split("\n"):
        if line.startswith("diff --git"):
            # "diff --git a/path/to/file b/path/to/file"
            parts = line.split()
            if len(parts) >= 4:
                path = parts[3]
                if path.startswith("b/"):
                    path = path[2:]
                files.append(path)
    return files


def run_tests(repo_dir: Path, test_patch: str, timeout: int = 300) -> dict:
    """Run the test files from the test patch using pytest.

    Returns a dict with passed/failed/error counts and raw output.
    """
    test_files = extract_test_files(test_patch)
    if not test_files:
        return {"passed": 0, "failed": 0, "error": 0, "output": "No test files found"}

    # Filter to existing files only
    existing = [f for f in test_files if (repo_dir / f).exists()]
    if not existing:
        return {
            "passed": 0,
            "failed": 0,
            "error": 0,
            "output": f"Test files not found: {test_files}",
        }

    # Detect test framework: Django uses its own test runner
    is_django = (repo_dir / "django").is_dir() and (repo_dir / "tests").is_dir()

    # Install repo dependencies if needed (first time only)
    setup_marker = repo_dir / ".theo_deps_installed"
    if not setup_marker.exists():
        try:
            if is_django:
                # Django needs its test requirements
                reqs = repo_dir / "tests" / "requirements" / "py3.txt"
                if reqs.exists():
                    subprocess.run([sys.executable, "-m", "pip", "install", "-q", "-r", str(reqs)],
                                   capture_output=True, timeout=120)
                # Install Django itself in dev mode
                subprocess.run([sys.executable, "-m", "pip", "install", "-q", "-e", str(repo_dir)],
                               capture_output=True, timeout=120)
            elif (repo_dir / "setup.py").exists() or (repo_dir / "pyproject.toml").exists():
                subprocess.run([sys.executable, "-m", "pip", "install", "-q", "-e", str(repo_dir)],
                               capture_output=True, timeout=120)
            setup_marker.touch()
        except Exception:
            pass

    if is_django:
        # Django test runner: python tests/runtests.py <test_module>
        test_labels = []
        for f in existing:
            # tests/auth_tests/test_validators.py -> auth_tests.test_validators
            if f.startswith("tests/"):
                label = f[6:].replace("/", ".").replace(".py", "")
                test_labels.append(label)
            else:
                test_labels.append(f)
        cmd = [
            sys.executable, "tests/runtests.py",
            "--verbosity", "1", "--parallel", "1",
        ] + test_labels
    else:
        # Default: pytest
        cmd = [
            sys.executable, "-m", "pytest",
            "--tb=short", "--no-header", "-q",
        ] + existing

    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=repo_dir,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1",
                 "PYTHONIOENCODING": "utf-8", "LANG": "C.UTF-8"},
        )
        output = (proc.stdout or "") + "\n" + (proc.stderr or "")
        result = parse_test_output(output)
        # Debug: if no tests parsed, log raw output
        if result["passed"] == 0 and result["failed"] == 0 and result["error"] == 0:
            print(f"  [debug] Test cmd: {' '.join(cmd[-3:])}")
            print(f"  [debug] Exit code: {proc.returncode}")
            print(f"  [debug] stdout last 300: {(proc.stdout or '')[-300:]}")
            print(f"  [debug] stderr last 300: {(proc.stderr or '')[-300:]}")
        return result
    except subprocess.TimeoutExpired:
        return {"passed": 0, "failed": 0, "error": 1, "output": "TIMEOUT"}
    except Exception as e:
        return {"passed": 0, "failed": 0, "error": 1, "output": str(e)}


def parse_test_output(output: str) -> dict:
    """Parse test output (pytest or Django test runner) to extract pass/fail counts."""
    result = {"passed": 0, "failed": 0, "error": 0, "output": output[-3000:]}

    import re

    # Django test runner: "Ran X tests in N.NNs" + "OK" or "FAILED (failures=N, errors=N)"
    ran_match = re.search(r"Ran (\d+) tests? in", output)
    if ran_match:
        total_ran = int(ran_match.group(1))
        # Parse FAILED line — may contain failures, errors, skipped, unexpected successes
        failures = 0
        errors = 0
        f_match = re.search(r"failures=(\d+)", output)
        if f_match:
            failures = int(f_match.group(1))
        e_match = re.search(r"errors=(\d+)", output)
        if e_match:
            errors = int(e_match.group(1))

        if "FAILED" in output.split("Ran")[-1]:
            result["failed"] = failures
            result["error"] = errors
            result["passed"] = max(0, total_ran - failures - errors)
        elif "OK" in output.split("Ran")[-1]:
            result["passed"] = total_ran
        return result

    # pytest short summary: "X passed, Y failed, Z error in N.NNs"
    match = re.search(r"(\d+) passed", output)
    if match:
        result["passed"] = int(match.group(1))

    match = re.search(r"(\d+) failed", output)
    if match:
        result["failed"] = int(match.group(1))

    match = re.search(r"(\d+) error", output)
    if match:
        result["error"] = int(match.group(1))

    return result


def evaluate_task(
    task: dict,
    work_dir: Path,
    timeout: int,
) -> TaskResult:
    """Run the full evaluation pipeline for a single SWE-bench task.

    Steps:
      1. Clone repo at base_commit
      2. Run the agent to produce a fix
      3. Apply the gold test patch
      4. Run tests to verify the fix
      5. Cleanup
    """
    instance_id = task["instance_id"]
    repo = task["repo"]
    base_commit = task["base_commit"]
    problem_statement = task["problem_statement"]
    test_patch = task.get("test_patch", "")

    start = time.time()
    repo_dir = None

    try:
        # Step 1: Clone
        repo_dir = clone_repo(repo, base_commit, work_dir)

        # Step 2: Run agent
        print(f"  Running agent...")
        exit_code, agent_output = run_agent(repo_dir, problem_statement, timeout)
        print(f"  Agent finished (exit_code={exit_code})")

        if exit_code == -1 and "TIMEOUT" in agent_output:
            elapsed = time.time() - start
            return TaskResult(
                instance_id=instance_id,
                repo=repo,
                success=False,
                time_seconds=elapsed,
                error="Agent timeout",
                agent_exit_code=exit_code,
            )

        # Step 3: Apply test patch
        print(f"  Applying test patch...")
        patch_ok = apply_test_patch(repo_dir, test_patch)

        if not patch_ok:
            elapsed = time.time() - start
            return TaskResult(
                instance_id=instance_id,
                repo=repo,
                success=False,
                time_seconds=elapsed,
                error="Failed to apply test patch",
                agent_exit_code=exit_code,
                patch_applied=False,
            )

        # Step 4: Run tests
        print(f"  Running tests...")
        test_results = run_tests(repo_dir, test_patch)
        elapsed = time.time() - start

        passed = test_results["passed"]
        failed = test_results["failed"]
        errored = test_results["error"]

        # Success = at least 1 test passed AND 0 failed AND 0 errors
        success = passed > 0 and failed == 0 and errored == 0

        error_msg = None
        if not success and failed == 0 and passed == 0 and errored == 0:
            error_msg = "No test results parsed"
        elif not success:
            # Extract first failure from output
            lines = test_results["output"].split("\n")
            failure_lines = [
                l for l in lines if "FAILED" in l or "ERROR" in l
            ][:3]
            error_msg = "; ".join(failure_lines) if failure_lines else "Tests failed"

        return TaskResult(
            instance_id=instance_id,
            repo=repo,
            success=success,
            time_seconds=elapsed,
            error=error_msg,
            agent_exit_code=exit_code,
            tests_passed=passed,
            tests_failed=failed,
            tests_error=errored,
            patch_applied=True,
        )

    except Exception as e:
        elapsed = time.time() - start
        return TaskResult(
            instance_id=instance_id,
            repo=repo,
            success=False,
            time_seconds=elapsed,
            error=str(e),
        )
    finally:
        if repo_dir:
            cleanup_repo(repo_dir)


# ---------------------------------------------------------------------------
# Results management
# ---------------------------------------------------------------------------


def load_existing_results() -> list[dict]:
    """Load previously saved results for --resume support."""
    if RESULTS_FILE.exists():
        with open(RESULTS_FILE) as f:
            return json.load(f).get("tasks", [])
    return []


def save_results(task_results: list[TaskResult]) -> None:
    """Save results to JSON with per-task and aggregate data."""
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    total = len(task_results)
    passed = sum(1 for r in task_results if r.success)
    failed = sum(1 for r in task_results if not r.success and r.error != "Agent timeout")
    errored = sum(1 for r in task_results if r.error and "timeout" in (r.error or "").lower())
    times = [r.time_seconds for r in task_results if r.time_seconds > 0]

    aggregate = AggregateResults(
        total=total,
        passed=passed,
        failed=failed,
        errored=errored,
        pass_rate=passed / total if total > 0 else 0.0,
        avg_time=sum(times) / len(times) if times else 0.0,
        total_time=sum(times),
    )

    output = {
        "metadata": {
            "dataset": HF_DATASET,
            "model": MODEL_NAME,
            "vllm_url": VLLM_URL,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S"),
        },
        "aggregate": asdict(aggregate),
        "tasks": [asdict(r) for r in task_results],
    }

    with open(RESULTS_FILE, "w") as f:
        json.dump(output, f, indent=2)

    print(f"\nResults saved to: {RESULTS_FILE}")


def print_summary(task_results: list[TaskResult]) -> None:
    """Print a summary table of results."""
    total = len(task_results)
    if total == 0:
        print("No tasks evaluated.")
        return

    passed = sum(1 for r in task_results if r.success)
    failed = total - passed
    times = [r.time_seconds for r in task_results]
    avg_time = sum(times) / len(times) if times else 0.0

    print(f"\n{'=' * 60}")
    print("SWE-bench Lite — Evaluation Summary")
    print(f"{'=' * 60}")
    print(f"  Total tasks:    {total}")
    print(f"  Passed:         {passed}")
    print(f"  Failed:         {failed}")
    print(f"  Pass rate:      {passed/total:.1%}")
    print(f"  Avg time/task:  {avg_time:.1f}s")
    print(f"  Total time:     {sum(times):.0f}s ({sum(times)/3600:.1f}h)")
    print(f"{'=' * 60}")

    # Per-repo breakdown
    repos: dict[str, list[TaskResult]] = {}
    for r in task_results:
        repos.setdefault(r.repo, []).append(r)

    print(f"\n{'Repo':<45} {'Pass':>5} {'Total':>6} {'Rate':>7}")
    print("-" * 65)
    for repo in sorted(repos.keys()):
        results = repos[repo]
        repo_passed = sum(1 for r in results if r.success)
        rate = repo_passed / len(results) if results else 0.0
        print(f"  {repo:<43} {repo_passed:>5} {len(results):>6} {rate:>6.0%}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(
        description="SWE-bench Lite Evaluation Harness for Theo Code Agent"
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Run only N tasks (for testing)",
    )
    parser.add_argument(
        "--filter",
        type=str,
        default=None,
        help="Run only tasks from repos matching this string (e.g. 'django')",
    )
    parser.add_argument(
        "--resume",
        action="store_true",
        help="Skip already-completed tasks from previous run",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=DEFAULT_TIMEOUT,
        help=f"Timeout per task in seconds (default: {DEFAULT_TIMEOUT})",
    )
    parser.add_argument(
        "--workdir",
        type=str,
        default=str(WORKDIR),
        help=f"Working directory for cloned repos (default: {WORKDIR})",
    )

    args = parser.parse_args()
    work_dir = Path(args.workdir)
    work_dir.mkdir(parents=True, exist_ok=True)

    print("=" * 60)
    print("SWE-bench Lite — Theo Code Agent Evaluation")
    print("=" * 60)
    print(f"  Model:    {MODEL_NAME}")
    print(f"  vLLM:     {VLLM_URL}")
    print(f"  Timeout:  {args.timeout}s per task")
    print(f"  Workdir:  {work_dir}")
    print()

    # Load dataset
    all_tasks = load_dataset()

    # Handle resume
    completed_ids: set[str] = set()
    existing_results: list[TaskResult] = []
    if args.resume:
        raw = load_existing_results()
        completed_ids = {r["instance_id"] for r in raw}
        existing_results = [TaskResult(**r) for r in raw]
        print(f"Resuming: {len(completed_ids)} tasks already completed")

    # Filter
    tasks = filter_tasks(all_tasks, args.filter, args.limit, completed_ids)

    if not tasks:
        print("No tasks to run.")
        if existing_results:
            print_summary(existing_results)
        return

    print(f"\nRunning {len(tasks)} tasks...\n")

    # Execute
    task_results: list[TaskResult] = list(existing_results)

    for i, task in enumerate(tasks, 1):
        instance_id = task["instance_id"]
        repo = task["repo"]
        print(f"[{i}/{len(tasks)}] {instance_id} ({repo})")

        result = evaluate_task(task, work_dir, args.timeout)
        task_results.append(result)

        status = "PASS" if result.success else "FAIL"
        print(
            f"  {status} | {result.time_seconds:.0f}s | "
            f"tests: {result.tests_passed}P/{result.tests_failed}F/{result.tests_error}E"
        )
        if result.error:
            print(f"  Error: {result.error[:120]}")

        # Save incrementally so we don't lose progress on crash
        save_results(task_results)

    # Final summary
    print_summary(task_results)


if __name__ == "__main__":
    main()
