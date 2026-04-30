#!/usr/bin/env python3
"""Cycle 14: LLM query rewriting + multi-query retrieval + RRF fusion + final rerank.

Steps:
  1. Read ground-truth queries (theo-code.json).
  2. For each query, ask Codex to generate 4 alternate phrasings.
  3. Write flattened query list to /tmp/probe-query-list.json (30 originals × 5 = 150).
  4. Run `cargo test ... benchmark_dump_for_query_list` to get BM25 top-50 per phrasing.
  5. Per original: RRF-fuse the 5 candidate lists → unified top-50.
  6. Send unified top-50 to Codex for final rerank → top-10.
  7. Compute MRR / R@5 / R@10 / nDCG@5 vs expected_files.

Output: /tmp/probe-llm-rerank-rewrite.metrics.txt
"""
import json
import math
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

GT_PATH = "crates/theo-engine-retrieval/tests/benchmarks/ground_truth/theo-code.json"
QUERY_LIST_PATH = "/tmp/probe-query-list.json"
MULTI_BM25_PATH = "/tmp/probe-multi-bm25.json"
METRICS_OUT = "/tmp/probe-llm-rerank-rewrite.metrics.txt"
AUTH_PATH = Path.home() / ".config" / "theo" / "auth.json"
ENDPOINT = "https://chatgpt.com/backend-api/codex/responses"
MODEL = "gpt-5.3-codex"


def load_oauth():
    with open(AUTH_PATH) as f:
        store = json.load(f)
    e = store.get("openai")
    if not e or e.get("type") != "oauth":
        raise SystemExit("No OpenAI OAuth in auth.json")
    if e.get("expires_at", 0) and e["expires_at"] <= time.time():
        raise SystemExit("OAuth token expired")
    return e["access_token"], e.get("account_id")


def codex_call(token, account_id, instructions, prompt):
    body = {
        "model": MODEL,
        "instructions": instructions,
        "input": [{"role": "user", "content": [{"type": "input_text", "text": prompt}]}],
        "stream": True,
        "store": False,
    }
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "OpenAI-Beta": "responses=experimental",
        "originator": "codex_cli_rs",
    }
    if account_id:
        headers["ChatGPT-Account-Id"] = account_id
    req = urllib.request.Request(ENDPOINT, data=json.dumps(body).encode(), headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            raw = r.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        body_err = e.read().decode("utf-8", errors="replace")[:300]
        raise SystemExit(f"HTTP {e.code}: {body_err}")

    parts = []
    for line in raw.splitlines():
        if not line.startswith("data: "):
            continue
        payload = line[6:]
        if payload == "[DONE]":
            break
        try:
            obj = json.loads(payload)
        except json.JSONDecodeError:
            continue
        if obj.get("type") == "response.output_text.delta":
            d = obj.get("delta", "")
            if isinstance(d, str):
                parts.append(d)
    return "".join(parts)


def parse_json_obj(text):
    s = text.strip()
    if s.startswith("```"):
        s = s.split("```", 2)[1] if s.count("```") >= 2 else s[3:]
        if s.startswith("json"):
            s = s[4:]
    start = s.find("{")
    if start < 0:
        return None
    depth = 0
    for i, c in enumerate(s[start:], start=start):
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                try:
                    return json.loads(s[start:i + 1])
                except json.JSONDecodeError:
                    return None
    return None


def step1_load_ground_truth():
    with open(GT_PATH) as f:
        return json.load(f)


def step2_rewrite_queries(token, account_id, gt):
    """For each query, generate 4 alternate phrasings."""
    rewrites = {}
    instructions = (
        "You generate alternate phrasings of code-search queries. Return ONLY valid JSON. "
        "No prose, no markdown."
    )
    for q in gt["queries"]:
        prompt = (
            f"Original query: {q['query']}\n\n"
            "Generate 4 alternative phrasings of this query that would surface DIFFERENT "
            "but related code files. Aim for variety:\n"
            "  1. Concrete identifiers / function names\n"
            "  2. Broader conceptual terms\n"
            "  3. Synonyms / related terminology\n"
            "  4. File path / module hints\n\n"
            'Return: {"rewrites": ["...", "...", "...", "..."]}'
        )
        text = codex_call(token, account_id, instructions, prompt)
        obj = parse_json_obj(text)
        if not obj or not isinstance(obj.get("rewrites"), list):
            print(f"  WARN: could not parse rewrites for {q['id']}, skipping rewrites")
            rewrites[q["id"]] = []
            continue
        rewrites[q["id"]] = [r for r in obj["rewrites"] if isinstance(r, str)][:4]
        print(f"  {q['id']:14s} '{q['query'][:35]:35s}' → {len(rewrites[q['id']])} rewrites")
    return rewrites


def step3_write_query_list(gt, rewrites):
    """Flatten: 30 originals + 4 rewrites each = up to 150 entries."""
    flat = []
    for q in gt["queries"]:
        flat.append({"id": f"{q['id']}:orig", "query": q["query"]})
        for i, rw in enumerate(rewrites.get(q["id"], [])):
            flat.append({"id": f"{q['id']}:rw{i}", "query": rw})
    Path(QUERY_LIST_PATH).write_text(json.dumps(flat))
    print(f"\nWrote {QUERY_LIST_PATH} ({len(flat)} queries)")
    return len(flat)


def step4_run_rust_dump():
    """Subprocess cargo to dump BM25 candidates per query."""
    print("\nRunning cargo BM25 dump (~2 min)...")
    cmd = [
        "cargo", "test", "-p", "theo-engine-retrieval",
        "--test", "benchmark_suite", "benchmark_dump_for_query_list",
        "--", "--ignored", "--nocapture",
    ]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    if r.returncode != 0:
        print(r.stderr[-1000:])
        raise SystemExit(f"cargo dump failed: exit {r.returncode}")
    print("  OK")


def step5_rrf_fuse_per_original(gt, rewrites):
    """Read multi-bm25 dump, fuse the 5 candidate lists per original via RRF."""
    with open(MULTI_BM25_PATH) as f:
        dump = json.load(f)
    by_id = {q["id"]: q["candidates"] for q in dump["queries"]}

    fused = {}
    K = 60.0  # RRF k (standard value)
    for q in gt["queries"]:
        # Collect all variant ids (original + rewrites that produced output)
        variant_ids = [f"{q['id']}:orig"] + [
            f"{q['id']}:rw{i}" for i in range(len(rewrites.get(q["id"], [])))
        ]
        # RRF fusion: score(d) = sum_i 1/(K + rank_i(d))
        scores = {}
        symbols_by_path = {}
        for vid in variant_ids:
            cands = by_id.get(vid, [])
            for rank, c in enumerate(cands, start=1):
                p = c["path"]
                scores[p] = scores.get(p, 0.0) + 1.0 / (K + rank)
                if p not in symbols_by_path:
                    symbols_by_path[p] = c.get("symbols", [])
        # Sort + take top-50
        ranked = sorted(scores.items(), key=lambda x: -x[1])[:50]
        fused[q["id"]] = [
            {"path": p, "rrf_score": s, "symbols": symbols_by_path.get(p, [])}
            for p, s in ranked
        ]
    return fused


def step6_final_rerank(token, account_id, gt, fused):
    """Send fused top-50 to Codex for final rerank → top-10."""
    instructions = (
        "You are a precise code-search reranker. Return ONLY valid JSON matching the schema. "
        "No prose, no markdown."
    )
    metrics = []
    for q in gt["queries"]:
        cands = fused.get(q["id"], [])
        if not cands:
            continue
        lines = [
            f"Query: {q['query']}",
            "",
            "Candidates (numbered from 0):",
        ]
        for i, c in enumerate(cands):
            syms = ", ".join(c.get("symbols", [])[:15])
            lines.append(f"  [{i}] {c['path']}  symbols=[{syms}]")
        lines.append("")
        lines.append(f'Return: {{"top_10": [N, ...]}} indices 0..{len(cands)-1}.')
        prompt = "\n".join(lines)

        text = codex_call(token, account_id, instructions, prompt)
        obj = parse_json_obj(text)
        if not obj or not isinstance(obj.get("top_10"), list):
            print(f"  {q['id']:14s} parse_error")
            continue
        indices = [x for x in obj["top_10"] if isinstance(x, int) and 0 <= x < len(cands)][:10]
        if not indices:
            continue
        returned = [cands[i]["path"] for i in indices]
        m = compute_metrics(returned, q["expected_files"])
        m["id"] = q["id"]
        m["category"] = q["category"]
        metrics.append(m)
        print(f"  {q['id']:14s} '{q['query'][:35]:35s}' MRR={m['mrr']:.2f} R@5={m['recall_at_5']:.2f}")
    return metrics


def compute_metrics(returned, expected):
    exp = set(expected)
    mrr_v = 0.0
    for i, f in enumerate(returned, start=1):
        if f in exp:
            mrr_v = 1.0 / i
            break
    r5 = sum(1 for f in returned[:5] if f in exp) / max(1, len(exp))
    r10 = sum(1 for f in returned[:10] if f in exp) / max(1, len(exp))
    p5 = sum(1 for f in returned[:5] if f in exp) / 5.0
    dcg5 = sum(1 / math.log2(i + 1) for i, f in enumerate(returned[:5], start=1) if f in exp)
    ideal5 = sum(1 / math.log2(i + 1) for i in range(1, min(5, len(exp)) + 1))
    ndcg5 = dcg5 / ideal5 if ideal5 > 0 else 0.0
    dcg10 = sum(1 / math.log2(i + 1) for i, f in enumerate(returned[:10], start=1) if f in exp)
    ideal10 = sum(1 / math.log2(i + 1) for i in range(1, min(10, len(exp)) + 1))
    ndcg10 = dcg10 / ideal10 if ideal10 > 0 else 0.0
    return {
        "mrr": mrr_v,
        "recall_at_5": r5,
        "recall_at_10": r10,
        "precision_at_5": p5,
        "ndcg_at_5": ndcg5,
        "ndcg_at_10": ndcg10,
    }


def main():
    token, account_id = load_oauth()
    print(f"OAuth loaded (account_id={account_id or 'none'})\n")

    print("=== Step 1: load ground truth ===")
    gt = step1_load_ground_truth()
    print(f"  {len(gt['queries'])} queries\n")

    print("=== Step 2: rewrite queries via Codex ===")
    rewrites = step2_rewrite_queries(token, account_id, gt)

    print("\n=== Step 3: write flattened query list ===")
    n_total = step3_write_query_list(gt, rewrites)

    print("\n=== Step 4: cargo BM25 dump ===")
    step4_run_rust_dump()

    print("\n=== Step 5: RRF fuse per original ===")
    fused = step5_rrf_fuse_per_original(gt, rewrites)
    avg_size = sum(len(v) for v in fused.values()) / max(1, len(fused))
    print(f"  fused candidate set avg size: {avg_size:.1f}")

    print("\n=== Step 6: final Codex rerank ===")
    metrics = step6_final_rerank(token, account_id, gt, fused)

    if not metrics:
        raise SystemExit("No metrics produced")

    n = len(metrics)
    avg = {
        k: sum(m[k] for m in metrics) / n
        for k in ("mrr", "recall_at_5", "recall_at_10", "precision_at_5", "ndcg_at_5", "ndcg_at_10")
    }
    summary = (
        f"pipeline=Codex Query Rewrite (4) → multi-BM25 → RRF → Codex Rerank\n"
        f"n_queries={n}/{len(gt['queries'])}\n"
        f"MRR={avg['mrr']:.3f}\n"
        f"Recall@5={avg['recall_at_5']:.3f}\n"
        f"Recall@10={avg['recall_at_10']:.3f}\n"
        f"nDCG@5={avg['ndcg_at_5']:.3f}\n"
        f"nDCG@10={avg['ndcg_at_10']:.3f}\n"
        f"P@5={avg['precision_at_5']:.3f}\n"
    )
    print("\n=== Aggregate ===")
    print(summary)
    Path(METRICS_OUT).write_text(summary)
    print(f"Wrote {METRICS_OUT}")


if __name__ == "__main__":
    main()
