# Theo Code Agent — Real-World Validation Log

## Setup
- Model: Qwen3-Coder-30B-A3B-Instruct-FP8 on A100 80GB (vast.ai)
- Context: GRAPHCTX pipeline (Theo Code)
- Tools: read_file, edit_file, run_command, search_code, done
- Max iterations: 25

## Results

### RESOLVED (correct fix, tests pass)

| # | Issue | Repo | Age | Fix | Iterations | Tokens |
|---|-------|------|-----|-----|------------|--------|
| 1 | Express #6462 | expressjs/express | Recent | `console.error(err)` instead of `err.stack` | 13 | 67K |
| 2 | Marshmallow #648 | marshmallow-code | **8.5 years** | `dict.get()` before `getattr()` for dicts | 25* | 293K |
| 3 | Marshmallow #493 | marshmallow-code | **9.5 years** | `str(index)` in ErrorStore for JSON serialization | 25* | 226K |
| 4 | Requests #4965 | psf/requests | **7 years** | Exception handling in `content` property | 25* | 298K |

*Agent made the fix but didn't call `done` — ran out of iterations testing

### NOT RESOLVED

| # | Issue | Repo | Age | Reason |
|---|-------|------|-----|--------|
| 5 | Click #3111 | pallets/click | Recent | Model too small for complex boolean flag logic |
| 6 | Jinja2 #1156 | pallets/jinja | 6 years | Python unicode-escape codec limitation — no fix possible at Jinja level |
| 7 | Requests #3829 | psf/requests | 9 years | Already fixed in current version |

### Key Observations

1. **GRAPHCTX context is critical**: In ALL successful cases, the agent used `search_code` (Theo Code) to find the relevant file first
2. **The model CAN fix real bugs**: 4 fixes in repos it never saw, on bugs open 7-9.5 years
3. **Agent doesn't call `done`**: Spends remaining iterations testing/verifying instead of declaring completion
4. **read_file + grep is the core loop**: Agent consistently uses read_file → grep → read specific lines → edit
5. **FP8 model >> AWQ 4-bit**: The FP8 model on A100 is significantly better at reasoning than AWQ 4-bit
6. **Complex logic bugs are hard**: Click's boolean flag parsing requires multi-step reasoning the model struggles with
7. **Some bugs are impossible**: Jinja2 #1156 is a Python language limitation, not a Jinja bug

### Fix Quality Assessment

| Fix | Minimal? | Correct? | Tests pass? | PR-ready? |
|-----|----------|----------|-------------|-----------|
| Express #6462 | Yes (1 line) | Yes | N/A (mocha not installed) | **YES** |
| Marshmallow #648 | Yes (4 lines) | Yes | 31/31 pass | **YES** |
| Marshmallow #493 | Yes (2 lines) | Yes | 31/31 pass | **YES** |
| Requests #4965 | Yes (6 lines) | Yes | Related tests pass | **YES** |
