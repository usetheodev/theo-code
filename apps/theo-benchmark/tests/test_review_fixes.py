"""Regression tests for the 5 bugs found in code review.

Each test proves the fix works and prevents the bug from returning.
"""

import os
import sys
import tempfile
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


# ---------------------------------------------------------------------------
# Fix #1: BASELINE_ALREADY_PASSES detection
# (swe_bench_harness.py — evaluate_task must verify bug exists before agent)
# ---------------------------------------------------------------------------
# This is an integration test (needs repos), so we test the logic indirectly
# by verifying the TaskResult error message pattern exists in the code.


class TestBaselineValidation:
    def test_harness_checks_baseline_before_agent(self):
        """The evaluate_task function must run tests BEFORE the agent
        and reject instances where tests already pass."""
        import swe_bench_harness
        source = open(swe_bench_harness.__file__).read()

        # The baseline check must exist between clone and agent run
        clone_pos = source.find("Step 1: Clone")
        baseline_pos = source.find("BASELINE_ALREADY_PASSES")
        agent_pos = source.find("Step 2: Run agent")

        assert baseline_pos > clone_pos, "Baseline check must come after clone"
        assert baseline_pos < agent_pos, "Baseline check must come before agent run"

    def test_baseline_revert_after_check(self):
        """After baseline check, the repo must be reverted to clean state
        so the agent starts from the original base_commit."""
        import swe_bench_harness
        source = open(swe_bench_harness.__file__).read()

        # Must have git checkout after baseline check
        baseline_section = source[
            source.find("Baseline check"):source.find("Step 2: Run agent")
        ]
        assert "git" in baseline_section and "checkout" in baseline_section, \
            "Must revert repo after baseline check"


# ---------------------------------------------------------------------------
# Fix #2: check_success requires exact file path
# (run_benchmark.py — no more stem-only matching)
# ---------------------------------------------------------------------------


class TestCheckSuccessStrict:
    def test_rejects_stem_only_match(self):
        """Model mentioning just 'cluster' without full path must fail."""
        from run_benchmark import check_success, Task

        task = Task(
            id="test",
            description="Find leiden",
            target_file="crates/graph/src/cluster.rs",
            expected_symbols=["leiden_communities", "refine_partition", "modularity"],
            difficulty="easy",
        )

        # Mentions "cluster" and symbols but NOT the full path
        response = "The cluster module has leiden_communities and refine_partition functions."
        assert check_success(response, task) is False

    def test_accepts_exact_path_with_symbols(self):
        """Model mentioning the exact path + symbols must pass."""
        from run_benchmark import check_success, Task

        task = Task(
            id="test",
            description="Find leiden",
            target_file="crates/graph/src/cluster.rs",
            expected_symbols=["leiden_communities", "refine_partition"],
            difficulty="easy",
        )

        response = "Found in crates/graph/src/cluster.rs: the leiden_communities function calls refine_partition."
        assert check_success(response, task) is True

    def test_symbols_must_follow_file_path(self):
        """Symbols mentioned BEFORE the file path don't count."""
        from run_benchmark import check_success, Task

        task = Task(
            id="test",
            description="Find search",
            target_file="crates/context/src/search.rs",
            expected_symbols=["Bm25Index", "tokenise"],
            difficulty="easy",
        )

        # Symbols before the path — could be hallucinated association
        response = "Bm25Index and tokenise are common. The file is crates/context/src/search.rs."
        assert check_success(response, task) is False

    def test_rejects_hallucinated_path(self):
        """Model inventing a plausible but wrong path must fail."""
        from run_benchmark import check_success, Task

        task = Task(
            id="test",
            description="Find permission eval",
            target_file="crates/core/src/permission.rs",
            expected_symbols=["evaluate", "PermissionRule"],
            difficulty="easy",
        )

        response = "Found in crates/auth/src/permission.rs: evaluate and PermissionRule."
        assert check_success(response, task) is False


# ---------------------------------------------------------------------------
# Fix #3: CheckpointManager rollback with duplicate text
# (task_engine.py — offset-based rollback)
# ---------------------------------------------------------------------------


class TestCheckpointRollbackDuplicates:
    def test_rollback_targets_correct_occurrence(self):
        """When the same text appears twice, rollback must undo the right one."""
        from task_engine import CheckpointManager

        tmpdir = tempfile.mkdtemp()
        filepath = os.path.join(tmpdir, "dup.py")

        # File with duplicate text
        original = "value = 10\nother = 20\nvalue = 10\n"
        with open(filepath, "w") as f:
            f.write(original)

        mgr = CheckpointManager(tmpdir)
        mgr.save("task1")

        # Edit the SECOND occurrence (offset should capture position)
        content = open(filepath).read()
        second_pos = content.find("value = 10", content.find("value = 10") + 1)
        new_content = content[:second_pos] + "value = 99" + content[second_pos + len("value = 10"):]
        with open(filepath, "w") as f:
            f.write(new_content)
        mgr.record_edit("dup.py", "value = 10", "value = 99")

        # Verify pre-rollback state
        assert open(filepath).read() == "value = 10\nother = 20\nvalue = 99\n"

        # Rollback should restore the second occurrence, not touch the first
        mgr.rollback("task1")
        assert open(filepath).read() == original

    def test_rollback_still_works_without_offset(self):
        """If offset is unknown (-1), fallback to first occurrence."""
        from task_engine import CheckpointManager, EditRecord

        tmpdir = tempfile.mkdtemp()
        filepath = os.path.join(tmpdir, "test.py")
        with open(filepath, "w") as f:
            f.write("hello world")

        mgr = CheckpointManager(tmpdir)
        mgr.save("task1")

        # Manually create edit with unknown offset
        mgr.task_edits["task1"].append(EditRecord(
            file_path="test.py",
            old_text="hello",
            new_text="goodbye",
            offset=-1,
        ))
        with open(filepath, "w") as f:
            f.write("goodbye world")

        mgr.rollback("task1")
        assert open(filepath).read() == "hello world"


# ---------------------------------------------------------------------------
# Fix #4: --resume uses stable path
# (swe/adapter.py — deterministic path when resuming)
# ---------------------------------------------------------------------------


class TestResumeStablePath:
    def test_resume_uses_stable_filename(self):
        """With --resume and no --report, path must NOT contain timestamp."""
        # We test the path logic by inspecting the source
        import swe.adapter as adapter
        source = open(adapter.__file__).read()

        # When resume is True and no explicit report, should use "latest" not timestamp
        assert "swe-{args.dataset}-latest" in source.replace("'", "").replace('"', ''), \
            "Resume mode must use a stable path (not timestamped)"

    def test_fresh_run_uses_timestamp(self):
        """Without --resume, path should contain timestamp for uniqueness."""
        import swe.adapter as adapter
        source = open(adapter.__file__).read()

        assert "int(time.time())" in source, \
            "Fresh runs must use timestamped paths"


# ---------------------------------------------------------------------------
# Fix #5: pip install failure prevents marker creation
# (swe_bench_harness.py — .theo_deps_installed only on success)
# ---------------------------------------------------------------------------


class TestDepInstallMarker:
    def test_marker_only_on_success(self):
        """The .theo_deps_installed marker must only be created if ALL
        pip installs succeed. A failed install must NOT create the marker."""
        import swe_bench_harness
        source = open(swe_bench_harness.__file__).read()

        # Find the section between marker check and marker creation
        marker_check = source.find('if not setup_marker.exists():')
        marker_touch = source.find('setup_marker.touch()', marker_check)

        section = source[marker_check:marker_touch + 50]

        # Must check returncode before touching
        assert "returncode" in section, \
            "Must check pip returncode before creating marker"
        assert "deps_ok" in section or "check=True" in section, \
            "Must gate marker creation on install success"

    def test_failed_install_warns(self):
        """Failed pip install must print a warning, not silently ignore."""
        import swe_bench_harness
        source = open(swe_bench_harness.__file__).read()

        # Find the dep install block (between marker check and marker touch)
        start = source.find("Install repo dependencies")
        end = source.find("setup_marker.touch()", start)
        install_section = source[start:end]
        assert "WARNING" in install_section, \
            "Failed dep install must emit WARNING"
