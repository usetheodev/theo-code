"""Phase 54 (prompt-ab-testing-plan) — sourceable setup.sh smoke tests.

Validate the new __theo_prompt_variant_setup function:
  - exports THEO_SYSTEM_PROMPT_FILE when variant is set + URL reachable
  - is a no-op when THEO_PROMPT_VARIANT is unset
  - falls back gracefully when URL is unreachable

Each test spins up a one-shot Python http.server on localhost serving a
fake variant file, points THEO_PROMPT_HOST at it, sources setup.sh, and
asserts the resulting environment.
"""

from __future__ import annotations

import http.server
import os
import socket
import socketserver
import subprocess
import tempfile
import threading
import time
import unittest
from contextlib import contextmanager
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SETUP_SH = ROOT / "tbench" / "setup.sh"


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@contextmanager
def serve_dir(directory: Path):
    """Start a one-shot HTTP server on a free port, yield (host_url, stop_fn)."""
    port = _free_port()
    handler_cls = lambda *a, **k: http.server.SimpleHTTPRequestHandler(
        *a, directory=str(directory), **k
    )
    httpd = socketserver.TCPServer(("127.0.0.1", port), handler_cls)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    # Wait until server is reachable
    for _ in range(20):
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                break
        except OSError:
            time.sleep(0.05)
    try:
        yield f"http://127.0.0.1:{port}"
    finally:
        httpd.shutdown()
        thread.join(timeout=2)


def _source_setup_and_eval(env: dict[str, str], expr: str = "$THEO_SYSTEM_PROMPT_FILE") -> str:
    """Source setup.sh under bash with `env`, then echo `expr`."""
    # We skip the binary install entirely (THEO_BIN_URL pointing to a port
    # that's not running short-circuits to 'failed all 5 attempts'). What
    # matters here is the prompt-variant block runs after.
    full_env = dict(os.environ)
    full_env.update(env)
    # Prevent the binary install from spending 60s+ in retries
    full_env["THEO_SKIP_BIN_INSTALL"] = "1"
    cmd = [
        "bash", "-c",
        f"source '{SETUP_SH}' >/tmp/theo-setup-out.log 2>&1; printf '%s' \"{expr}\"",
    ]
    out = subprocess.run(
        cmd, env=full_env, capture_output=True, text=True, timeout=30
    )
    return out.stdout


class TestPromptVariantSetup(unittest.TestCase):
    def test_exports_prompt_file_when_variant_reachable(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            (tdp / "prompts").mkdir()
            (tdp / "prompts" / "sota-test.md").write_text("FAKE PROMPT BODY")
            prompt_dst = tdp / "out" / "prompt.md"
            with serve_dir(tdp) as host_url:
                path = _source_setup_and_eval({
                    "THEO_PROMPT_VARIANT": "sota-test",
                    "THEO_PROMPT_HOST": host_url,
                    "THEO_PROMPT_PATH": str(prompt_dst),
                })
                self.assertEqual(path, str(prompt_dst))
                # File must actually contain the served body
                self.assertEqual(prompt_dst.read_text(), "FAKE PROMPT BODY")

    def test_no_op_when_variant_unset(self) -> None:
        path = _source_setup_and_eval({})
        # No variant requested → THEO_SYSTEM_PROMPT_FILE stays unset
        self.assertEqual(path, "")

    def test_falls_back_when_url_unreachable(self) -> None:
        # Point at a port that nothing is listening on. With 5 retries +
        # exponential backoff (max 31s sleep) but tight connect-timeout (5s),
        # this completes within ~40s — wrap with a generous test timeout.
        with tempfile.TemporaryDirectory() as td:
            prompt_dst = Path(td) / "prompt.md"
            path = _source_setup_and_eval({
                "THEO_PROMPT_VARIANT": "sota-test",
                "THEO_PROMPT_HOST": "http://127.0.0.1:1",  # connect refused
                "THEO_PROMPT_PATH": str(prompt_dst),
            })
            # On unreachable URL, the function unsets the var → empty
            self.assertEqual(path, "")


if __name__ == "__main__":
    unittest.main(verbosity=2)
