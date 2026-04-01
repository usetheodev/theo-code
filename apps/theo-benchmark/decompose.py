"""
Hybrid Task Decomposer — Graph + Templates + Small LLM fallback.

Decomposes features into tasks using 3 layers:
1. GRAPH: structural analysis (affected files, dependencies, test coverage)
2. TEMPLATES: deterministic decomposition for common patterns
3. LLM FALLBACK: small model for ambiguous cases only

The graph provides WHERE (files, functions).
Templates provide HOW (order, pattern).
LLM provides WHAT (intent interpretation) — only when needed.
"""

import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from enum import Enum
from typing import Optional

import requests


# ---------------------------------------------------------------------------
# Intent Classification (deterministic first, LLM fallback)
# ---------------------------------------------------------------------------

class TaskType(Enum):
    NEW_MODULE = "new_module"       # Create new file/module
    BUG_FIX = "bug_fix"            # Fix existing bug
    REFACTOR = "refactor"          # Restructure without behavior change
    NEW_ENDPOINT = "new_endpoint"  # Add API route/endpoint
    INTEGRATION = "integration"    # Connect existing modules
    UNKNOWN = "unknown"            # Needs LLM to classify


# Keywords for deterministic classification
INTENT_KEYWORDS = {
    TaskType.NEW_MODULE: [
        "add", "create", "implement", "new", "build", "introduce",
        "module", "class", "service", "middleware", "component",
    ],
    TaskType.BUG_FIX: [
        "fix", "bug", "error", "crash", "broken", "wrong", "incorrect",
        "fails", "issue", "regression", "patch",
    ],
    TaskType.REFACTOR: [
        "refactor", "extract", "move", "rename", "reorganize", "split",
        "decouple", "simplify", "clean", "restructure",
    ],
    TaskType.NEW_ENDPOINT: [
        "endpoint", "route", "api", "handler", "view", "controller",
        "get", "post", "put", "delete", "rest", "graphql",
    ],
    TaskType.INTEGRATION: [
        "integrate", "connect", "wire", "hook", "plugin", "middleware",
        "pipe", "chain", "compose",
    ],
}


def classify_intent(description: str) -> TaskType:
    """Classify the intent from keywords. No LLM needed for 80% of cases."""
    desc_lower = description.lower()
    scores = {}

    for task_type, keywords in INTENT_KEYWORDS.items():
        score = sum(1 for kw in keywords if kw in desc_lower)
        # Bonus for strong indicators
        if task_type == TaskType.BUG_FIX and any(w in desc_lower for w in ["fix", "bug", "error"]):
            score += 3
        if task_type == TaskType.NEW_MODULE and any(w in desc_lower for w in ["create", "add new", "implement"]):
            score += 3
        scores[task_type] = score

    best = max(scores, key=scores.get)
    if scores[best] >= 2:
        return best
    return TaskType.UNKNOWN


# ---------------------------------------------------------------------------
# Structure Analysis (from graph — deterministic)
# ---------------------------------------------------------------------------

@dataclass
class StructuralAnalysis:
    """What the graph tells us about the codebase."""
    affected_files: list[str] = field(default_factory=list)
    entry_points: list[str] = field(default_factory=list)     # Functions that need modification
    test_files: list[str] = field(default_factory=list)        # Tests that cover affected code
    co_change_files: list[str] = field(default_factory=list)   # Files that usually change together
    risk_level: str = "LOW"
    total_files: int = 0
    total_symbols: int = 0


def analyze_structure(description: str, repo_path: str, theo_code_bin: str) -> StructuralAnalysis:
    """Use GRAPHCTX to analyze which parts of the codebase are relevant."""
    analysis = StructuralAnalysis()

    try:
        # Get context from GRAPHCTX
        result = subprocess.run(
            [theo_code_bin, "context", repo_path, description],
            capture_output=True, text=True, timeout=60
        )
        output = result.stdout

        # Parse affected files from context
        for line in output.split("\n"):
            if line.startswith("### "):
                file_path = line[4:].strip()
                if file_path and not file_path.startswith("("):
                    analysis.affected_files.append(file_path)

            # Parse stats
            if "Files parsed:" in line:
                parts = line.split(":")
                if len(parts) > 1:
                    try:
                        analysis.total_files = int(parts[-1].strip().split("/")[0])
                    except ValueError:
                        pass
            if "Symbols:" in line:
                parts = line.split(":")
                if len(parts) > 1:
                    try:
                        analysis.total_symbols = int(parts[-1].strip())
                    except ValueError:
                        pass

        # Get impact analysis for the most relevant file
        if analysis.affected_files:
            main_file = analysis.affected_files[0]
            # Extract relative path
            if main_file.startswith(repo_path):
                main_file = main_file[len(repo_path):].lstrip("/")

            impact = subprocess.run(
                [theo_code_bin, "impact", repo_path, main_file],
                capture_output=True, text=True, timeout=30
            )
            for line in impact.stdout.split("\n"):
                if line.strip().startswith("- ") and "test" in line.lower():
                    analysis.test_files.append(line.strip()[2:])
                if line.strip().startswith("- ") and "co_change" in line.lower():
                    analysis.co_change_files.append(line.strip()[2:])

        # Risk assessment
        if len(analysis.affected_files) > 5:
            analysis.risk_level = "HIGH"
        elif len(analysis.affected_files) > 2:
            analysis.risk_level = "MEDIUM"

    except Exception:
        pass

    # Fallback: use grep to find relevant files
    if not analysis.affected_files:
        try:
            keywords = re.findall(r'\b[a-z_]+\b', description.lower())[:5]
            for kw in keywords:
                if len(kw) > 3:
                    result = subprocess.run(
                        ["grep", "-rl", kw, repo_path, "--include=*.py", "--include=*.js", "--include=*.rs"],
                        capture_output=True, text=True, timeout=10
                    )
                    for f in result.stdout.strip().split("\n")[:3]:
                        if f and f not in analysis.affected_files:
                            analysis.affected_files.append(f)
        except Exception:
            pass

    # Find test files
    if not analysis.test_files:
        try:
            result = subprocess.run(
                ["find", repo_path, "-name", "test_*", "-o", "-name", "*_test.*"],
                capture_output=True, text=True, timeout=10
            )
            analysis.test_files = [f for f in result.stdout.strip().split("\n") if f][:5]
        except Exception:
            pass

    return analysis


# ---------------------------------------------------------------------------
# Template Engine (deterministic decomposition)
# ---------------------------------------------------------------------------

@dataclass
class TaskSpec:
    """A decomposed task with precise targets from the graph."""
    id: str
    description: str
    target_files: list[str] = field(default_factory=list)
    depends_on: list[str] = field(default_factory=list)
    risk: str = "LOW"
    task_type: str = "create"  # create, modify, test, verify


def template_new_module(description: str, analysis: StructuralAnalysis) -> list[TaskSpec]:
    """Template: CREATE new module → INTEGRATE → TEST."""
    tasks = []

    # Task 1: Create the new module
    tasks.append(TaskSpec(
        id="task1",
        description=f"Create the new module/file. {description}. Write clean, minimal code with docstrings.",
        target_files=[],  # New file — no existing target
        risk="LOW",
        task_type="create",
    ))

    # Task 2: Integrate into existing code
    if analysis.affected_files:
        integration_files = analysis.affected_files[:3]
        tasks.append(TaskSpec(
            id="task2",
            description=f"Integrate the new module into existing code. Modify: {', '.join(integration_files)}",
            target_files=integration_files,
            depends_on=["task1"],
            risk=analysis.risk_level,
            task_type="modify",
        ))

    # Task 3: Write/update tests
    test_targets = analysis.test_files[:2] if analysis.test_files else ["tests/"]
    tasks.append(TaskSpec(
        id=f"task{len(tasks)+1}",
        description=f"Write tests for the new functionality. Test files: {', '.join(test_targets)}",
        target_files=test_targets,
        depends_on=[t.id for t in tasks],
        risk="LOW",
        task_type="test",
    ))

    return tasks


def template_bug_fix(description: str, analysis: StructuralAnalysis) -> list[TaskSpec]:
    """Template: REPRODUCE → LOCATE → FIX → VERIFY."""
    tasks = []

    # Task 1: Reproduce the bug
    tasks.append(TaskSpec(
        id="task1",
        description=f"Reproduce the bug with a minimal test script. Use reproduce() tool. Bug: {description}",
        target_files=[],
        risk="LOW",
        task_type="verify",
    ))

    # Task 2: Locate and fix
    fix_files = analysis.affected_files[:3] if analysis.affected_files else []
    tasks.append(TaskSpec(
        id="task2",
        description=f"Find and fix the bug. Likely in: {', '.join(fix_files) if fix_files else 'unknown — use search_code'}. Use trace_variable() to understand data flow before editing.",
        target_files=fix_files,
        depends_on=["task1"],
        risk=analysis.risk_level,
        task_type="modify",
    ))

    # Task 3: Verify fix
    tasks.append(TaskSpec(
        id="task3",
        description="Verify the fix by re-running the reproducer from task1. Ensure existing tests still pass.",
        target_files=analysis.test_files[:2],
        depends_on=["task2"],
        risk="LOW",
        task_type="verify",
    ))

    return tasks


def template_refactor(description: str, analysis: StructuralAnalysis) -> list[TaskSpec]:
    """Template: IDENTIFY callers → EXTRACT → UPDATE refs → TEST."""
    tasks = []

    tasks.append(TaskSpec(
        id="task1",
        description=f"Identify all callers and dependents of the code to refactor. Use search_code and grep.",
        target_files=analysis.affected_files[:5],
        risk="LOW",
        task_type="verify",
    ))

    tasks.append(TaskSpec(
        id="task2",
        description=f"Perform the refactoring. {description}. Keep the same external behavior.",
        target_files=analysis.affected_files[:3],
        depends_on=["task1"],
        risk="HIGH",
        task_type="modify",
    ))

    tasks.append(TaskSpec(
        id="task3",
        description="Update all callers and references to use the refactored code.",
        target_files=analysis.co_change_files[:3] + analysis.affected_files[3:5],
        depends_on=["task2"],
        risk="MEDIUM",
        task_type="modify",
    ))

    if analysis.test_files:
        tasks.append(TaskSpec(
            id="task4",
            description=f"Run all tests to verify refactoring didn't break anything. Fix any failures.",
            target_files=analysis.test_files[:3],
            depends_on=["task3"],
            risk="LOW",
            task_type="test",
        ))

    return tasks


def template_new_endpoint(description: str, analysis: StructuralAnalysis) -> list[TaskSpec]:
    """Template: HANDLER → ROUTE → VALIDATION → TEST."""
    tasks = []

    tasks.append(TaskSpec(
        id="task1",
        description=f"Create the handler/view function. {description}",
        target_files=analysis.affected_files[:2],
        risk="LOW",
        task_type="create",
    ))

    tasks.append(TaskSpec(
        id="task2",
        description="Register the route/endpoint and add input validation.",
        target_files=analysis.affected_files[:2],
        depends_on=["task1"],
        risk="MEDIUM",
        task_type="modify",
    ))

    tasks.append(TaskSpec(
        id="task3",
        description="Write tests for the new endpoint: happy path, validation errors, edge cases.",
        target_files=analysis.test_files[:2] if analysis.test_files else [],
        depends_on=["task2"],
        risk="LOW",
        task_type="test",
    ))

    return tasks


TEMPLATES = {
    TaskType.NEW_MODULE: template_new_module,
    TaskType.BUG_FIX: template_bug_fix,
    TaskType.REFACTOR: template_refactor,
    TaskType.NEW_ENDPOINT: template_new_endpoint,
    TaskType.INTEGRATION: template_new_module,  # Same pattern as new module
}


# ---------------------------------------------------------------------------
# LLM Fallback (small model, minimal tokens)
# ---------------------------------------------------------------------------

def llm_decompose(description: str, analysis: StructuralAnalysis,
                   vllm_url: str, model_name: str) -> list[TaskSpec]:
    """Fallback: use LLM to decompose when templates don't match.

    Uses minimal tokens — only asks for task list, not implementation details.
    """
    affected = ", ".join(analysis.affected_files[:5]) or "unknown"
    tests = ", ".join(analysis.test_files[:3]) or "none found"

    prompt = f"""Break this into 3-5 tasks. Return JSON array only.

Feature: {description}
Affected files: {affected}
Test files: {tests}
Risk: {analysis.risk_level}

Return format: [{{"id":"task1","description":"...","target_files":["..."]}}]
JSON only, no explanation:"""

    try:
        resp = requests.post(
            f"{vllm_url}/v1/chat/completions",
            json={
                "model": model_name,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": 500,  # Small — just the task list
                "temperature": 0.1,
            },
            timeout=30
        )
        content = resp.json()["choices"][0]["message"]["content"]

        # Parse JSON
        if "```" in content:
            content = content.split("```")[1]
            if content.startswith("json"):
                content = content[4:]

        raw_tasks = json.loads(content.strip())
        return [
            TaskSpec(
                id=t.get("id", f"task{i+1}"),
                description=t.get("description", ""),
                target_files=t.get("target_files", []),
            )
            for i, t in enumerate(raw_tasks)
        ]
    except Exception as e:
        # Ultimate fallback: single task
        return [TaskSpec(
            id="task1",
            description=description,
            target_files=analysis.affected_files[:3],
        )]


# ---------------------------------------------------------------------------
# Main Decomposer (orchestrates all 3 layers)
# ---------------------------------------------------------------------------

def decompose(
    description: str,
    repo_path: str,
    theo_code_bin: str = "./theo-code",
    vllm_url: str = "http://localhost:8000",
    model_name: str = "",
) -> list[TaskSpec]:
    """Hybrid decomposition: Graph + Templates + LLM fallback.

    Layer 1: GRAPH analyzes structure (always runs — deterministic)
    Layer 2: TEMPLATES decompose by pattern (80% of cases)
    Layer 3: LLM decomposes ambiguous cases (20% fallback)
    """

    # Layer 1: Graph analysis (always)
    analysis = analyze_structure(description, repo_path, theo_code_bin)

    # Layer 2: Intent classification (deterministic)
    intent = classify_intent(description)

    # Layer 3: Template or LLM
    if intent != TaskType.UNKNOWN and intent in TEMPLATES:
        # Deterministic template — no LLM needed
        tasks = TEMPLATES[intent](description, analysis)
        source = f"template:{intent.value}"
    else:
        # LLM fallback — small model, minimal tokens
        tasks = llm_decompose(description, analysis, vllm_url, model_name)
        source = "llm_fallback"

    # Enrich tasks with graph data
    for task in tasks:
        if not task.target_files and analysis.affected_files:
            task.target_files = analysis.affected_files[:2]
        if not task.risk:
            task.risk = analysis.risk_level

    return tasks, intent, analysis, source


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys

    examples = [
        "Add rate limiting middleware to the API with configurable limits per IP",
        "Fix the bug where response.content returns empty bytes on second access",
        "Refactor the search module to extract BM25 into a separate class",
        "Add a new /users/:id endpoint that returns user profile with validation",
        "Implement WebSocket support for real-time notifications",
    ]

    print("=== Hybrid Task Decomposer ===\n")

    for desc in examples:
        intent = classify_intent(desc)
        print(f"Feature: {desc[:60]}...")
        print(f"  Intent: {intent.value}")
        print(f"  Template: {'YES' if intent != TaskType.UNKNOWN else 'NO (LLM needed)'}")
        print()
