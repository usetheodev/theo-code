"""Tests for tbench/agent.py — Phase 46.

Stdlib unittest only. Tests focus on the PURE-Python helpers we own:
  - version() includes the git SHA when available
  - parse_result(stdout) extracts the JSON line + computes cost_usd
  - parse_result handles malformed input gracefully
  - parse_result picks the LAST valid line (in case of multiple)
  - _OTLP_ENV_KEYS contract is stable

terminal_bench is a heavy dep — we patch its imports so the test runs
without it.
"""

from __future__ import annotations

import json
import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

# Stub the terminal_bench imports BEFORE importing tbench.agent so the
# adapter loads even on machines without terminal-bench installed.
_TB_STUB = type(sys)("terminal_bench")
_TB_AGENTS = type(sys)("terminal_bench.agents")
_TB_INSTALLED = type(sys)("terminal_bench.agents.installed_agents")
_TB_ABSTRACT = type(sys)("terminal_bench.agents.installed_agents.abstract_installed_agent")
_TB_MODELS = type(sys)("terminal_bench.harness_models")


class _StubAbstract:
    def __init__(self, *args, **kwargs):
        pass


class _StubTerminalCommand:
    def __init__(self, command: str, max_timeout_sec: float, block: bool):
        self.command = command
        self.max_timeout_sec = max_timeout_sec
        self.block = block


_TB_ABSTRACT.AbstractInstalledAgent = _StubAbstract
_TB_MODELS.TerminalCommand = _StubTerminalCommand
sys.modules.setdefault("terminal_bench", _TB_STUB)
sys.modules.setdefault("terminal_bench.agents", _TB_AGENTS)
sys.modules.setdefault("terminal_bench.agents.installed_agents", _TB_INSTALLED)
sys.modules.setdefault(
    "terminal_bench.agents.installed_agents.abstract_installed_agent", _TB_ABSTRACT
)
sys.modules.setdefault("terminal_bench.harness_models", _TB_MODELS)

from tbench.agent import TheoAgent, _OTLP_ENV_KEYS  # noqa: E402


class TestTheoAgentMetadata(unittest.TestCase):
    def test_name_is_theo_code(self) -> None:
        self.assertEqual(TheoAgent.name(), "theo-code")

    def test_version_starts_with_semver(self) -> None:
        v = TheoAgent.version()
        self.assertTrue(v.startswith("0.1.0+"), f"version was {v!r}")

    def test_version_includes_git_sha_or_unknown(self) -> None:
        v = TheoAgent.version()
        # Suffix is either a 7-char SHA or 'unknown'
        suffix = v.split("+", 1)[1]
        self.assertTrue(
            suffix == "unknown" or (len(suffix) == 7 and all(c.isalnum() for c in suffix)),
            f"unexpected version suffix: {suffix!r}",
        )

    def test_otlp_env_keys_includes_endpoint(self) -> None:
        self.assertIn("OTLP_ENDPOINT", _OTLP_ENV_KEYS)
        self.assertIn("OTLP_PROTOCOL", _OTLP_ENV_KEYS)
        self.assertIn("OTLP_SERVICE_NAME", _OTLP_ENV_KEYS)


class TestParseResult(unittest.TestCase):
    def _build_line(self, **overrides) -> str:
        payload = {
            "schema": "theo.headless.v2",
            "success": True,
            "summary": "ok",
            "iterations": 3,
            "tokens": {"input": 1000, "output": 200, "total": 1200},
            "tools": {"total": 5, "success": 5},
            "model": "gpt-5.4",
            "files_edited": ["src/main.rs"],
        }
        payload.update(overrides)
        return json.dumps(payload)

    def test_extracts_known_schema(self) -> None:
        stdout = "[debug] starting\n" + self._build_line() + "\n"
        result = TheoAgent.parse_result(stdout)
        self.assertTrue(result.get("success"))
        self.assertEqual(result.get("iterations"), 3)
        self.assertEqual(result.get("model"), "gpt-5.4")

    def test_attaches_cost_usd_using_pricing_table(self) -> None:
        # 1000 input * $5/1M + 200 output * $15/1M = $0.005 + $0.003 = $0.008
        stdout = self._build_line()
        result = TheoAgent.parse_result(stdout)
        self.assertAlmostEqual(result["cost_usd"], 0.008, places=4)

    def test_attaches_adapter_version(self) -> None:
        stdout = self._build_line()
        result = TheoAgent.parse_result(stdout)
        self.assertIn("adapter_version", result)
        self.assertTrue(result["adapter_version"].startswith("0.1.0+"))

    def test_picks_last_valid_line_when_multiple_present(self) -> None:
        stdout = (
            self._build_line(success=False, iterations=1) + "\n" +
            self._build_line(success=True, iterations=42) + "\n"
        )
        result = TheoAgent.parse_result(stdout)
        # parse_result iterates REVERSED and returns the FIRST hit found
        # walking backwards — that's the LAST line in the original order.
        self.assertEqual(result["iterations"], 42)
        self.assertTrue(result["success"])

    def test_handles_v1_schema_for_backward_compat(self) -> None:
        line = json.dumps({
            "schema": "theo.headless.v1",
            "success": True,
            "tokens": {"input": 100, "output": 50},
            "model": "gpt-5.4",
        })
        result = TheoAgent.parse_result(line)
        # v1 is accepted because the prefix match `theo.headless.*` is loose
        self.assertTrue(result.get("success"))

    def test_returns_empty_when_no_valid_json_line(self) -> None:
        stdout = "[noise] no JSON here\nanother line\n"
        self.assertEqual(TheoAgent.parse_result(stdout), {})

    def test_returns_empty_when_stdout_blank(self) -> None:
        self.assertEqual(TheoAgent.parse_result(""), {})

    def test_skips_malformed_json_lines(self) -> None:
        stdout = "{not valid json\n" + self._build_line() + "\n"
        result = TheoAgent.parse_result(stdout)
        self.assertTrue(result.get("success"))

    def test_cost_usd_is_zero_when_model_missing(self) -> None:
        line = json.dumps({
            "schema": "theo.headless.v2",
            "success": True,
            "tokens": {"input": 1000, "output": 100},
            # no model field
        })
        # Force-clear THEO_MODEL env so the fallback also yields ""
        with patch.dict(os.environ, {}, clear=False):
            os.environ.pop("THEO_MODEL", None)
            result = TheoAgent.parse_result(line)
        # Unknown model → cost 0
        self.assertEqual(result.get("cost_usd"), 0.0)

    def test_cost_usd_handles_missing_tokens_safely(self) -> None:
        line = json.dumps({
            "schema": "theo.headless.v2",
            "success": True,
            "model": "gpt-5.4",
            # no tokens key
        })
        result = TheoAgent.parse_result(line)
        self.assertEqual(result.get("cost_usd"), 0.0)


class TestRunAgentCommands(unittest.TestCase):
    def test_run_commands_returns_single_command(self) -> None:
        agent = TheoAgent()
        cmds = agent._run_agent_commands("do the thing")
        self.assertEqual(len(cmds), 1)

    def test_run_commands_quotes_task_description_safely(self) -> None:
        agent = TheoAgent()
        # Task description with single quotes + backticks — must not break out
        cmds = agent._run_agent_commands("test 'with quotes' and `backticks`")
        cmd_str = cmds[0].command
        # shlex.quote wraps in single quotes; embedded single quotes use
        # the standard shell idiom '"'"' (close-q, dquoted-q, open-q).
        self.assertIn("theo --headless", cmd_str)
        self.assertIn("'\"'\"'", cmd_str)
        # And backticks must be inside the single-quoted region (literal,
        # not command-substituted by the outer shell)
        self.assertIn("`backticks`", cmd_str)

    def test_run_commands_uses_max_iter_env_var(self) -> None:
        agent = TheoAgent()
        with patch.dict(os.environ, {"THEO_MAX_ITER": "77"}, clear=False):
            cmds = agent._run_agent_commands("x")
        self.assertIn("--max-iter 77", cmds[0].command)

    def test_run_commands_default_max_iter_is_50(self) -> None:
        agent = TheoAgent()
        with patch.dict(os.environ, {}, clear=False):
            os.environ.pop("THEO_MAX_ITER", None)
            cmds = agent._run_agent_commands("x")
        self.assertIn("--max-iter 50", cmds[0].command)


class TestEnvForwarding(unittest.TestCase):
    def test_env_forwards_otlp_endpoint_when_set(self) -> None:
        agent = TheoAgent()
        with patch.dict(
            os.environ,
            {"OTLP_ENDPOINT": "http://collector:4317"},
            clear=False,
        ):
            env = agent._env
        self.assertEqual(env.get("OTLP_ENDPOINT"), "http://collector:4317")

    def test_env_omits_unset_vars(self) -> None:
        agent = TheoAgent()
        with patch.dict(os.environ, {}, clear=True):
            env = agent._env
        for k in _OTLP_ENV_KEYS:
            self.assertNotIn(k, env)

    def test_env_uses_constructor_model_over_env(self) -> None:
        agent = TheoAgent(model_name="gpt-5.4-mini")
        with patch.dict(os.environ, {"THEO_MODEL": "ignored"}, clear=False):
            env = agent._env
        self.assertEqual(env.get("THEO_MODEL"), "gpt-5.4-mini")


if __name__ == "__main__":
    unittest.main(verbosity=2)
