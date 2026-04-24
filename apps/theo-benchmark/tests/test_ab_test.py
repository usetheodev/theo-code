"""Phase 55 (prompt-ab-testing-plan) — unit tests for ab_test orchestrator.

Covers the pure-Python helpers that run on the host (independent of tb):
  - select_tasks_alphabetically
  - write_manifest
  - build_tb_command
  - main(--dry-run) end-to-end with --task-ids-file fixture
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from runner.ab_test import (  # noqa: E402
    build_tb_command,
    main,
    parse_dataset_spec,
    select_tasks_alphabetically,
    write_manifest,
)


class TestParseDatasetSpec(unittest.TestCase):
    def test_parses_name_and_version(self) -> None:
        self.assertEqual(parse_dataset_spec("terminal-bench-core==0.1.1"),
                         ("terminal-bench-core", "0.1.1"))

    def test_returns_none_version_when_no_separator(self) -> None:
        self.assertEqual(parse_dataset_spec("local-path"), ("local-path", None))

    def test_strips_whitespace(self) -> None:
        self.assertEqual(parse_dataset_spec(" name == 1.0 "), ("name", "1.0"))


class TestSelectTasks(unittest.TestCase):
    def test_load_first_n_tasks_alphabetically_returns_n(self) -> None:
        tasks = ["zeta", "alpha", "beta", "gamma", "delta"]
        result = select_tasks_alphabetically(tasks, 3)
        self.assertEqual(result, ["alpha", "beta", "delta"])

    def test_load_first_n_tasks_alphabetically_is_deterministic(self) -> None:
        tasks = ["zeta", "alpha", "beta", "gamma", "delta"]
        first = select_tasks_alphabetically(tasks, 3)
        second = select_tasks_alphabetically(list(reversed(tasks)), 3)
        # Same input set → same output regardless of input ordering
        self.assertEqual(first, second)

    def test_select_returns_all_when_n_larger_than_available(self) -> None:
        result = select_tasks_alphabetically(["a", "b"], 10)
        self.assertEqual(result, ["a", "b"])

    def test_select_returns_empty_when_n_zero(self) -> None:
        self.assertEqual(select_tasks_alphabetically(["a", "b"], 0), [])

    def test_select_rejects_negative_n(self) -> None:
        with self.assertRaises(ValueError):
            select_tasks_alphabetically(["a"], -1)


class TestWriteManifest(unittest.TestCase):
    def test_write_manifest_includes_provenance_pin(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            out = Path(td)
            path = write_manifest(
                out,
                variants=["sota", "sota-lean"],
                task_ids=["t1", "t2", "t3"],
                theo_sha="abc1234",
                model="gpt-5.4",
                dataset="terminal-bench-core==0.1.1",
                started_at="2026-04-24T12:00:00+00:00",
            )
            self.assertTrue(path.exists())
            data = json.loads(path.read_text())
            self.assertEqual(data["schema"], "ab.manifest.v1")
            self.assertEqual(data["variants"], ["sota", "sota-lean"])
            self.assertEqual(data["task_ids"], ["t1", "t2", "t3"])
            self.assertEqual(data["n_tasks"], 3)
            self.assertEqual(data["theo_sha"], "abc1234")
            self.assertEqual(data["model"], "gpt-5.4")
            self.assertEqual(data["started_at"], "2026-04-24T12:00:00+00:00")


class TestBuildTbCommand(unittest.TestCase):
    def test_command_includes_task_id_per_task(self) -> None:
        cmd = build_tb_command(
            tb_bin="tb",
            dataset="terminal-bench-core==0.1.1",
            output_path=Path("/out"),
            task_ids=["alpha", "beta"],
            n_concurrent=4,
        )
        # Two --task-id flags, one per task
        self.assertEqual(cmd.count("--task-id"), 2)
        self.assertIn("alpha", cmd)
        self.assertIn("beta", cmd)

    def test_command_carries_dataset_and_agent_path(self) -> None:
        cmd = build_tb_command(
            tb_bin="tb",
            dataset="terminal-bench-core==0.1.1",
            output_path=Path("/out"),
            task_ids=["x"],
            n_concurrent=4,
        )
        joined = " ".join(cmd)
        self.assertIn("--dataset terminal-bench-core==0.1.1", joined)
        self.assertIn("--agent-import-path tbench.agent:TheoAgent", joined)
        self.assertIn("--no-upload-results", joined)


class TestMainDryRun(unittest.TestCase):
    def test_dry_run_writes_manifest_and_prints_commands(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            ids_file = tdp / "ids.txt"
            ids_file.write_text("zeta\nalpha\nbeta\n")
            out_dir = tdp / "out"
            rc = main([
                "--variants", "sota,sota-lean",
                "--n-tasks", "2",
                "--task-ids-file", str(ids_file),
                "--output-dir", str(out_dir),
                "--dry-run",
            ])
            self.assertEqual(rc, 0)
            manifest = json.loads((out_dir / "manifest.json").read_text())
            self.assertEqual(manifest["variants"], ["sota", "sota-lean"])
            # Alphabetical sort of zeta/alpha/beta → alpha, beta (first 2)
            self.assertEqual(manifest["task_ids"], ["alpha", "beta"])

    def test_dry_run_rejects_single_variant(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            ids_file = tdp / "ids.txt"
            ids_file.write_text("alpha\n")
            with self.assertRaises(SystemExit):
                main([
                    "--variants", "sota",
                    "--task-ids-file", str(ids_file),
                    "--output-dir", str(tdp / "out"),
                    "--dry-run",
                ])


if __name__ == "__main__":
    unittest.main(verbosity=2)
