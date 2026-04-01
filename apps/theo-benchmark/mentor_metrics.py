"""
Mentor v2 Metrics — Measures supervisory skill development.

Based on Anthropic research (Shen & Tamkin, 2026) which found:
- AI Delegation: 19% quiz score (worst)
- Conceptual Inquiry: 65% (good)
- Generation-Then-Comprehension: 86% (best)

We measure whether Mentor v2's 4-phase approach (DECIDE→GENERATE→ALERT→VERIFY)
produces the high-learning patterns rather than delegation.

Metrics tracked per session:
1. Phases delivered: did the mentor show all 4 phases?
2. Risks identified: how many ⚠ alerts were shown?
3. Alternatives presented: did the mentor show options in DECIDE phase?
4. Review items: how many checklist items in VERIFY phase?
5. Developer interactions: questions asked, topics explored
6. Engagement score: ratio of interactive vs passive steps
"""

import json
import re
import os
import time
from dataclasses import dataclass, field, asdict


@dataclass
class SessionMetrics:
    """Metrics for a single Mentor session."""
    session_id: str = ""
    task: str = ""
    timestamp: float = 0.0

    # Phase coverage (0-4)
    phase_decide: bool = False
    phase_generate: bool = False
    phase_alert: bool = False
    phase_verify: bool = False
    phases_covered: int = 0  # out of 4

    # Content quality
    risks_identified: int = 0
    alternatives_presented: int = 0
    review_checklist_items: int = 0
    tradeoffs_explained: int = 0
    failure_modes_shown: int = 0

    # Developer engagement
    dev_questions: int = 0
    dev_risk_inquiries: int = 0      # asked "risks"
    dev_alternative_inquiries: int = 0  # asked "alternatives"
    dev_review_inquiries: int = 0    # asked "review"
    dev_why_inquiries: int = 0       # asked "why"
    dev_change_requests: int = 0     # asked "change:"
    total_dev_interactions: int = 0

    # Learning pattern classification
    pattern: str = ""  # delegation, progressive_reliance, iterative_debugging,
                       # generation_comprehension, hybrid_explanation, conceptual_inquiry
    predicted_retention: float = 0.0  # estimated % based on pattern

    # Timing
    total_time_seconds: float = 0.0
    steps_count: int = 0


def analyze_session(output_text: str, dev_interactions: list = None) -> SessionMetrics:
    """Analyze a Mentor session output and compute metrics."""
    m = SessionMetrics()
    m.timestamp = time.time()

    # Phase detection
    if "Phase 1: DECIDE" in output_text or "DECIDE" in output_text:
        m.phase_decide = True
    if "Phase 2: GENERATE" in output_text or "GENERATE" in output_text:
        m.phase_generate = True
    if "Phase 3: ALERT" in output_text or "ALERT" in output_text:
        m.phase_alert = True
    if "Phase 4: VERIFY" in output_text or "VERIFY" in output_text:
        m.phase_verify = True
    m.phases_covered = sum([m.phase_decide, m.phase_generate, m.phase_alert, m.phase_verify])

    # Risk counting
    m.risks_identified = output_text.count("⚠") + output_text.count("Risk:")

    # Alternative counting
    m.alternatives_presented = len(re.findall(r'(?:Approach|Option|Alternative)\s+[ABC]', output_text))
    if m.alternatives_presented == 0:
        m.alternatives_presented = len(re.findall(r'(?:A\)|B\)|C\))', output_text))

    # Review checklist
    m.review_checklist_items = len(re.findall(r'(?:✅|☐|□|\d+\.)\s+', output_text.split("Review")[-1])) if "Review" in output_text else 0

    # Tradeoffs
    m.tradeoffs_explained = output_text.lower().count("tradeoff") + output_text.lower().count("trade-off") + output_text.lower().count("trade off")

    # Failure modes
    m.failure_modes_shown = output_text.lower().count("would break") + output_text.lower().count("could fail") + output_text.lower().count("failure mode") + output_text.lower().count("edge case")

    # Developer interactions
    if dev_interactions:
        for interaction in dev_interactions:
            m.total_dev_interactions += 1
            lower = interaction.lower()
            if lower == "risks":
                m.dev_risk_inquiries += 1
            elif lower == "alternatives":
                m.dev_alternative_inquiries += 1
            elif lower == "review":
                m.dev_review_inquiries += 1
            elif lower == "why":
                m.dev_why_inquiries += 1
            elif lower.startswith("change:"):
                m.dev_change_requests += 1
            else:
                m.dev_questions += 1

    # Classify learning pattern
    m.pattern, m.predicted_retention = classify_pattern(m)

    # Step counting
    m.steps_count = output_text.count("🔧 Tool:")

    return m


def classify_pattern(m: SessionMetrics) -> tuple:
    """Classify the interaction pattern and predict retention.

    Based on Shen & Tamkin (2026) Figure 11:
    - AI Delegation: only code generation, no questions → 19%
    - Progressive AI Reliance: starts with questions, then delegates → 35%
    - Iterative AI Debugging: lots of debugging queries → 24%
    - Conceptual Inquiry: conceptual questions, independent error resolution → 65%
    - Hybrid Code-Explanation: code + explanations → 68%
    - Generation-Then-Comprehension: generate then understand → 86%
    """

    # FINDING-2 fix: detect iterative_debugging pattern
    # Lots of debugging queries with minimal phase coverage = debugging loop
    if m.phases_covered <= 1 and m.total_dev_interactions >= 3 and m.dev_questions >= 2:
        return "iterative_debugging", 0.24

    # If mentor showed all 4 phases AND risks/alternatives
    if m.phases_covered >= 3 and m.risks_identified >= 3:
        # FINDING-1 fix: include dev_why_inquiries in active engagement check
        active_inquiries = (m.dev_risk_inquiries + m.dev_alternative_inquiries
                           + m.dev_review_inquiries + m.dev_why_inquiries)
        if m.total_dev_interactions > 0 and active_inquiries > 0:
            # Developer actively engaged with supervisory features
            return "generation_comprehension", 0.86
        else:
            # Mentor narrated but dev was passive — still better than delegation
            return "hybrid_explanation", 0.68

    elif m.phases_covered >= 2 and m.alternatives_presented >= 2:
        # Showed alternatives, dev can make informed decisions
        return "conceptual_inquiry", 0.65

    elif m.phases_covered >= 2:
        # Some phases but limited risks/alternatives
        return "hybrid_explanation", 0.68

    elif m.phases_covered == 1 and m.phase_generate:
        # Only generated code, no explanation
        if m.total_dev_interactions == 0:
            return "ai_delegation", 0.19
        else:
            return "progressive_reliance", 0.35

    else:
        # Fallback
        return "hybrid_explanation", 0.68


def format_report(m: SessionMetrics) -> str:
    """Format metrics as a human-readable report."""
    lines = [
        "╔══════════════════════════════════════════════════╗",
        "║       MENTOR v2 — Session Quality Report         ║",
        "╚══════════════════════════════════════════════════╝",
        "",
        f"Task: {m.task[:60]}",
        f"Duration: {m.total_time_seconds:.0f}s, Steps: {m.steps_count}",
        "",
        "── Phase Coverage ──",
        f"  {'✅' if m.phase_decide else '❌'} DECIDE: Present alternatives and tradeoffs",
        f"  {'✅' if m.phase_generate else '❌'} GENERATE: Write code with narration",
        f"  {'✅' if m.phase_alert else '❌'} ALERT: Show risks and edge cases",
        f"  {'✅' if m.phase_verify else '❌'} VERIFY: Test and provide review checklist",
        f"  Coverage: {m.phases_covered}/4 phases",
        "",
        "── Supervisory Content ──",
        f"  ⚠ Risks identified: {m.risks_identified}",
        f"  🔀 Alternatives presented: {m.alternatives_presented}",
        f"  📋 Review checklist items: {m.review_checklist_items}",
        f"  ⚖️ Tradeoffs explained: {m.tradeoffs_explained}",
        f"  💥 Failure modes shown: {m.failure_modes_shown}",
        "",
        "── Developer Engagement ──",
        f"  💬 Total interactions: {m.total_dev_interactions}",
        f"  ❓ Questions asked: {m.dev_questions}",
        f"  ⚠ Risk inquiries: {m.dev_risk_inquiries}",
        f"  🔀 Alternative inquiries: {m.dev_alternative_inquiries}",
        f"  📋 Review inquiries: {m.dev_review_inquiries}",
        "",
        "── Learning Pattern Classification ──",
        f"  Pattern: {m.pattern.replace('_', ' ').title()}",
        f"  Predicted retention: {m.predicted_retention*100:.0f}%",
        "",
    ]

    # Comparison bar
    patterns = [
        ("AI Delegation", 0.19),
        ("Iterative Debugging", 0.24),
        ("Progressive Reliance", 0.35),
        ("Conceptual Inquiry", 0.65),
        ("Hybrid Explanation", 0.68),
        ("Generation-Comprehension", 0.86),
    ]
    lines.append("── Retention Comparison (Shen & Tamkin 2026) ──")
    for name, score in patterns:
        bar = "█" * int(score * 30)
        marker = " ◄ YOU" if name.lower().replace(" ", "_").replace("-", "_") == m.pattern else ""
        lines.append(f"  {name:28s} {bar} {score*100:.0f}%{marker}")

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI: Analyze a session log file
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    # Demo: analyze the drag-and-drop session output
    sample = """
    📚 ## Phase 1: DECIDE
    Approach A: HTML5 drag and drop
    Approach B: React DnD library
    I recommend A because...

    📚 ## Phase 2: GENERATE
    Adding draggable="true" to cards...

    📚 ## Phase 3: ALERT
    ⚠ Risk: No touch support on mobile
    ⚠ Risk: Drag events don't work in older browsers
    ⚠ Risk: No visual feedback during drag
    ⚠ Edge case: What if the user drags to the same column?

    📚 ## Phase 4: VERIFY
    ✅ Drag between columns works
    ✅ API call updates status
    Review checklist:
    1. Check mobile touch events
    2. Check error handling on failed API call
    3. Verify state sync via WebSocket

    🔧 Tool: search_code
    🔧 Tool: read_file
    🔧 Tool: edit_file
    🔧 Tool: run_command
    🔧 Tool: done
    """

    metrics = analyze_session(sample)
    metrics.task = "Add drag-and-drop to Kanban board"
    metrics.total_time_seconds = 224.7
    print(format_report(metrics))
