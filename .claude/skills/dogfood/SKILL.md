---
name: dogfood
description: FAANG-level dogfood testing of the Theo agent CLI. Runs the agent binary against real projects, validates all modes (agent/plan/pilot), tests skills, sub-agents, batch tool, roadmap integration, and produces a structured quality report with pass/fail verdicts.
allowed-tools: Bash(*), Read, Write, Grep, Glob, Agent
---

# Dogfood Test Suite — Theo Agent CLI

You are a Staff AI Systems Engineer at a FAANG company performing dogfood testing of the Theo Code autonomous agent. Your job is to run the ACTUAL binary against REAL projects, not unit tests. You test the system as a user would — end-to-end, with real LLM calls, real files, real output.

## Workspace

Binary: `cargo run --bin theo-code --`
Workspace: `/home/paulo/Projetos/usetheo/theo-code`

## Test Project Setup

Before each test suite, create a fresh test project:

```bash
TEST_DIR=$(mktemp -d /tmp/theo-dogfood-XXXXX)
mkdir -p "$TEST_DIR/src" "$TEST_DIR/.theo/plans" "$TEST_DIR/.theo/skills"

# Minimal Rust project
cat > "$TEST_DIR/Cargo.toml" << 'EOF'
[package]
name = "dogfood-test"
version = "0.1.0"
edition = "2021"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
EOF

cat > "$TEST_DIR/src/main.rs" << 'EOF'
fn main() { println!("Hello from dogfood test"); }
EOF
```

## Test Suites

Run suites based on $ARGUMENTS:
- `all` or empty → run all suites
- `quick` → Suites 1-3 only (fast validation)
- `modes` → Suite 2 only (Agent/Plan/Ask)
- `pilot` → Suite 4 only (Pilot + Roadmap)
- `skills` → Suite 5 only (Skills system)
- `batch` → Suite 6 only (BatchTool)
- `stress` → Suite 7 only (Edge cases + limits)

### Suite 1: Smoke Test (Build + Basic Interaction)

```bash
# 1.1 Binary compiles
cargo build --bin theo-code 2>&1 | tail -3
# PASS if: no errors

# 1.2 Help text
cargo run --bin theo-code 2>&1 | head -10
# PASS if: shows "theo-code v0.1.0" and lists agent, pilot, context, impact, stats

# 1.3 Version flag
cargo run --bin theo-code -- --version 2>&1
# PASS if: "theo-code v0.1.0"

# 1.4 Unit tests green
cargo test -p theo-agent-runtime 2>&1 | grep "test result"
# PASS if: 0 failed
```

### Suite 2: Agent Modes (Agent / Plan / Ask)

Use pipe mode (echo | cargo run) for non-interactive testing.

```bash
# 2.1 Agent mode — basic task (single-shot, no REPL)
timeout 120 cargo run --bin theo-code -- agent "Crie um arquivo src/lib.rs com uma função pub fn add(a: i32, b: i32) -> i32" --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-agent.txt

# PASS if: src/lib.rs was created with add function
# CHECK: cat "$TEST_DIR/src/lib.rs" | grep "fn add"

# 2.2 Plan mode — generates roadmap file
echo 'Implementar um módulo de validação com testes' | \
  timeout 120 cargo run --bin theo-code -- agent --mode plan --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-plan.txt

# PASS if: .theo/plans/*.md file created
# CHECK: ls "$TEST_DIR/.theo/plans/"
# CHECK: grep "Microtasks" "$TEST_DIR/.theo/plans/"*.md

# 2.3 Ask mode — asks questions instead of acting
timeout 120 cargo run --bin theo-code -- agent "Refatore o código para melhorar a qualidade" --mode ask --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-ask.txt

# PASS if: output contains questions (?) without file modifications
# CHECK: grep -c "?" /tmp/dogfood-ask.txt  (should be > 0)
# CHECK: verify no files were modified: find "$TEST_DIR/src" -newer "$TEST_DIR/Cargo.toml" -name "*.rs" | wc -l  (should be 0 or only lib.rs from 2.1)

# 2.4 Mode switching banner
echo '/exit' | cargo run --bin theo-code -- agent --mode plan --repo "$TEST_DIR" 2>&1 | head -5
# PASS if: banner shows "Mode: plan"
```

### Suite 3: Tools Validation

```bash
# 3.1 Read tool
echo 'Leia o arquivo Cargo.toml e me diga o nome do package' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-read.txt
# PASS if: output mentions "dogfood-test"

# 3.2 Batch tool (if agent uses it)
echo 'Leia os arquivos Cargo.toml e src/main.rs ao mesmo tempo' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-batch.txt
# PASS if: both files content appear in output OR batch tool used
# CHECK: grep -E "batch|Batch|Read.*Cargo.*Read.*main" /tmp/dogfood-batch.txt

# 3.3 Think tool
echo 'Pense sobre qual seria a melhor arquitetura para este projeto' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-think.txt
# PASS if: 💭 appears in output (think tool used)
# CHECK: grep "💭" /tmp/dogfood-think.txt

# 3.4 Doom loop detection — verify it doesn't exist in output
echo 'Leia o arquivo inexistente.rs repetidamente' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-doom.txt
# PASS if: agent doesn't loop forever (exits within timeout)
```

### Suite 4: Pilot + Roadmap

```bash
# 4.1 Pilot with inline promise
timeout 180 cargo run --bin theo-code -- pilot \
  "Criar uma função multiply em src/lib.rs com teste" \
  --complete "cargo test passa com teste de multiply" \
  --calls 3 --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-pilot.txt

# PASS if: "Pilot Complete" or "Max calls reached" in output
# CHECK: grep -E "Pilot Complete|Max calls" /tmp/dogfood-pilot.txt
# CHECK: grep "multiply" "$TEST_DIR/src/lib.rs" 2>/dev/null

# 4.2 Pilot with PROMPT.md
cat > "$TEST_DIR/.theo/PROMPT.md" << 'EOF'
Adicionar uma função divide(a: f64, b: f64) -> Result<f64, String> com tratamento de divisão por zero e teste.
EOF

timeout 180 cargo run --bin theo-code -- pilot --calls 3 --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-prompt.txt
# PASS if: pilot runs and attempts the task
# CHECK: grep -E "Pilot|Loop" /tmp/dogfood-prompt.txt

# 4.3 Pilot with roadmap execution
cat > "$TEST_DIR/.theo/plans/01-test-roadmap.md" << 'ROADMAP'
# Roadmap: Test Roadmap

## Microtasks

### Task 1: Create helper module
- **Arquivo(s)**: src/helpers.rs
- **O que fazer**: Create a helpers module with a greet function
- **Critério de aceite**: File exists with pub fn greet
- **DoD**: cargo check passes

### Task 2: Wire module in main
- **Arquivo(s)**: src/main.rs
- **O que fazer**: Add mod helpers and call greet from main
- **Critério de aceite**: cargo run prints greeting
- **DoD**: cargo build passes
ROADMAP

timeout 180 cargo run --bin theo-code -- pilot "Executar roadmap" --calls 5 --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-roadmap.txt
# PASS if: roadmap detected, tasks attempted
# CHECK: grep "Roadmap:" /tmp/dogfood-roadmap.txt
# CHECK: grep "pending tasks" /tmp/dogfood-roadmap.txt

# 4.4 Roadmap task completion marking
# CHECK: grep "✅" "$TEST_DIR/.theo/plans/01-test-roadmap.md"
```

### Suite 5: Skills System

```bash
# 5.1 /skills command lists skills
echo -e '/skills\n/exit' | cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-skills.txt
# PASS if: lists commit, test, review, build, explain, fix, refactor, pr, doc, deps
# CHECK: grep -c "commit\|test\|review\|build\|explain" /tmp/dogfood-skills.txt

# 5.2 Test skill invocation
echo 'rode os testes' | \
  timeout 120 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-test-skill.txt
# PASS if: [Verifier] appears (sub-agent spawned for test skill)
# CHECK: grep "Verifier" /tmp/dogfood-test-skill.txt

# 5.3 Custom project skill override
cat > "$TEST_DIR/.theo/skills/greet.md" << 'SKILL'
---
name: greet
trigger: when the user says hello or greet
mode: in_context
---
Respond with "Olá! Sou o Theo Agent. Como posso ajudar?" and nothing else. Call done immediately.
SKILL

echo 'olá' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-custom-skill.txt
# PASS if: output contains greeting (skill loaded from project .theo/skills/)
```

### Suite 6: BatchTool

```bash
# 6.1 Batch in system prompt
cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 <<< '/exit' | grep -i batch
# PASS if: system prompt or help mentions batch

# 6.2 Batch tool available in definitions
echo 'Quais ferramentas voce tem disponíveis?' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-tools.txt
# PASS if: agent mentions batch tool or uses it
```

### Suite 7: Edge Cases + Stress

```bash
# 7.1 Empty input handling
echo '' | timeout 30 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1
# PASS if: exits cleanly without crash

# 7.2 Pilot with --calls 1
timeout 60 cargo run --bin theo-code -- pilot "Hello" --calls 1 --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-calls1.txt
# PASS if: "Max calls reached" in output

# 7.3 Non-existent repo
cargo run --bin theo-code -- agent --repo /nonexistent/path 2>&1
# PASS if: shows error message, exit code != 0

# 7.4 Project without .theo/ dir
BARE_DIR=$(mktemp -d /tmp/theo-bare-XXXXX)
echo "fn main() {}" > "$BARE_DIR/main.rs"
echo 'Olá' | timeout 60 cargo run --bin theo-code -- agent --repo "$BARE_DIR" 2>&1 | tee /tmp/dogfood-bare.txt
# PASS if: works without .theo/ (graceful degradation)

# 7.5 Token tracking visible
echo 'O que é este projeto?' | \
  timeout 60 cargo run --bin theo-code -- agent --repo "$TEST_DIR" 2>&1 | tee /tmp/dogfood-tokens.txt
# PASS if: token count appears in output (e.g., "1.2k tokens")
# CHECK: grep -E "[0-9]+\.?[0-9]*[kM]? ?tokens" /tmp/dogfood-tokens.txt
```

## Report Format

After running each suite, produce a structured report:

```
═══════════════════════════════════════════
  THEO DOGFOOD REPORT — [date]
═══════════════════════════════════════════

Suite 1: Smoke Test
  1.1 Binary compiles .............. PASS
  1.2 Help text ................... PASS
  1.3 Version flag ................ PASS
  1.4 Unit tests green ............ PASS (247 passed)

Suite 2: Agent Modes
  2.1 Agent mode basic task ....... PASS (created src/lib.rs)
  2.2 Plan mode roadmap ........... FAIL (no roadmap file created)
  2.3 Ask mode questions .......... PASS (3 questions asked)
  2.4 Mode banner ................. PASS (shows "Mode: plan")

[... etc ...]

═══════════════════════════════════════════
  SUMMARY: 18/22 PASS | 3 FAIL | 1 SKIP
═══════════════════════════════════════════

FAILURES:
  2.2 Plan mode — LLM did not call write tool for roadmap
    → Root cause: model compliance issue
    → Recommendation: strengthen system prompt enforcement

  4.3 Roadmap execution — Task 2 not completed
    → Root cause: agent ran out of iterations
    → Recommendation: increase max_iterations for pilot tasks

QUALITY ASSESSMENT:
  ┌────────────────────┬────────┐
  │ Category           │ Grade  │
  ├────────────────────┼────────┤
  │ Build & Compile    │ A      │
  │ Agent Modes        │ B+     │
  │ Tool Execution     │ A      │
  │ Pilot/Roadmap      │ B      │
  │ Skills System      │ A-     │
  │ Edge Cases         │ A      │
  │ Token Efficiency   │ B+     │
  ├────────────────────┼────────┤
  │ OVERALL            │ B+     │
  └────────────────────┴────────┘

RECOMMENDATIONS:
  1. [priority] description
  2. [priority] description
```

## Grading Rubric

| Grade | Criteria |
|-------|----------|
| A | 100% pass, no issues |
| A- | 90%+ pass, minor cosmetic issues |
| B+ | 80%+ pass, functional but rough edges |
| B | 70%+ pass, notable gaps |
| C | 50%+ pass, significant issues |
| F | <50% pass, broken |

## Rules

1. ALWAYS run the actual binary — never simulate or mock
2. Use timeout on every command (prevent hanging)
3. Capture ALL output to files for evidence
4. Check results with grep/cat — don't trust the agent's self-report
5. Be BRUTALLY HONEST in the report — this is FAANG-level QA
6. If a test fails, investigate WHY (read logs, check files)
7. Clean up test directories after the suite
8. Report token usage per suite if visible

Argumento: $ARGUMENTS
