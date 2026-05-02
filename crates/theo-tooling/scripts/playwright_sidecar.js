#!/usr/bin/env node
// T2.1 — Playwright sidecar.
//
// Reads JSON-RPC requests from stdin (one per line; the Rust client
// writes line-delimited JSON since the actions are small) and writes
// responses to stdout. Errors go to stderr for the Rust side to log.
//
// Wire format mirrors `crates/theo-tooling/src/browser/protocol.rs`:
//   request:  {id: u64, action: "open"|"click"|..., ...action-specific fields}
//   response: {id: u64, result: {...}} | {id: u64, error: {...}}
//
// Usage from Rust:
//   const child = spawn('node', [path.join(__dirname, 'playwright_sidecar.js')]);
//   write JSON line → read JSON line → parse → BrowserResponse
//
// Capability gate: the Rust caller refuses to spawn this sidecar
// unless `Capability::Browser` is set. There is no auto-launch of
// Chromium without explicit user opt-in.

'use strict';

const readline = require('readline');

let chromium;
try {
  ({ chromium } = require('playwright'));
} catch (_e) {
  // Playwright not installed — surface on the first action.
  chromium = null;
}

let browser = null;
let page = null;

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

function emit(id, outcome) {
  process.stdout.write(JSON.stringify({ id, ...outcome }) + '\n');
}

function emitOk(id, result) {
  emit(id, { result });
}

function emitErr(id, err) {
  emit(id, { error: err });
}

async function ensureBrowser(id) {
  if (chromium === null) {
    emitErr(id, {
      playwright_missing:
        'install with: cd <theo-root> && npx playwright install chromium',
    });
    return false;
  }
  if (browser === null) {
    browser = await chromium.launch({ headless: true });
  }
  if (page === null) {
    page = await browser.newPage();
  }
  return true;
}

async function handle(req) {
  const { id, action } = req;
  try {
    switch (action) {
      case 'open': {
        if (!(await ensureBrowser(id))) return;
        const resp = await page.goto(req.url, { waitUntil: 'load' });
        if (resp === null) {
          emitErr(id, {
            navigation_failed: `no response for ${req.url}`,
          });
          return;
        }
        const final_url = page.url();
        const title = await page.title();
        emitOk(id, { kind: 'navigated', final_url, title });
        return;
      }
      case 'click': {
        if (page === null) return emitErr(id, { not_open: null });
        await page.click(req.selector);
        emitOk(id, { kind: 'empty' });
        return;
      }
      case 'type': {
        if (page === null) return emitErr(id, { not_open: null });
        await page.fill(req.selector, req.text);
        emitOk(id, { kind: 'empty' });
        return;
      }
      case 'screenshot': {
        if (page === null) return emitErr(id, { not_open: null });
        const fmt = req.format === 'jpeg' ? 'jpeg' : 'png';
        const buf = await page.screenshot({
          fullPage: req.full_page === true,
          type: fmt,
        });
        const data = buf.toString('base64');
        const media_type = fmt === 'png' ? 'image/png' : 'image/jpeg';
        emitOk(id, { kind: 'screenshot', media_type, data });
        return;
      }
      case 'eval': {
        if (page === null) return emitErr(id, { not_open: null });
        try {
          const value = await page.evaluate(req.js);
          emitOk(id, { kind: 'eval_result', value });
        } catch (e) {
          emitErr(id, { eval_failed: String(e && e.message ? e.message : e) });
        }
        return;
      }
      case 'wait_for_selector': {
        if (page === null) return emitErr(id, { not_open: null });
        const timeout = Number.isFinite(req.timeout_ms) ? req.timeout_ms : 5000;
        try {
          await page.waitForSelector(req.selector, { timeout });
          emitOk(id, { kind: 'selector_found' });
        } catch (_e) {
          emitErr(id, {
            selector_timeout: {
              selector: req.selector,
              timeout_ms: timeout,
            },
          });
        }
        return;
      }
      case 'close': {
        if (page !== null) {
          await page.close();
          page = null;
        }
        if (browser !== null) {
          await browser.close();
          browser = null;
        }
        emitOk(id, { kind: 'empty' });
        return;
      }
      default:
        emitErr(id, { internal: `unknown action: ${action}` });
        return;
    }
  } catch (e) {
    emitErr(id, {
      internal: `${action} failed: ${e && e.message ? e.message : e}`,
    });
  }
}

rl.on('line', async (line) => {
  if (!line.trim()) return;
  let req;
  try {
    req = JSON.parse(line);
  } catch (e) {
    process.stderr.write(`sidecar: bad JSON: ${e.message}\n`);
    return;
  }
  await handle(req);
});

rl.on('close', async () => {
  if (page !== null) await page.close().catch(() => {});
  if (browser !== null) await browser.close().catch(() => {});
  process.exit(0);
});

process.on('SIGTERM', async () => {
  if (browser !== null) await browser.close().catch(() => {});
  process.exit(0);
});
