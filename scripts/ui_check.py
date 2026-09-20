#!/usr/bin/env python3
"""Drive the table UI in a real browser and report what the eye would see.

This exists because the table has no unit tests: it is HTML, CSS and a
WebSocket, and the failures that matter (a button the hand covers, a tile
rendered at its intrinsic SVG size, a pond that does not fit) are geometry,
not logic. Chrome is driven over the DevTools protocol, so the page is the
real page and the game state is the real server's.

Two modes:

    python3 scripts/ui_check.py fit     # does it fit, at four window sizes
    python3 scripts/ui_check.py play    # play a whole tonpuu game, catch errors

`fit` is the regression check for layout; `play` is the end-to-end check for
rounds, wins, draws, calls and the final overlay. Both need the server running:

    ./target/release/mmj-serve --port 8787 --checkpoint data/checkpoints/ck-ab.bin

Exit status is non-zero when a check fails, so this can be run after any UI
change the way the Rust tests are run after any engine change.
"""

import asyncio
import json
import subprocess
import sys
import time
import urllib.request

try:
    import websockets
except ImportError:  # pragma: no cover - environment guard
    sys.exit("needs the `websockets` package (the project venv has it)")

URL = "http://127.0.0.1:8787/"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
PORT = 9416
SIZES = ["1600,1000", "1440,900", "1280,800", "1152,720"]
PLAY_SECONDS = 420

# --- the two probes -------------------------------------------------------

FIT_PROBE = r"""
(() => {
  const w = innerWidth, h = innerHeight;
  const bad = [];
  for (const e of document.querySelectorAll('*')) {
    const r = e.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    if (r.right > w + 1 || r.left < -1 || r.bottom > h + 1) {
      bad.push({sel: e.id || (e.tagName + '.' + String(e.className).split(' ')[0]),
                right: Math.round(r.right), bottom: Math.round(r.bottom)});
    }
  }
  const rect = (s) => { const e = document.querySelector(s); if (!e) return null;
    const r = e.getBoundingClientRect();
    return {x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height)}; };
  const bar = document.getElementById('action-bar');
  // stand in for a real call window: the worst case is four buttons at once
  if (bar) bar.innerHTML =
    '<button>吃</button><button>碰</button><button>立直</button><button class="pass">跳过</button>';
  const hit = (x, y) => { const e = document.elementFromPoint(x, y); return e ? e.tagName : null; };
  const buttons = bar ? [...bar.querySelectorAll('button')].map(b => {
    const r = b.getBoundingClientRect();
    return {t: hit(r.x + r.width / 2, r.y + r.height / 2), ok: r.width > 30 && r.height > 24};
  }) : [];
  const ponds = ['across','left','right','self'].map(s => {
    const frame = document.getElementById('pond-' + s);
    const r = frame ? frame.getBoundingClientRect() : null;
    const grid = frame ? frame.querySelector('.pond-grid') : null;
    return {s, tiles: frame ? frame.querySelectorAll('.tile').length : -1,
            w: r ? Math.round(r.width) : 0, h: r ? Math.round(r.height) : 0,
            rot: grid ? getComputedStyle(grid).transform : null};
  });
  return JSON.stringify({
    size: [w, h],
    doc: [document.documentElement.scrollWidth, document.documentElement.scrollHeight],
    badCount: bad.length, bad: bad.slice(0, 5),
    bar: rect('#action-bar'), hand: rect('#hand-area'), buttons,
    handTiles: document.querySelectorAll('#hand .tile').length,
    handTile: rect('#hand .tile'),
    ponds, centre: rect('#centre-panel'), ring: rect('#ring'),
    wall: document.getElementById('wall').textContent,
  });
})()"""

PLAY_STEP = r"""
(() => {
  const out = [];
  const overlay = document.getElementById('overlay');
  if (overlay && !overlay.classList.contains('hidden')) {
    const b = document.getElementById('overlay-close');
    if (b) { b.click(); out.push('overlay-closed:' + document.getElementById('overlay-title').textContent); }
    return out.join(',');
  }
  // take any call that is offered, so calls, kans and the riichi toggle all run
  const bar = document.getElementById('action-bar');
  if (bar) {
    const btns = [...bar.querySelectorAll('button')];
    const call = btns.find(b => /碰|吃|杠|荣|自摸/.test(b.textContent));
    if (call) { call.click(); out.push('call:' + call.textContent.trim()); return out.join(','); }
  }
  const discard = document.getElementById('btn-discard');
  if (discard && !discard.disabled) { discard.click(); out.push('discard'); return out.join(','); }
  const t = document.querySelector('#hand .tile.clickable');
  if (t) { t.click(); out.push('click-tile'); return out.join(','); }
  return 'idle';
})()"""

PLAY_STATUS = r"""
(() => JSON.stringify({
  round: document.getElementById('round-name').textContent,
  wall: document.getElementById('wall').textContent,
  hand: document.querySelectorAll('#hand .tile').length,
  ponds: ['across','left','right','self'].map(s => document.querySelectorAll('#pond-' + s + ' .tile').length),
  selfMelds: document.querySelectorAll('#melds-self .meld').length,
  selfScore: document.querySelector('#seat-self .score')
      ? document.querySelector('#seat-self .score').textContent : null,
  oppBacks: document.querySelectorAll('.seat .backs .back').length,
  logLines: document.querySelectorAll('#log .ev').length,
  latest: document.getElementById('log-latest').textContent.slice(0, 60),
  overlayTitle: document.getElementById('overlay').classList.contains('hidden')
      ? null : document.getElementById('overlay-title').textContent,
  gameOver: document.getElementById('overlay-title').textContent === '对局结束',
}))()"""


class Browser:
    """A headless Chrome with one page, driven over the DevTools protocol."""

    def __init__(self, size):
        self.proc = None
        self.ws = None
        self.n = 0
        self.size = size
        self.problems = []
        self.console = []

    def _targets(self):
        deadline = time.time() + 45
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(
                    f"http://127.0.0.1:{PORT}/json", timeout=2
                ) as r:
                    return json.load(r)
            except Exception:
                time.sleep(0.4)
        raise SystemExit("chrome devtools never came up")

    async def __aenter__(self):
        self.proc = subprocess.Popen(
            [CHROME, "--headless=new", "--disable-gpu", "--no-sandbox", "--no-first-run",
             "--no-default-browser-check", f"--user-data-dir=/tmp/chrome-uicheck-{PORT}",
             f"--remote-debugging-port={PORT}", f"--window-size={self.size}", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        page = next(t for t in self._targets() if t["type"] == "page")
        self.ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=32 << 20)
        await self.call("Page.enable")
        await self.call("Runtime.enable")
        await self.call("Page.navigate", {"url": URL})
        await asyncio.sleep(3)
        return self

    async def __aexit__(self, *exc):
        if self.ws:
            await self.ws.close()
        if self.proc:
            self.proc.terminate()

    async def call(self, method, params=None):
        self.n += 1
        ident = self.n
        await self.ws.send(json.dumps({"id": ident, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(await self.ws.recv())
            if msg.get("method") == "Runtime.exceptionThrown":
                d = msg["params"]["exceptionDetails"]
                text = d.get("exception", {}).get("description") or d.get("text", "")
                self.problems.append("exception: " + text[:300])
            elif msg.get("method") == "Runtime.consoleAPICalled":
                p = msg["params"]
                text = " ".join(str(a.get("value", a.get("description", ""))) for a in p.get("args", []))
                if p.get("type") in ("error", "warning"):
                    self.console.append(f"{p['type']}: {text[:200]}")
            if msg.get("id") == ident:
                return msg.get("result", {})

    async def ev(self, expr):
        r = await self.call("Runtime.evaluate",
                            {"expression": expr, "returnByValue": True, "awaitPromise": True})
        if "exceptionDetails" in r:
            self.problems.append("probe failed: " + json.dumps(r["exceptionDetails"])[:200])
        return r.get("result", {}).get("value")

    async def size_to(self, size):
        w, h = size.split(",")
        await self.call("Emulation.setDeviceMetricsOverride",
                        {"width": int(w), "height": int(h), "deviceScaleFactor": 1, "mobile": False})
        await asyncio.sleep(0.6)

    async def new_game(self, length="tonpuu"):
        await self.ev(f"document.getElementById('sel-length').value = '{length}';"
                      "document.getElementById('btn-new').click()")

    async def play(self, turns):
        """Click through `turns` human decisions, letting the bots move."""
        for _ in range(turns):
            await self.ev(PLAY_STEP)
            await asyncio.sleep(1.1)


async def check_fit():
    failures = []
    async with Browser("1600,1000") as b:
        await b.new_game()
        await b.play(12)
        for size in SIZES:
            await b.size_to(size)
            r = json.loads(await b.ev(FIT_PROBE))
            flag = "ok "
            if r["badCount"] or r["doc"][0] - r["size"][0] > 0 or r["doc"][1] - r["size"][1] > 0:
                flag = "BAD"
                failures.append(f"{size}: {r['badCount']} overflowing elements {r['bad']}")
            if not all(x["ok"] and x["t"] == "BUTTON" for x in r["buttons"]):
                flag = "BAD"
                failures.append(f"{size}: action buttons not hit-testable: {r['buttons']}")
            if any(p["w"] == 0 or p["h"] == 0 for p in r["ponds"]):
                flag = "BAD"
                failures.append(f"{size}: a pond has no box: {r['ponds']}")
            # Each pond is turned to face its owner, so the four rotations must
            # be distinct and the side ponds must read down the screen.
            rotations = [p["rot"] for p in r["ponds"]]
            if len(set(rotations)) != 4:
                flag = "BAD"
                failures.append(f"{size}: ponds are not oriented per seat: {rotations}")
            side = {p["s"]: (p["w"], p["h"]) for p in r["ponds"]}
            if side["left"][1] <= side["left"][0] or side["right"][1] <= side["right"][0]:
                flag = "BAD"
                failures.append(f"{size}: side ponds are not vertical: {side}")
            if side["self"][1] >= side["self"][0] or side["across"][1] >= side["across"][0]:
                flag = "BAD"
                failures.append(f"{size}: self/across ponds are not horizontal: {side}")
            boxes = " ".join(f"{p['s']}={p['w']}x{p['h']}" for p in r["ponds"])
            print(f"[{flag}] {size}  doc={r['doc']} overflow={r['badCount']} "
                  f"hand={r['handTile']['w']}x{r['handTile']['h']} "
                  f"bar_bottom={r['bar']['y'] + r['bar']['h']} < hand_top={r['hand']['y']}  "
                  f"{boxes}  wall={r['wall']}")
    return failures


async def check_play():
    failures = []
    async with Browser("1440,900") as b:
        await b.new_game("tonpuu")
        t0 = time.time()
        actions = {}
        status = {}
        while time.time() - t0 < PLAY_SECONDS:
            step = await b.ev(PLAY_STEP)
            for part in str(step).split(","):
                if part:
                    actions[part] = actions.get(part, 0) + 1
            await asyncio.sleep(0.05 if step != "idle" else 0.2)
            raw = await b.ev(PLAY_STATUS)
            if not raw:
                continue
            status = json.loads(raw)
            if status.get("gameOver"):
                break
        print(f"played {time.time() - t0:.0f}s, actions: {actions}")
        print(f"final: {json.dumps(status, ensure_ascii=False)}")
        if not status.get("gameOver"):
            failures.append("the game did not reach its final overlay in time")
        if status.get("selfMelds") is None:
            failures.append("no self-meld box")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


async def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "fit"
    if mode == "fit":
        failures = await check_fit()
    elif mode == "play":
        failures = await check_play()
    else:
        sys.exit(f"unknown mode {mode!r}; use fit or play")
    if failures:
        print("\nFAILED:")
        for f in failures:
            print("  -", f)
        sys.exit(1)
    print("\nall checks passed")


if __name__ == "__main__":
    asyncio.run(main())
