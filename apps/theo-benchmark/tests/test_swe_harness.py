"""Unit tests for swe_bench_harness.py — test evaluation pipeline."""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from swe_bench_harness import parse_test_output, extract_test_files, filter_tasks


# ---------------------------------------------------------------------------
# parse_test_output
# ---------------------------------------------------------------------------


class TestParseTestOutput:
    def test_pytest_all_pass(self):
        output = "===== 5 passed in 1.23s ====="
        result = parse_test_output(output)

        assert result["passed"] == 5
        assert result["failed"] == 0
        assert result["error"] == 0

    def test_pytest_mixed(self):
        output = "===== 3 passed, 2 failed, 1 error in 5.00s ====="
        result = parse_test_output(output)

        assert result["passed"] == 3
        assert result["failed"] == 2
        assert result["error"] == 1

    def test_django_all_pass(self):
        output = """
Ran 42 tests in 3.456s

OK
"""
        result = parse_test_output(output)

        assert result["passed"] == 42
        assert result["failed"] == 0
        assert result["error"] == 0

    def test_django_failures(self):
        output = """
Ran 10 tests in 1.234s

FAILED (failures=2, errors=1)
"""
        result = parse_test_output(output)

        assert result["passed"] == 7
        assert result["failed"] == 2
        assert result["error"] == 1

    def test_django_only_failures(self):
        output = """
Ran 5 tests in 0.5s

FAILED (failures=3)
"""
        result = parse_test_output(output)

        assert result["passed"] == 2
        assert result["failed"] == 3
        assert result["error"] == 0

    def test_no_test_output(self):
        result = parse_test_output("some random output\nno test data here")

        assert result["passed"] == 0
        assert result["failed"] == 0
        assert result["error"] == 0

    def test_empty_output(self):
        result = parse_test_output("")

        assert result["passed"] == 0
        assert result["failed"] == 0


# ---------------------------------------------------------------------------
# extract_test_files
# ---------------------------------------------------------------------------


class TestExtractTestFiles:
    def test_extracts_files_from_diff(self):
        patch = """diff --git a/tests/test_auth.py b/tests/test_auth.py
--- a/tests/test_auth.py
+++ b/tests/test_auth.py
@@ -1,5 +1,10 @@
diff --git a/tests/test_views.py b/tests/test_views.py
"""
        files = extract_test_files(patch)

        assert "tests/test_auth.py" in files
        assert "tests/test_views.py" in files

    def test_empty_patch(self):
        assert extract_test_files("") == []

    def test_no_diff_lines(self):
        assert extract_test_files("just some text\nnot a patch") == []


# ---------------------------------------------------------------------------
# filter_tasks
# ---------------------------------------------------------------------------


class TestFilterTasks:
    def setup_method(self):
        self.tasks = [
            {"instance_id": "django__django-12345", "repo": "django/django"},
            {"instance_id": "flask__flask-100", "repo": "pallets/flask"},
            {"instance_id": "django__django-67890", "repo": "django/django"},
            {"instance_id": "requests__requests-50", "repo": "psf/requests"},
        ]

    def test_no_filters(self):
        result = filter_tasks(self.tasks)
        assert len(result) == 4

    def test_repo_filter(self):
        result = filter_tasks(self.tasks, repo_filter="django")
        assert len(result) == 2
        assert all("django" in t["repo"] for t in result)

    def test_limit(self):
        result = filter_tasks(self.tasks, limit=2)
        assert len(result) == 2

    def test_completed_ids(self):
        completed = {"django__django-12345", "flask__flask-100"}
        result = filter_tasks(self.tasks, completed_ids=completed)
        assert len(result) == 2
        assert all(t["instance_id"] not in completed for t in result)

    def test_combined_filters(self):
        result = filter_tasks(self.tasks, repo_filter="django", limit=1)
        assert len(result) == 1
