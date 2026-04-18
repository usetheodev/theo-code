#!/usr/bin/env python3
"""
Task Engine — Manages task execution with tree explosion prevention.

Solves the O(N²) problem where unexpected errors spawn sub-tasks that spawn
more sub-tasks, filling the context and losing the original goal.

Core mechanisms:
1. Context Stack — push/pop context at task boundaries (call stack semantics)
2. Checkpoints — git stash before each task, rollback on failure
3. Circuit Breaker — max depth=3, max alternatives=2, then BLOCK and skip
4. Impact Prediction — warn about risky tasks before executing
5. Spec Reconciliation — update the spec when reality diverges
"""

import json
import os
import subprocess
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class TaskStatus(Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    DONE = "done"
    FAILED = "failed"
    BLOCKED = "blocked"       # Circuit breaker tripped
    SKIPPED = "skipped"       # Dependency failed


@dataclass
class Task:
    id: str
    description: str
    status: TaskStatus = TaskStatus.PENDING
    parent_id: Optional[str] = None
    depth: int = 0
    attempt: int = 0          # Which alternative approach (0, 1, 2)
    max_attempts: int = 2
    children: list = field(default_factory=list)
    result: str = ""
    error: str = ""
    checkpoint: str = ""      # Git stash ref
    context_snapshot: str = "" # Compressed context at task start


# ---------------------------------------------------------------------------
# Circuit Breaker
# ---------------------------------------------------------------------------

MAX_DEPTH = 3          # Max sub-task nesting depth
MAX_ALTERNATIVES = 2   # Max different approaches per failed task
MAX_TOTAL_SUBTASKS = 10  # Max total sub-tasks across entire execution


class CircuitBreaker:
    """Prevents task explosion by limiting depth and breadth."""

    def __init__(self):
        self.total_subtasks = 0
        self.tripped = False
        self.trip_reason = ""

    def can_create_subtask(self, parent_depth: int) -> tuple[bool, str]:
        """Check if we're allowed to create another sub-task."""
        if self.tripped:
            return False, f"Circuit breaker TRIPPED: {self.trip_reason}"

        if parent_depth >= MAX_DEPTH:
            return False, f"Max depth ({MAX_DEPTH}) reached. STOP diving deeper."

        if self.total_subtasks >= MAX_TOTAL_SUBTASKS:
            self.tripped = True
            self.trip_reason = f"Total sub-tasks ({MAX_TOTAL_SUBTASKS}) exceeded"
            return False, self.trip_reason

        return True, ""

    def record_subtask(self):
        self.total_subtasks += 1

    def reset(self):
        self.total_subtasks = 0
        self.tripped = False
        self.trip_reason = ""


# ---------------------------------------------------------------------------
# Checkpoint Manager (Undo Stack — no git dependency)
# ---------------------------------------------------------------------------

@dataclass
class EditRecord:
    """A single file edit that can be undone."""
    file_path: str
    old_text: str
    new_text: str
    offset: int = -1  # Character offset where new_text was placed (-1 = unknown)
    timestamp: float = 0.0


class CheckpointManager:
    """Undo-stack based checkpoints. Zero dependencies — no git, no copies.

    Tracks every edit_file call WITH the offset where the edit was applied.
    Rollback uses the offset to replace the correct occurrence, even when
    the same text appears multiple times in the file.

    Each task gets its own edit stack. Rolling back a task undoes ONLY
    that task's edits, preserving edits from other tasks.
    """

    def __init__(self, repo_path: str):
        self.repo_path = repo_path
        # task_id -> list of edits made during that task
        self.task_edits: dict[str, list[EditRecord]] = {}
        self.current_task: Optional[str] = None

    def save(self, task_id: str) -> str:
        """Start tracking edits for a task (checkpoint = empty edit list)."""
        self.task_edits[task_id] = []
        self.current_task = task_id
        return task_id

    def record_edit(self, file_path: str, old_text: str, new_text: str):
        """Record an edit for the current task.

        Reads the file to find the offset of new_text so rollback
        can target the exact occurrence.
        """
        if not (self.current_task and self.current_task in self.task_edits):
            return

        # Find the offset of new_text in the current file content
        offset = -1
        full_path = os.path.join(self.repo_path, file_path)
        try:
            content = open(full_path).read()
            offset = content.find(new_text)
        except Exception:
            pass

        self.task_edits[self.current_task].append(EditRecord(
            file_path=file_path,
            old_text=old_text,
            new_text=new_text,
            offset=offset,
            timestamp=time.time(),
        ))

    def rollback(self, task_id: str) -> bool:
        """Undo all edits made during a task, in reverse order.

        Uses stored offsets to replace the correct occurrence when the
        same text appears multiple times in a file.
        """
        edits = self.task_edits.get(task_id, [])
        if not edits:
            return True

        success = True
        for edit in reversed(edits):
            full_path = os.path.join(self.repo_path, edit.file_path)
            try:
                content = open(full_path).read()
                if edit.new_text not in content:
                    success = False
                    continue

                # Use offset for precise rollback when available
                idx = content.find(edit.new_text, max(0, edit.offset)) if edit.offset >= 0 else -1

                if idx == -1:
                    # Offset stale (file changed) — find first occurrence
                    idx = content.find(edit.new_text)

                if idx >= 0:
                    content = content[:idx] + edit.old_text + content[idx + len(edit.new_text):]
                    open(full_path, "w").write(content)
                else:
                    success = False
            except Exception:
                success = False

        self.task_edits[task_id] = []
        return success

    def get_edit_count(self, task_id: str) -> int:
        """How many edits were made during a task."""
        return len(self.task_edits.get(task_id, []))

    def get_modified_files(self, task_id: str) -> list[str]:
        """Which files were modified during a task."""
        edits = self.task_edits.get(task_id, [])
        return list(set(e.file_path for e in edits))


# ---------------------------------------------------------------------------
# Context Stack
# ---------------------------------------------------------------------------

class ContextStack:
    """Manages context like a call stack — push on task entry, pop on exit.

    When entering a sub-task, the parent context is COMPRESSED and saved.
    When exiting (success or failure), the parent context is RESTORED.
    This prevents sub-task noise from polluting the parent's context.
    """

    def __init__(self):
        self.stack: list[dict] = []

    def push(self, task_id: str, context: str, goal: str):
        """Save current context before diving into a sub-task."""
        self.stack.append({
            "task_id": task_id,
            "context": context,
            "goal": goal,
            "timestamp": time.time(),
        })

    def pop(self) -> Optional[dict]:
        """Restore parent context after sub-task completes."""
        if self.stack:
            return self.stack.pop()
        return None

    def current_depth(self) -> int:
        return len(self.stack)

    def original_goal(self) -> str:
        """Get the original top-level goal (prevents losing sight of it)."""
        if self.stack:
            return self.stack[0]["goal"]
        return ""

    def breadcrumb(self) -> str:
        """Generate a breadcrumb trail showing where we are in the task tree."""
        if not self.stack:
            return ""
        trail = " → ".join(f"[{s['task_id']}]" for s in self.stack)
        return f"Task path: {trail} (depth {len(self.stack)})"


# ---------------------------------------------------------------------------
# Task Engine
# ---------------------------------------------------------------------------

class TaskEngine:
    """Executes tasks from a spec with explosion prevention.

    Instead of blindly following the spec, it:
    1. Predicts risks before each task (using impact analysis)
    2. Saves checkpoints (git)
    3. Limits sub-task depth (circuit breaker)
    4. Manages context (stack-based push/pop)
    5. Tries alternatives on failure (max 2)
    6. Blocks and skips when stuck (instead of infinite loops)
    """

    def __init__(self, repo_path: str, theo_code_bin: str = "./theo-code"):
        self.repo_path = repo_path
        self.theo_code_bin = theo_code_bin
        self.circuit_breaker = CircuitBreaker()
        self.checkpoint_mgr = CheckpointManager(repo_path)
        self.context_stack = ContextStack()
        self.tasks: dict[str, Task] = {}
        self.execution_log: list[dict] = []

    def add_task(self, task_id: str, description: str, parent_id: str = None) -> Task:
        """Add a task to the execution plan."""
        parent_depth = self.tasks[parent_id].depth if parent_id else 0
        task = Task(
            id=task_id,
            description=description,
            parent_id=parent_id,
            depth=parent_depth + 1 if parent_id else 0,
        )
        self.tasks[task_id] = task
        if parent_id and parent_id in self.tasks:
            self.tasks[parent_id].children.append(task_id)
        return task

    def predict_risk(self, task: Task) -> dict:
        """Use GRAPHCTX impact analysis to predict task risk."""
        try:
            # Extract keywords from task description
            keywords = task.description.split()[:5]
            query = " ".join(keywords)

            result = subprocess.run(
                [self.theo_code_bin, "context", self.repo_path, query],
                capture_output=True, text=True, timeout=30
            )

            # Count affected files from context
            file_count = result.stdout.count("### ")
            risk_level = "LOW" if file_count <= 2 else "MEDIUM" if file_count <= 5 else "HIGH"

            return {
                "risk": risk_level,
                "affected_files": file_count,
                "recommendation": f"{'Proceed' if risk_level == 'LOW' else 'Proceed with caution' if risk_level == 'MEDIUM' else 'Consider splitting into smaller tasks'}"
            }
        except Exception:
            return {"risk": "UNKNOWN", "affected_files": 0, "recommendation": "Proceed"}

    def execute_task(self, task_id: str, agent_fn) -> TaskStatus:
        """Execute a task with Freeze/Thaw context isolation.

        Main flow context is FROZEN before any sub-problem resolution.
        Sub-problems run in a FRESH context (no noise from main flow).
        After resolution, main context is THAWED (perfectly preserved).
        Only the RESULT of the sub-flow is merged back.
        """
        task = self.tasks.get(task_id)
        if not task:
            return TaskStatus.FAILED

        # Check circuit breaker (prevents infinite depth, NOT blocking)
        if task.depth > 0:
            can_proceed, reason = self.circuit_breaker.can_create_subtask(task.depth - 1)
            if not can_proceed:
                task.status = TaskStatus.FAILED
                task.error = reason
                self.log(task_id, "DEPTH_LIMIT", reason)
                return TaskStatus.FAILED

        # Predict risk
        risk = self.predict_risk(task)
        self.log(task_id, "RISK", f"{risk['risk']} ({risk['affected_files']} files)")

        # Save checkpoint
        self.checkpoint_mgr.save(task_id)

        # === FREEZE main context ===
        frozen_breadcrumb = self.context_stack.breadcrumb()
        frozen_goal = self.context_stack.original_goal() or task.description
        self.context_stack.push(task_id, "", task.description)

        # Build context for the MAIN attempt
        context = self.build_task_context(task, frozen_breadcrumb, frozen_goal, risk)

        task.status = TaskStatus.IN_PROGRESS
        self.log(task_id, "START", f"depth={task.depth}")

        # === MAIN ATTEMPT ===
        success, result, error = agent_fn(task.description, context)

        if success:
            task.status = TaskStatus.DONE
            task.result = result
            self.log(task_id, "DONE", result[:100])
            self.context_stack.pop()
            return TaskStatus.DONE

        # === MAIN FAILED — enter sub-flow resolution ===
        self.log(task_id, "FAILED", f"Main attempt: {error[:100]}")

        # Rollback main attempt edits
        self.checkpoint_mgr.rollback(task_id)
        self.checkpoint_mgr.save(task_id)  # fresh checkpoint for sub-flow

        # === FREEZE main context (preserve it perfectly) ===
        # The main flow's messages/state are NOT passed to the sub-flow
        self.log(task_id, "FREEZE", "Main context frozen. Starting fresh sub-flow.")

        for attempt in range(1, task.max_attempts + 1):
            task.attempt = attempt

            # === CREATE fresh sub-context (clean slate) ===
            # Only contains: the error + the task description + relevant code
            # NO noise from the main flow's conversation history
            sub_context = self.build_sub_flow_context(task, error, attempt, frozen_goal)

            self.log(task_id, f"SUB_FLOW_{attempt}", f"Fresh context, resolving: {error[:80]}")
            self.circuit_breaker.record_subtask()

            # Execute sub-flow in clean context
            sub_success, sub_result, sub_error = agent_fn(
                f"[SUB-FLOW] Fix this error from task '{task.description}': {error}",
                sub_context
            )

            if sub_success:
                # === THAW main context + merge only the result ===
                task.status = TaskStatus.DONE
                task.result = f"(resolved via sub-flow #{attempt}) {sub_result}"
                self.log(task_id, "THAW", f"Sub-flow resolved. Main context restored.")
                self.log(task_id, "DONE", task.result[:100])
                self.context_stack.pop()
                return TaskStatus.DONE

            # Sub-flow failed — rollback its edits, try next approach
            self.checkpoint_mgr.rollback(task_id)
            self.checkpoint_mgr.save(task_id)
            error = sub_error or error  # Update error for next attempt
            self.log(task_id, f"SUB_FLOW_{attempt}_FAILED", sub_error[:100] if sub_error else "no detail")

        # All sub-flows exhausted — THAW main context, mark as failed but continue
        self.context_stack.pop()
        task.status = TaskStatus.FAILED
        task.error = f"All {task.max_attempts} sub-flows failed: {error}"
        self.log(task_id, "THAW", "All sub-flows exhausted. Main context restored. Continuing.")
        return TaskStatus.FAILED

    def build_sub_flow_context(self, task: Task, error: str, attempt: int, original_goal: str) -> str:
        """Build a FRESH context for a sub-flow. Clean slate — no main flow noise.

        Contains only:
        1. What went wrong (the error)
        2. What we're trying to do (task description)
        3. What approach to try (based on attempt number)
        4. The original goal (so we don't lose sight of it)
        """
        lines = [
            f"ORIGINAL GOAL: {original_goal}",
            f"CURRENT TASK: {task.description}",
            f"",
            f"ERROR TO RESOLVE: {error}",
            f"",
            f"This is sub-flow attempt #{attempt}/{task.max_attempts}.",
        ]

        if attempt == 1:
            lines.append("Try the most direct fix for this error.")
        elif attempt == 2:
            lines.append("The direct fix didn't work. Try a WORKAROUND or alternative approach.")
            lines.append("Think about: different library, different pattern, skip the problematic part.")
        else:
            lines.append(f"Previous {attempt-1} approaches failed. Try something completely different.")
            lines.append("Consider: simplifying the requirement, using a stub, or deferring this part.")

        lines.extend([
            "",
            "RULES FOR SUB-FLOW:",
            "- Fix ONLY this specific error. Don't change unrelated code.",
            "- Make the MINIMAL change needed.",
            "- If you can't fix it, explain what's blocking and call done.",
            "- Your changes will be ROLLED BACK if you fail, so the main flow is safe.",
        ])

        return "\n".join(lines)

    def execute_spec(self, spec_tasks: list[dict], agent_fn) -> dict:
        """Execute an entire spec with explosion prevention.

        Args:
            spec_tasks: List of {"id": "task1", "description": "..."} dicts
            agent_fn: Function(description, context) -> (success, result, error)

        Returns:
            Summary dict with results per task
        """
        # Add all tasks
        for t in spec_tasks:
            self.add_task(t["id"], t["description"])

        results = {}
        failed_count = 0
        completed_results = []  # Track what was done for context carry-forward

        for t in spec_tasks:
            task = self.tasks[t["id"]]

            # Build carry-forward context: what previous tasks accomplished
            # This is the FROZEN main flow context — clean and minimal
            carry_forward = ""
            if completed_results:
                carry_forward = "Previous tasks completed:\n"
                for cr in completed_results[-3:]:  # Last 3 results only
                    carry_forward += f"  ✅ {cr['id']}: {cr['summary'][:80]}\n"

            # Store carry-forward in context stack for the task
            task_context_extra = carry_forward

            status = self.execute_task(t["id"], lambda desc, ctx: agent_fn(
                desc, ctx + "\n\n" + task_context_extra if task_context_extra else ctx
            ))

            results[t["id"]] = {
                "status": status.value,
                "result": task.result[:200] if task.result else "",
                "error": task.error[:200] if task.error else "",
                "attempts": task.attempt + 1,
                "sub_flows_used": task.attempt,
            }

            if status == TaskStatus.DONE:
                completed_results.append({
                    "id": t["id"],
                    "summary": task.result[:100],
                })
            elif status == TaskStatus.FAILED:
                failed_count += 1
                # DON'T skip future tasks — they might still work
                # Just note the failure in carry-forward
                completed_results.append({
                    "id": t["id"],
                    "summary": f"FAILED: {task.error[:80]}",
                })

        # Summary
        done_count = sum(1 for r in results.values() if r["status"] == "done")
        total = len(results)

        return {
            "tasks": results,
            "summary": f"{done_count}/{total} tasks completed",
            "failed": failed_count,
            "circuit_breaker_tripped": self.circuit_breaker.tripped,
            "execution_log": self.execution_log,
        }

    def build_task_context(self, task: Task, breadcrumb: str, original_goal: str, risk: dict) -> str:
        """Build focused context for the agent, including tree position."""
        lines = []

        if breadcrumb:
            lines.append(breadcrumb)

        if task.depth > 0:
            lines.append(f"\n⚠ You are in a SUB-TASK (depth {task.depth}/{MAX_DEPTH}).")
            lines.append(f"Original goal: {original_goal}")
            lines.append(f"Current sub-task: {task.description}")
            lines.append(f"DO NOT create more sub-tasks. Fix this directly or report it as blocked.")
        else:
            lines.append(f"Task: {task.description}")

        if risk["risk"] != "LOW":
            lines.append(f"\nRisk: {risk['risk']} — {risk['recommendation']}")

        if task.attempt > 0:
            lines.append(f"\n⚠ This is attempt #{task.attempt + 1}. Previous approach failed.")
            lines.append("Try a FUNDAMENTALLY different approach, not a variation of the same fix.")

        return "\n".join(lines)

    def log(self, task_id: str, event: str, detail: str):
        """Log execution events."""
        self.execution_log.append({
            "task_id": task_id,
            "event": event,
            "detail": detail,
            "timestamp": time.time(),
            "depth": self.context_stack.current_depth(),
        })
        print(f"  [{task_id}] {event}: {detail[:120]}")


# ---------------------------------------------------------------------------
# Usage example
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    # Example: Execute a spec with the task engine
    spec = [
        {"id": "task1", "description": "Add user authentication to the API"},
        {"id": "task2", "description": "Configure JWT token validation"},
        {"id": "task3", "description": "Add rate limiting middleware"},
        {"id": "task4", "description": "Write integration tests"},
    ]

    def mock_agent(description, context):
        """Mock agent that simulates task execution."""
        print(f"    Agent: {description[:60]}...")
        if "JWT" in description:
            return False, "", "jwt library not installed"
        return True, f"Completed: {description[:50]}", ""

    engine = TaskEngine(".", "./theo-code")
    result = engine.execute_spec(spec, mock_agent)

    print(f"\n=== RESULT: {result['summary']} ===")
    for tid, r in result["tasks"].items():
        print(f"  {tid}: {r['status']} {'— ' + r['error'] if r['error'] else ''}")
