#!/usr/bin/env python3
"""LLM-as-reranker probe.

Reads BM25 top-K candidates from /tmp/probe-bm25-candidates.json (produced by
`benchmark_dump_bm25_candidates` Rust test), reranks each query's candidates
via the ChatGPT-Codex Responses API (using the OAuth token already stored in
~/.config/theo/auth.json), then computes MRR / R@5 / R@10 / nDCG@5 against
the ground truth.

Output: /tmp/probe-llm-rerank.metrics.txt (aggregates).

This is a measurement probe — does not modify any production code.
"""
import json
import math
import os
import sys
import time
from pathlib import Path

import urllib.request
import urllib.error

CANDIDATES_PATH = sys.argv[1] if len(sys.argv) > 1 else "/tmp/probe-bm25-candidates.json"
AUTH_PATH = Path.home() / ".config" / "theo" / "auth.json"
METRICS_OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/probe-llm-rerank.metrics.txt"
ENDPOINT = "https://chatgpt.com/backend-api/codex/responses"
MODEL = "gpt-5.3-codex"


def load_oauth():
    with open(AUTH_PATH) as f:
        store = json.load(f)
    entry = store.get("openai")
    if not entry or entry.get("type") != "oauth":
        raise SystemExit("No OpenAI OAuth entry in auth.json")
    expires_at = entry.get("expires_at", 0)
    if expires_at and expires_at <= time.time():
        raise SystemExit("OAuth token expired — run `theo login` again")
    return entry["access_token"], entry.get("account_id")


def build_prompt(query: str, candidates: list) -> str:
    lines = [
        f"You are a code-search reranker. Given the user query and a list of candidate files (with their symbols), return the indices of the 10 MOST RELEVANT files in order from most to least relevant.",
        "",
        f"User query: {query}",
        "",
        "Candidates (numbered from 0):",
    ]
    for i, c in enumerate(candidates):
        syms = ", ".join(c.get("symbols", [])[:15])
        lines.append(f"  [{i}] {c['path']}  symbols=[{syms}]")
    lines.append("")
    lines.append('Return ONLY a JSON object: {"top_10": [N, N, N, ...]} where each N is an index from 0 to ' + str(len(candidates) - 1) + ". No prose, no markdown fences.")
    return "\n".join(lines)


def call_codex(access_token: str, account_id: str | None, prompt: str) -> str:
    body = {
        "model": MODEL,
        "instructions": "You are a precise code-search reranker. Return ONLY valid JSON matching the schema requested. No prose, no markdown.",
        "input": [
            {
                "role": "user",
                "content": [{"type": "input_text", "text": prompt}],
            }
        ],
        "stream": True,
        "store": False,
    }
    headers = {
        "Authorization": f"Bearer {access_token}",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "OpenAI-Beta": "responses=experimental",
        "originator": "codex_cli_rs",
    }
    if account_id:
        headers["ChatGPT-Account-Id"] = account_id

    req = urllib.request.Request(
        ENDPOINT,
        data=json.dumps(body).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        err_body = e.read().decode("utf-8", errors="replace")[:300]
        raise SystemExit(f"HTTP {e.code} from codex: {err_body}")

    return parse_sse_text(raw)


def parse_sse_text(raw: str) -> str:
    """Codex SSE: events are 'event: response.output_text.delta' with data
    JSON containing partial deltas, plus a final 'response.completed' event.
    Concatenate all output_text deltas."""
    text_parts = []
    for line in raw.splitlines():
        if not line.startswith("data: "):
            continue
        payload = line[len("data: "):]
        if payload == "[DONE]":
            break
        try:
            obj = json.loads(payload)
        except json.JSONDecodeError:
            continue
        ev_type = obj.get("type", "")
        if ev_type == "response.output_text.delta":
            delta = obj.get("delta", "")
            if isinstance(delta, str):
                text_parts.append(delta)
        elif ev_type == "response.completed":
            # Already collected all deltas
            pass
    return "".join(text_parts)


def parse_top_10(text: str, n_candidates: int) -> list[int] | None:
    """Try to extract JSON {"top_10": [...]} from LLM output."""
    # Find first { ... } matching "top_10"
    s = text.strip()
    # Strip possible code fences
    if s.startswith("```"):
        s = s.split("```")[1] if "```" in s[3:] else s[3:]
        if s.startswith("json"):
            s = s[4:]
    # Find balanced JSON
    start = s.find("{")
    if start < 0:
        return None
    depth = 0
    end = -1
    for i, c in enumerate(s[start:], start=start):
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                end = i
                break
    if end < 0:
        return None
    try:
        obj = json.loads(s[start:end + 1])
    except json.JSONDecodeError:
        return None
    top = obj.get("top_10")
    if not isinstance(top, list):
        return None
    out = []
    for x in top:
        if isinstance(x, int) and 0 <= x < n_candidates:
            out.append(x)
        if len(out) >= 10:
            break
    return out if out else None


# ------- metrics ------------

def recall_at_k(returned: list[str], expected: list[str], k: int) -> float:
    if not expected:
        return 1.0
    exp = set(expected)
    hits = sum(1 for f in returned[:k] if f in exp)
    return hits / len(exp)


def mrr(returned: list[str], expected: list[str]) -> float:
    exp = set(expected)
    for i, f in enumerate(returned, start=1):
        if f in exp:
            return 1.0 / i
    return 0.0


def ndcg_at_k(returned: list[str], expected: list[str], k: int) -> float:
    exp = set(expected)
    dcg = 0.0
    for i, f in enumerate(returned[:k], start=1):
        if f in exp:
            dcg += 1.0 / math.log2(i + 1)
    ideal = sum(1.0 / math.log2(i + 1) for i in range(1, min(k, len(exp)) + 1))
    return dcg / ideal if ideal > 0 else 0.0


def precision_at_k(returned: list[str], expected: list[str], k: int) -> float:
    exp = set(expected)
    return sum(1 for f in returned[:k] if f in exp) / k


def main():
    if not Path(CANDIDATES_PATH).exists():
        raise SystemExit(f"Missing {CANDIDATES_PATH}. Run `cargo test ... benchmark_dump_bm25_candidates` first.")

    with open(CANDIDATES_PATH) as f:
        dump = json.load(f)

    access_token, account_id = load_oauth()
    print(f"Loaded OAuth (account_id={account_id or 'none'})")

    metrics = []
    failures = []

    for q in dump["queries"]:
        cands = q["candidates"]
        if not cands:
            continue
        prompt = build_prompt(q["query"], cands)
        try:
            text = call_codex(access_token, account_id, prompt)
        except Exception as e:
            failures.append((q["id"], "api_error", str(e)[:120]))
            continue

        indices = parse_top_10(text, len(cands))
        if not indices:
            failures.append((q["id"], "parse_error", text[:120]))
            continue

        reranked = [cands[i]["path"] for i in indices]
        expected = q["expected_files"]
        m = {
            "id": q["id"],
            "query": q["query"],
            "category": q["category"],
            "difficulty": q["difficulty"],
            "mrr": mrr(reranked, expected),
            "recall_at_5": recall_at_k(reranked, expected, 5),
            "recall_at_10": recall_at_k(reranked, expected, 10),
            "precision_at_5": precision_at_k(reranked, expected, 5),
            "ndcg_at_5": ndcg_at_k(reranked, expected, 5),
            "ndcg_at_10": ndcg_at_k(reranked, expected, 10),
        }
        metrics.append(m)
        print(f"  {q['id']:14s} '{q['query'][:40]:40s}' MRR={m['mrr']:.2f} R@5={m['recall_at_5']:.2f} nDCG@5={m['ndcg_at_5']:.2f}")

    if not metrics:
        raise SystemExit("No queries produced metrics — all failed")

    n = len(metrics)
    avg = {
        k: sum(m[k] for m in metrics) / n
        for k in ("mrr", "recall_at_5", "recall_at_10", "precision_at_5", "ndcg_at_5", "ndcg_at_10")
    }

    summary = (
        f"pipeline=BM25→ChatGPT-Codex Rerank (gpt-5-codex via OAuth)\n"
        f"n_queries={n}/{len(dump['queries'])}\n"
        f"failures={len(failures)}\n"
        f"MRR={avg['mrr']:.3f}\n"
        f"Recall@5={avg['recall_at_5']:.3f}\n"
        f"Recall@10={avg['recall_at_10']:.3f}\n"
        f"nDCG@5={avg['ndcg_at_5']:.3f}\n"
        f"nDCG@10={avg['ndcg_at_10']:.3f}\n"
        f"P@5={avg['precision_at_5']:.3f}\n"
    )

    print()
    print("=== Aggregate ===")
    print(summary)

    if failures:
        print(f"\nFailures ({len(failures)}):")
        for fid, kind, msg in failures[:10]:
            print(f"  {fid} [{kind}]: {msg}")

    Path(METRICS_OUT).write_text(summary)
    print(f"\nWrote {METRICS_OUT}")


if __name__ == "__main__":
    main()
