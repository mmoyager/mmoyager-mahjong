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
    python3 scripts/ui_check.py multi    # announcements, and one panel per winner
    python3 scripts/ui_check.py seats    # every seat, and a whole half game
    python3 scripts/ui_check.py match    # how a match ends, and reconnect resume
    python3 scripts/ui_check.py tiles    # every tile kind maps to artwork that paints
    python3 scripts/ui_check.py paint    # every tile on screen actually paints
    python3 scripts/ui_check.py pace     # discards paced per seat, call marker, forced discard
    python3 scripts/ui_check.py meld     # 副露 layout: 暗杠 backs, sideways-tile slot, 加杠 stack
    python3 scripts/ui_check.py stage    # a batch is played out: beats, no early draw, late shouts

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
# How long a game-driven check may take. The table plays one beat per action now
# (the server hands over a whole batch and the client stages it), so a hand takes
# roughly twice as long as it did when the check could rely on the bots answering
# instantly. The timing-sensitive checks are `pace` and `stage`; the rest run at
# the fastest beat and only need the game to move.
PLAY_SECONDS = 420
MATCH_SECONDS = 900

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
    const over = document.getElementById('overlay-title').textContent === '对局结束';
    const b = document.getElementById('overlay-close');
    if (b) { b.click(); out.push('overlay-closed:' + document.getElementById('overlay-title').textContent); }
    // A finished match has nothing to click: start another one, otherwise the
    // loop parks on a dead table.
    if (over) { document.getElementById('btn-new').click(); out.push('new-game'); }
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

    async def pace_to(self, index):
        """Set the table's beat.

        The long game-driven checks (a whole 東風戦, a half game, a match) run at
        the fastest step so they finish in minutes; the *default* one-second beat
        is what `stage` and `pace` measure, and those are the checks that care
        about timing.
        """
        await self.ev(f"document.getElementById('sel-pace').value = '{index}';"
                      "document.getElementById('sel-pace').dispatchEvent(new Event('change'))")

    async def play(self, turns):
        """Click through `turns` human decisions, letting the bots move."""
        for _ in range(turns):
            await self.ev(PLAY_STEP)
            await asyncio.sleep(1.1)


async def dismiss_panels(b, tries=6):
    """Click through any open overlay (settlements, the match result, help)."""
    for _ in range(tries):
        st = json.loads(await b.ev("""JSON.stringify({
            open: !document.getElementById('overlay').classList.contains('hidden')})"""))
        if not st["open"]:
            return
        await b.ev("document.getElementById('overlay-close').click()")
        await asyncio.sleep(0.15)


async def check_fit():
    failures = []
    async with Browser("1600,1000") as b:
        await b.pace_to(4)
        await b.new_game()
        await b.play(12)
        # A settlement panel is modal: it covers the buttons on purpose, so it
        # has to be dismissed before the layout underneath can be measured.
        await dismiss_panels(b)
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
        await b.pace_to(4)
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
    // 暗槓 is deliberately not in this list: after 立直 it stays legal as long
    // as the wait does not change, which the engine checks before offering it.
    callButtons: btns.map(b => b.textContent.trim())
      .filter(x => /^吃|^碰|大明杠|加杠/.test(x)),
    clickable: document.querySelectorAll('#hand .tile.clickable').length,
    sideways: document.querySelectorAll('#pond-self .tile.rot').length,
    banner: document.getElementById('banner').classList.contains('hidden')
        ? null : document.getElementById('banner').textContent,
    overlay: document.getElementById('overlay').classList.contains('hidden')
        ? null : document.getElementById('overlay-title').textContent,
  });
})()"""


async def check_riichi():
    """After 立直 the hand is locked: no 吃 / 碰 / 杠 may ever be offered."""
    failures = []
    declared_rounds = 0
    windows = 0
    riichi_banners = 0
    violations = []
    async with Browser("1440,900") as b:
        await b.pace_to(4)
        await b.new_game("tonpuu")
        t0 = time.time()
        armed = False
        while time.time() - t0 < PLAY_SECONDS:
            # The hint-driven player, so the hand actually reaches tenpai.
            step = str(await b.ev(SETTLE_STEP))
            raw = await b.ev(RIICHI_STATUS)
            if raw:
                st = json.loads(raw)
                declared = "立直" in st["label"]
                if declared and not armed:
                    armed = True
                    declared_rounds += 1
                    print(f"  declared 立直 in {st['round']} "
                          f"(sideways tile in pond: {st['sideways']})")
                if st.get("banner") and "立直" in st["banner"]:
                    riichi_banners += 1
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
        print(f"立直 rounds: {declared_rounds}; post-立直 observations: {windows}; "
              f"立直 announcements seen: {riichi_banners}")
        if riichi_banners == 0:
            failures.append("no 立直 announcement was ever shown")
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
        await b.pace_to(4)
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
        if rep["options"] > 0:
            # Analysing one replay must produce a panel: either a precisely
            # rebuilt game or the stored-log summary marked as such. An old file
            # used to be refused outright, which left the whole panel useless.
            await b.ev("document.getElementById('replay-run').click()")
            await asyncio.sleep(6)
            an = json.loads(await b.ev("""JSON.stringify({
                status: document.getElementById('replay-status').textContent,
                body: document.getElementById('replay-body').textContent.length,
                headings: document.querySelectorAll('#replay-body h3').length})"""))
            print(f"  replay analysis: status={an['status']!r} body={an['body']} chars")
            if "失败" in an["status"]:
                failures.append(f"analysing a saved replay failed: {an['status']}")
            if an["body"] < 50:
                failures.append("the replay analysis panel is empty")

        # Bad news must not be hidden: a dropped socket has to be visible, and a
        # toast has to be painted above any panel that happens to be open.
        conn = json.loads(await b.ev("""JSON.stringify((() => {
            document.getElementById('replay-overlay').classList.add('hidden');
            const chip = document.getElementById('conn-chip');
            const before = {exists: !!chip, hidden: chip && chip.classList.contains('hidden')};
            setConnected(false);
            const down = {hidden: chip.classList.contains('hidden'),
                          text: chip.textContent,
                          offline: document.body.classList.contains('offline')};
            setConnected(true);
            toast('检查提示层');
            return {before, down};
        })())"""))
        z = json.loads(await b.ev("""JSON.stringify((() => {
            overlay('检查面板', '<p>x</p>', '关闭', true);
            const zs = {toast: +getComputedStyle(document.getElementById('toast')).zIndex,
                        overlay: +getComputedStyle(document.getElementById('overlay')).zIndex};
            document.getElementById('overlay').classList.add('hidden');
            return zs;
        })())"""))
        print(f"  offline chip: {conn['before']} -> {conn['down']}; z-index {z}")
        if not conn["before"]["exists"] or not conn["before"]["hidden"]:
            failures.append(f"no hidden connection chip: {conn['before']}")
        if conn["down"]["hidden"] or not conn["down"]["offline"]:
            failures.append(f"a dropped socket is not shown: {conn['down']}")
        if z["toast"] <= z["overlay"]:
            failures.append(f"toasts are painted under panels: {z}")

        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- settlement checks ------------------------------------------------------

# Measure the shade of a pond tile in each combination that the class names can
# produce. Built in a throwaway `.pond-grid`, so the real ponds are untouched.
SHADE_PROBE = r"""
(() => {
  const host = document.createElement('div');
  host.className = 'pond-grid';
  host.style.cssText = 'position:fixed;left:-9999px;top:0';
  document.body.appendChild(host);
  const cases = {
    plain: '',
    queued: 'queued',
    tsumogiri: 'tsumogiri',
    called: 'called',
    called_tsumogiri: 'called tsumogiri',
    queued_tsumogiri: 'queued tsumogiri',
  };
  const out = {};
  for (const [name, extra] of Object.entries(cases)) {
    const t = tileEl(4, {small: true, extra});
    host.appendChild(t);
    out[name] = Math.round(parseFloat(getComputedStyle(t).opacity) * 100) / 100;
  }
  host.remove();
  return JSON.stringify(out);
})()"""

# The shades the table means: a queued tile is invisible, ツモ切り is dimmed, a
# tile that was called away is dimmed harder, and a plain tile is untouched.
SHADES = {"plain": 1.0, "queued": 0.0, "tsumogiri": 0.62, "called": 0.42,
          "called_tsumogiri": 0.42, "queued_tsumogiri": 0.0}


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
  // Declare riichi whenever it is offered, like a player would.
  const ripple = btns.find(b => b.classList.contains('riichi-toggle'));
  if (ripple && !ripple.classList.contains('on')) {
    ripple.click();
    const t = document.querySelector('#hand .tile.clickable');
    if (t) { t.click(); return 'declare-riichi'; }
  }

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
        await b.pace_to(4)
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
                    # An abortive draw never compares hands and pays nothing, so
                    # only an exhaustive draw has a tenpai list to check.
                    if "荒牌流局" in after["text"]:
                        if "听牌" not in after["text"]:
                            failures.append(f"exhaustive draw does not list tenpai: {after['text']}")
                        if after["rows"] < 4 and "不支付罚符" not in after["text"]:
                            failures.append(f"draw neither paid nor said why: {after['text']}")
                        if after["rows"] >= 4:
                            stats["draw-paid"] += 1
                            if "罚符" not in after["text"]:
                                failures.append(f"draw with a payment does not mention 罚符: {after['text']}")
                    elif after["rows"] >= 4:
                        failures.append(f"an abortive draw paid somebody: {after['text']}")
                elif title in ("自摸！", "荣和！"):
                    key = ("own-" + pending) if own else "bot-win"
                    stats[key] = stats.get(key, 0) + 1
                    if "番" not in after["text"]:
                        failures.append(f"win settlement has no han: {after['text']}")
                    if after["rows"] < 4:
                        failures.append(f"win settlement has no score table: {after['text']}")
                    if after["tiles"] == 0:
                        failures.append(f"win settlement shows no hand ({title}): {after['text']}")
                    painted = json.loads(await b.ev("""JSON.stringify((() => {
                        const ts = [...document.querySelectorAll('#overlay-body .tile')];
                        // A face-down tile (the outer two of an 暗槓) has no
                        // printed face on purpose, so it is not "unpainted".
                        const up = ts.filter(t => !t.classList.contains('down'));
                        return {n: ts.length, faceUp: up.length,
                                painted: up.filter(t => {
                            const f = t.querySelector('.tile-face');
                            return f && getComputedStyle(f).backgroundImage.includes('/tiles/');
                        }).length};
                    })())"""))
                    if painted["painted"] < painted["faceUp"]:
                        failures.append(
                            f"settlement hand tiles do not paint "
                            f"({painted['painted']}/{painted['faceUp']} face-up of {painted['n']})")
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

        # An abortive draw ends the hand with the wall nearly full, which reads
        # as a bug unless the panel says who declared it, why, and how much wall
        # was left. The event shape injected here is the server's own.
        # The shout and the panel are staged on the playback clock, and `render`
        # is what starts a plan for a batch — in a real game the state message
        # that follows the events does that.
        await b.ev("(() => { handle({type: 'events', events: [{Ryuukyoku: "
                   "{reason: 'NineTerminals', tenpai: [false, false, false, false], "
                   "deltas: [0, 0, 0, 0], by: 1, wall_remaining: 66}}]});"
                   " render(); })()")
        # The panel is staged on the playback clock, so it can be a few beats
        # behind the injection: wait for *this* panel rather than for a fixed
        # time, or a real hand's settlement still on screen gets read instead.
        abort = {"title": "", "text": ""}
        for _ in range(80):
            got = json.loads(await b.ev('''JSON.stringify({
                title: document.getElementById("overlay-title").textContent,
                text: document.getElementById("overlay-body").textContent.replace(/\s+/g, " ")})'''))
            if "九种九牌" in got["text"]:
                abort = got
                break
            await asyncio.sleep(0.15)
        print(f"  abort panel: {abort['title']} :: {abort['text'][:100]}")
        if "九种九牌" not in abort["text"] or "宣布" not in abort["text"]:
            failures.append(f"an abort does not name its reason and declarer: {abort['text'][:90]}")
        if "66" not in abort["text"]:
            failures.append(f"an abort does not report the remaining wall: {abort['text'][:90]}")

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


# --- announcement and multi-winner checks -----------------------------------

# A double ron cannot be waited for, so the client is handed the exact event
# shape the server produces (the shape itself is pinned by `protocol` mode):
# two Win events, seat 1 and seat 2, both ronning seat 0's discard.
DOUBLE_RON = r"""
(() => {
  const score = {yaku: [["Riichi", 1], ["Pinfu", 1]], han: 2, fu: 30, yakuman: 0,
                 base: 960, is_dealer: false, dora_han: 0, ura_han: 0, aka_han: 0};
  const ev = (seat, deltas, paid, sticks) => ({Win: {seat, from: 0, tile: 52, score,
      deltas, riichi_sticks_taken: sticks, paid, nagashi: false,
      hand: [0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52], melds: []}});
  handle({type: 'events', events: [ev(2, [0, -15700, 9700, 8000], 9700, 0),
                                   ev(1, [0, -15700, 9700, 8000], 8000, 2)]});
  return 'injected';
})()"""

ANNOUNCE_STATUS = r"""
(() => {
  const el = document.getElementById('banner');
  const ov = document.getElementById('overlay');
  return JSON.stringify({
    banner: el.classList.contains('hidden') ? null : el.textContent,
    bannerVisible: !el.classList.contains('hidden'),
    panel: ov.classList.contains('hidden') ? null : document.getElementById('overlay-title').textContent,
    seat: ov.classList.contains('hidden') ? ''
      : ((document.getElementById('overlay-body').textContent.trim()
          .match(/^(.+?) (?:荣和|自摸)/) || ['', ''])[1]),
    tiles: document.querySelectorAll('#overlay-body .tile').length,
  });
})()"""


async def check_multi():
    """Announcements and the settlement of several winners, one panel each."""
    failures = []
    async with Browser("1440,900") as b:
        await b.pace_to(4)
        await b.new_game("tonpuu")
        await b.ev(DOUBLE_RON)
        banners, panels = [], []
        first_sight = None
        first_banner_class = None
        expect_new = True
        t0 = time.time()
        while time.time() - t0 < 8:
            st = json.loads(await b.ev(ANNOUNCE_STATUS))
            if st["banner"] and st["banner"] not in banners:
                banners.append(st["banner"])
                # Position matters for the announcement under test (the double
                # ron), not for whatever the live game announced first; and it
                # has to be read while that announcement is the current one.
                if "双响" in st["banner"] and first_banner_class is None:
                    first_banner_class = await b.ev(
                        "document.getElementById('banner').className")
            if first_sight is None and (st["bannerVisible"] or st["panel"]):
                first_sight = {"banner": st["bannerVisible"], "panel": st["panel"]}
            if st["panel"]:
                if expect_new:
                    panels.append({"title": st["panel"], "seat": st["seat"], "tiles": st["tiles"]})
                    expect_new = False
                # Dismiss the first panel with Esc and the second with the
                # button: both must advance the queue rather than drop it.
                if len(panels) == 1:
                    await b.ev("document.dispatchEvent(new KeyboardEvent('keydown',"
                               " {key: 'Escape', bubbles: true}))")
                else:
                    await b.ev("document.getElementById('overlay-close').click()")
                expect_new = True
                await asyncio.sleep(0.25)
                continue
            await asyncio.sleep(0.06)
        print(f"  announcements: {banners}")
        # The banner must sit at the seat it is about: a centre banner leaves the
        # player working out who declared. The injected winner is seat 1, which
        # is to the observer's right.
        print(f"  banner position: {first_banner_class!r}")
        if not first_banner_class or "at-right" not in first_banner_class:
            failures.append(
                f"a right-hand player's banner is not at its seat: {first_banner_class!r}")
        for i, p in enumerate(panels):
            print(f"  panel {i + 1}: {p['title']} first-seat={p['seat']} tiles={p['tiles']}")
        if not any("双响" in x for x in banners):
            failures.append(f"a double ron was not announced: {banners}")
        if len(panels) < 2:
            failures.append(f"a double ron produced {len(panels)} panel(s), expected 2")
        else:
            # Seat 1 sits next to the discarder (seat 0) in play order, so its
            # panel must come first: that is the counter-clockwise settlement.
            if not panels[0]["seat"].endswith("AI 1"):
                failures.append(f"first panel is not the closest winner: {panels[0]}")
            if any(p["tiles"] == 0 for p in panels):
                failures.append("a winner's panel showed no hand")
        if first_sight and first_sight["panel"] and not first_sight["banner"]:
            failures.append("the settlement appeared before its announcement")

        # The hand that decides a match must still be settled before the result,
        # and while the board is held no live control may remain on screen: the
        # render that would remove them is deferred, so a stale button could
        # still send an action the table had left.
        await b.ev("""(() => {
            const score = {yaku: [["Pinfu", 1]], han: 1, fu: 30, yakuman: 0, base: 240,
                           is_dealer: false, dora_han: 0, ura_han: 0, aka_han: 0};
            document.getElementById('overlay').classList.add('hidden');
            const bar = document.getElementById('action-bar');
            bar.innerHTML = '<button>荣和</button><button>跳过</button>';
            absorbEvents([{Win: {seat: 1, from: 0, tile: 4, score, deltas: [0, 0, 0, 0],
                riichi_sticks_taken: 0, paid: 1000, pao_payer: null, nagashi: false,
                hand: [0, 4, 8, 12], melds: []}}]);
            render();
            handle({type: 'game_end', scores: [0, 0, 0, 0], ranking: [0, 1, 2, 3], rounds: 8});
        })()""")
        await asyncio.sleep(0.3)
        stale = await b.ev("document.getElementById('action-bar').querySelectorAll('button').length")
        if stale:
            failures.append(f"{stale} call buttons survived into the settlement hold")
        order = []
        for _ in range(60):
            st = json.loads(await b.ev("""JSON.stringify({
                open: !document.getElementById('overlay').classList.contains('hidden'),
                title: document.getElementById('overlay-title').textContent})"""))
            if st["open"] and (not order or order[-1] != st["title"]):
                order.append(st["title"])
                await b.ev("document.getElementById('overlay-close').click()")
                await asyncio.sleep(0.2)
                if st["title"] == "对局结束":
                    break
            await asyncio.sleep(0.2)
        print(f"  match end: {order}")
        if "对局结束" not in order or len(order) < 2:
            failures.append(f"the deciding hand was not settled before the result: {order}")
        if b.problems:
            failures.append(f"page exceptions: {b.problems[:2]}")
        if b.console:
            failures.append(f"console errors: {b.console[:2]}")
        return failures


# --- seat and match-length checks -------------------------------------------

SEAT_STATUS = r"""
(() => JSON.stringify({
  round: document.getElementById('round-name').textContent,
  honba: document.getElementById('honba').textContent,
  centre: document.getElementById('centre-wind').textContent,
  wall: document.getElementById('wall').textContent,
  wind: document.querySelector('#seat-self .wind') ? document.querySelector('#seat-self .wind').textContent : null,
  selfLabel: document.getElementById('label-self').textContent,
  hand: document.querySelectorAll('#hand .tile').length,
  oppLabels: ['across','left','right'].map(s => document.getElementById('label-' + s).textContent),
  dealerMarks: document.querySelectorAll('.seat-head.dealer').length,
  selfDealer: !!document.querySelector('#seat-self .dealer-tag'),
  panel: document.getElementById('overlay').classList.contains('hidden')
      ? null : document.getElementById('overlay-title').textContent,
  ranking: document.querySelectorAll('#overlay-body table tr').length,
}))()"""


async def check_seats():
    """Play from every seat, and play a whole hanchan: the table must stay
    consistent no matter where the observer sits, and a match must end in a
    ranking panel rather than hanging."""
    failures = []
    async with Browser("1280,800") as b:
        for seat in (0, 1, 2, 3):
            await b.ev(f"""(() => {{
                document.getElementById('sel-seat').value = '{seat}';
                document.getElementById('sel-length').value = 'hanchan';
                document.getElementById('btn-new').click(); }})()""")
            await asyncio.sleep(1.2)
            for _ in range(6):
                await b.ev(SETTLE_STEP)
                await asyncio.sleep(0.3)
            await dismiss_panels(b)
            st = json.loads(await b.ev(SEAT_STATUS))
            print(f"  seat {seat}: wind={st['wind']} hand={st['hand']} self-pond={st['selfLabel']!r} "
                  f"opponents={st['oppLabels']}")
            if st["wind"] is None:
                failures.append(f"seat {seat}: no wind marker for the player")
            if not st["selfLabel"].startswith("你"):
                failures.append(f"seat {seat}: the player's pond is labelled {st['selfLabel']!r}")
            if st["hand"] < 13:
                failures.append(f"seat {seat}: only {st['hand']} tiles in hand")
            if any("你" in x for x in st["oppLabels"]):
                failures.append(f"seat {seat}: an opponent is labelled 你: {st['oppLabels']}")
            if st["dealerMarks"] + (1 if st["selfDealer"] else 0) != 1:
                failures.append(f"seat {seat}: dealer marks = {st['dealerMarks']} (+self {st['selfDealer']})")

        # A full half game, which also exercises 南 rounds, dealer repeats and
        # the 撃飛 end condition. A 半荘 is up to eight hands and the table now
        # plays a beat per action, so this needs the longer budget: it is the
        # slowest check in the suite by design.
        await b.pace_to(4)
        await b.ev("document.getElementById('sel-seat').value = '0';"
                   "document.getElementById('btn-new').click()")
        await asyncio.sleep(1.5)
        rounds, t0, final = [], time.time(), None
        while time.time() - t0 < MATCH_SECONDS:
            st = json.loads(await b.ev(SEAT_STATUS))
            tag = (st["round"], st["honba"])
            if not rounds or rounds[-1] != tag:
                rounds.append(tag)
            if st["panel"] == "对局结束":
                final = st
                break
            await b.ev(PLAY_STEP)
            await asyncio.sleep(0.05)
        print(f"  half game: {len(rounds)} rounds, first={rounds[0] if rounds else None}, "
              f"last={rounds[-1] if rounds else None}")
        reached_south = any(r.startswith("南") for r, _ in rounds)
        if not final:
            failures.append("a half game never reached its final panel")
        elif final["ranking"] < 4:
            failures.append(f"the final ranking table has {final['ranking']} rows")
        # A half game can end early: 撃飛 ends it as soon as somebody is below
        # zero, and that is a legitimate finish with a ranking panel.
        elif not reached_south:
            print("  note: the half game ended before 南 (撃飛 or アガリやめ)")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- match-level checks -----------------------------------------------------

MATCH_STATUS = r"""
(() => JSON.stringify({
  panel: document.getElementById('overlay').classList.contains('hidden')
      ? null : document.getElementById('overlay-title').textContent,
  round: document.getElementById('round-name').textContent,
  scores: state && state.view ? state.view.players.map(p => p.score).join(',') : null,
  seed: state ? state.seed : null,
  hand: document.querySelectorAll('#hand .tile').length,
}))()"""


async def check_match():
    """A match must end with the deciding hand settled *before* the result, and
    a dropped socket must resume the same match rather than deal a new one."""
    failures = []
    async with Browser("1280,800") as b:
        await b.ev("document.getElementById('sel-length').value = 'tonpuu';"
                   "document.getElementById('btn-new').click()")
        await asyncio.sleep(1.2)
        t0, order, last = time.time(), [], None
        while time.time() - t0 < MATCH_SECONDS:
            st = json.loads(await b.ev(MATCH_STATUS))
            if st["panel"] and st["panel"] != last:
                order.append(st["panel"])
                last = st["panel"]
                if st["panel"] == "对局结束":
                    break
            await b.ev(PLAY_STEP)
            await asyncio.sleep(0.05)
        print(f"  panels at the end of the match: {order}")
        if "对局结束" not in order:
            failures.append("the match result panel never appeared")
        elif len(order) < 2:
            failures.append(f"the deciding hand was not settled before the result: {order}")

        before = json.loads(await b.ev(MATCH_STATUS))
        await b.ev("socket.close()")      # the client reconnects on its own
        await asyncio.sleep(3.5)
        after = json.loads(await b.ev(MATCH_STATUS))
        print(f"  reconnect: round {before['round']} -> {after['round']}, "
              f"scores {'same' if before['scores'] == after['scores'] else 'CHANGED'}")
        if after["seed"] is None:
            failures.append("no state arrived after the reconnect")
        elif before["seed"] != after["seed"]:
            failures.append(f"the reconnect started a different match: "
                            f"{before['seed']} -> {after['seed']}")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- tile artwork checks ----------------------------------------------------

TILE_PROBE = r"""
(async () => {
  const out = {kinds: {}, files: {}};
  for (let k = 0; k < 34; k++) {
    const file = tileFile(k * 4);
    const url = '/tiles/' + file + '.svg?v=' + TILE_REVISION;
    const img = new Image();
    let loaded = true;
    img.src = url;
    try { await img.decode(); } catch (e) { loaded = false; }
    let ink = 0, red = 0;
    if (loaded) {
      const W = 46, H = 62;
      const c = document.createElement('canvas'); c.width = W; c.height = H;
      const ctx = c.getContext('2d'); ctx.drawImage(img, 0, 0, W, H);
      const d = ctx.getImageData(0, 0, W, H).data;
      for (let i = 0; i < d.length; i += 4) {
        if (d[i + 3] > 8) { ink++; if (d[i] > 150 && d[i + 1] < 90 && d[i + 2] < 90) red++; }
      }
    }
    out.kinds[k] = {file, loaded, ink, red};
  }
  return JSON.stringify(out);
})()"""


async def check_tiles():
    """Every tile kind must map to artwork that exists, loads and paints.

    A wrong file name is invisible until a player meets that tile: 白 once
    pointed at the pack's *placeholder* face, whose artwork is a red "?" glyph,
    and nobody noticed until it turned up in a hand.
    """
    failures = []
    # The white dragon is intentionally blank (白板): `Haku.svg` draws nothing and
    # the tile shows its plain body. Every other kind must paint something.
    BLANK_KINDS = {31}
    async with Browser("1280,800") as b:
        await b.pace_to(4)
        await b.new_game("tonpuu")
        await asyncio.sleep(1.0)
        res = json.loads(await b.ev(TILE_PROBE))["kinds"]
        for k in range(34):
            v = res[str(k)]
            if not v["loaded"]:
                failures.append(f"kind {k}: {v['file']}.svg does not load")
                continue
            if k in BLANK_KINDS:
                if v["ink"] != 0:
                    failures.append(f"kind {k} ({v['file']}) should be blank, paints {v['ink']} px")
                continue
            if v["ink"] < 50:
                failures.append(f"kind {k} ({v['file']}) paints almost nothing ({v['ink']} px)")
        # The dragons are colour-coded in this set; a red-white dragon means the
        # wrong file is being used.
        red = res["33"]
        white = res["31"]
        green = res["32"]
        print(f"  dragons: 白={white['file']}({white['ink']} px) 發={green['file']}({green['ink']} px) "
              f"中={red['file']}({red['ink']} px, {red['red']} red)")
        if red["ink"] and red["red"] < red["ink"] * 0.5:
            failures.append(f"中 is not mostly red ({red['red']}/{red['ink']} px)")
        if white["ink"] != 0:
            failures.append(f"白 is not blank: {white['file']} paints {white['ink']} px")
        # Every honour must map to a *different* file.
        honour_files = [res[str(k)]["file"] for k in range(27, 34)]
        if len(set(honour_files)) != len(honour_files):
            failures.append(f"two honours share a file: {honour_files}")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- paint sweep ------------------------------------------------------------

PAINT_SWEEP = r"""
(() => {
  const tiles = [...document.querySelectorAll('.tile')];
  const bad = {noFace: [], noBg: [], zeroBox: [], offscreen: []};
  for (const t of tiles) {
    const k = t.dataset.kind;
    const f = t.querySelector('.tile-face');
    const r = t.getBoundingClientRect();
    if (!f && !t.classList.contains('no-asset')) { bad.noFace.push(k); continue; }
    if (f && !getComputedStyle(f).backgroundImage.includes('/tiles/') && k !== '31') bad.noBg.push(k);
    if (r.width < 4 || r.height < 4) bad.zeroBox.push(k + ':' + Math.round(r.width) + 'x' + Math.round(r.height));
    if (r.right < 0 || r.bottom < 0 || r.left > innerWidth || r.top > innerHeight) bad.offscreen.push(k);
  }
  return JSON.stringify({total: tiles.length,
    noFace: bad.noFace.length, noBg: bad.noBg.length,
    zeroBox: bad.zeroBox.length, offscreen: bad.offscreen.length,
    samples: [...bad.noBg, ...bad.zeroBox, ...bad.noFace].slice(0, 6)});
})()"""


async def check_paint():
    """Every tile on screen must actually paint.

    Counting elements is not enough: the settlement panel once showed fourteen
    `.tile` elements at the right size with no ink in any of them. This sweeps
    the whole document, on the table and inside any open panel.
    """
    failures = []
    async with Browser("1280,800") as b:
        await b.pace_to(4)
        await b.new_game("tonpuu")
        # Give the faces a moment: they are probes, so a brand-new hand paints a
        # frame or two late. A gap that survives a second is a bug.
        for label, settle in (("table", 1.0),):
            await asyncio.sleep(settle)
            st = json.loads(await b.ev(PAINT_SWEEP))
            print(f"  {label}: {st}")
            for key in ("noFace", "noBg", "zeroBox", "offscreen"):
                if st[key]:
                    failures.append(f"{label}: {st[key]} tiles with {key} {st['samples']}")
        # And inside a settlement panel.
        for _ in range(400):
            if await b.ev("!document.getElementById('overlay').classList.contains('hidden')"):
                await asyncio.sleep(0.5)
                st = json.loads(await b.ev(PAINT_SWEEP))
                print(f"  panel: {st}")
                for key in ("noFace", "noBg", "zeroBox"):
                    if st[key]:
                        failures.append(f"panel: {st[key]} tiles with {key} {st['samples']}")
                break
            await b.ev(SETTLE_STEP)
            await asyncio.sleep(0.05)
        else:
            print("  note: no panel appeared in this run")

        # With the artwork unreachable, every tile must still be readable: a
        # player should see a text face, not an empty box or a broken-image
        # glyph. This is how the undefined NUMERAL in that path was found.
        await b.ev("document.getElementById('overlay').classList.add('hidden')")
        await b.call("Network.enable")
        await b.call("Network.setBlockedURLs", {"urls": ["*/tiles/*"]})
        await b.pace_to(4)
        await b.new_game("tonpuu")
        await asyncio.sleep(2.5)
        fallback = json.loads(await b.ev("""JSON.stringify((() => {
            const tiles = [...document.querySelectorAll('#hand .tile')];
            return {hand: tiles.length,
                    withText: tiles.filter(t => t.textContent.trim().length > 0).length,
                    facesLeft: tiles.filter(t => t.querySelector('.tile-face')).length,
                    samples: tiles.slice(0, 4).map(t => t.textContent.trim())};
        })())"""))
        print(f"  without artwork: {fallback}")
        await b.call("Network.setBlockedURLs", {"urls": []})
        if fallback["hand"] and fallback["withText"] < fallback["hand"]:
            failures.append(
                f"with no artwork, {fallback['hand'] - fallback['withText']} tiles show nothing")
        if fallback["facesLeft"]:
            failures.append("a failed face element was left in the DOM")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- 副露 (called sets) ------------------------------------------------------

# The layout rules, asserted against the client's own meld renderer. Everything
# here is measured — the sideways tile's slot, which tiles are face down, the net
# rotation of a stacked tile, how much it covers the tile under it — rather than
# counted, because "three tiles in a box" is what the old, wrong rendering
# already produced.
#
# The last block is the falsification: the old flat row must *fail* these rules,
# so an edit that quietly drops the layout is caught by the probe that is meant
# to catch it.
MELD_LAYOUT = r"""
(() => {
  const fails = [];
  const want = (ok, msg) => { if (!ok) fails.push(msg); };
  const host = document.createElement('div');
  host.className = 'melds';
  host.style.cssText = 'position:fixed;left:0;top:0;opacity:0;pointer-events:none;z-index:-1';
  document.body.appendChild(host);

  // The melder sits at seat 0, so `from` is also the offset from the melder:
  // 3 = 上家 on their left, 2 = 対面, 1 = 下家 on their right.
  const build = (kind, from, tiles, called) => {
    const g = meldGroup({kind, tiles: tiles.concat([0, 0, 0, 0]).slice(0, 4),
                         len: tiles.length, from, called}, 0, true);
    host.appendChild(g);
    return g;
  };
  const kids = (g) => [...g.children];
  const rotIndex = (g) => kids(g).findIndex(k => k.classList.contains('rot'));
  const rotOf = (g) => kids(g).find(k => k.classList.contains('rot')) || null;
  const downIdx = (g) => kids(g).map((k, i) => k.classList.contains('down') ? i : -1)
                                 .filter(i => i >= 0);
  // Net rotation including every ancestor's transform: a child turned to cancel
  // its parent's rotation reads 0 here, which is what 加杠 needs.
  const netDeg = (el) => {
    let acc = new DOMMatrix();
    for (let e = el; e && e !== document.body; e = e.parentElement) {
      const t = getComputedStyle(e).transform;
      if (t && t !== 'none') acc = new DOMMatrix(t).multiply(acc);
    }
    return Math.round(Math.atan2(acc.b, acc.a) * 180 / Math.PI);
  };
  const box = (el) => { const r = el.getBoundingClientRect();
    return {l: r.left, t: r.top, r: r.right, b: r.bottom}; };
  const overlap = (a, b) => {
    const w = Math.min(a.r, b.r) - Math.max(a.l, b.l);
    const h = Math.min(a.b, b.b) - Math.max(a.t, b.t);
    if (w <= 0 || h <= 0) return 0;
    const small = Math.min((a.r - a.l) * (a.b - a.t), (b.r - b.l) * (b.b - b.t));
    return (w * h) / small;
  };

  // 碰: the sideways tile's *slot* is what records the seat it came from.
  for (const [from, slot] of [[3, 0], [2, 1], [1, 2]]) {
    const g = build('Pon', from, [4, 5, 6], 5);
    want(kids(g).length === 3, `碰 from ${from}: ${kids(g).length} tiles, want 3`);
    want(rotIndex(g) === slot,
         `碰 from ${from}: sideways tile at slot ${rotIndex(g)}, want ${slot}`);
    want(downIdx(g).length === 0, `碰 from ${from}: a tile is face down`);
  }

  // 吃: the called tile lies sideways at the left end, whoever it came from.
  for (const from of [3, 2, 1]) {
    const g = build('Chi', from, [8, 12, 17], 12);
    const rot = rotOf(g);
    want(rotIndex(g) === 0, `吃 from ${from}: sideways tile at slot ${rotIndex(g)}, want 0`);
    want(!!rot && rot.dataset.tile === '12',
         `吃 from ${from}: the tile lying sideways is ${rot && rot.dataset.tile}, want the called 12`);
  }

  // 大明杠: four face up, the sideways tile first / second / fourth.
  for (const [from, slot] of [[3, 0], [2, 1], [1, 3]]) {
    const g = build('Minkan', from, [4, 5, 6, 7], 5);
    want(kids(g).length === 4, `大明杠 from ${from}: ${kids(g).length} tiles, want 4`);
    want(rotIndex(g) === slot,
         `大明杠 from ${from}: sideways tile at slot ${rotIndex(g)}, want ${slot}`);
    want(downIdx(g).length === 0, `大明杠 from ${from}: a tile is face down`);
  }

  // 暗杠: the outer pair is face down, and the middle pair still names the tile.
  const ankan = build('Ankan', 0, [4, 5, 6, 7], 4);
  want(kids(ankan).length === 4, `暗杠: ${kids(ankan).length} tiles, want 4`);
  want(JSON.stringify(downIdx(ankan)) === '[0,3]',
       `暗杠: face-down tiles at slots ${JSON.stringify(downIdx(ankan))}, want [0,3]`);
  want(rotIndex(ankan) === -1, '暗杠: a tile is lying sideways');
  want(ankan.querySelectorAll('.tile-face').length === 2,
       `暗杠: ${ankan.querySelectorAll('.tile-face').length} printed faces, want 2`);

  // 加杠: the added tile rides upright on the sideways tile.
  const kakan = build('Kakan', 1, [4, 5, 6, 7], 7);
  want(kids(kakan).length === 3, `加杠: ${kids(kakan).length} tiles in the row, want 3`);
  const krot = rotOf(kakan);
  want(!!krot, '加杠: no tile is lying sideways');
  if (krot) {
    const stacked = krot.querySelector('.tile.stacked');
    want(!!stacked, '加杠: the added tile is not on the sideways tile');
    want(krot.querySelectorAll(':scope > .tile.stacked').length === 1,
         `加杠: the sideways tile has `
         + krot.querySelectorAll(':scope > .tile.stacked').length
         + ' stacked children, want 1');
    if (stacked) {
      want(stacked.dataset.tile === '7',
           `加杠: the stacked tile is ${stacked.dataset.tile}, want the added 7`);
      want(Math.abs(netDeg(stacked)) < 2,
           `加杠: the stacked tile is turned ${netDeg(stacked)}deg, want upright`);
      want(Math.abs(Math.abs(netDeg(krot)) - 90) < 2,
           `加杠: the sideways tile is turned ${netDeg(krot)}deg, want 90`);
      const ov = overlap(box(krot), box(stacked));
      want(ov > 0.3,
           `加杠: the added tile covers only ${Math.round(ov * 100)}% of the sideways tile`);
    }
  }

  // 加杠 records where its 碰 came from, not where the added tile came from: the
  // added tile is self-drawn, so the raw `from` always reads as the melder and
  // the sideways tile would point at the wrong seat. This is the bug the engine
  // now carries `pon_from` for.
  const kakanFrom = meldGroup({kind: 'Kakan', tiles: [4, 5, 6, 7], len: 4,
                               from: 1, called: 7, pon_from: 3}, 0, true);
  host.appendChild(kakanFrom);
  want(rotIndex(kakanFrom) === 0,
       `加杠 (碰 from 上家): sideways tile at slot ${rotIndex(kakanFrom)}, want 0`);
  want((kakanFrom.getAttribute('aria-label') || '').indexOf('上家') >= 0,
       `加杠 (碰 from 上家): the label says ${kakanFrom.getAttribute('aria-label')}`);

  // Every set says in words what it is and where it came from: the sideways slot
  // is the convention, but a player who does not know it must still be told.
  const spoken = [
    ['Pon', 3, [4, 5, 6], 5, '上家'], ['Pon', 1, [4, 5, 6], 5, '下家'],
    ['Chi', 3, [8, 12, 17], 12, '上家'], ['Ankan', 0, [4, 5, 6, 7], 4, '自家'],
  ];
  for (const [kind, from, tiles, called, word] of spoken) {
    const g = build(kind, from, tiles, called);
    const label = g.getAttribute('aria-label') || '';
    want(label.indexOf(word) >= 0, `${kind}: the label does not name ${word} (${label})`);
    want((g.getAttribute('title') || '').length > 0, `${kind}: no tooltip`);
  }

  const checked = host.querySelectorAll('.meld').length;
  // The old rendering: three tiles in a flat row. It must violate the rules
  // above, or they are not testing anything.
  const old = document.createElement('div');
  old.className = 'meld';
  [4, 5, 6].forEach(t => old.appendChild(tileEl(t, {small: true})));
  host.appendChild(old);
  want(rotIndex(old) === -1 && downIdx(old).length === 0,
       'self-test: the old flat row satisfies the layout rules, so they prove nothing');

  host.remove();
  return JSON.stringify({fails, checked});
})()"""

# The same rules, read off whatever is on the table right now.
MELD_LIVE = r"""
JSON.stringify([...document.querySelectorAll('.melds .meld')].map(g => {
  const kids = [...g.children];
  return {kind: g.dataset.meld, source: g.dataset.source, sideways: g.dataset.sideways,
          n: kids.length,
          rot: kids.findIndex(k => k.classList.contains('rot')),
          down: kids.map((k, i) => k.classList.contains('down') ? i : -1).filter(i => i >= 0),
          faces: g.querySelectorAll('.tile-face').length,
          // A set the table has already played out must be visible. A meld that
          // is still waiting its beat is fine, but a hand that *keeps* hiding one
          // is a set the player can never see again.
          shown: Math.round(parseFloat(getComputedStyle(g).opacity) || 0),
          label: g.getAttribute('aria-label') || ''};
}))"""

MELD_NAMES = {"chi": "吃", "pon": "碰", "ankan": "暗杠", "minkan": "大明杠", "kakan": "加杠"}

# Take a call whenever the table offers one. The layout check needs real 副露,
# and the trained agents mostly keep their hands closed until late.
CALL_STEP = r"""
(() => {
  const bar = document.getElementById('action-bar');
  if (!bar) return null;
  const call = [...bar.querySelectorAll('button')]
    .find(b => /^(碰|吃|杠|暗杠|加杠|大明杠)/.test(b.textContent.trim()));
  if (!call) return null;
  const label = call.textContent.trim();
  call.click();
  return label;
})()"""


def meld_live_failures(found):
    """Check the called sets the table is showing against the layout rules."""
    bad = []
    for m in found:
        name = MELD_NAMES.get(m["kind"])
        if not name:
            bad.append(f"unknown meld kind {m['kind']!r}")
            continue
        want = 4 if m["kind"] in ("ankan", "minkan", "kakan") else 3
        if m["n"] != want:
            bad.append(f"{name}: {m['n']} tiles, want {want}")
        if m["kind"] == "ankan":
            if m["down"] != [0, 3]:
                bad.append(f"{name}: face-down slots {m['down']}, want [0, 3]")
            if m["rot"] != -1:
                bad.append(f"{name}: a tile is lying sideways")
            if m["faces"] != 2:
                bad.append(f"{name}: {m['faces']} printed faces, want 2")
        elif m["kind"] == "kakan":
            if m["rot"] < 0:
                bad.append(f"{name}: nothing is lying sideways")
        else:
            if m["sideways"] is None:
                bad.append(f"{name}: no sideways slot recorded")
            elif m["rot"] != int(m["sideways"]) - 1:
                bad.append(f"{name}: sideways tile at slot {m['rot']}, "
                           f"but the layout says {int(m['sideways']) - 1}")
        if name not in m["label"]:
            bad.append(f"{name}: the label does not say {name} ({m['label']!r})")
        if m.get("shown") == 0:
            bad.append(f"{name}: the set is still hidden after waiting a few beats "
                       f"({m['label']!r}) — a call that is never revealed is a set the "
                       "player cannot see again")
    return bad


async def check_meld():
    """Called sets must be laid out the way a table lays them out.

    The sideways tile is the only thing that tells a player where a called tile
    came from, so this checks it twice: the renderer is driven directly over all
    five kinds and all three sources, and then whatever the table shows during a
    real game is read back and held to the same rules.
    """
    failures = []
    async with Browser("1280,800") as b:
        if await b.ev("typeof meldGroup === 'function'") is not True:
            return ["the client does not expose meldGroup, so the layout cannot be probed"]
        st = json.loads(await b.ev(MELD_LAYOUT))
        print(f"  layout rules: {st['checked']} sets built, {len(st['fails'])} violations")
        failures.extend(st["fails"])

        # And on the real table: call whenever a call is offered, so the check
        # sees real 副露 rather than waiting for the bots to open a hand (the
        # trained agents mostly stay closed).
        await b.pace_to(4)
        await b.new_game("tonpuu")
        found = []
        called = []
        for _ in range(900):
            found = json.loads(await b.ev(MELD_LIVE))
            if found:
                break
            what = await b.ev(CALL_STEP)
            if what:
                called.append(what)
                # The decision was just answered: let the next state arrive
                # instead of clicking at a table that no longer exists.
                await asyncio.sleep(0.2)
                continue
            await b.ev(SETTLE_STEP)
            await asyncio.sleep(0.05)
        if called:
            print(f"  called: {called[:4]}")
        if not found:
            print("  note: no called set appeared in this run")
        else:
            # A set is rendered hidden until its own beat arrives, so give the
            # playback a few beats before judging whether one stayed hidden: this
            # is what catches a call that is never revealed at all.
            for _ in range(40):
                if all(m.get("shown") for m in found):
                    break
                await asyncio.sleep(0.15)
                found = json.loads(await b.ev(MELD_LIVE)) or found
            print("  on the table: " + "; ".join(
                f"{MELD_NAMES.get(m['kind'], m['kind'])} n={m['n']} "
                f"slot={m['rot'] + 1} down={m['down']} shown={m.get('shown')} {m['label']}"
                for m in found[:4]))
            failures.extend(meld_live_failures(found))
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- staging a hand: the playback clock --------------------------------------

# The table must play a batch out rather than render it. Two failures this
# catches, both reported by the player and both invisible to a check that only
# looks at the finished picture:
#
#   * the freshly drawn tile appearing while the opponents' discards are still
#     landing — the player's next decision on screen before the table had played;
#   * a 荣和 shouted before the tile it happened on was in the pond.
#
# Timing is measured *in the page*: the probes below wrap the client's own
# playback steps and watch with a 20 ms interval, because a DevTools round trip
# per sample is slower than the beats being measured.
STAGE_HOOKS = r"""
(() => {
  if (window.__stage) return 'already';
  const S = {events: [], violations: [], marks: {}, plans: [], landings: []};
  window.__stage = S;
  const now = () => Math.round(performance.now());
  const queuedPerSeat = () => [0, 1, 2, 3].map(s => {
    const pond = document.getElementById(pondForRel(relativeSeat(s)));
    return pond ? pond.querySelectorAll('.tile.queued').length : -1;
  });
  const wrapStep = (name) => {
    const orig = window[name];
    if (typeof orig !== 'function') return;
    window[name] = function (arg) {
      // For a shout the staged headline has to be read *before* the call: showing
      // it is what consumes it.
      const staged = (name === 'showHeadline' && typeof pendingHeadline !== 'undefined')
        ? pendingHeadline : null;
      const r = orig.apply(this, arguments);
      if (name === 'revealDiscard') {
        // Sample *after* the reveal — the flight starts inside it. Is the tile
        // flying, and does its offset start away from the slot it lands in?
        const pond = document.getElementById(pondForRel(relativeSeat(arg)));
        const tiles = pond ? [...pond.querySelectorAll('.pond-grid .tile')] : [];
        const el = tiles.filter(t => !t.classList.contains('queued')).pop();
        let fly = null;
        if (el) {
          const cs = getComputedStyle(el);
          fly = {cls: el.classList.contains('flying'),
                 anim: cs.animationName,
                 x: parseFloat(cs.getPropertyValue('--fly-x')) || 0,
                 y: parseFloat(cs.getPropertyValue('--fly-y')) || 0};
          // And where it is at 200 ms of a 260 ms flight: a long tail is motion
          // the eye has already finished reading, and on a table that plays a beat
          // per action it also eats into the next beat.
          setTimeout(() => {
            if (!el.isConnected || !el.classList.contains('flying')) return;
            const m = new DOMMatrix(getComputedStyle(el).transform);
            S.landings.push({t: now(), dx: Math.round(m.e * 10) / 10,
                             dy: Math.round(m.f * 10) / 10});
          }, 200);
        }
        S.events.push({t: now(), what: 'discard', seat: arg, fly});
      } else if (name === 'revealMeld') {
        S.events.push({t: now(), what: 'call', seat: arg});
      } else if (name === 'showHeadline') {
        // Read what is about to be shouted and what the table still owes the
        // player: a shout that arrives with discards still in the queue is a
        // shout about a tile nobody can see yet.
        S.events.push({t: now(), what: 'headline',
                       text: staged && staged.shout ? staged.shout.text : null,
                       seat: staged && staged.shout ? staged.shout.seat : null,
                       queued: document.querySelectorAll('#ring .pond-grid .tile.queued').length,
                       queuedPerSeat: queuedPerSeat()});
      }
      S.marks[name] = (S.marks[name] || 0) + 1;
      return r;
    };
  };
  ['revealDiscard', 'revealMeld', 'showHeadline'].forEach(wrapStep);

  // Every plan, so a failure can be diagnosed instead of guessed at: how many
  // events the server sent, how many tiles were waiting, and whether two discards
  // were put on the same beat (which is what "revealed at once" looks like).
  const origPlan = window.planBatch;
  if (typeof origPlan === 'function') {
    window.planBatch = function (batch, pending) {
      const r = origPlan.apply(this, arguments);
      const at = {};
      r.plan.forEach(s => { if (s.what === 'discard') at[s.at] = (at[s.at] || 0) + 1; });
      const collide = Object.values(at).filter(n => n > 1).length;
      S.plans.push({t: now(), events: (batch || []).length, pending: (pending || []).length,
                    steps: r.plan.length, collide,
                    kinds: r.plan.map(s => s.what).join(' '),
                    sent: (batch || []).filter(e => e.Discard)
                            .map(e => e.Discard.seat + ':' + e.Discard.tile),
                    waiting: (pending || []).map(p => p.seat + ':' + p.tile),
                    ats: r.plan.filter(s => s.what === 'discard').map(s => s.at),
                    lead: r.plan.length ? r.plan[0].at : null});
      if (S.plans.length > 400) S.plans.shift();
      return r;
    };
  }

  // The overlay opening is the moment the panel covers the table.
  const overlay = document.getElementById('overlay');
  new MutationObserver(() => {
    const open = !overlay.classList.contains('hidden');
    if (open) S.events.push({t: now(), what: 'panel',
                             title: document.getElementById('overlay-title').textContent});
  }).observe(overlay, {attributes: true, attributeFilter: ['class']});

  // The banner is the shout; it is shown by an inline style-free class change.
  const banner = document.getElementById('banner');
  new MutationObserver(() => {
    if (!banner.classList.contains('hidden')) {
      S.events.push({t: now(), what: 'banner', text: banner.textContent});
    }
  }).observe(banner, {attributes: true, attributeFilter: ['class']});

  // Self-test for the flash rule, the same way the meld probe proves its own
  // rules: build a queued tile, run the landing animation on it, and measure it
  // mid-animation. A queued tile must stay invisible; if a future edit animates
  // opacity to 1 regardless of `--tg-op`, this reports it.
  (() => {
    const host = document.createElement('div');
    host.className = 'pond-grid';
    host.style.cssText = 'position:fixed;left:-9999px;top:0';
    const t = tileEl(4, {small: true, extra: 'queued flying'});
    t.style.setProperty('--fly-x', '40px');
    t.style.setProperty('--fly-y', '-30px');
    host.appendChild(t);
    document.body.appendChild(host);
    S.selftest = {queuedWhileAnimated: null, animatedOpacity: null};
    setTimeout(() => {
      const cs = getComputedStyle(t);
      S.selftest.queuedWhileAnimated = Math.round(parseFloat(cs.opacity) * 100) / 100;
      S.selftest.animatedOpacity = cs.opacity;
      host.remove();
    }, 90);
  })();

  // Sampling inside the page: is the player's own drawn tile on screen while a
  // pond tile is still waiting its turn?
  S.timer = setInterval(() => {
    const drawn = !!document.querySelector('#hand .tile.drawn');
    const queuedEls = [...document.querySelectorAll('#ring .pond-grid .tile.queued')];
    const queued = queuedEls.length;
    // A tile that has not been played yet must not be painted, *however* it is
    // animated: a CSS animation outranks a plain `opacity: 0`, and that is exactly
    // how the whole batch used to flash on screen for 160 ms and then vanish.
    const flashing = queuedEls.filter(t => parseFloat(getComputedStyle(t).opacity) > 0.05);
    if (flashing.length) {
      S.violations.push({t: now(), what: 'flash', flashing: flashing.length, queued,
                         hand: document.querySelectorAll('#hand .tile').length,
                         bar: document.getElementById('action-bar').textContent});
    }
    if (drawn && queued) {
      S.violations.push({t: now(), drawn, queued, per: queuedPerSeat(),
                         plans: S.plans.length,
        hand: document.querySelectorAll('#hand .tile').length,
        bar: document.getElementById('action-bar').textContent});
    }
  }, 20);
  return 'hooked';
})()"""


def stage_failures(log, pace_ms, allow_gap):
    """Read the page's own log back and judge the staging.

    `log` is the list of {t, what, ...} the probes recorded. Returns a list of
    human-readable failures, plus a short summary line for the report.
    """
    bad = []
    events = sorted(log.get("events", []), key=lambda e: e["t"])
    reveals = [e for e in events if e["what"] == "discard"]
    if len(reveals) < 4:
        bad.append(f"only {len(reveals)} discards were staged in this run")
    gaps = [b["t"] - a["t"] for a, b in zip(reveals, reveals[1:])]
    if gaps:
        too_fast = [g for g in gaps if g < allow_gap]
        if too_fast:
            bad.append(f"discards landed {min(gaps)} ms apart (at least {allow_gap} wanted): "
                       "the table is playing at machine speed")
    for v in log.get("violations", [])[:3]:
        if v.get("what") == "flash":
            bad.append(f"a discard that has not been played yet was painted: {v}")
            continue
        bad.append(f"the drawn tile was on screen while {v['queued']} discards were still "
                   f"queued (hand={v['hand']} bar={v['bar']!r} perSeat={v.get('per')})")
    # A shout must not name a tile the player cannot see yet. Two cases:
    #   * a win or a draw ends the hand, so *nothing* may still be queued — the
    #     tile that won it has to be in the pond;
    #   * a 立直 rides on the declarer's own discard, so that player's pond has to
    #     be complete (the sideways tile is the whole announcement).
    ending = ("荣和", "自摸", "双响", "三响", "流局满贯", "流局", "途中流局")
    shouted = 0
    for e in events:
        if e["what"] != "headline" or e.get("text") is None:
            continue
        shouted += 1
        if e["text"] in ending:
            if e.get("queued"):
                bad.append(f"{e['text']} was shouted with {e['queued']} discards still "
                           f"queued: the tile it is about is not on the table yet")
        elif e["text"] == "立直":
            # 「リーチ」 is said *before* the tile goes down (that is the order at a
            # table, and what 電脳麻将's replay does), so the declarer's own
            # discard may still be one beat away — but no more than that, and no
            # other seat may still be owed a discard from before the shout.
            seat = e.get("seat")
            per = e.get("queuedPerSeat") or []
            if seat is not None and 0 <= seat < len(per) and per[seat] > 1:
                bad.append(f"立直 was shouted with {per[seat]} of seat {seat}'s own "
                           f"discards still queued, so the shout is not about the tile "
                           f"being placed")

    # And a settlement panel is the last thing of all: it covers the table, so by
    # then the hand has to be complete on screen.
    panels = [e for e in events if e["what"] == "panel"]
    for p in panels:
        if not [r for r in reveals if r["t"] <= p["t"]]:
            bad.append("a settlement panel opened before any discard had been staged")
    flew = [r for r in reveals if (r.get("fly") or {}).get("cls")]
    offsets = [(r.get("fly") or {}) for r in flew]
    far = [o for o in offsets if abs(o.get("x", 0)) + abs(o.get("y", 0)) > 20]
    if reveals and not flew:
        bad.append("no discard flew in from the player who threw it: the tiles simply appear "
                   "in the pond, which says neither who threw nor which tile is new")
    elif len(far) < len(flew):
        bad.append(f"{len(flew) - len(far)} of {len(flew)} discards flew in with no offset, "
                   "so they appeared at their slot instead of coming from the hand")
    landings = log.get("landings", [])
    if landings:
        worst = max(max(abs(l["dx"]), abs(l["dy"])) for l in landings)
        if worst > 2:
            bad.append(f"a flying tile was still {worst:.1f} px from its slot at 200 ms of a "
                       "260 ms flight: the motion has a tail nobody is reading")
    summary = (f"{len(reveals)} discards staged, {len(flew)} flew in "
               f"({len(landings)} measured, worst landing offset "
               f"{max((max(abs(l['dx']), abs(l['dy'])) for l in landings), default=0):.1f} px), "
               f"gaps={gaps[:8]}, "
               f"shouts={shouted}, panels={len(panels)}, "
               f"drawn-early={len(log.get('violations', []))}, "
               f"flashes={sum(1 for v in log.get('violations', []) if v.get('what') == 'flash')}")
    return bad, summary


async def check_stage():
    """A batch must be *played*, not rendered.

    Three things are asserted, all of them things the player could see going
    wrong: every discard gets its own beat (at least most of the pace setting
    apart), the player's own drawn tile stays off screen until the batch is
    finished, and a shout or a settlement panel never precedes the tile that
    caused it.
    """
    failures = []
    async with Browser("1280,800") as b:
        # The pace is the subject, so it is set rather than inherited: the check
        # browsers share a profile directory per port, and a previous run (or the
        # `pace` check) leaves its own choice in localStorage.
        await b.ev("document.getElementById('sel-pace').value = '2';"
                   "document.getElementById('sel-pace').dispatchEvent(new Event('change'))")
        if await b.ev(STAGE_HOOKS) is None:
            return ["the page did not accept the staging probes"]
        pace_ms = int(await b.ev("pace()"))
        print(f"  pace step: {pace_ms} ms")
        # Play until a hand has actually ended and its panel has been seen: the
        # panel is the case the player complained about (the shout and the panel
        # arriving before the tile that won the hand was on the table), and the
        # human takes a tsumo/ron whenever one is offered.
        played = 0
        for _ in range(3000):
            what = await b.ev(SETTLE_STEP)
            if what == "new-game":
                played += 1
            if played >= 1:
                got = json.loads(await b.ev(
                    "JSON.stringify({panels: window.__stage.events.filter(e => e.what === 'panel').length,"
                    " headline: window.__stage.marks.showHeadline || 0})"))
                if got["panels"] >= 1 and got["headline"] >= 2:
                    break
            await asyncio.sleep(0.03)
        log = json.loads(await b.ev(
            "JSON.stringify({events: window.__stage.events.slice(-400),"
            " violations: window.__stage.violations, marks: window.__stage.marks,"
            " plans: window.__stage.plans.slice(-60),"
            " selftest: window.__stage.selftest,"
            " landings: window.__stage.landings,"
            " panels_seen: window.__stage.events.filter(e => e.what === 'panel').length})"))
        st = log.get("selftest") or {}
        drawn_then = st.get("queuedWhileAnimated")
        if drawn_then is None:
            failures.append("the flash self-test did not run")
        elif drawn_then > 0.05:
            failures.append(f"a queued tile is painted at opacity {drawn_then} — an animation "
                            "is overriding `opacity: 0`, which is what made every new discard "
                            "flash before its beat")
        for p in log.get("plans", []):
            if p.get("collide"):
                print(f"  plan with two discards on one beat: {p}")
        early = [v for v in log.get("violations", []) if v.get("what") != "flash"]
        for v in early[:2]:
            print(f"  drawn-early at {v['t']} (queued per seat {v.get('per')})")
            for p in log.get("plans", []):
                if abs(p["t"] - v["t"]) < 4000:
                    print(f"    plan t={p['t']} lead={p.get('lead')} ats={p.get('ats')} "
                          f"events={p['events']} pending={p['pending']} kinds={p['kinds']}")
        reveals = sorted([e for e in log.get("events", []) if e["what"] == "discard"],
                         key=lambda e: e["t"])
        short = [(x, y) for x, y in zip(reveals, reveals[1:])
                 if y["t"] - x["t"] < int(pace_ms * 0.75)]
        for x, y in short[:2]:
            print(f"  short gap: reveal at {x['t']} then {y['t']}")
            for p in log.get("plans", [])[-6:]:
                print(f"    plan t={p['t']} lead={p.get('lead')} ats={p.get('ats')} "
                      f"events={p['events']} pending={p['pending']}")
        bad, summary = stage_failures(log, pace_ms, int(pace_ms * 0.75))
        print(f"  {summary}")
        failures.extend(bad)
        # The probes must have seen something, or they are not testing anything.
        marks = log.get("marks") or {}
        if not marks.get("revealDiscard"):
            failures.append("the staging probes never saw a discard being revealed")
        if not marks.get("showHeadline") or not log.get("panels_seen"):
            failures.append("no hand ended in this run, so the shout/panel ordering "
                            "was not tested — raise the budget or check the server")
        if b.problems:
            failures.append(f"{len(b.problems)} page exceptions (first: {b.problems[0]})")
        if b.console:
            failures.append(f"{len(b.console)} console errors (first: {b.console[0]})")
        return failures


# --- pacing and attention cues ----------------------------------------------

async def check_pace():
    """Discards must appear one seat at a time, the tile a call is about must be
    marked, and a forced (riichi) discard must be shown before it happens."""
    failures = []
    async with Browser("1280,800") as b:
        await b.new_game("tonpuu")
        await asyncio.sleep(1.2)
        # Our own turn, then watch the table play the batch out.
        await b.ev("(() => { const t = document.querySelector('#hand .tile.clickable'); if (t) t.click(); })()")
        sequence = []
        for _ in range(50):
            st = json.loads(await b.ev("""JSON.stringify({
                visible: ['self','right','across','left'].map(s =>
                    document.querySelectorAll('#pond-' + s + ' .pond-grid .tile:not(.queued)').length),
                queued: document.querySelectorAll('.pond-grid .tile.queued').length})"""))
            tag = (tuple(st["visible"]), st["queued"])
            if not sequence or sequence[-1] != tag:
                sequence.append(tag)
            await asyncio.sleep(0.05)
        print(f"  reveal sequence: {sequence[:8]}")
        # A batch that lands in several ponds must be seen to grow one pond at a
        # time: exactly one pond gains a tile between samples.
        growth = 0
        for a, c in zip(sequence, sequence[1:]):
            before, after = a[0], c[0]
            gained = [i for i in range(4) if after[i] > before[i]]
            if len(gained) > 1:
                growth += 1
        if not any(x[1] > 0 for x in sequence):
            failures.append("no discard was ever queued: the table plays instantly")
        if growth:
            failures.append(f"{growth} samples showed several ponds filling at once")
        if len(sequence) < 3:
            failures.append(f"the discards appeared in one step: {sequence}")

        # The tile a call is about must be marked, and named.
        for _ in range(600):
            st = json.loads(await b.ev("""JSON.stringify({
                marked: document.querySelectorAll('.tile.callable').length,
                tinted: !!document.querySelector('.pond-slot.callable'),
                hint: (document.querySelector('#action-bar .call-hint') || {}).textContent || null})"""))
            if st["hint"]:
                print(f"  call window: {st['hint']!r} marked={st['marked']} pondTinted={st['tinted']}")
                if st["marked"] != 1:
                    failures.append(f"the callable tile is not marked: {st}")
                if not st["tinted"]:
                    failures.append("the pond the call is about is not tinted")
                if "打出的" not in st["hint"]:
                    failures.append(f"the call hint does not name the tile: {st['hint']!r}")
                break
            await b.ev(SETTLE_STEP)
            await asyncio.sleep(0.05)
        else:
            print("  note: no call window appeared in this run")

        # After riichi the only legal action is the forced discard: the client must
        # show it (drawn tile marked, a note in the hand line) before playing it.
        #
        # The slowest step is set here so the beat being checked is long enough to
        # catch, and the budget is generous: whether a riichi happens at all
        # depends on the cards, and a short wait fails on a slow hand rather than
        # on a broken feature.
        await b.ev("document.getElementById('sel-pace').value = '0';"
                   "document.getElementById('sel-pace').dispatchEvent(new Event('change'))")
        saw = False
        declared = 0
        hands = 0
        for _ in range(4000):
            st = json.loads(await b.ev("""JSON.stringify({
                forced: !!document.querySelector('#hand .tile.drawn.auto-target'),
                note: document.getElementById('hand-info').classList.contains('auto-note')})"""))
            if st["forced"]:
                saw = True
                print(f"  forced discard shown (note={st['note']})")
                if not st["note"]:
                    failures.append("the forced discard is marked but not explained")
                break
            what = await b.ev(SETTLE_STEP)
            if what == "declare-riichi":
                declared += 1
            elif what == "new-game":
                hands += 1
            await asyncio.sleep(0.05)
        # Whether a riichi happens at all is up to the cards, not the client: if
        # the player never got there, say so instead of reporting a broken
        # feature. Once one *is* declared, the forced discard must be shown.
        if not saw:
            at = await b.ev("document.getElementById('round-name').textContent")
            if declared:
                failures.append(f"{declared} 立直 were declared but the forced discard was "
                                f"never shown ({hands} hands played, now at {at})")
            else:
                print(f"  note: no 立直 in this run, so the forced discard was not exercised "
                      f"({hands} hands played, now at {at})")

        # How dark a pond tile ends up is decided by four rules that overlap
        # (queued / ツモ切り / called / plain), and they were reasoned about
        # rather than measured the first time — a tile that is both ツモ切り and
        # called, or one that is still queued, is exactly where the wrong rule
        # wins. Measure the shades instead of trusting the cascade.
        shades = json.loads(await b.ev(SHADE_PROBE))
        print(f"  pond shades: {shades}")
        for name in ("plain", "queued", "tsumogiri", "called", "called_tsumogiri",
                     "queued_tsumogiri"):
            if shades.get(name) != SHADES[name]:
                failures.append(f"pond shade {name}: {shades.get(name)}, want {SHADES[name]}")

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
    elif mode == "riichi":
        failures = await check_riichi()
    elif mode == "panels":
        failures = await check_panels()
    elif mode == "settle":
        failures = await check_settle()
    elif mode == "protocol":
        failures = await check_protocol()
    elif mode == "multi":
        failures = await check_multi()
    elif mode == "seats":
        failures = await check_seats()
    elif mode == "match":
        failures = await check_match()
    elif mode == "tiles":
        failures = await check_tiles()
    elif mode == "paint":
        failures = await check_paint()
    elif mode == "pace":
        failures = await check_pace()
    elif mode == "meld":
        failures = await check_meld()
    elif mode == "stage":
        failures = await check_stage()
    else:
        sys.exit(f"unknown mode {mode!r}; use fit, play, riichi, panels, settle, protocol, "
                 f"multi, seats, match, tiles, paint, pace, meld or stage")
    if failures:
        print("\nFAILED:")
        for f in failures:
            print("  -", f)
        sys.exit(1)
    print("\nall checks passed")


if __name__ == "__main__":
    asyncio.run(main())
