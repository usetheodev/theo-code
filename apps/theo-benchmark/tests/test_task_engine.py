"""Unit tests for task_engine.py — circuit breaker, checkpoint, context stack."""

import os
import sys
import tempfile
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from task_engine import (
    CircuitBreaker,
    CheckpointManager,
    ContextStack,
    TaskEngine,
    TaskStatus,
    MAX_DEPTH,
    MAX_TOTAL_SUBTASKS,
)


# ---------------------------------------------------------------------------
# CircuitBreaker
# ---------------------------------------------------------------------------


class TestCircuitBreaker:
    def test_allows_shallow_subtask(self):
        cb = CircuitBreaker()
        allowed, reason = cb.can_create_subtask(parent_depth=0)

        assert allowed is True
        assert reason == ""

    def test_blocks_at_max_depth(self):
        cb = CircuitBreaker()
        allowed, reason = cb.can_create_subtask(parent_depth=MAX_DEPTH)

        assert allowed is False
        assert "Max depth" in reason

    def test_blocks_after_max_total(self):
        cb = CircuitBreaker()
        for _ in range(MAX_TOTAL_SUBTASKS):
            cb.record_subtask()

        allowed, reason = cb.can_create_subtask(parent_depth=0)

        assert allowed is False
        assert cb.tripped is True

    def test_stays_tripped(self):
        cb = CircuitBreaker()
        cb.tripped = True
        cb.trip_reason = "test"

        allowed, _ = cb.can_create_subtask(parent_depth=0)
        assert allowed is False

    def test_reset_clears_state(self):
        cb = CircuitBreaker()
        cb.tripped = True
        cb.total_subtasks = 50

        cb.reset()

        assert cb.tripped is False
        assert cb.total_subtasks == 0


# ---------------------------------------------------------------------------
# CheckpointManager
# ---------------------------------------------------------------------------


class TestCheckpointManager:
    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = CheckpointManager(self.tmpdir)

    def test_save_and_record(self):
        self.mgr.save("task1")
        self.mgr.record_edit("test.py", "old", "new")

        assert self.mgr.get_edit_count("task1") == 1
        assert self.mgr.get_modified_files("task1") == ["test.py"]

    def test_rollback_restores_content(self):
        filepath = os.path.join(self.tmpdir, "test.py")
        with open(filepath, "w") as f:
            f.write("original content")

        self.mgr.save("task1")
        # Simulate edit
        with open(filepath, "w") as f:
            f.write("modified content")
        self.mgr.record_edit("test.py", "original content", "modified content")

        success = self.mgr.rollback("task1")

        assert success is True
        assert open(filepath).read() == "original content"

    def test_rollback_empty_task(self):
        self.mgr.save("task1")
        assert self.mgr.rollback("task1") is True

    def test_rollback_nonexistent_task(self):
        assert self.mgr.rollback("nonexistent") is True

    def test_multiple_edits_rollback_in_order(self):
        f1 = os.path.join(self.tmpdir, "a.py")
        with open(f1, "w") as f:
            f.write("AAA")

        self.mgr.save("task1")

        # First edit
        with open(f1, "w") as f:
            f.write("BBB")
        self.mgr.record_edit("a.py", "AAA", "BBB")

        # Second edit
        with open(f1, "w") as f:
            f.write("CCC")
        self.mgr.record_edit("a.py", "BBB", "CCC")

        self.mgr.rollback("task1")

        assert open(f1).read() == "AAA"


# ---------------------------------------------------------------------------
# ContextStack
# ---------------------------------------------------------------------------


class TestContextStack:
    def test_push_pop(self):
        cs = ContextStack()
        cs.push("task1", "ctx", "goal")

        assert cs.current_depth() == 1

        frame = cs.pop()
        assert frame["task_id"] == "task1"
        assert frame["goal"] == "goal"
        assert cs.current_depth() == 0

    def test_original_goal(self):
        cs = ContextStack()
        cs.push("task1", "", "original goal")
        cs.push("task2", "", "sub goal")

        assert cs.original_goal() == "original goal"

    def test_breadcrumb(self):
        cs = ContextStack()
        cs.push("t1", "", "")
        cs.push("t2", "", "")

        bc = cs.breadcrumb()
        assert "[t1]" in bc
        assert "[t2]" in bc
        assert "depth 2" in bc

    def test_empty_stack(self):
        cs = ContextStack()

        assert cs.current_depth() == 0
        assert cs.original_goal() == ""
        assert cs.breadcrumb() == ""
        assert cs.pop() is None


# ---------------------------------------------------------------------------
# TaskEngine.execute_spec
# ---------------------------------------------------------------------------


class TestTaskEngineExecuteSpec:
    def test_all_tasks_succeed(self):
        engine = TaskEngine(".", "./nonexistent")
        spec = [
            {"id": "t1", "description": "Task 1"},
            {"id": "t2", "description": "Task 2"},
        ]

        def mock_agent(desc, ctx):
            return True, f"Done: {desc[:20]}", ""

        result = engine.execute_spec(spec, mock_agent)

        assert result["summary"] == "2/2 tasks completed"
        assert result["failed"] == 0
        assert result["circuit_breaker_tripped"] is False

    def test_task_failure_continues(self):
        engine = TaskEngine(".", "./nonexistent")
        spec = [
            {"id": "t1", "description": "Will fail"},
            {"id": "t2", "description": "Should still run"},
        ]

        call_count = 0

        def mock_agent(desc, ctx):
            nonlocal call_count
            call_count += 1
            if "fail" in desc.lower():
                return False, "", "simulated failure"
            return True, "Done", ""

        result = engine.execute_spec(spec, mock_agent)

        # t2 should still execute even though t1 failed
        assert result["tasks"]["t2"]["status"] == "done"
        assert result["failed"] >= 1

    def test_empty_spec(self):
        engine = TaskEngine(".", "./nonexistent")
        result = engine.execute_spec([], lambda d, c: (True, "", ""))

        assert result["summary"] == "0/0 tasks completed"
