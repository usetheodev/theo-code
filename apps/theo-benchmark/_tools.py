#!/usr/bin/env python3
"""
Shared tool definitions and utilities for benchmark modules.

Extracted from the former theo_agent.py so that theo_mentor.py (interactive
pair-programming mode) can reuse tool schemas and helper functions without
pulling in a full Python agent reimplementation.

IMPORTANT: These are ONLY used by theo_mentor.py (interactive mode).
All autonomous benchmarks go through `_headless.py` → `theo --headless`.
"""

from __future__ import annotations

import os
import re
import subprocess
from typing import Optional

# ---------------------------------------------------------------------------
# Config (from environment)
# ---------------------------------------------------------------------------

VLLM_URL = os.environ.get("VLLM_URL", "http://localhost:8000")
MODEL_NAME = os.environ.get(
    "MODEL_NAME", "Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8"
)
THEO_CODE_BIN = os.environ.get("THEO_CODE_BIN", "./theo-code")
MAX_ITERATIONS = int(os.environ.get("THEO_MAX_ITERATIONS", "15"))

# ---------------------------------------------------------------------------
# Tool schemas (OpenAI function-calling format)
# ---------------------------------------------------------------------------

TOOLS = [
    {"type": "function", "function": {
        "name": "read_file",
        "description": "Read file with line numbers. Use start_line/end_line for large files.",
        "parameters": {"type": "object", "properties": {
            "path": {"type": "string", "description": "File path"},
            "start_line": {"type": "string", "description": "Start line (1-based)"},
            "end_line": {"type": "string", "description": "End line"},
        }, "required": ["path"]}
    }},
    {"type": "function", "function": {
        "name": "create_file",
        "description": "Create a new file with content.",
        "parameters": {"type": "object", "properties": {
            "path": {"type": "string", "description": "File path to create"},
            "content": {"type": "string", "description": "File content"},
        }, "required": ["path", "content"]}
    }},
    {"type": "function", "function": {
        "name": "edit_file",
        "description": "Replace exact text in a file. old_text must match exactly.",
        "parameters": {"type": "object", "properties": {
            "path": {"type": "string", "description": "File path"},
            "old_text": {"type": "string", "description": "Exact text to find"},
            "new_text": {"type": "string", "description": "Replacement text"},
        }, "required": ["path", "old_text", "new_text"]}
    }},
    {"type": "function", "function": {
        "name": "run_command",
        "description": "Run shell command (grep, tests, build).",
        "parameters": {"type": "object", "properties": {
            "command": {"type": "string", "description": "Shell command"},
        }, "required": ["command"]}
    }},
    {"type": "function", "function": {
        "name": "search_code",
        "description": "Search codebase with GRAPHCTX. Returns relevant files and code.",
        "parameters": {"type": "object", "properties": {
            "query": {"type": "string", "description": "What to search for"},
        }, "required": ["query"]}
    }},
    {"type": "function", "function": {
        "name": "reproduce",
        "description": "Create and run a script that demonstrates a bug.",
        "parameters": {"type": "object", "properties": {
            "code": {"type": "string", "description": "Python code to run"},
            "description": {"type": "string", "description": "What this tests"},
        }, "required": ["code", "description"]}
    }},
    {"type": "function", "function": {
        "name": "debug",
        "description": "Insert temp debug print, run command, capture output, remove print.",
        "parameters": {"type": "object", "properties": {
            "file": {"type": "string", "description": "File to instrument"},
            "line": {"type": "integer", "description": "Line number"},
            "expression": {"type": "string", "description": "Expression to print"},
            "run_command": {"type": "string", "description": "Command to run after"},
        }, "required": ["file", "line", "expression", "run_command"]}
    }},
    {"type": "function", "function": {
        "name": "trace_variable",
        "description": "Show ALL assignments and uses of a variable in a file.",
        "parameters": {"type": "object", "properties": {
            "file": {"type": "string", "description": "File path"},
            "function": {"type": "string", "description": "Function name"},
            "variable": {"type": "string", "description": "Variable to trace"},
        }, "required": ["file", "function", "variable"]}
    }},
    {"type": "function", "function": {
        "name": "done",
        "description": "Call when task is complete.",
        "parameters": {"type": "object", "properties": {
            "summary": {"type": "string", "description": "What was accomplished"},
        }, "required": ["summary"]}
    }},
]

# ---------------------------------------------------------------------------
# Tool execution
# ---------------------------------------------------------------------------


def execute_tool(name: str, args: dict, repo_path: str) -> str:
    """Execute a tool and return result string."""
    try:
        if name == "read_file":
            return _read_file(args["path"], repo_path, args.get("start_line"), args.get("end_line"))
        elif name == "create_file":
            return _create_file(args["path"], args["content"], repo_path)
        elif name == "edit_file":
            return _edit_file(args["path"], args["old_text"], args["new_text"], repo_path)
        elif name == "run_command":
            return _run_command(args["command"], repo_path)
        elif name == "search_code":
            return _search_code(args["query"], repo_path)
        elif name == "reproduce":
            return _reproduce(args["code"], args.get("description", ""), repo_path)
        elif name == "debug":
            return _debug(args["file"], int(args.get("line", 1)), args["expression"], args["run_command"], repo_path)
        elif name == "trace_variable":
            return _trace_variable(args["file"], args["function"], args["variable"], repo_path)
        elif name == "done":
            return f"DONE: {args['summary']}"
        return f"Unknown tool: {name}"
    except Exception as e:
        return f"Error in {name}: {e}"


# ---------------------------------------------------------------------------
# Tool implementations
# ---------------------------------------------------------------------------


def _read_file(path, repo_path, start_line=None, end_line=None):
    full = os.path.join(repo_path, path)
    if not os.path.exists(full):
        return f"File not found: {path}"
    lines = open(full).read().split("\n")
    total = len(lines)
    if start_line:
        s = max(0, int(str(start_line)) - 1)
        e = int(str(end_line)) if end_line else min(s + 100, total)
        numbered = [f"{s+1+i:4d} | {l}" for i, l in enumerate(lines[s:e])]
        return f"[Lines {s+1}-{min(e,total)} of {total}]\n" + "\n".join(numbered)
    if total > 150:
        head = [f"{i+1:4d} | {lines[i]}" for i in range(50)]
        tail = [f"{total-19+i:4d} | {lines[total-20+i]}" for i in range(20)]
        return "\n".join(head) + f"\n\n... ({total-70} lines omitted)\n\n" + "\n".join(tail)
    return "\n".join(f"{i+1:4d} | {l}" for i, l in enumerate(lines))


def _create_file(path, content, repo_path):
    full = os.path.join(repo_path, path)
    os.makedirs(os.path.dirname(full) or ".", exist_ok=True)
    with open(full, "w") as f:
        f.write(content)
    lines = content.count("\n") + 1
    return f"Created {path} ({lines} lines)"


def _edit_file(path, old_text, new_text, repo_path):
    full = os.path.join(repo_path, path)
    if not os.path.exists(full):
        if not old_text or old_text.strip() == "":
            os.makedirs(os.path.dirname(full) or ".", exist_ok=True)
            with open(full, "w") as f:
                f.write(new_text)
            return f"Created {path} ({new_text.count(chr(10))+1} lines)"
        return f"File not found: {path}."
    content = open(full).read()
    if old_text not in content:
        # Fuzzy whitespace matching
        normalized_content = " ".join(content.split())
        normalized_old = " ".join(old_text.split())
        if normalized_old in normalized_content:
            lines = content.split("\n")
            old_lines = old_text.strip().split("\n")
            for i in range(len(lines)):
                if old_lines[0].strip() in lines[i]:
                    match = True
                    for j, ol in enumerate(old_lines):
                        if i + j >= len(lines) or ol.strip() not in lines[i + j]:
                            match = False
                            break
                    if match:
                        new_lines = new_text.strip().split("\n")
                        indent = len(lines[i]) - len(lines[i].lstrip())
                        indent_str = lines[i][:indent]
                        replaced = lines[:i]
                        for nl in new_lines:
                            replaced.append(indent_str + nl.strip() if nl.strip() else "")
                        replaced.extend(lines[i + len(old_lines):])
                        open(full, "w").write("\n".join(replaced))
                        return f"Edited {path} (fuzzy match at line {i+1})"
        return f"old_text not found in {path}. Must match EXACTLY."
    open(full, "w").write(content.replace(old_text, new_text, 1))
    return f"Edited {path}"


def _run_command(command, repo_path):
    dangerous = ["rm -rf /", "dd if=", "mkfs", "> /dev"]
    if any(d in command for d in dangerous):
        return f"Blocked: {command}"
    try:
        r = subprocess.run(command, shell=True, cwd=repo_path, capture_output=True, text=True, timeout=120)
        out = (r.stdout + r.stderr)[:3000]
        return out or "(no output)"
    except subprocess.TimeoutExpired:
        return "Timeout (120s)"


def _search_code(query, repo_path):
    keywords = [w for w in query.split() if len(w) > 2]
    if not keywords:
        keywords = [query]
    results = []
    for kw in keywords[:3]:
        try:
            r = subprocess.run(
                ["grep", "-rn", "--include=*.py", "-l", kw, "."],
                cwd=repo_path, capture_output=True, text=True, timeout=15
            )
            for f in r.stdout.strip().split("\n")[:10]:
                if f and f not in results:
                    results.append(f)
        except Exception:
            pass
    if not results:
        return "No results"
    output = f"Files matching '{query}':\n"
    for f in results[:8]:
        output += f"\n--- {f} ---\n"
        try:
            r = subprocess.run(
                ["grep", "-n", keywords[0], f],
                cwd=repo_path, capture_output=True, text=True, timeout=5
            )
            output += r.stdout[:500] + "\n"
        except Exception:
            pass
    return output[:6000]


def _reproduce(code, description, repo_path):
    path = os.path.join(repo_path, "_repro.py")
    try:
        with open(path, "w") as f:
            f.write(f"import sys; sys.path.insert(0, '.')\n# {description}\n{code}")
        r = subprocess.run(["python3", path], capture_output=True, text=True, timeout=30, cwd=repo_path)
        out = (r.stdout + r.stderr)[:2000]
        status = "PASSED" if r.returncode == 0 else f"FAILED (exit {r.returncode})"
        return f"=== REPRODUCER: {description} ===\n{status}\n{out}\n=== END ==="
    except subprocess.TimeoutExpired:
        return "REPRODUCER TIMEOUT"
    finally:
        try:
            os.remove(path)
        except Exception:
            pass


def _debug(file, line, expression, run_cmd, repo_path):
    full = os.path.join(repo_path, file)
    if not os.path.exists(full):
        return f"File not found: {file}"
    original = open(full).read()
    try:
        lines = original.split("\n")
        idx = max(0, line - 1)
        indent = len(lines[idx]) - len(lines[idx].lstrip()) if idx < len(lines) else 0
        debug_line = f'{" "*indent}print(f"DEBUG[{file}:{line}]: {expression} = {{{expression}}}")'
        lines.insert(idx, debug_line)
        open(full, "w").write("\n".join(lines))
        r = subprocess.run(run_cmd, shell=True, capture_output=True, text=True, timeout=30, cwd=repo_path)
        out = r.stdout + r.stderr
        debug_out = [l for l in out.split("\n") if "DEBUG[" in l][:10]
        return "=== DEBUG ===\n" + "\n".join(debug_out) + ("\n(no output)" if not debug_out else "") + "\n=== END ==="
    finally:
        open(full, "w").write(original)


def _trace_variable(file, function, variable, repo_path):
    full = os.path.join(repo_path, file)
    if not os.path.exists(full):
        return f"File not found: {file}"
    lines = open(full).read().split("\n")
    pattern = re.compile(r'\b' + re.escape(variable) + r'\b')
    trace = [f"=== TRACE: '{variable}' in {file} ==="]
    for i, line in enumerate(lines):
        if pattern.search(line):
            s = line.strip()
            if not s or s.startswith("#"):
                continue
            action = "ASSIGN" if f"{variable} =" in line and "==" not in line else \
                     "CHECK" if "if " in s else "RETURN" if "return " in s else "USE"
            trace.append(f"  {i+1:4d} [{action:6s}]: {s[:120]}")
    trace.append(f"=== END ({len(trace)-1} refs) ===")
    return "\n".join(trace)


# ---------------------------------------------------------------------------
# Validation helpers
# ---------------------------------------------------------------------------


def validate_edit(path, old_text, new_text, repo_path):
    """Syntax + lint check after edit."""
    full = os.path.join(repo_path, path)
    ext = path.rsplit(".", 1)[-1] if "." in path else ""
    lines = [f"Diff: -{old_text[:60]}... +{new_text[:60]}..."]

    if ext == "py":
        r = subprocess.run(["python3", "-m", "py_compile", full],
                           capture_output=True, text=True, timeout=10, cwd=repo_path)
        if r.returncode != 0:
            err = r.stderr.strip().split("\n")[-1]
            lines.append(f"SYNTAX ERROR: {err}")
            return "\n".join(lines)
        lines.append("Syntax OK")

    return "\n".join(lines)


def auto_verify(reproducer_code, repo_path):
    """Re-run last reproducer to check if fix worked."""
    if not reproducer_code:
        return None
    result = _reproduce(reproducer_code, "AUTO-VERIFY", repo_path)
    if "PASSED" in result:
        output_part = result.split("Output:", 1)[1] if "Output:" in result else result
        if not any(w in output_part.lower() for w in ["bug", "fail", "error", "assert"]):
            return True
    return False


# ---------------------------------------------------------------------------
# Undo Stack
# ---------------------------------------------------------------------------


class UndoStack:
    """Track edits, rollback on failure."""

    def __init__(self):
        self.edits: list[tuple[str, str, str, str]] = []  # (path, old, new, repo)

    def record(self, path, old_text, new_text, repo_path):
        self.edits.append((path, old_text, new_text, repo_path))

    def rollback(self):
        for path, old_text, new_text, repo_path in reversed(self.edits):
            full = os.path.join(repo_path, path)
            try:
                content = open(full).read()
                if new_text in content:
                    open(full, "w").write(content.replace(new_text, old_text, 1))
            except Exception:
                pass
        self.edits.clear()


# ---------------------------------------------------------------------------
# Simple decomposer (for mentor mode only)
# ---------------------------------------------------------------------------


def decompose_task(description, repo_path):
    """Graph + Template decomposition. Zero LLM tokens."""
    desc = description.lower()

    if any(w in desc for w in ["fix", "bug", "error", "broken", "crash"]):
        intent = "bug_fix"
    elif any(w in desc for w in ["refactor", "extract", "move", "rename"]):
        intent = "refactor"
    else:
        intent = "new_feature"

    # Get affected files from GRAPHCTX
    theo_bin = os.environ.get("THEO_CODE_BIN", THEO_CODE_BIN)
    affected = []
    try:
        r = subprocess.run(
            [theo_bin, "context", repo_path, description],
            capture_output=True, text=True, timeout=60
        )
        for line in r.stdout.split("\n"):
            if line.startswith("### "):
                f = line[4:].strip()
                if f:
                    affected.append(f)
    except Exception:
        pass

    # Template-based decomposition
    if intent == "bug_fix":
        tasks = [
            {"id": "reproduce", "desc": f"Reproduce: {description}"},
            {"id": "fix", "desc": f"Fix: {description}"},
            {"id": "verify", "desc": "Verify fix passes tests"},
        ]
    elif intent == "refactor":
        tasks = [
            {"id": "identify", "desc": "Identify callers and dependents"},
            {"id": "refactor", "desc": f"Refactor: {description}"},
            {"id": "test", "desc": "Run tests to verify no regressions"},
        ]
    else:
        tasks = [
            {"id": "implement", "desc": f"Implement: {description}"},
            {"id": "integrate", "desc": "Integrate into existing code"},
            {"id": "test", "desc": "Write and run tests"},
        ]

    return tasks, intent, affected
