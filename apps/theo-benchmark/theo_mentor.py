#!/usr/bin/env python3
"""
Theo Mentor — AI Pair Programming that teaches while coding.

Unlike autonomous mode where the agent silently fixes everything,
Mentor mode NARRATES every step:
- Explains WHAT it's doing and WHY
- The developer can INTERRUPT at any point to ask questions
- Developer can request changes to the plan
- After any interruption → refresh context + re-plan → continue

Based on Anthropic research (Shen & Tamkin, 2026):
- "Generation-Then-Comprehension" pattern preserves 86% of learning
- "AI Delegation" pattern preserves only 19%
- Mentor mode enforces the high-learning patterns

Usage:
    python3 theo_mentor.py --repo /path --task "Fix the auth bug"

    During execution, type at any prompt:
    - Press Enter to continue
    - Type a question → agent answers, refreshes context, continues
    - Type "change: <new direction>" → agent re-plans
    - Type "explain" → agent explains current code in detail
    - Type "stop" → stop execution
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from typing import Optional

import requests

# Import from theo_agent
sys.path.insert(0, os.path.dirname(__file__))
from theo_agent import (
    TOOLS, execute_tool, validate_edit, auto_verify,
    UndoStack, decompose_task, VLLM_URL, MODEL_NAME, THEO_CODE_BIN,
    MAX_ITERATIONS
)

# ---------------------------------------------------------------------------
# Mentor System Prompt
# ---------------------------------------------------------------------------

MENTOR_SYSTEM = """You are Theo Mentor — an AI that writes code AND teaches the developer to SUPERVISE it.

CORE PRINCIPLE: You write the code (you're better at it). But the developer MUST be able to evaluate, review, and supervise what you write. Your job is to build their SUPERVISORY skills, not their coding skills.

Think of yourself as an autopilot that narrates to the pilot: "I'm turning left because of crosswind. If this were a thunderstorm instead, I'd go around. Watch the altitude indicator — that's how you'd know if I'm making a mistake."

THE 4-PHASE WORKFLOW FOR EVERY ACTION:

## Phase 1: DECIDE (before coding)
Present 2-3 approaches. Explain tradeoffs. Let the developer CHOOSE.
"I see two ways to do this:
  A) Add middleware (clean but adds latency)
  B) Inline check (fast but duplicates logic)
  Which would you prefer? I'd suggest A because..."

## Phase 2: GENERATE (while coding)
Write the code. Narrate your reasoning. Highlight non-obvious decisions.
"I'm using a dict here instead of a set because we need O(1) lookup by key, not just membership. Note: if this data grows beyond 10K entries, we'd want to switch to Redis."

## Phase 3: ALERT (after coding — this is the critical part)
Show what could go WRONG. This teaches the dev to spot AI mistakes.
"⚠ Risk: This function doesn't handle None input. In production, a None would cause a crash at line 45.
⚠ Risk: I assumed UTC timezone. If the system uses local time, the expiry check will be wrong.
⚠ Edge case: What happens when the list is empty? Right now it returns None, but the caller expects a list."

## Phase 4: VERIFY (prove it works)
Run tests, show the output. Explain what a FAILING test would look like.
"✅ Test passes. But here's what would indicate a problem:
- If you see 'TypeError: NoneType' → the input validation I mentioned is missing
- If the test takes >100ms → the algorithm is O(n²) instead of O(n), check the inner loop"

TOOLS: read_file, create_file, edit_file, run_command, search_code, reproduce, debug, trace_variable, done

TEACHING FOCUS — WHAT TO EXPLAIN:
1. TRADEOFFS: "I chose X over Y because Z. If requirements change to W, you'd want Y instead."
2. RISKS: "This code assumes A. If A is false, here's what breaks."
3. FAILURE MODES: "If this fails, you'll see error X. That means Y. Fix by Z."
4. ARCHITECTURE: "This module depends on B and C. Changing B would require updating this too."
5. REVIEW SIGNALS: "When reviewing AI-generated code like this, check for: unclosed resources, missing error handling, hardcoded values."

NEVER just write code silently. ALWAYS include Phases 1-4.
End with: "🔍 Review checklist: [3 things the dev should verify about this code]"

{context}

=== CODE CONTEXT ===
{graphctx}
=== END ==="""


# ---------------------------------------------------------------------------
# Interactive Loop with Developer
# ---------------------------------------------------------------------------

def get_developer_input(step_num: int, last_explanation: str) -> Optional[str]:
    """Pause for developer input between steps.

    Returns:
        None — continue as normal
        str — developer's question/request
    """
    try:
        print(f"\n  {'─'*50}")
        print(f"  [Step {step_num}] Press Enter to continue, or type:")
        print(f"  - A question → I'll answer and continue")
        print(f"  - 'risks' → Show me what could go wrong with this code")
        print(f"  - 'alternatives' → What other approaches could we use?")
        print(f"  - 'change: ...' → Change direction")
        print(f"  - 'why' → Why did you choose this approach?")
        print(f"  - 'review' → What should I check when reviewing this?")
        print(f"  - 'stop' → Stop here")
        dev_input = input(f"  > ").strip()
        return dev_input if dev_input else None
    except (EOFError, KeyboardInterrupt):
        return None


def refresh_context(repo_path: str, task: str) -> str:
    """Refresh GRAPHCTX context after developer interaction."""
    try:
        r = subprocess.run(
            [THEO_CODE_BIN, "context", repo_path, task],
            capture_output=True, text=True, timeout=60
        )
        lines = []
        for line in r.stdout.split("\n"):
            if line.startswith("--- Item") or line.startswith("## ") or \
               line.startswith("### ") or line.startswith("```"):
                lines.append(line)
            if line.startswith("--- Timing"):
                break
        return "\n".join(lines)[:6000]
    except:
        return ""


def refresh_plan(task: str, repo_path: str, completed_steps: list, dev_request: str = "") -> list:
    """Re-plan remaining tasks based on current state and developer input."""
    tasks, intent, affected = decompose_task(task, repo_path)

    # If developer requested a change, modify the plan
    if dev_request:
        # Add the developer's request as a priority task
        tasks.insert(0, {
            "id": "dev_request",
            "desc": f"Developer requested: {dev_request}"
        })

    # Remove already completed steps
    completed_ids = {s["id"] for s in completed_steps}
    tasks = [t for t in tasks if t["id"] not in completed_ids]

    return tasks


# ---------------------------------------------------------------------------
# Mentor Agent Loop
# ---------------------------------------------------------------------------

def run_mentor(repo_path: str, task: str, vllm_url: str = VLLM_URL,
               model_name: str = MODEL_NAME, interactive: bool = True):
    """Run the mentor pair programming session."""

    print(f"\n{'='*60}")
    print(f"THEO MENTOR — Pair Programming Session")
    print(f"{'='*60}")
    print(f"Repo: {repo_path}")
    print(f"Task: {task[:80]}")
    print(f"\nI'll explain every step. You can interrupt anytime to ask questions.")
    print(f"{'='*60}\n")

    # Plan
    tasks, intent, affected = decompose_task(task, repo_path)
    print(f"## Plan")
    print(f"**Intent:** {intent}")
    print(f"**Affected files:** {', '.join(affected[:5]) or 'I need to search'}")
    print(f"**Steps I'll take:**")
    for i, t in enumerate(tasks):
        print(f"  {i+1}. {t['desc'][:70]}")
    print()

    # Check if developer wants to adjust plan
    if interactive:
        dev_input = get_developer_input(0, "plan")
        if dev_input:
            if dev_input.lower() == "stop":
                print("\nSession ended by developer.")
                return
            elif dev_input.lower().startswith("change:"):
                change = dev_input[7:].strip()
                print(f"\n  Adjusting plan based on your input: {change}")
                tasks = refresh_plan(task + ". " + change, repo_path, [], change)
                print(f"  New plan: {len(tasks)} steps")
                for t in tasks:
                    print(f"    - {t['desc'][:60]}")

    # Get initial context
    graphctx = refresh_context(repo_path, task)

    # Build system prompt
    system = MENTOR_SYSTEM.format(
        context=f"Current plan: {len(tasks)} steps. Developer is watching and can ask questions.",
        graphctx=graphctx
    )

    messages = [
        {"role": "system", "content": system},
        {"role": "user", "content": f"Let's work on this together. Task: {task}\n\nStart by explaining your approach, then begin step 1."},
    ]

    reproducer_code = None
    undo = UndoStack()
    step_num = 0
    completed_steps = []

    for iteration in range(1, MAX_ITERATIONS + 1):
        step_num += 1

        # Call LLM
        try:
            resp = requests.post(f"{vllm_url}/v1/chat/completions", json={
                "model": model_name, "messages": messages,
                "max_tokens": 4096, "temperature": 0.2,  # Slightly more creative for explanations
                "tools": TOOLS, "tool_choice": "auto",
            }, timeout=120)
            data = resp.json()
        except Exception as e:
            print(f"\n  API error: {e}")
            continue

        if "error" in data:
            print(f"\n  Error: {data['error']}")
            break

        msg = data["choices"][0]["message"]
        content = msg.get("content") or ""
        tool_calls = msg.get("tool_calls") or []

        # Parse Hermes format
        if not tool_calls and "<function=" in content:
            for m in re.finditer(r'<function=(\w+)>(.*?)</function>', content, re.DOTALL):
                fn = m.group(1)
                args = {}
                for pm in re.finditer(r'<parameter=(\w+)>(.*?)</parameter>', m.group(2), re.DOTALL):
                    args[pm.group(1)] = pm.group(2).strip()
                tool_calls.append({"name": fn, "args": args})

        # === DISPLAY MENTOR'S EXPLANATION ===
        if content:
            print(f"\n{'─'*60}")
            # Format the explanation nicely
            for line in content.split("\n"):
                if line.startswith("##"):
                    print(f"\n  📚 {line}")
                elif line.startswith("**Why"):
                    print(f"  💡 {line}")
                elif line.startswith("**Insight"):
                    print(f"  🔍 {line}")
                elif line.startswith("**Decision"):
                    print(f"  🎯 {line}")
                elif line.strip():
                    print(f"  {line}")

        # === DEVELOPER INTERACTION POINT ===
        if interactive and not tool_calls:
            dev_input = get_developer_input(step_num, content)
            if dev_input:
                if dev_input.lower() == "stop":
                    print("\n  Session ended.")
                    break
                elif dev_input.lower() == "risks":
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": "Show me ALL the risks with the current code. What could go wrong in production? What assumptions are you making? What edge cases are unhandled? What would a failure look like?"})
                    continue
                elif dev_input.lower() == "alternatives":
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": "What alternative approaches could we use instead? Present 2-3 options with tradeoffs. I want to understand the design space."})
                    continue
                elif dev_input.lower() == "why":
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": "Why did you choose this approach over alternatives? What's the tradeoff? When would a different approach be better?"})
                    continue
                elif dev_input.lower() == "review":
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": "If I'm reviewing this code as a senior engineer, what should I look for? Give me a specific checklist of things that could be wrong with AI-generated code like this."})
                    continue
                elif dev_input.lower().startswith("change:"):
                    change = dev_input[7:].strip()
                    print(f"\n  🔄 Refreshing context and re-planning...")
                    graphctx = refresh_context(repo_path, task + " " + change)
                    tasks = refresh_plan(task + " " + change, repo_path, completed_steps, change)
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": f"The developer wants to change direction: {change}. Adjust your approach and continue."})
                    print(f"  New plan: {len(tasks)} remaining steps")
                    continue
                else:
                    # Developer asked a question
                    print(f"\n  📝 Good question! Let me answer...")
                    messages.append({"role": "assistant", "content": content})
                    messages.append({"role": "user", "content": f"The developer asks: {dev_input}\n\nAnswer their question thoroughly, then continue with the task."})
                    continue

        if not tool_calls:
            messages.append({"role": "assistant", "content": content})
            messages.append({"role": "user", "content": "Continue with the next step. Remember to explain what you're doing."})
            continue

        # === EXECUTE TOOLS (with narration) ===
        messages.append({"role": "assistant", "content": content})

        for tc in tool_calls:
            fn = tc.get("name") or tc.get("function", {}).get("name", "")
            try:
                raw_args = tc.get("args") or tc.get("function", {}).get("arguments", "{}")
                args = json.loads(raw_args) if isinstance(raw_args, str) else raw_args
            except:
                args = {}

            print(f"\n  🔧 Tool: {fn}", end="")

            result = execute_tool(fn, args, repo_path)

            # Track reproducer
            if fn == "reproduce":
                reproducer_code = args.get("code")

            # Validate edits
            if fn in ("edit_file", "create_file") and ("Edited" in result or "Created" in result):
                undo.record(args.get("path", ""), args.get("old_text", ""), args.get("new_text", args.get("content", "")), repo_path)

                if fn == "edit_file":
                    val = validate_edit(args["path"], args.get("old_text", ""), args.get("new_text", ""), repo_path)
                    result += f"\n{val}"
                    if "SYNTAX ERROR" in val:
                        print(" ✗ Syntax error!", end="")

                if reproducer_code:
                    verified = auto_verify(reproducer_code, repo_path)
                    if verified:
                        result += "\n✅ Fix verified!"
                        print(" ✅", end="")
                    elif verified is False:
                        result += "\n❌ Fix didn't work yet."
                        print(" ❌", end="")

            print()

            # Done
            if fn == "done":
                summary = args.get("summary", "")
                print(f"\n{'='*60}")
                print(f"📚 SESSION COMPLETE")
                print(f"{'='*60}")
                print(f"\n{summary}")
                print(f"\n{'='*60}")
                return

            # Show truncated result
            result_preview = result[:300].replace("\n", "\n  ")
            print(f"  → {result_preview}")

            messages.append({"role": "user", "content": f"Tool result ({fn}):\n{result[:3000]}\n\nExplain what this result means and what you'll do next."})

        # Developer interaction after tool execution
        if interactive:
            dev_input = get_developer_input(step_num, "tool results")
            if dev_input:
                if dev_input.lower() == "stop":
                    break
                elif dev_input.lower() == "explain":
                    messages.append({"role": "user", "content": "Explain the last result in detail. What does it mean?"})
                elif dev_input.lower() == "why":
                    messages.append({"role": "user", "content": "Why did you use this tool? What were you looking for?"})
                elif dev_input.lower().startswith("change:"):
                    change = dev_input[7:].strip()
                    graphctx = refresh_context(repo_path, task + " " + change)
                    messages.append({"role": "user", "content": f"Developer wants to change: {change}. Adjust and continue."})
                else:
                    messages.append({"role": "user", "content": f"Developer asks: {dev_input}\n\nAnswer, then continue."})

    print(f"\n{'='*60}")
    print(f"Session ended (max iterations reached)")
    print(f"{'='*60}")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Theo Mentor — AI Pair Programming")
    parser.add_argument("--repo", required=True, help="Repository path")
    parser.add_argument("--task", required=True, help="What to work on")
    parser.add_argument("--vllm-url", default=VLLM_URL)
    parser.add_argument("--model", default=MODEL_NAME)
    parser.add_argument("--non-interactive", action="store_true", help="Run without pausing for input")
    args = parser.parse_args()

    # Check API
    try:
        r = requests.get(f"{args.vllm_url}/v1/models", timeout=5)
        print(f"Model: {r.json()['data'][0]['id']}")
    except Exception as e:
        print(f"ERROR: {e}")
        sys.exit(1)

    run_mentor(
        args.repo, args.task,
        vllm_url=args.vllm_url,
        model_name=args.model,
        interactive=not args.non_interactive
    )


if __name__ == "__main__":
    main()
