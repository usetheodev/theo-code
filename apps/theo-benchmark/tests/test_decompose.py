"""Unit tests for decompose.py — intent classification and template decomposition."""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from decompose import (
    classify_intent,
    TaskType,
    StructuralAnalysis,
    template_bug_fix,
    template_new_module,
    template_refactor,
    template_new_endpoint,
)


# ---------------------------------------------------------------------------
# classify_intent
# ---------------------------------------------------------------------------


class TestClassifyIntent:
    def test_bug_fix_keywords(self):
        assert classify_intent("Fix the auth bug where tokens expire") == TaskType.BUG_FIX
        assert classify_intent("Error in payment processing") == TaskType.BUG_FIX
        # "crash" alone scores 1 (below threshold of 2) — only strong indicators trigger
        assert classify_intent("Fix crash when user submits empty form") == TaskType.BUG_FIX

    def test_new_module_keywords(self):
        assert classify_intent("Create a new rate limiter module") == TaskType.NEW_MODULE
        assert classify_intent("Add a caching middleware") == TaskType.NEW_MODULE
        assert classify_intent("Implement user authentication service") == TaskType.NEW_MODULE

    def test_refactor_keywords(self):
        # Needs 2+ refactor keywords to beat threshold
        assert classify_intent("Refactor and extract the search module") == TaskType.REFACTOR
        assert classify_intent("Rename and reorganize the user service") == TaskType.REFACTOR
        # "extract" alone ties with new_module — not enough
        assert classify_intent("Extract BM25 into separate class") == TaskType.UNKNOWN

    def test_endpoint_keywords(self):
        # "implement"/"add" trigger NEW_MODULE bonus (+3) — competes with endpoint
        # Pure endpoint keywords without new_module triggers work:
        assert classify_intent("Define route and handler for REST API endpoint") == TaskType.NEW_ENDPOINT
        # When tied, first-in-dict wins — document this behavior
        assert classify_intent("Implement the POST /users endpoint route handler") in (
            TaskType.NEW_MODULE, TaskType.NEW_ENDPOINT
        )

    def test_ambiguous_returns_unknown(self):
        assert classify_intent("improve performance somehow") == TaskType.UNKNOWN
        assert classify_intent("x y z") == TaskType.UNKNOWN

    def test_case_insensitive(self):
        assert classify_intent("FIX THE BUG") == TaskType.BUG_FIX
        assert classify_intent("Create New Module") == TaskType.NEW_MODULE


# ---------------------------------------------------------------------------
# Template outputs
# ---------------------------------------------------------------------------


class TestTemplateBugFix:
    def test_produces_three_tasks(self):
        analysis = StructuralAnalysis(
            affected_files=["src/auth.py"],
            test_files=["tests/test_auth.py"],
        )
        tasks = template_bug_fix("Fix auth timeout", analysis)

        assert len(tasks) == 3
        assert tasks[0].id == "task1"  # Reproduce
        assert tasks[1].id == "task2"  # Fix
        assert tasks[2].id == "task3"  # Verify
        assert "task1" in tasks[1].depends_on
        assert "task2" in tasks[2].depends_on

    def test_includes_affected_files(self):
        analysis = StructuralAnalysis(affected_files=["src/main.py", "src/utils.py"])
        tasks = template_bug_fix("Fix crash", analysis)

        assert "src/main.py" in tasks[1].target_files


class TestTemplateNewModule:
    def test_minimum_two_tasks(self):
        analysis = StructuralAnalysis()
        tasks = template_new_module("Add logging module", analysis)

        assert len(tasks) >= 2
        assert tasks[0].task_type == "create"

    def test_includes_integration_when_files_exist(self):
        analysis = StructuralAnalysis(affected_files=["src/app.py"])
        tasks = template_new_module("Add auth module", analysis)

        assert len(tasks) >= 3
        assert any(t.task_type == "modify" for t in tasks)


class TestTemplateRefactor:
    def test_produces_ordered_tasks(self):
        analysis = StructuralAnalysis(
            affected_files=["src/search.py", "src/index.py"],
            test_files=["tests/test_search.py"],
        )
        tasks = template_refactor("Extract BM25", analysis)

        assert len(tasks) >= 3
        assert tasks[0].task_type == "verify"  # Identify callers
        assert tasks[1].task_type == "modify"  # Refactor
        assert "task1" in tasks[1].depends_on


class TestTemplateNewEndpoint:
    def test_produces_three_tasks(self):
        analysis = StructuralAnalysis(
            affected_files=["src/routes.py"],
            test_files=["tests/test_api.py"],
        )
        tasks = template_new_endpoint("Add /users endpoint", analysis)

        assert len(tasks) == 3
        assert tasks[0].task_type == "create"  # Handler
        assert tasks[2].task_type == "test"    # Tests
