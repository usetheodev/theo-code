#!/usr/bin/env python3
"""
Theo Agent — Autonomous Coding Agent that plans, executes, validates, and corrects.

A single entry point that orchestrates the full loop:
  PLAN → EXECUTE → VALIDATE → CORRECT → repeat

Usage:
    # Fix a bug
    python3 theo_agent.py --repo /path/to/repo --task "Fix the auth bug where tokens expire immediately"

    # Implement a feature
    python3 theo_agent.py --repo /path/to/repo --task "Add rate limiting to the API"

    # Refactor
    python3 theo_agent.py --repo /path/to/repo --task "Extract the search logic into a separate module"

Environment:
    VLLM_URL=http://localhost:8000
    THEO_CODE_BIN=./theo-code
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from typing import Optional

import requests

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

VLLM_URL = os.environ.get("VLLM_URL", "http://localhost:8000")
MODEL_NAME = os.environ.get("MODEL_NAME", "Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8")
THEO_CODE_BIN = os.environ.get("THEO_CODE_BIN", "./theo-code")
MAX_ITERATIONS = int(os.environ.get("THEO_MAX_ITERATIONS", "15"))
MAX_SUB_FLOWS = 2

# ---------------------------------------------------------------------------
# Tools
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
        "description": "Create a new file with content. Use for new modules, tests, configs.",
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
        "description": "Create and run a script that demonstrates a bug. ALWAYS do this before fixing.",
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
        "description": "Show ALL assignments and uses of a variable in a file. Shows data flow.",
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
# Tool Execution
# ---------------------------------------------------------------------------

def execute_tool(name: str, args: dict, repo_path: str) -> str:
    """Execute a tool and return result."""
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
    # Auto-create: if file doesn't exist and old_text is empty, create it
    if not os.path.exists(full):
        if not old_text or old_text.strip() == "":
            os.makedirs(os.path.dirname(full) or ".", exist_ok=True)
            with open(full, "w") as f:
                f.write(new_text)
            return f"Created {path} ({new_text.count(chr(10))+1} lines)"
        return f"File not found: {path}. To create a new file, use create_file or edit_file with empty old_text."
    content = open(full).read()
    if old_text not in content:
        # Try with stripped whitespace (common LLM error: tabs vs spaces, trailing whitespace)
        stripped_old = old_text.strip()
        found_line = None
        for i, line in enumerate(content.split("\n")):
            if stripped_old.split("\n")[0].strip() in line:
                found_line = i + 1
                break
        # Try fuzzy: normalize whitespace
        normalized_content = " ".join(content.split())
        normalized_old = " ".join(old_text.split())
        if normalized_old in normalized_content:
            # Whitespace mismatch — find and replace with original whitespace awareness
            lines = content.split("\n")
            old_lines = old_text.strip().split("\n")
            for i in range(len(lines)):
                if old_lines[0].strip() in lines[i]:
                    # Found start — try to match all old_lines
                    match = True
                    for j, ol in enumerate(old_lines):
                        if i + j >= len(lines) or ol.strip() not in lines[i + j]:
                            match = False
                            break
                    if match:
                        # Replace preserving indentation
                        new_lines = new_text.strip().split("\n")
                        indent = len(lines[i]) - len(lines[i].lstrip())
                        indent_str = lines[i][:indent]
                        replaced = lines[:i]
                        for nl in new_lines:
                            if nl.strip():
                                replaced.append(indent_str + nl.strip())
                            else:
                                replaced.append("")
                        replaced.extend(lines[i + len(old_lines):])
                        open(full, "w").write("\n".join(replaced))
                        return f"Edited {path} (fuzzy whitespace match at line {i+1})"

        hint = f" Nearest match at line {found_line}." if found_line else ""
        # Show the actual content around the expected location
        context_hint = ""
        if found_line:
            lines = content.split("\n")
            start = max(0, found_line - 3)
            end = min(len(lines), found_line + 3)
            context_hint = "\nActual content around line {}:\n{}".format(
                found_line, "\n".join(f"  {start+1+i}: {lines[start+i]}" for i in range(end - start)))
        return f"old_text not found in {path}. Must match EXACTLY (check whitespace/indentation).{hint}{context_hint}"
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
    # In-loop search uses grep (instant) — GRAPHCTX context is already in system prompt
    return _search_code_grep(query, repo_path)


def _search_code_grep(query, repo_path):
    """Fast grep-based search fallback for large repos."""
    # Split query into keywords and search for each
    keywords = [w for w in query.split() if len(w) > 2]
    if not keywords:
        keywords = [query]

    results = []
    for kw in keywords[:3]:  # max 3 keywords
        try:
            r = subprocess.run(
                ["grep", "-rn", "--include=*.py", "-l", kw, "."],
                cwd=repo_path, capture_output=True, text=True, timeout=15
            )
            files = r.stdout.strip().split("\n")[:10]
            for f in files:
                if f and f not in results:
                    results.append(f)
        except:
            pass

    if not results:
        return "No results"

    # Show first lines of top matches with context
    output = f"Files matching '{query}':\n"
    for f in results[:8]:
        output += f"\n--- {f} ---\n"
        try:
            r = subprocess.run(
                ["grep", "-n", keywords[0], f],
                cwd=repo_path, capture_output=True, text=True, timeout=5
            )
            output += r.stdout[:500] + "\n"
        except:
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
        return f"REPRODUCER TIMEOUT"
    finally:
        try: os.remove(path)
        except: pass


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
            if not s or s.startswith("#"): continue
            action = "ASSIGN" if f"{variable} =" in line and "==" not in line else \
                     "CHECK" if "if " in s else "RETURN" if "return " in s else "USE"
            trace.append(f"  {i+1:4d} [{action:6s}]: {s[:120]}")
    trace.append(f"=== END ({len(trace)-1} refs) ===")
    return "\n".join(trace)


# ---------------------------------------------------------------------------
# Validation after edit
# ---------------------------------------------------------------------------

def validate_edit(path, old_text, new_text, repo_path):
    """Syntax + lint + diff preview after each edit."""
    full = os.path.join(repo_path, path)
    ext = path.rsplit(".", 1)[-1] if "." in path else ""
    lines = [f"Diff: -{old_text[:60]}... +{new_text[:60]}..."]

    if ext == "py":
        r = subprocess.run(["python3", "-m", "py_compile", full],
                          capture_output=True, text=True, timeout=10, cwd=repo_path)
        if r.returncode != 0:
            err = r.stderr.strip().split("\n")[-1]
            lines.append(f"✗ SYNTAX ERROR: {err}")
            return "\n".join(lines)
        lines.append("✓ Syntax OK")
    elif ext in ("js", "mjs"):
        r = subprocess.run(["node", "-c", full], capture_output=True, text=True, timeout=10)
        if r.returncode != 0:
            lines.append(f"✗ SYNTAX ERROR: {r.stderr.strip().split(chr(10))[0]}")
            return "\n".join(lines)
        lines.append("✓ Syntax OK")

    return "\n".join(lines)


def auto_verify(reproducer_code, repo_path):
    """Re-run last reproducer to check if fix worked."""
    if not reproducer_code:
        return None
    result = _reproduce(reproducer_code, "AUTO-VERIFY", repo_path)
    # Simple heuristic: if it passes and doesn't mention "bug" or "fail" in output
    if "PASSED" in result:
        output_part = result.split("Output:", 1)[1] if "Output:" in result else result
        if not any(w in output_part.lower() for w in ["bug", "fail", "error", "assert"]):
            return True
    return False


# ---------------------------------------------------------------------------
# Undo Stack
# ---------------------------------------------------------------------------

class UndoStack:
    """Track edits per phase, rollback on failure."""
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
            except: pass
        self.edits.clear()


# ---------------------------------------------------------------------------
# Hybrid Decomposer (inline — no separate file needed)
# ---------------------------------------------------------------------------

def decompose_task(description, repo_path):
    """Graph + Template decomposition. Zero LLM tokens."""
    desc = description.lower()

    # Classify intent
    if any(w in desc for w in ["fix", "bug", "error", "broken", "crash"]):
        intent = "bug_fix"
    elif any(w in desc for w in ["refactor", "extract", "move", "rename"]):
        intent = "refactor"
    elif any(w in desc for w in ["add", "create", "implement", "new", "build"]):
        intent = "new_feature"
    else:
        intent = "new_feature"

    # Get affected files from GRAPHCTX — run once, cache result
    affected = []
    graphctx_raw = ""
    try:
        r = subprocess.run([THEO_CODE_BIN, "context", repo_path, description],
                          capture_output=True, text=True, timeout=180)
        for line in r.stdout.split("\n"):
            if line.startswith("### "):
                affected.append(line[4:].strip())
            if line.startswith("--- Item") or line.startswith("## ") or line.startswith("### ") or line.startswith("```"):
                graphctx_raw += line + "\n"
            if line.startswith("--- Timing"):
                break
        # Save pre-computed context for run_loop to reuse
        if graphctx_raw.strip():
            ctx_file = os.path.join(repo_path, ".theo_context.txt")
            with open(ctx_file, "w") as f:
                f.write(graphctx_raw[:8000])
    except:
        pass
    # Grep fallback if GRAPHCTX found nothing
    if not affected:
        keywords = [w for w in description.split() if len(w) > 3][:3]
        for kw in keywords:
            try:
                r = subprocess.run(["grep", "-rn", "--include=*.py", "-l", kw, "."],
                                  cwd=repo_path, capture_output=True, text=True, timeout=10)
                for f in r.stdout.strip().split("\n")[:5]:
                    if f and f not in affected:
                        affected.append(f)
            except: pass

    # Apply template — SIMPLE: one focused task per intent
    files_hint = ', '.join(affected[:5]) if affected else 'use search_code to find relevant files'
    if intent == "bug_fix":
        tasks = [
            {"id": "fix", "desc": f"Find and fix this bug: {description}\n\nLikely files: {files_hint}\n\nSTRATEGY: 1) search_code to locate the bug 2) read_file the relevant code 3) edit_file to fix it 4) done. Do NOT write tests. Do NOT reproduce. Just FIX the code."},
        ]
    elif intent == "refactor":
        tasks = [
            {"id": "refactor", "desc": f"Refactor: {description}\n\nLikely files: {files_hint}"},
        ]
    else:  # new_feature
        tasks = [
            {"id": "implement", "desc": f"Implement: {description}\n\nLikely files: {files_hint}"},
        ]

    return tasks, intent, affected


# ---------------------------------------------------------------------------
# Core Agent Loop
# ---------------------------------------------------------------------------

def _build_context_loop(state, iteration, max_iter, task_desc):
    """Build a Context Loop summary — injected into the conversation to prevent drift.

    Structure:
      WHAT WAS DONE: actions taken so far
      PROBLEMS: what failed and why
      NEXT STEPS: what the agent should do next
    """
    remaining = max_iter - iteration
    files_list = ', '.join(list(state['files_read'])[:8]) or 'none'
    edits_list = ', '.join(state['edits_files']) or 'none'

    # Diagnose problems
    problems = []
    if state["edit_attempts"] > 0 and state["edits_succeeded"] == 0:
        problems.append(f"All {state['edit_attempts']} edit attempts FAILED. " +
                       (f"Reasons: {'; '.join(state['edit_failures'][-3:])}" if state['edit_failures'] else
                        "Likely cause: old_text doesn't match file content exactly."))
    if state["done_blocked"] > 0:
        problems.append(f"done() was blocked {state['done_blocked']}x because no real changes in git diff.")
    if state["searches_done"] > 4 and state["edit_attempts"] == 0:
        problems.append("Too many searches without editing. You have enough context — make the edit.")
    if len(state["files_read"]) > 6 and state["edit_attempts"] == 0:
        problems.append(f"Read {len(state['files_read'])} files but never edited. Stop reading, start fixing.")

    # Next steps based on phase
    if state["edits_succeeded"] > 0:
        next_steps = "You've made successful edits. Call done() with a summary of what you changed."
    elif state["edit_attempts"] > 0:
        next_steps = ("Previous edits failed (old_text mismatch). "
                     "Use read_file on the target file AGAIN, copy the EXACT text, then call edit_file.")
    else:
        next_steps = ("Call edit_file NOW. Pick the most relevant file from your reads and make the change. "
                     "The old_text parameter must be copied EXACTLY from the file.")

    ctx = f"""── CONTEXT LOOP (iteration {iteration}/{max_iter}, {remaining} remaining) ──
TASK: {task_desc[:200]}
PHASE: {state['phase']}
DONE: read {len(state['files_read'])} files, {state['searches_done']} searches, {state['edit_attempts']} edit attempts ({state['edits_succeeded']} succeeded)
FILES READ: {files_list}
FILES EDITED: {edits_list}
{"PROBLEMS: " + " | ".join(problems) if problems else "NO PROBLEMS"}
NEXT: {next_steps}
──"""
    return ctx


def run_loop(repo_path, task_desc, context="", verbose=True):
    """Run agent loop for a single task. Returns (success, summary, edits)."""

    # Get GRAPHCTX context — pre-computed once, reused across iterations
    # Check for pre-computed context file first (set by harness or decompose_task)
    graphctx = ""
    ctx_file = os.path.join(repo_path, ".theo_context.txt")
    if os.path.exists(ctx_file):
        graphctx = open(ctx_file).read()
    else:
        try:
            r = subprocess.run([THEO_CODE_BIN, "context", repo_path, task_desc],
                              capture_output=True, text=True, timeout=120)
            for line in r.stdout.split("\n"):
                if line.startswith("--- Item") or line.startswith("## ") or line.startswith("### ") or line.startswith("```"):
                    graphctx += line + "\n"
                if line.startswith("--- Timing"):
                    break
            # Cache for reuse
            if graphctx.strip():
                with open(ctx_file, "w") as f:
                    f.write(graphctx)
        except: pass
    if len(graphctx) > 8000:
        graphctx = graphctx[:8000] + "\n..."

    system = f"""You are Theo Agent — an autonomous coding agent that fixes bugs and implements features.

TOOLS: search_code, read_file, edit_file, create_file, run_command, done

CRITICAL RULES:
1. You MUST call edit_file or create_file within the first 8 iterations. Reading forever without editing is failure.
2. edit_file old_text must match the file EXACTLY (copy-paste from read_file output).
3. After a successful edit → call done() IMMEDIATELY with a summary.
4. Do NOT write new tests. Do NOT create test files. Focus ONLY on fixing the bug or implementing the feature.
5. Do NOT call done() without making at least one edit. If you can't find the fix, try harder.

WORKFLOW:
1. search_code → find relevant files
2. read_file → understand the code (read ONLY the relevant section, not whole files)
3. edit_file → make the fix
4. done → report what you changed

{context}

=== CODE CONTEXT ===
{graphctx}
=== END ==="""

    messages = [
        {"role": "system", "content": system},
        {"role": "user", "content": f"Task: {task_desc}"},
    ]

    # ── Persistent State Machine ──
    state = {
        "phase": "explore",       # explore → edit → verify → done
        "files_read": set(),      # files the agent has read
        "searches_done": 0,       # search_code calls
        "edit_attempts": 0,       # edit_file calls (including failures)
        "edit_failures": [],      # why edits failed (for learning)
        "edits_succeeded": 0,     # successful edits
        "edits_files": [],        # files actually changed
        "done_blocked": 0,        # times done() was blocked
        "context_loops": [],      # accumulated learnings
    }
    undo = UndoStack()
    edits = []

    # Context Loop interval: emit every N iterations
    CTX_LOOP_INTERVAL = max(3, MAX_ITERATIONS // 4)

    for i in range(1, MAX_ITERATIONS + 1):
        if verbose:
            print(f"  [{i}/{MAX_ITERATIONS}]", end=" ", flush=True)

        # ── Context Loop: emit summary every N iterations ──
        if i > 1 and i % CTX_LOOP_INTERVAL == 0:
            ctx_loop = _build_context_loop(state, i, MAX_ITERATIONS, task_desc)
            state["context_loops"].append(ctx_loop)
            messages.append({"role": "user", "content": ctx_loop})
            if verbose:
                print(f"📋", end=" ")

        # ── Quality Control: phase transition based on state ──
        if state["phase"] == "explore" and (i >= MAX_ITERATIONS // 3 or state["searches_done"] >= 3):
            state["phase"] = "edit"
            messages.append({"role": "user", "content":
                f"PHASE→EDIT: You've read {len(state['files_read'])} files and done {state['searches_done']} searches. "
                f"STOP exploring. Call edit_file NOW. "
                f"Files seen: {', '.join(list(state['files_read'])[:5])}"})

        if state["phase"] == "edit" and i >= (MAX_ITERATIONS * 2) // 3 and state["edits_succeeded"] == 0:
            messages.append({"role": "user", "content":
                "EMERGENCY: Few iterations left, NO successful edit. "
                "Call edit_file NOW. old_text must match EXACTLY — copy from read_file output."})

        # ── Call LLM ──
        try:
            resp = requests.post(f"{VLLM_URL}/v1/chat/completions", json={
                "model": MODEL_NAME, "messages": messages,
                "max_tokens": 4096, "temperature": 0.1,
                "tools": TOOLS, "tool_choice": "auto",
            }, timeout=120)
            data = resp.json()
        except Exception as e:
            if verbose: print(f"API error: {e}")
            continue

        if "error" in data:
            if verbose: print(f"Error: {data['error']}")
            break

        msg = data["choices"][0]["message"]
        content = msg.get("content") or ""
        tool_calls = msg.get("tool_calls") or []

        # Parse Hermes format if needed
        if not tool_calls and "<function=" in content:
            for m in re.finditer(r'<function=(\w+)>(.*?)</function>', content, re.DOTALL):
                fn = m.group(1)
                args = {}
                for pm in re.finditer(r'<parameter=(\w+)>(.*?)</parameter>', m.group(2), re.DOTALL):
                    args[pm.group(1)] = pm.group(2).strip()
                tool_calls.append({"name": fn, "args": args})

        if not tool_calls:
            if verbose: print(f"💬 {content[:80]}")
            messages.append({"role": "assistant", "content": content})
            messages.append({"role": "user", "content": "Use tools to make changes. Call edit_file to fix the code."})
            continue

        messages.append({"role": "assistant", "content": content})

        for tc in tool_calls:
            fn = tc.get("name") or tc.get("function", {}).get("name", "")
            try:
                raw_args = tc.get("args") or tc.get("function", {}).get("arguments", "{}")
                args = json.loads(raw_args) if isinstance(raw_args, str) else raw_args
            except: args = {}

            if verbose: print(f"🔧 {fn}", end=" ", flush=True)

            # ── 1. PROMISE GATE: block done() if promise not fulfilled ──
            if fn == "done":
                # Check: did we actually change any files?
                try:
                    diff = subprocess.run(["git", "diff", "--stat"], cwd=repo_path,
                                         capture_output=True, text=True, timeout=10)
                    has_real_diff = bool(diff.stdout.strip())
                except:
                    has_real_diff = state["edits_succeeded"] > 0

                if not has_real_diff:
                    state["done_blocked"] += 1
                    if verbose: print(f"🚫 BLOCKED({state['done_blocked']})", end=" ")

                    if state["done_blocked"] >= 3:
                        # After 3 blocks, let it go — agent is truly stuck
                        if verbose: print(f"\n  ❌ GAVE UP: no edits after {state['done_blocked']} blocks")
                        return False, "Agent could not produce an edit", edits

                    # Re-inject the task — agent must keep going
                    block_msg = (
                        f"BLOCKED: You called done() but git diff shows NO changes. "
                        f"You have NOT fulfilled the task. You MUST call edit_file to modify code before calling done. "
                        f"Task reminder: {task_desc[:300]}\n"
                        f"Files you've read: {', '.join(list(state['files_read'])[:8])}\n"
                        f"Call edit_file NOW."
                    )
                    messages.append({"role": "user", "content": f"Tool result (done):\n{block_msg}"})
                    continue
                else:
                    # Promise fulfilled — real changes exist
                    summary = args.get("summary", "")
                    if verbose: print(f"\n  ✅ DONE: {summary[:100]}")
                    return True, summary, edits

            # ── 2. Execute tool and update state ──
            result = execute_tool(fn, args, repo_path)

            # Update persistent state
            if fn == "read_file":
                state["files_read"].add(args.get("path", ""))
            elif fn == "search_code":
                state["searches_done"] += 1
            elif fn in ("edit_file", "create_file"):
                state["edit_attempts"] += 1
                if "Edited" in result or "Created" in result or "fuzzy" in result:
                    state["edits_succeeded"] += 1
                    state["phase"] = "verify"
                    old = args.get("old_text", "")
                    new = args.get("new_text", args.get("content", ""))
                    undo.record(args.get("path", ""), old, new, repo_path)
                    edits.append(args.get("path", ""))
                    state["edits_files"].append(args.get("path", ""))

                    # Validate syntax
                    if fn == "edit_file":
                        val = validate_edit(args.get("path",""), old, new, repo_path)
                        result += f"\n{val}"
                        if "SYNTAX ERROR" in val:
                            if verbose: print("✗", end=" ")
                else:
                    # Edit failed — track reason and add guidance
                    fail_reason = result.split("\n")[0][:100] if result else "unknown"
                    state["edit_failures"].append(f"{args.get('path','?')}: {fail_reason}")
                    result += "\nHINT: old_text must match EXACTLY. Use read_file to see the exact content, then copy-paste."

            if verbose: print("→", end=" ")
            messages.append({"role": "user", "content": f"Tool result ({fn}):\n{result[:3000]}"})

        if verbose: print()

    # Max iterations — emit final Context Loop
    final_ctx = _build_context_loop(state, MAX_ITERATIONS, MAX_ITERATIONS, task_desc)
    if verbose:
        print(f"\n  📋 Final Context Loop:")
        for line in final_ctx.split("\n"):
            print(f"     {line}")

    if state["edits_succeeded"] > 0:
        return True, f"Completed with {state['edits_succeeded']} edits (max iterations)", edits
    return False, f"Max iterations without edits. {final_ctx}", edits


# ---------------------------------------------------------------------------
# Main: Plan → Execute → Validate → Correct
# ---------------------------------------------------------------------------

@dataclass
class AgentResult:
    task: str
    success: bool
    tasks_completed: int
    tasks_total: int
    summary: str
    files_changed: list = field(default_factory=list)
    elapsed: float = 0.0


def run(repo_path: str, task: str, verbose: bool = True) -> AgentResult:
    """The full agent loop: Plan → Execute → Validate → Correct."""
    start = time.time()

    if verbose:
        print(f"\n{'='*60}")
        print(f"THEO AGENT")
        print(f"{'='*60}")
        print(f"Repo: {repo_path}")
        print(f"Task: {task[:80]}")
        print()

    # === PLAN ===
    if verbose: print("[PLAN] Decomposing task (Graph + Templates)...")
    tasks, intent, affected = decompose_task(task, repo_path)
    if verbose:
        print(f"  Intent: {intent}")
        print(f"  Affected: {', '.join(affected[:3]) or 'unknown'}")
        print(f"  Tasks: {len(tasks)}")
        for t in tasks:
            print(f"    [{t['id']}] {t['desc'][:70]}")
        print()

    # === EXECUTE + VALIDATE + CORRECT ===
    completed = 0
    all_files = []
    summaries = []
    carry_forward = ""

    for ti, task_spec in enumerate(tasks):
        task_id = task_spec["id"]
        task_desc = task_spec["desc"]

        if verbose:
            print(f"[EXECUTE] {task_id}: {task_desc[:60]}...")

        context = carry_forward

        # Main attempt
        success, summary, edit_files = run_loop(repo_path, task_desc, context, verbose)

        if success:
            completed += 1
            all_files.extend(edit_files)
            summaries.append(f"{task_id}: {summary[:80]}")
            carry_forward += f"\n✅ {task_id} done: {summary[:60]}"
            if verbose: print()
            continue

        # === CORRECT: Freeze/Thaw sub-flow ===
        if verbose:
            print(f"\n  [CORRECT] {task_id} failed. Freezing context, starting sub-flow...")

        undo = UndoStack()

        for attempt in range(1, MAX_SUB_FLOWS + 1):
            sub_context = (
                f"ORIGINAL TASK: {task}\n"
                f"CURRENT SUB-TASK: {task_desc}\n"
                f"PREVIOUS ATTEMPT FAILED. Try a DIFFERENT approach.\n"
                f"Attempt {attempt}/{MAX_SUB_FLOWS}.\n"
                f"{carry_forward}"
            )

            if verbose:
                print(f"  [SUB-FLOW {attempt}]")

            success, summary, edit_files = run_loop(repo_path, task_desc, sub_context, verbose)

            if success:
                completed += 1
                all_files.extend(edit_files)
                summaries.append(f"{task_id} (sub-flow #{attempt}): {summary[:80]}")
                carry_forward += f"\n✅ {task_id} done (via sub-flow): {summary[:60]}"
                if verbose: print()
                break
            else:
                # Rollback sub-flow edits
                undo.rollback()

        if not success:
            carry_forward += f"\n❌ {task_id} failed: {summary[:60]}"
            if verbose:
                print(f"  [FAILED] {task_id} exhausted all sub-flows. Continuing.\n")

    # === SUMMARY ===
    elapsed = time.time() - start
    result = AgentResult(
        task=task,
        success=completed == len(tasks),
        tasks_completed=completed,
        tasks_total=len(tasks),
        summary="\n".join(summaries),
        files_changed=list(set(all_files)),
        elapsed=elapsed,
    )

    if verbose:
        print(f"{'='*60}")
        print(f"RESULT: {completed}/{len(tasks)} tasks completed")
        print(f"{'='*60}")
        for s in summaries:
            print(f"  ✅ {s}")
        if completed < len(tasks):
            print(f"  ❌ {len(tasks) - completed} tasks failed")
        print(f"Files changed: {result.files_changed}")
        print(f"Time: {elapsed:.1f}s")

    return result


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Theo Agent — Autonomous Coding Agent")
    parser.add_argument("--repo", required=True, help="Repository path")
    parser.add_argument("--task", required=True, help="What to do")
    parser.add_argument("--vllm-url", default=VLLM_URL)
    parser.add_argument("--model", default=MODEL_NAME)
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()


    vllm_url = args.vllm_url
    model_name = args.model

    # Check API
    try:
        r = requests.get(f"{VLLM_URL}/v1/models", timeout=5)
        print(f"Model: {r.json()['data'][0]['id']}")
    except Exception as e:
        print(f"ERROR: {e}")
        sys.exit(1)

    result = run(args.repo, args.task, verbose=not args.quiet)

    # Save result
    with open("theo_agent_result.json", "w") as f:
        json.dump(asdict(result), f, indent=2)


if __name__ == "__main__":
    main()
