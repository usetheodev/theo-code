#!/usr/bin/env python3
"""
Theo Code Agent Loop — Autonomous coding agent powered by GRAPHCTX context.

Takes a real GitHub issue, assembles context via Theo Code, then runs
an agent loop where Qwen 3 uses tools to read, edit, and test code
until the issue is resolved.

Usage:
    python3 benchmark/agent_loop.py --repo <path> --issue "<description>" [--vllm-url <url>]
"""

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

import requests

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

VLLM_URL = os.environ.get("VLLM_URL", "http://localhost:8000")
MODEL_NAME = os.environ.get("MODEL_NAME", "cpatonn/Qwen3-Coder-30B-A3B-Instruct-AWQ-4bit")
THEO_CODE_BIN = os.environ.get("THEO_CODE_BIN", "./target/release/theo-code")
MAX_ITERATIONS = 25
MAX_TOKENS_PER_RESPONSE = 4096

# ---------------------------------------------------------------------------
# Tools definition (OpenAI function calling format)
# ---------------------------------------------------------------------------

TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a file. For large files, use start_line and end_line to read specific line ranges.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file from repo root"
                    },
                    "start_line": {
                        "type": "string",
                        "description": "Start line number (1-based). Optional."
                    },
                    "end_line": {
                        "type": "string",
                        "description": "End line number. Optional."
                    }
                },
                "required": ["path"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "edit_file",
            "description": "Replace text in a file. Provide the exact old text to find and the new text to replace it with.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact text to find in the file (must match exactly)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Text to replace old_text with"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "run_command",
            "description": "Run a shell command in the repo directory. Use for running tests, building, or checking output.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    }
                },
                "required": ["command"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "search_code",
            "description": "Search the codebase for relevant code using Theo Code's GRAPHCTX engine. Returns files and functions matching your query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of what you're looking for"
                    }
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "reproduce",
            "description": "Create and run a Python script that reproduces the bug. ALWAYS use this BEFORE attempting a fix. The script should demonstrate the incorrect behavior.",
            "parameters": {
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Python code that reproduces the bug. Include assertions that fail due to the bug."
                    },
                    "description": {
                        "type": "string",
                        "description": "One-line description of what the reproducer tests"
                    }
                },
                "required": ["code", "description"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "debug",
            "description": "Insert a temporary debug print in a file, run a command, capture output, then remove the print. Use this to trace execution flow without permanent changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "File to instrument"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number to insert debug print BEFORE"
                    },
                    "expression": {
                        "type": "string",
                        "description": "Python expression to print (e.g., 'type(value)', 'locals()', 'self.__class__.__name__')"
                    },
                    "run_command": {
                        "type": "string",
                        "description": "Command to run after inserting the debug print (e.g., 'python3 repro.py')"
                    }
                },
                "required": ["file", "line", "expression", "run_command"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "trace_variable",
            "description": "Trace ALL assignments and uses of a variable within a function. Shows data flow: where the variable is set, checked, and used. Essential for understanding multi-step bugs.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "File path"},
                    "function": {"type": "string", "description": "Function name (e.g., '__init__')"},
                    "variable": {"type": "string", "description": "Variable to trace (e.g., 'self.default')"}
                },
                "required": ["file", "function", "variable"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "done",
            "description": "Call this when you have finished fixing the issue. Provide a summary of what you changed.",
            "parameters": {
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Summary of the changes made to fix the issue"
                    }
                },
                "required": ["summary"]
            }
        }
    }
]

# ---------------------------------------------------------------------------
# Tool execution
# ---------------------------------------------------------------------------

def execute_tool(name: str, args: dict, repo_path: str) -> str:
    """Execute a tool and return the result as a string."""
    try:
        if name == "read_file":
            return tool_read_file(args["path"], repo_path, args.get("start_line"), args.get("end_line"))
        elif name == "edit_file":
            return tool_edit_file(args["path"], args["old_text"], args["new_text"], repo_path)
        elif name == "run_command":
            return tool_run_command(args["command"], repo_path)
        elif name == "search_code":
            return tool_search_code(args["query"], repo_path)
        elif name == "reproduce":
            return tool_reproduce(args["code"], args.get("description", "reproducer"), repo_path)
        elif name == "debug":
            return tool_debug(
                args["file"], int(args.get("line", 1)),
                args["expression"], args["run_command"], repo_path
            )
        elif name == "trace_variable":
            return tool_trace_variable(
                args["file"], args["function"], args["variable"], repo_path
            )
        elif name == "done":
            return f"DONE: {args['summary']}"
        else:
            return f"Unknown tool: {name}"
    except Exception as e:
        return f"Error executing {name}: {e}"


def tool_read_file(path: str, repo_path: str, start_line=None, end_line=None) -> str:
    """Fix 5: Robust read_file with proper int parsing and line numbers."""
    full_path = os.path.join(repo_path, path)
    if not os.path.exists(full_path):
        return f"File not found: {path}. Use run_command('find . -name \"filename\"') to locate it."
    try:
        content = open(full_path).read()
        lines = content.split("\n")
        total = len(lines)

        # Parse start_line/end_line robustly (can be str, int, or None)
        s_parsed = None
        e_parsed = None
        try:
            if start_line is not None:
                s_parsed = max(0, int(str(start_line).strip()) - 1)
            if end_line is not None:
                e_parsed = int(str(end_line).strip())
        except (ValueError, TypeError):
            pass

        if s_parsed is not None:
            e = e_parsed if e_parsed else min(s_parsed + 100, total)
            selected = lines[s_parsed:e]
            # Add line numbers for easier reference
            numbered = [f"{s_parsed+1+i:4d} | {line}" for i, line in enumerate(selected)]
            header = f"[Lines {s_parsed+1}-{min(e, total)} of {total} in {path}]\n"
            return header + "\n".join(numbered)

        if total > 150:
            # Show first 50 + last 20 lines with line numbers
            head = [f"{i+1:4d} | {lines[i]}" for i in range(min(50, total))]
            tail = [f"{total-19+i:4d} | {lines[total-20+i]}" for i in range(20)]
            return "\n".join(head) + f"\n\n... ({total - 70} lines omitted. Use read_file with start_line/end_line to see specific ranges.)\n\n" + "\n".join(tail)
        return "\n".join(f"{i+1:4d} | {line}" for i, line in enumerate(lines))
    except Exception as e:
        return f"Error reading {path}: {e}"


def tool_edit_file(path: str, old_text: str, new_text: str, repo_path: str) -> str:
    full_path = os.path.join(repo_path, path)
    if not os.path.exists(full_path):
        return f"File not found: {path}"
    try:
        content = open(full_path).read()
        if old_text not in content:
            # Try with normalized whitespace
            import re
            normalized_content = re.sub(r'\s+', ' ', content)
            normalized_old = re.sub(r'\s+', ' ', old_text)
            if normalized_old not in normalized_content:
                return f"old_text not found in {path}. Make sure it matches exactly."
            # Find approximate location
            return f"old_text not found exactly in {path}. Check whitespace and indentation."

        new_content = content.replace(old_text, new_text, 1)
        open(full_path, "w").write(new_content)
        return f"Successfully edited {path}. Replaced {len(old_text)} chars with {len(new_text)} chars."
    except Exception as e:
        return f"Error editing {path}: {e}"


def tool_run_command(command: str, repo_path: str) -> str:
    # Safety: block destructive commands
    dangerous = ["rm -rf", "rm -r /", "dd if=", "mkfs", "> /dev"]
    for d in dangerous:
        if d in command:
            return f"Blocked dangerous command: {command}"

    try:
        result = subprocess.run(
            command, shell=True, cwd=repo_path,
            capture_output=True, text=True, timeout=120
        )
        output = result.stdout + result.stderr
        if len(output) > 3000:
            output = output[:3000] + f"\n... (truncated, {len(output)} total chars)"
        return output if output.strip() else "(no output)"
    except subprocess.TimeoutExpired:
        return "Command timed out after 60 seconds"
    except Exception as e:
        return f"Error running command: {e}"


def tool_reproduce(code: str, description: str, repo_path: str) -> str:
    """Create a reproducer script, run it, return output."""
    repro_path = os.path.join(repo_path, "_repro.py")
    try:
        # Write the reproducer
        with open(repro_path, "w") as f:
            # Add repo to sys.path for imports
            f.write(f"import sys; sys.path.insert(0, '.')\n")
            f.write(f"# Reproducer: {description}\n")
            f.write(code)

        # Run it
        result = subprocess.run(
            ["python3", repro_path],
            capture_output=True, text=True, timeout=30, cwd=repo_path
        )
        output = result.stdout + result.stderr
        if len(output) > 2000:
            output = output[:2000] + "\n... (truncated)"

        exit_code = result.returncode
        status = "✓ PASSED (exit 0)" if exit_code == 0 else f"✗ FAILED (exit {exit_code})"

        return f"=== REPRODUCER: {description} ===\n{status}\nOutput:\n{output}\n=== END REPRODUCER ==="
    except subprocess.TimeoutExpired:
        return f"=== REPRODUCER: {description} ===\n✗ TIMEOUT (30s)\n=== END REPRODUCER ==="
    except Exception as e:
        return f"Error running reproducer: {e}"
    finally:
        # Clean up
        try:
            os.remove(repro_path)
        except OSError:
            pass


def tool_debug(file: str, line: int, expression: str, run_cmd: str, repo_path: str) -> str:
    """Insert temporary debug print, run command, capture output, remove print."""
    full_path = os.path.join(repo_path, file)
    if not os.path.exists(full_path):
        return f"File not found: {file}"

    try:
        # Read original content
        original = open(full_path).read()
        lines = original.split("\n")

        line_idx = max(0, int(line) - 1)
        if line_idx >= len(lines):
            return f"Line {line} out of range (file has {len(lines)} lines)"

        # Detect indentation of target line
        target_line = lines[line_idx]
        indent = len(target_line) - len(target_line.lstrip())
        indent_str = " " * indent

        # Insert debug print
        debug_line = f'{indent_str}print(f"DEBUG[{file}:{line}]: {expression} = {{{expression}}}")'
        lines.insert(line_idx, debug_line)

        # Write instrumented file
        with open(full_path, "w") as f:
            f.write("\n".join(lines))

        # Run the command
        result = subprocess.run(
            run_cmd, shell=True, capture_output=True, text=True,
            timeout=30, cwd=repo_path
        )
        output = result.stdout + result.stderr
        if len(output) > 2000:
            output = output[:2000] + "\n... (truncated)"

        # Restore original file
        with open(full_path, "w") as f:
            f.write(original)

        # Extract debug lines from output
        debug_output = [l for l in output.split("\n") if "DEBUG[" in l]
        other_output = [l for l in output.split("\n") if "DEBUG[" not in l and l.strip()][:10]

        result_lines = [f"=== DEBUG TRACE: {expression} at {file}:{line} ==="]
        if debug_output:
            for dl in debug_output[:10]:
                result_lines.append(f"  {dl}")
        else:
            result_lines.append("  (no debug output captured — line may not have been reached)")
        if other_output:
            result_lines.append("Other output:")
            for ol in other_output[:5]:
                result_lines.append(f"  {ol}")
        result_lines.append("=== END DEBUG ===")
        return "\n".join(result_lines)

    except subprocess.TimeoutExpired:
        # Restore original
        with open(full_path, "w") as f:
            f.write(original)
        return f"DEBUG TIMEOUT: command took >30s"
    except Exception as e:
        # Restore original
        try:
            with open(full_path, "w") as f:
                f.write(original)
        except Exception:
            pass
        return f"Debug error: {e}"


def tool_trace_variable(file: str, function: str, variable: str, repo_path: str) -> str:
    """Trace all assignments and uses of a variable in the entire file.

    Uses grep to find ALL lines with the variable, then classifies each as
    ASSIGN, CHECK, RETURN, or USE. Simple and reliable.
    """
    full_path = os.path.join(repo_path, file)
    if not os.path.exists(full_path):
        return f"File not found: {file}"

    try:
        content = open(full_path).read()
        all_lines = content.split("\n")
        import re
        var_pattern = re.compile(r'\b' + re.escape(variable) + r'\b')

        trace = [f"=== DATA FLOW: '{variable}' in {file} ==="]

        for i, line in enumerate(all_lines):
            if var_pattern.search(line):
                stripped = line.strip()
                if not stripped or stripped.startswith("#"):
                    continue

                if f"{variable} =" in line and "==" not in line.split(variable + " =")[0][-2:]:
                    action = "ASSIGN"
                elif f"if " in stripped and variable in stripped:
                    action = "CHECK"
                elif f"return " in stripped and variable in stripped:
                    action = "RETURN"
                else:
                    action = "USE"

                trace.append(f"  Line {i+1:4d} [{action:6s}]: {stripped[:120]}")

        if len(trace) == 1:
            trace.append(f"  (no references to '{variable}' found)")

        trace.append(f"=== END TRACE ({len(trace)-1} references) ===")
        return "\n".join(trace)

    except Exception as e:
        return f"Error tracing: {e}"


def validate_edit(file_path: str, old_text: str, new_text: str, repo_path: str) -> str:
    """Comprehensive real-time validation after each edit.

    8 checks in <500ms total:
    1. Edit sanity (did it actually change? is it minimal?)
    2. Diff preview (what changed, with line numbers)
    3. Bracket/paren matching (common LLM error)
    4. Syntax check (py_compile, node -c)
    5. Lint on changed code (ruff/pyflakes — undefined names, unused imports)
    6. Import validation (are all imports resolvable?)
    7. Conflict markers check (<<<< ==== >>>>)
    8. Indentation consistency (tabs vs spaces, wrong level)
    """
    full_path = os.path.join(repo_path, file_path)
    ext = file_path.rsplit(".", 1)[-1] if "." in file_path else ""
    lines = ["=== EDIT VALIDATION ===", f"File: {file_path}"]
    issues = []

    try:
        content = open(full_path).read()
    except Exception:
        lines.append("ERROR: Cannot read file after edit")
        lines.append("=== END VALIDATION ===")
        return "\n".join(lines)

    # --- CHECK 1: Edit sanity ---
    if old_text.strip() == new_text.strip():
        issues.append("⚠ NOOP: old_text and new_text are identical (nothing changed)")

    old_line_count = old_text.count("\n") + 1
    new_line_count = new_text.count("\n") + 1
    diff_size = abs(new_line_count - old_line_count)
    if diff_size > 50:
        issues.append(f"⚠ LARGE EDIT: {diff_size} lines difference. Consider smaller changes.")

    # --- CHECK 2: Diff preview ---
    old_lines = old_text.strip().split("\n")
    new_lines = new_text.strip().split("\n")
    lines.append("Diff:")
    for ol in old_lines[:4]:
        lines.append(f"  - {ol.rstrip()}")
    for nl in new_lines[:4]:
        lines.append(f"  + {nl.rstrip()}")
    if len(new_lines) > 4:
        lines.append(f"  ... (+{len(new_lines) - 4} more lines)")
    lines.append(f"  ({old_line_count} lines → {new_line_count} lines)")

    # --- CHECK 3: Bracket/paren matching ---
    openers = {"(", "[", "{"}
    closers = {")": "(", "]": "[", "}": "{"}
    stack = []
    in_string = False
    string_char = None
    prev_ch = ""
    for i, ch in enumerate(content):
        # Skip characters inside strings
        if ch in ('"', "'") and prev_ch != "\\":
            if not in_string:
                in_string = True
                string_char = ch
            elif ch == string_char:
                in_string = False
        prev_ch = ch
        if in_string:
            continue
        if ch in openers:
            stack.append((ch, i))
        elif ch in closers:
            expected = closers[ch]
            if stack and stack[-1][0] == expected:
                stack.pop()
            else:
                line_num = content[:i].count("\n") + 1
                issues.append(f"✗ BRACKET MISMATCH: unexpected '{ch}' at line {line_num}")
                break
    if stack and len(stack) <= 3:
        ch, pos = stack[-1]
        line_num = content[:pos].count("\n") + 1
        issues.append(f"✗ UNCLOSED '{ch}' opened at line {line_num}")

    # --- CHECK 4: Syntax check ---
    syntax_ok = True
    try:
        if ext == "py":
            r = subprocess.run(
                ["python3", "-m", "py_compile", full_path],
                capture_output=True, text=True, timeout=10, cwd=repo_path
            )
            if r.returncode != 0:
                syntax_ok = False
                err = r.stderr.strip().split("\n")[-1] if r.stderr else "Syntax error"
                issues.append(f"✗ SYNTAX ERROR: {err}")
            else:
                lines.append("Syntax: ✓ OK")
        elif ext in ("js", "mjs", "cjs"):
            r = subprocess.run(
                ["node", "-c", full_path],
                capture_output=True, text=True, timeout=10, cwd=repo_path
            )
            if r.returncode != 0:
                syntax_ok = False
                err = r.stderr.strip().split("\n")[0] if r.stderr else "Syntax error"
                issues.append(f"✗ SYNTAX ERROR: {err}")
            else:
                lines.append("Syntax: ✓ OK")
        elif ext == "rs":
            lines.append("Syntax: (Rust — verify with 'cargo check')")
        elif ext in ("ts", "tsx"):
            lines.append("Syntax: (TypeScript — verify with 'npx tsc --noEmit')")
        elif ext == "go":
            r = subprocess.run(
                ["go", "vet", full_path],
                capture_output=True, text=True, timeout=10, cwd=repo_path
            )
            if r.returncode != 0:
                issues.append(f"✗ GO VET: {r.stderr.strip().split(chr(10))[0]}")
            else:
                lines.append("Syntax: ✓ OK")
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    # --- CHECK 5: Lint (Python: ruff or pyflakes) ---
    if ext == "py" and syntax_ok:
        lint_found = False
        for linter_cmd in [
            ["ruff", "check", full_path, "--select", "F,W", "--no-fix", "--output-format", "concise"],
            ["python3", "-m", "pyflakes", full_path],
        ]:
            try:
                r = subprocess.run(
                    linter_cmd, capture_output=True, text=True, timeout=10, cwd=repo_path
                )
                output = (r.stdout + r.stderr).strip()
                if output:
                    lint_lines = output.split("\n")[:5]
                    # Filter to only show issues in changed lines
                    relevant = [l for l in lint_lines if any(kw in l.lower() for kw in
                        ["undefined", "unused", "import", "error", "f821", "f811", "e999"])]
                    if relevant:
                        lines.append(f"Lint: ⚠ {len(relevant)} issue(s)")
                        for ll in relevant[:3]:
                            issues.append(f"⚠ LINT: {ll.strip()}")
                    else:
                        lines.append("Lint: ✓ Clean")
                else:
                    lines.append("Lint: ✓ Clean")
                lint_found = True
                break
            except (FileNotFoundError, subprocess.TimeoutExpired):
                continue
        if not lint_found:
            lines.append("Lint: (no linter available — install ruff: pip install ruff)")

    # --- CHECK 6: Import validation (Python) ---
    if ext == "py" and syntax_ok:
        # Check for names used in new_text that might need imports
        import re
        new_names = set(re.findall(r'\b([A-Z][a-zA-Z]+)\b', new_text))  # PascalCase = likely classes
        stdlib_modules = {"os", "sys", "re", "json", "logging", "typing", "datetime",
                         "collections", "functools", "itertools", "pathlib", "copy",
                         "abc", "enum", "dataclasses", "unittest", "math", "hashlib"}

        for name in new_names:
            name_lower = name.lower()
            if name_lower in stdlib_modules:
                # Check if this module is imported
                if f"import {name_lower}" not in content and f"from {name_lower}" not in content:
                    issues.append(f"⚠ MISSING IMPORT: '{name_lower}' used but not imported. Add 'import {name_lower}'")

    # --- CHECK 7: Conflict markers ---
    if "<<<<<<" in content or "=======" in content or ">>>>>>" in content:
        issues.append("✗ CONFLICT MARKERS: File contains git merge conflict markers (<<<<<<, =======, >>>>>>)")

    # --- CHECK 8: Indentation consistency ---
    file_lines = content.split("\n")
    has_tabs = any(l.startswith("\t") for l in file_lines if l.strip())
    has_spaces = any(l.startswith("  ") for l in file_lines if l.strip())
    if has_tabs and has_spaces:
        issues.append("⚠ MIXED INDENTATION: File has both tabs and spaces. Use one consistently.")

    # New code indentation check
    if ext == "py":
        new_code_lines = new_text.split("\n")
        for i, line in enumerate(new_code_lines):
            if line and not line.strip().startswith("#"):
                indent = len(line) - len(line.lstrip())
                if indent % 4 != 0 and indent > 0:
                    issues.append(f"⚠ INDENT: Line {i+1} of edit has {indent}-space indent (expected multiple of 4)")
                    break

    # --- SUMMARY ---
    if issues:
        lines.append("")
        lines.append(f"Issues ({len(issues)}):")
        for issue in issues:
            lines.append(f"  {issue}")
        if any("✗" in i for i in issues):
            lines.append("")
            lines.append("FIX THE ERRORS above before proceeding.")
    else:
        lines.append("All checks: ✓ PASSED")

    lines.append("=== END VALIDATION ===")
    return "\n".join(lines)


def tool_refresh_after_edit(edited_file: str, repo_path: str, query: str = "") -> str:
    """Run GRAPHCTX refresh after an edit: incremental graph update + impact analysis.

    This is the core of real-time feedback. After each edit, the system tells
    the LLM what changed structurally and what else needs attention.
    """
    try:
        cmd = [THEO_CODE_BIN, "refresh", repo_path, edited_file]
        if query:
            cmd.append(query)
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30
        )
        output = result.stdout.strip()
        if not output:
            return ""

        # Format as a context update for the LLM
        lines = [
            "=== CONTEXT UPDATE (after your edit) ===",
        ]
        for line in output.split("\n"):
            if line.strip() and not line.startswith("==="):
                lines.append(line)
        lines.append("=== Use this information to decide your next action. If the fix is complete, call done. ===")
        return "\n".join(lines)
    except subprocess.TimeoutExpired:
        return ""
    except Exception:
        return ""


def tool_search_code(query: str, repo_path: str) -> str:
    try:
        result = subprocess.run(
            [THEO_CODE_BIN, "context", repo_path, query],
            capture_output=True, text=True, timeout=120
        )
        output = result.stdout
        # Extract just the context items (skip stats)
        lines = output.split("\n")
        context_lines = []
        in_items = False
        for line in lines:
            if line.startswith("--- Item"):
                in_items = True
            if in_items:
                context_lines.append(line)
            if line.startswith("--- Timing"):
                break

        context = "\n".join(context_lines)
        if len(context) > 8000:
            context = context[:8000] + "\n... (truncated)"
        return context if context.strip() else "No results found."
    except Exception as e:
        return f"Error searching: {e}"

# ---------------------------------------------------------------------------
# GRAPHCTX context assembly
# ---------------------------------------------------------------------------

def get_initial_context(issue_description: str, repo_path: str) -> str:
    """Get GRAPHCTX context for the issue."""
    try:
        result = subprocess.run(
            [THEO_CODE_BIN, "context", repo_path, issue_description],
            capture_output=True, text=True, timeout=120
        )
        output = result.stdout
        # Extract context items
        lines = output.split("\n")
        context_lines = []
        in_items = False
        for line in lines:
            if line.startswith("--- Item") or line.startswith("## ") or line.startswith("### ") or line.startswith("```"):
                in_items = True
            if in_items:
                context_lines.append(line)
            if line.startswith("--- Timing"):
                break
        return "\n".join(context_lines)
    except Exception as e:
        return f"Error getting context: {e}"

# ---------------------------------------------------------------------------
# LLM interaction
# ---------------------------------------------------------------------------

def parse_hermes_tool_calls(content: str) -> list:
    """Fix 6: Robust parser for Hermes XML tool calls with fallback strategies."""
    import re
    calls = []

    # Strategy 1: Hermes XML format
    # <tool_call><function=NAME><parameter=KEY>VALUE</parameter></function></tool_call>
    pattern = r'<function=(\w+)>(.*?)</function>'
    for match in re.finditer(pattern, content, re.DOTALL):
        fn_name = match.group(1)
        body = match.group(2)
        args = {}
        param_pattern = r'<parameter=(\w+)>(.*?)</parameter>'
        for pm in re.finditer(param_pattern, body, re.DOTALL):
            args[pm.group(1)] = pm.group(2).strip()
        if fn_name:
            calls.append({"name": fn_name, "args": args})

    if calls:
        return calls

    # Strategy 2: JSON inside <tool_call> tags
    json_pattern = r'<tool_call>\s*(\{.*?\})\s*</tool_call>'
    for match in re.finditer(json_pattern, content, re.DOTALL):
        try:
            data = json.loads(match.group(1))
            calls.append({
                "name": data.get("name", ""),
                "args": data.get("arguments", data.get("parameters", {})),
            })
        except json.JSONDecodeError:
            # Try fixing common JSON issues: trailing commas, single quotes
            raw = match.group(1)
            raw = re.sub(r',\s*}', '}', raw)  # remove trailing commas
            raw = raw.replace("'", '"')  # single to double quotes
            try:
                data = json.loads(raw)
                calls.append({
                    "name": data.get("name", ""),
                    "args": data.get("arguments", data.get("parameters", {})),
                })
            except json.JSONDecodeError:
                pass

    if calls:
        return calls

    # Strategy 3: Bare function call pattern (no XML tags)
    # read_file("path/to/file") or edit_file(path="x", old_text="y", new_text="z")
    bare_pattern = r'\b(read_file|edit_file|run_command|search_code|done)\s*\('
    for match in re.finditer(bare_pattern, content):
        fn_name = match.group(1)
        # Can't reliably parse args from bare calls — return just the name
        calls.append({"name": fn_name, "args": {}})

    return calls


def call_llm(messages: list, tools: list = None) -> dict:
    """Call vLLM OpenAI-compatible API with tool support."""
    url = f"{VLLM_URL}/v1/chat/completions"
    payload = {
        "model": MODEL_NAME,
        "messages": messages,
        "max_tokens": MAX_TOKENS_PER_RESPONSE,
        "temperature": 0.1,
    }
    if tools:
        payload["tools"] = tools
        payload["tool_choice"] = "auto"

    try:
        resp = requests.post(url, json=payload, timeout=120)
        resp.raise_for_status()
        return resp.json()
    except Exception as e:
        return {"error": str(e)}

# ---------------------------------------------------------------------------
# Agent Loop
# ---------------------------------------------------------------------------

@dataclass
class AgentResult:
    issue: str
    repo: str
    success: bool
    iterations: int
    summary: str
    total_tokens: int
    tools_used: list = field(default_factory=list)
    elapsed_seconds: float = 0.0
    edits_made: list = field(default_factory=list)

def run_agent(repo_path: str, issue_description: str, verbose: bool = True) -> AgentResult:
    """Run the agent loop to fix an issue."""
    start_time = time.time()

    if verbose:
        print(f"\n{'='*60}")
        print(f"THEO CODE AGENT")
        print(f"{'='*60}")
        print(f"Repo:  {repo_path}")
        print(f"Issue: {issue_description[:80]}...")
        print()

    # Step 1: Get GRAPHCTX context
    if verbose:
        print("[1] Assembling GRAPHCTX context...")
    context = get_initial_context(issue_description, repo_path)
    # Limit context to ~8K chars to fit in model's context window
    if len(context) > 8000:
        context = context[:8000] + "\n... (context truncated)"
    if verbose:
        context_lines = context.count("\n")
        print(f"    Context: {context_lines} lines, {len(context)} chars")

    # Step 2: Build system prompt (Fix 2: force done + Fix 4: chain-of-thought)
    system_prompt = f"""You are an expert debugging agent. Fix the issue using a SCIENTIFIC METHOD.

TOOLS:
- read_file(path, start_line?, end_line?) — Read file with line numbers
- edit_file(path, old_text, new_text) — Replace exact text (must match perfectly)
- run_command(command) — Shell command (grep, tests, compile)
- search_code(query) — GRAPHCTX search for relevant code
- reproduce(code, description) — Create and run a bug reproducer (ALWAYS first!)
- debug(file, line, expression, run_command) — Insert temp debug print, run, capture, cleanup
- trace_variable(file, function, variable) — Trace ALL assignments/uses of a variable in a function. Shows DATA FLOW.
- done(summary) — Call when fix is complete

AFTER EACH EDIT: The system automatically re-runs your reproducer. If the bug is still present, you'll be told to try a DIFFERENT approach.

MANDATORY 5-STEP WORKFLOW:

STEP 1 — REPRODUCE: Create a minimal script with reproduce() that demonstrates the bug.
  This confirms you understand the problem. If reproduce shows no bug, re-read the issue.

STEP 2 — LOCATE: Use search_code + grep to find the exact file and function.
  Read the function with read_file(path, start_line, end_line).

STEP 3 — TRACE: Use debug() to insert prints at key points. Run the reproducer.
  See the ACTUAL execution flow. Don't guess — observe.
  Example: debug("lib/core.py", 45, "type(value)", "python3 _repro.py")

STEP 4 — FIX: Now that you've seen the execution trace, make the MINIMAL edit.
  The system will auto-validate syntax, lint, brackets, and show impact analysis.

STEP 5 — VERIFY: Run your reproducer again. If it passes, call done immediately.

RULES:
- ALWAYS reproduce() before trying to fix. No exceptions.
- ALWAYS debug() before editing complex bugs. See, don't guess.
- After each edit, the system AUTO-RERUNS your reproducer. If it still fails, TRY A DIFFERENT APPROACH.
- If your first fix doesn't work, analyze WHY it failed and try a fundamentally different fix.
- edit_file old_text must match EXACTLY (whitespace, indentation).
- Make the SMALLEST change possible (1-10 lines).
- When reasoning about a bug, trace the DATA FLOW: what value does the variable have at each step?
- If stuck after 15 iterations, call done with what you found.

=== RELEVANT CODE CONTEXT ===
{context}
=== END CONTEXT ==="""

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": f"Fix this issue:\n\n{issue_description}"},
    ]

    total_tokens = 0
    tools_used = []
    edits_made = []
    summary = ""
    last_reproducer_code = None  # Track last reproducer for auto-verify
    failed_fix_count = 0  # Track how many fixes didn't work

    # Step 3: Agent loop
    for iteration in range(1, MAX_ITERATIONS + 1):
        if verbose:
            print(f"\n[Iteration {iteration}/{MAX_ITERATIONS}]")

        response = call_llm(messages, TOOLS)

        if "error" in response:
            if verbose:
                print(f"  ERROR: {response['error']}")
            break

        choice = response["choices"][0]
        message = choice["message"]
        total_tokens += response.get("usage", {}).get("total_tokens", 0)

        # Check if the model wants to use tools
        tool_calls = message.get("tool_calls") or []
        content_text = message.get("content") or ""

        # Fallback: parse tool calls from content XML (Hermes format)
        # <tool_call>\n<function=name>\n<parameter=key>value</parameter>\n</function>\n</tool_call>
        if not tool_calls and "<tool_call>" in content_text:
            parsed = parse_hermes_tool_calls(content_text)
            if parsed:
                tool_calls = parsed

        if not tool_calls:
            if verbose:
                print(f"  Response: {content_text[:200]}...")

            content_lower = content_text.lower()
            if "done" in content_lower and ("fixed" in content_lower or "resolved" in content_lower or "complete" in content_lower):
                summary = content_text
                if verbose:
                    print(f"\n  Agent says DONE")
                break

            messages.append({"role": "assistant", "content": content_text})
            messages.append({"role": "user", "content": "Use the available tools. Call read_file to read code, edit_file to fix it, run_command to test, done when finished."})
            continue

        # Process tool calls
        messages.append({"role": "assistant", "content": content_text})

        for tool_call in tool_calls:
            if isinstance(tool_call, dict) and "function" in tool_call:
                fn_name = tool_call["function"]["name"]
                try:
                    args_raw = tool_call["function"].get("arguments", "{}")
                    fn_args = json.loads(args_raw) if isinstance(args_raw, str) else args_raw
                except (json.JSONDecodeError, TypeError):
                    fn_args = {}
            else:
                # Parsed from Hermes format
                fn_name = tool_call.get("name", "")
                fn_args = tool_call.get("args", {})

            if verbose:
                args_preview = json.dumps(fn_args)[:100]
                print(f"  Tool: {fn_name}({args_preview})")

            # Execute the tool
            result = execute_tool(fn_name, fn_args, repo_path)
            tools_used.append(fn_name)

            # Track reproducers for auto-verify
            if fn_name == "reproduce":
                last_reproducer_code = fn_args.get("code", "")

            if fn_name == "edit_file" and "Successfully" in result:
                edited_path = fn_args.get("path", "")
                edits_made.append({
                    "file": edited_path,
                    "old": fn_args.get("old_text", "")[:50],
                    "new": fn_args.get("new_text", "")[:50],
                })

                # === REAL-TIME VALIDATION (lint + syntax + diff) ===
                validation = validate_edit(edited_path, fn_args.get("old_text", ""), fn_args.get("new_text", ""), repo_path)
                if validation:
                    result += f"\n\n{validation}"
                    if verbose:
                        for line in validation.split("\n")[:5]:
                            if line.strip():
                                print(f"    🔍 {line.strip()}")

                # === REAL-TIME DIFF FEEDBACK ===
                refresh_result = tool_refresh_after_edit(edited_path, repo_path, issue_description)
                if refresh_result and refresh_result.strip():
                    result += f"\n\n{refresh_result}"
                    if verbose:
                        refresh_preview = refresh_result[:200].replace("\n", " ")
                        print(f"    📊 {refresh_preview}")

                # === AUTO-VERIFY: re-run reproducer after edit ===
                if last_reproducer_code:
                    verify_result = tool_reproduce(
                        last_reproducer_code,
                        "AUTO-VERIFY after edit",
                        repo_path
                    )
                    if "PASSED" in verify_result and "Bug" not in verify_result and "bug" not in verify_result.split("Output:")[1] if "Output:" in verify_result else "":
                        result += f"\n\n✅ AUTO-VERIFY: Reproducer passes! Bug appears fixed. Call done now."
                        if verbose:
                            print(f"    ✅ AUTO-VERIFY: Bug appears fixed!")
                    else:
                        failed_fix_count += 1
                        result += f"\n\n❌ AUTO-VERIFY: Bug NOT fixed (attempt {failed_fix_count}). Reproducer output:\n{verify_result}"
                        result += f"\n\nYour fix did NOT solve the bug. Try a DIFFERENT approach."
                        if failed_fix_count >= 2:
                            result += f"\n\nHINT: You've tried {failed_fix_count} fixes that didn't work. Step back and trace the DATA FLOW of the variable through the function. Use debug() to see actual values at each assignment."
                        if verbose:
                            print(f"    ❌ AUTO-VERIFY: Fix #{failed_fix_count} failed — bug still present")

            if fn_name == "done":
                summary = fn_args.get("summary", result)
                if verbose:
                    print(f"\n  DONE: {summary}")

                elapsed = time.time() - start_time
                return AgentResult(
                    issue=issue_description,
                    repo=repo_path,
                    success=True,
                    iterations=iteration,
                    summary=summary,
                    total_tokens=total_tokens,
                    tools_used=tools_used,
                    elapsed_seconds=elapsed,
                    edits_made=edits_made,
                )

            if verbose:
                result_preview = result[:200].replace("\n", " ")
                print(f"    → {result_preview}")

            # Add tool result to conversation
            tool_call_id = tool_call.get("id", f"call_{iteration}_{fn_name}")
            if "id" in tool_call:
                messages.append({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": result[:4000],
                })
            else:
                # Hermes format — use user message for result
                messages.append({
                    "role": "user",
                    "content": f"Tool result for {fn_name}:\n{result[:4000]}",
                })

    elapsed = time.time() - start_time

    return AgentResult(
        issue=issue_description,
        repo=repo_path,
        success=bool(summary),
        iterations=MAX_ITERATIONS,
        summary=summary or "Agent did not complete",
        total_tokens=total_tokens,
        tools_used=tools_used,
        elapsed_seconds=elapsed,
        edits_made=edits_made,
    )

# ---------------------------------------------------------------------------
# Issue definitions for open-source repos
# ---------------------------------------------------------------------------

ISSUES = [
    {
        "repo_url": None,  # Use local repo
        "repo_path": ".",
        "issue": "The `stem` function in crates/context/src/search.rs has a bug: it converts 'testing' to 'test' (removes 'ing') but 'testing' should actually stem to 'test'. However, the word 'string' incorrectly stems to 'str' (removes 'ing'). Fix the stem function to not remove 'ing' from words where the remaining part is less than 3 characters.",
        "verify_command": "cargo test -p theo-code-context search::tests",
        "expected_file": "crates/context/src/search.rs",
        "difficulty": "easy",
    },
]

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Theo Code Agent Loop")
    parser.add_argument("--repo", default=".", help="Path to the repository")
    parser.add_argument("--issue", help="Issue description to fix")
    parser.add_argument("--vllm-url", default=VLLM_URL, help="vLLM API URL")
    parser.add_argument("--run-all", action="store_true", help="Run all predefined issues")
    parser.add_argument("--quiet", action="store_true", help="Less output")
    args = parser.parse_args()


    vllm_url = args.vllm_url

    # Check API
    try:
        resp = requests.get(f"{vllm_url}/v1/models", timeout=5)
        models = resp.json()
        print(f"Model: {models['data'][0]['id']}")
    except Exception as e:
        print(f"ERROR: Cannot reach vLLM at {vllm_url}: {e}")
        sys.exit(1)

    if args.issue:
        # Single issue mode
        result = run_agent(args.repo, args.issue, verbose=not args.quiet)
        print(f"\n{'='*60}")
        print(f"Result: {'SUCCESS' if result.success else 'FAILED'}")
        print(f"Iterations: {result.iterations}")
        print(f"Tokens: {result.total_tokens}")
        print(f"Tools: {result.tools_used}")
        print(f"Edits: {result.edits_made}")
        print(f"Time: {result.elapsed_seconds:.1f}s")
        print(f"Summary: {result.summary}")

    elif args.run_all:
        # Run all predefined issues
        results = []
        for issue_def in ISSUES:
            repo = issue_def.get("repo_path", args.repo)
            result = run_agent(repo, issue_def["issue"], verbose=not args.quiet)
            results.append(result)

            # Verify if specified
            if issue_def.get("verify_command"):
                print(f"\n  Verifying: {issue_def['verify_command']}")
                verify = subprocess.run(
                    issue_def["verify_command"], shell=True, cwd=repo,
                    capture_output=True, text=True, timeout=120
                )
                passed = verify.returncode == 0
                print(f"  Verification: {'PASS' if passed else 'FAIL'}")

        # Summary
        print(f"\n{'='*60}")
        print("SUMMARY")
        print(f"{'='*60}")
        successes = sum(1 for r in results if r.success)
        print(f"Success: {successes}/{len(results)}")
        for r in results:
            status = "PASS" if r.success else "FAIL"
            print(f"  [{status}] {r.issue[:60]}... ({r.iterations} iter, {r.total_tokens} tok)")

        # Save results
        with open("benchmark/agent_results.json", "w") as f:
            json.dump([asdict(r) for r in results], f, indent=2)

    else:
        print("Usage: --issue '<description>' or --run-all")
        print("Example: python3 benchmark/agent_loop.py --issue 'Fix the stem function bug'")

if __name__ == "__main__":
    main()
