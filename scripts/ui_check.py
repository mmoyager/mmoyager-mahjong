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
    python3 scripts/ui_check.py riichi  # after 立直 no call may be offered
    python3 scripts/ui_check.py panels  # panels, clipping and overlays
    python3 scripts/ui_check.py settle  # every ended hand shows a settlement
    python3 scripts/ui_check.py protocol # the player's own moves reach the client

`fit` is the regression check for layout; `play` is the end-to-end check for
rounds, wins, draws, calls and the final overlay; `riichi` is the rules check
that a declared riichi locks the hand (no 吃/碰/杠, only the drawn tile);
`panels` covers overlays and clipping; `settle` checks that every hand that ends
shows its settlement, including the ones the player ends themselves. Both need the server running:

    ./target/release/mmj-serve --port 8787 --checkpoint data/checkpoints/ck-ab.bin

Exit status is non-zero when a check fails, so this can be run after any UI
change the way the Rust tests are run after any engine change. Modes are
independent and may be run concurrently (each takes its own DevTools port).
"""

import asyncio
import json
import os
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
# Derived from the pid so two checks can run at once: a shared DevTools port and
# user-data-dir would make the second run attach to the first one's browser and
# die when that run closed it. UI_CHECK_PORT overrides.
PORT = int(os.environ.get("UI_CHECK_PORT", 9416 + (os.getpid() % 400)))
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


# --- the riichi rules check ------------------------------------------------

# One step of a game played to reach riichi and then watch what is offered: the
# human never calls (so the hand stays closed and tenpai stays reachable),
# declares 立直 the moment it is offered, and afterwards reports what the UI
# offers — which must be nothing but a pass and the drawn tile.
RIICHI_STEP = r"""
(() => {
  const out = [];
  const overlay = document.getElementById('overlay');
  if (overlay && !overlay.classList.contains('hidden')) {
    const over = document.getElementById('overlay-title').textContent === '对局结束';
    document.getElementById('overlay-close').click();
    // A finished game offers no decisions, so start another one: the point is
    // to collect enough 立直 declarations, not to play one particular game.
    if (over) { document.getElementById('btn-new').click(); return 'new-game'; }
    return 'overlay-closed';
  }
  const bar = document.getElementById('action-bar');
  const btns = bar ? [...bar.querySelectorAll('button')] : [];
  const toggle = btns.find(b => b.classList.contains('riichi-toggle'));
  if (toggle && !toggle.classList.contains('on')) {
    toggle.click();
    const t = document.querySelector('#hand .tile.clickable');
    if (t) t.click();
    return 'declare-riichi';
  }
  // Play on without calling: pass every call window, discard when it is ours.
  const pass = btns.find(b => b.textContent.trim() === '跳过');
  if (pass) { pass.click(); return 'pass'; }
  const discard = document.getElementById('btn-discard');
  if (discard && !discard.disabled) { discard.click(); return 'discard'; }
  const t = document.querySelector('#hand .tile.clickable');
  if (t) { t.click(); return 'click-tile'; }
  return 'idle';
})()"""

RIICHI_STATUS = r"""
(() => {
  const btns = [...document.getElementById('action-bar').querySelectorAll('button')];
  return JSON.stringify({
    round: document.getElementById('round-name').textContent,
    label: document.getElementById('label-self').textContent,
    callButtons: btns.map(b => b.textContent.trim()).filter(x => /^吃|^碰|杠/.test(x)),
    clickable: document.querySelectorAll('#hand .tile.clickable').length,
    sideways: document.querySelectorAll('#pond-self .tile.rot').length,
    overlay: document.getElementById('overlay').classList.contains('hidden')
        ? null : document.getElementById('overlay-title').textContent,
  });
})()"""


async def check_riichi():
    """After 立直 the hand is locked: no 吃 / 碰 / 杠 may ever be offered."""
    failures = []
    declared_rounds = 0
    windows = 0
    violations = []
    async with Browser("1440,900") as b:
        await b.new_game("tonpuu")
        t0 = time.time()
        armed = False
        while time.time() - t0 < PLAY_SECONDS:
            step = str(await b.ev(RIICHI_STEP))
            raw = await b.ev(RIICHI_STATUS)
            if raw:
                st = json.loads(raw)
                declared = "立直" in st["label"]
                if declared and not armed:
                    armed = True
                    declared_rounds += 1
                    print(f"  declared 立直 in {st['round']} "
                          f"(sideways tile in pond: {st['sideways']})")
                if declared:
                    windows += 1
                    if st["callButtons"]:
                        violations.append(f"{st['round']}: offered {st['callButtons']}")
                    if st["clickable"] > 1:
                        violations.append(
                            f"{st['round']}: {st['clickable']} clickable hand tiles after 立直")
                if not declared:
                    armed = False
            if declared_rounds >= 3:
                break
            await asyncio.sleep(0.05 if step != "idle" else 0.2)
        print(f"立直 rounds: {declared_rounds}; post-立直 observations: {windows}")
        if declared_rounds == 0:
            failures.append("never reached a 立直 declaration, so nothing was checked")
        if violations:
            failures.append(f"illegal calls offered after 立直: {violations[:5]}")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- panel and overlay checks ----------------------------------------------

PANEL_PROBE = r"""
(() => {
  const box = (s) => { const e = document.querySelector(s); if (!e) return null;
    const r = e.getBoundingClientRect();
    return {x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height),
            clipped: e.scrollHeight > e.clientHeight + 1 || e.scrollWidth > e.clientWidth + 1}; };
  const overlap = (a, b) => {
    if (!a || !b || !a.w || !b.w) return 0;
    const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
    const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
    return (ox > 0 && oy > 0) ? Math.round(ox * oy) : 0;
  };
  const seats = ['across','left','right'].map(s => {
    const e = document.getElementById('seat-' + s);
    return {s, clipped: e.scrollHeight > e.clientHeight + 1 || e.scrollWidth > e.clientWidth + 1};
  });
  return JSON.stringify({
    seats,
    handArea: box('#hand-area'),
    hint: box('#hint-box'),
    hintHidden: document.getElementById('hint-box').classList.contains('hidden'),
    bar: box('#action-bar'),
    barButtons: document.querySelectorAll('#action-bar button').length,
    hintOverBar: overlap(box('#hint-box'), box('#action-bar')),
    hintOverHand: overlap(box('#hint-box'), box('#hand-area')),
    doraCount: document.querySelectorAll('#dora-tiles .tile').length,
    overlayOpen: !document.getElementById('overlay').classList.contains('hidden'),
  });
})()"""


async def check_panels():
    """Things the geometry and rules checks cannot see: a panel that covers a
    button, a seat box that clips its own tiles, a help dialog that does not."""
    failures = []
    async with Browser("1440,900") as b:
        await b.new_game("tonpuu")

        # 1. the shortcut dialog
        await b.ev("document.getElementById('keys-btn').click()")
        dlg = json.loads(await b.ev("""JSON.stringify({
            open: !document.getElementById('overlay').classList.contains('hidden'),
            rows: document.querySelectorAll('#overlay-body tr').length,
            dismiss: document.getElementById('overlay-close').textContent})"""))
        print(f"  shortcut dialog: open={dlg['open']} rows={dlg['rows']} dismiss={dlg['dismiss']!r}")
        if not dlg["open"] or dlg["rows"] < 6:
            failures.append(f"the shortcut dialog did not open properly: {dlg}")
        await b.ev("document.getElementById('overlay-close').click()")

        # 2. wait for a call window (the action bar only exists when there is a
        #    decision to make), then ask for a hint on top of it
        t0 = time.time()
        armed = False
        while time.time() - t0 < 120:
            st = json.loads(await b.ev("""JSON.stringify({
                buttons: document.querySelectorAll('#action-bar button').length})"""))
            if st["buttons"] > 0:
                armed = True
                break
            # Keep the game moving: a call window only exists while play runs.
            step = str(await b.ev(PLAY_STEP))
            await asyncio.sleep(0.05 if step != "idle" else 0.2)
        if not armed:
            failures.append("never saw an action bar with buttons")
        # Falsification hook: with the panel forced back to its old anchored
        # corner the check must complain, otherwise it is not testing anything.
        if os.environ.get("UI_CHECK_OLD_HINT"):
            await b.ev("""(() => { const st = document.createElement('style');
                st.textContent = '#hint-box { top: auto !important; bottom: 16px !important }';
                document.head.appendChild(st); })()""")
        await b.ev("document.getElementById('btn-hint').click()")
        await asyncio.sleep(2.5)
        r = json.loads(await b.ev(PANEL_PROBE))
        print(f"  with a hint open: bar buttons={r['barButtons']} "
              f"hint-over-bar={r['hintOverBar']} hint-over-hand={r['hintOverHand']}")
        if r["hintHidden"]:
            failures.append("the hint panel did not open")
        if r["hintOverBar"]:
            failures.append(f"the hint panel covers the action bar by {r['hintOverBar']}px²")
        if r["hintOverHand"]:
            failures.append(f"the hint panel covers the hand by {r['hintOverHand']}px²")
        for s in r["seats"]:
            if s["clipped"]:
                failures.append(f"seat box {s['s']} clips its own tiles")
        if r["handArea"]["clipped"]:
            failures.append("the hand area clips its contents")

        # 3. five dora indicators (four kans) must still fit the centre panel
        fits = json.loads(await b.ev("""JSON.stringify((() => {
            const d = document.getElementById('dora-tiles');
            const before = d.innerHTML;
            for (let i = 0; i < 5; i++) {
              const t = document.createElement('div'); t.className = 'tile small'; d.appendChild(t);
            }
            const dr = d.getBoundingClientRect();
            const cr = document.getElementById('centre-panel').getBoundingClientRect();
            const over = Math.round(dr.right - cr.right);
            d.innerHTML = before;
            return {over}; })())"""))
        print(f"  five dora indicators overflow the centre panel by {fits['over']}px")
        if fits["over"] > 0:
            failures.append(f"five dora indicators overflow the centre by {fits['over']}px")

        # 4. the replay / analysis panel
        await b.ev("document.getElementById('btn-replays').click()")
        await asyncio.sleep(1.0)
        rep = json.loads(await b.ev("""JSON.stringify({
            open: !document.getElementById('replay-overlay').classList.contains('hidden'),
            options: document.getElementById('replay-select').options.length})"""))
        print(f"  replay panel: open={rep['open']} saved replays={rep['options']}")
        if not rep["open"]:
            failures.append("the replay panel did not open")

        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- settlement checks ------------------------------------------------------

# Play for the *human's own* hand-ending action: take a tsumo or a ron whenever
# one is offered, otherwise discard. This is the path that used to end the hand
# without any settlement, because the server dropped the events of the player's
# own move.
SETTLE_STEP = r"""
(() => {
  const overlay = document.getElementById('overlay');
  if (overlay && !overlay.classList.contains('hidden')) {
    const over = document.getElementById('overlay-title').textContent === '对局结束';
    document.getElementById('overlay-close').click();
    if (over) { document.getElementById('btn-new').click(); return 'new-game'; }
    return 'closed';
  }
  const btns = [...document.getElementById('action-bar').querySelectorAll('button')];
  const win = btns.find(b => /^自摸/.test(b.textContent.trim()));
  if (win) { win.click(); return 'tsumo'; }
  const ron = btns.find(b => /^荣和/.test(b.textContent.trim()));
  if (ron) { ron.click(); return 'ron'; }
  const pass = btns.find(b => b.textContent.trim() === '跳过');
  if (pass) { pass.click(); return 'pass'; }

  // Play the hand the way the baseline recommends, so the human actually wins
  // sometimes: clicking the first tile at random never reaches a win. One hint
  // per decision, consumed once, and wait for it rather than guessing — asking
  // on every poll swamps the endpoint, and discarding while waiting throws the
  // advice away.
  const box = document.getElementById('hint-box');
  const clickable = [...document.querySelectorAll('#hand .tile.clickable')];
  const sig = document.getElementById('round-name').textContent + '|'
    + document.getElementById('wall').textContent + '|'
    + clickable.map(e => e.getAttribute('aria-label')).join(',');
  if (!box.classList.contains('hidden')) {
    if (window.__hintUsedFor !== sig) {
      const title = box.querySelector('.hint-title');
      const m = title
        ? title.textContent.match(/推荐：\s*(?:立直并打出|打出)?\s*(?:赤)?([0-9][mps]|[东南西北白发中])/)
        : null;
      if (m) {
        const t = clickable.find(e => (e.getAttribute('aria-label') || '').endsWith(m[1]));
        if (t) {
          window.__hintUsedFor = sig;
          box.classList.add('hidden');
          t.click();
          return 'hint-play:' + m[1];
        }
      }
    }
    // The advice was unusable (a call, a kan, or already consumed). Discard
    // something rather than stalling the hand: a stuck player blocks every
    // other seat, and then no hand ever ends and nothing is checked.
    box.classList.add('hidden');
    if (clickable.length) {
      window.__hintUsedFor = sig;
      clickable[0].click();
      return 'fallback-discard';
    }
    return 'hint-dropped';
  }
  if (clickable.length) {
    const asked = window.__askedFor;
    if (asked && asked.sig === sig && Date.now() - asked.at < 700) {
      return 'waiting';
    }
    window.__askedFor = { sig, at: Date.now() };
    document.getElementById('btn-hint').click();
    return 'ask-hint';
  }
  const discard = document.getElementById('btn-discard');
  if (discard && !discard.disabled) { discard.click(); return 'discard'; }
  return 'idle';
})()"""

SETTLE_STATUS = r"""
(() => {
  const open = !document.getElementById('overlay').classList.contains('hidden');
  const body = document.getElementById('overlay-body');
  return JSON.stringify({
    open,
    title: document.getElementById('overlay-title').textContent,
    text: body.textContent.replace(/\s+/g, ' ').trim().slice(0, 300),
    tiles: body.querySelectorAll('.tile').length,
    rows: body.querySelectorAll('table tr').length,
    round: document.getElementById('round-name').textContent,
  });
})()"""


async def check_settle():
    """Every hand that ends must show a settlement — including the ones the
    player ends themselves. That last case is the one the server got wrong: it
    used to drop the events produced by the player's own action, so a tsumo, a
    ron or a final discard went straight to the next hand with no settlement."""
    failures = []
    stats = {"own-tsumo": 0, "own-ron": 0, "own-draw": 0, "bot-win": 0,
             "draw": 0, "draw-paid": 0, "panels": 0}
    async with Browser("1440,900") as b:
        await b.new_game("tonpuu")
        t0 = time.time()
        pending = None
        while time.time() - t0 < PLAY_SECONDS:
            # Read the board *first*: SETTLE_STEP dismisses any open panel, so
            # polling after it would never see a settlement.
            after = json.loads(await b.ev(SETTLE_STATUS))
            if after["open"]:
                stats["panels"] = stats.get("panels", 0) + 1
                own = pending is not None
                title = after["title"]
                if title == "流局":
                    stats["own-draw" if own else "draw"] += 1
                    if "听牌" not in after["text"]:
                        failures.append(f"draw settlement does not list tenpai: {after['text']}")
                    if "○" in after["text"]:
                        stats["draw-paid"] += 1
                        if "罚符" not in after["text"]:
                            failures.append(f"draw with tenpai does not mention 罚符: {after['text']}")
                        if after["rows"] < 4:
                            failures.append(f"draw with tenpai has no score table: {after['text']}")
                    elif "不听" not in after["text"]:
                        failures.append(f"all-noten draw does not say so: {after['text']}")
                elif title in ("自摸！", "荣和！"):
                    key = ("own-" + pending) if own else "bot-win"
                    stats[key] = stats.get(key, 0) + 1
                    if "番" not in after["text"]:
                        failures.append(f"win settlement has no han: {after['text']}")
                    if after["rows"] < 4:
                        failures.append(f"win settlement has no score table: {after['text']}")
                    if after["tiles"] == 0:
                        failures.append(f"win settlement shows no hand ({title}): {after['text']}")
                step = str(await b.ev(SETTLE_STEP))   # dismiss the panel
                pending = None
                await asyncio.sleep(0.05)
                continue
            step = str(await b.ev(SETTLE_STEP))
            if step in ("tsumo", "ron", "discard"):
                # The player's own move can end the hand, and that is the path
                # this check exists for.
                pending = step
            await asyncio.sleep(0.05 if step != "idle" else 0.2)

        print(f"  settlements: {stats}")
        # Winning a hand ourselves cannot be forced, so these are reported but
        # not failed: `protocol` mode pins the mechanism that used to lose them.
        if stats["own-tsumo"] + stats["own-ron"] == 0:
            print("  note: no hand was won by us in this run")
        if stats["own-draw"] == 0:
            print("  note: no hand ended on our own discard in this run")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- protocol checks (fast, no browser) -------------------------------------

async def check_protocol():
    """The messages the client needs in order to show a settlement at all.

    This is the cheap, deterministic half of the settlement story: the server
    must send an `events` message for the *player's own* action. It used to drop
    those (it submitted the action and ignored the events it produced), so a
    tsumo, a ron, or a final discard went straight to the next hand with no
    settlement and no record of the move.
    """
    failures = []
    async with websockets.connect("ws://127.0.0.1:8787/ws", max_size=32 << 20) as ws:
        await ws.send(json.dumps({"type": "new_game", "seat": 0, "length": "tonpuu", "bot": "nn"}))
        saw_own_discard = False
        event_msgs = 0
        hand = 0
        while hand < 8 and not saw_own_discard:
            try:
                msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=15))
            except asyncio.TimeoutError:
                print("  stopped waiting: the server sent nothing more")
                break
            if msg.get("type") == "events":
                event_msgs += 1
                for e in msg.get("events", []):
                    d = e.get("Discard")
                    if d and d.get("seat") == msg.get("human", 0):
                        saw_own_discard = True
            elif msg.get("type") == "state":
                if not msg.get("decision"):
                    continue
                acts = msg["decision"]["actions"]
                pick = next((a for a in acts if isinstance(a, str) and a in ("Tsumo", "Ron")), None)
                if pick is None:
                    pick = next((a for a in acts if isinstance(a, dict) and "Discard" in a), None)
                if pick is None:
                    continue
                hand += 1
                await ws.send(json.dumps({"type": "action", "action": pick}))
        print(f"  events messages seen: {event_msgs}")
        print(f"  the player's own discard was reported as an event: {saw_own_discard}")
        if not saw_own_discard:
            failures.append("the player's own action produced no events, so no settlement "
                            "can be shown for a tsumo, a ron or a final discard")

        # The hint payload has to be renderable: actionable label plus a ranking.
        await ws.send(json.dumps({"type": "hint"}))
        got = None
        for _ in range(6):
            m = json.loads(await asyncio.wait_for(ws.recv(), timeout=30))
            if m.get("type") == "hint":
                got = m
                break
        if got is None:
            failures.append("no hint answer arrived")
        else:
            print(f"  hint: baseline={got.get('baseline')!r} "
                  f"rows={len((got.get('net') or {}).get('top') or [])}")
            if not got.get("baseline"):
                failures.append("the hint carries no baseline recommendation")
            if not got.get("text"):
                failures.append("the hint carries no text")
    return failures


async def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "fit"
    if mode == "fit":
        failures = await check_fit()
    elif mode == "play":
        failures = await check_play()
    elif mode == "riichi":
        failures = await check_riichi()
    elif mode == "panels":
        failures = await check_panels()
    elif mode == "settle":
        failures = await check_settle()
    elif mode == "protocol":
        failures = await check_protocol()
    else:
        sys.exit(f"unknown mode {mode!r}; use fit, play, riichi, panels, settle or protocol")
    if failures:
        print("\nFAILED:")
        for f in failures:
            print("  -", f)
        sys.exit(1)
    print("\nall checks passed")


if __name__ == "__main__":
    asyncio.run(main())
