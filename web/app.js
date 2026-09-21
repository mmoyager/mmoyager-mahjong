// mmoyager mahjong — client.
//
// One WebSocket carries the whole game. The server owns the rules; this file
// only renders the view it is given and sends back the player's action.

const HONOR_FACE = { 27: "东", 28: "南", 29: "西", 30: "北", 31: "白", 32: "發", 33: "中" };
const WIND_FACE = { 27: "东", 28: "南", 29: "西", 30: "北" };
const ROUND_WIND_FACE = { 27: "东", 28: "南", 29: "西", 30: "北" };

// Yaku enum variant -> Chinese name. Keep in sync with mmj-core/src/score.rs.
const YAKU_NAMES = {
  Riichi: "立直", Ippatsu: "一发", MenzenTsumo: "门前清自摸和", Pinfu: "平和",
  Tanyao: "断幺九", Iipeiko: "一杯口", RoundWind: "场风牌", SeatWind: "自风牌",
  Haku: "役牌 白", Hatsu: "役牌 发", Chun: "役牌 中", Rinshan: "岭上开花",
  Chankan: "抢杠", Haitei: "海底摸月", Houtei: "河底捞鱼", DoubleRiichi: "两立直",
  SanshokuDoujun: "三色同顺", Ittsuu: "一气通贯", Chanta: "混全带幺九",
  Chiitoitsu: "七对子", Toitoi: "对对和", Sanankou: "三暗刻", Sankantsu: "三杠子",
  SanshokuDoukou: "三色同刻", Honroutou: "混老头", Shousangen: "小三元",
  Honitsu: "混一色", Junchan: "纯全带幺九", Ryanpeiko: "二杯口", Chinitsu: "清一色",
  Kokushi: "国士无双", KokushiJuusanmen: "国士无双十三面", Suuankou: "四暗刻",
  SuuankouTanki: "四暗刻单骑", Daisangen: "大三元", Shousuushii: "小四喜",
  Daisuushii: "大四喜", Tsuuiisou: "字一色", Chinroutou: "清老头",
  Ryuuiisou: "绿一色", Chuuren: "九莲宝灯", ChuurenJunsei: "纯正九莲宝灯",
  Suukantsu: "四杠子", Tenhou: "天和", Chiihou: "地和", Renhou: "人和",
  NagashiMangan: "流局满贯",
};

const DRAW_REASONS = {
  Exhaustive: "荒牌流局", NineTerminals: "九种九牌", FourWinds: "四风连打",
  FourRiichi: "四家立直", FourKans: "四槓散了", TripleRon: "三家和了",
};

// How long one player's turn takes, in milliseconds. The server hands over a
// whole batch at once — three bots answer instantly — so this is the clock the
// client plays that batch back on: one beat per discard, and another for a call.
//
// The default is a full second per player, because a table that answers in a
// frame is unreadable: you cannot see who threw what, and the next tile is in
// your hand before the last three players have played. Faster settings exist for
// someone who already knows the table; the floor is 450 ms, which is above the
// 220 ms landing animation, so no two discards are ever mid-animation at once.
const PACE_STEPS = [
  { name: "极慢", ms: 1600 },
  { name: "慢", ms: 1200 },
  { name: "正常", ms: 1000 },
  { name: "快", ms: 700 },
  { name: "极快", ms: 450 },
];
const PACE_DEFAULT = 2;
let paceIndex = PACE_DEFAULT;

let socket = null;
let state = null;
let botNames = ["你", "AI", "AI", "AI"];
let riichiMode = false;
let logs = [];
let deltaScores = null;
/// The state that arrived while a settlement panel was open.
let pendingState = null;

// How each pond is turned to face its owner, indexed by relative seat
// (0 self, 1 right, 2 across, 3 left). This is what the established clients do:
// the side ponds read down the screen and the opposite one reads upside down,
// because that is the direction those players threw their tiles.
const POND_ROT = { 0: 0, 1: 270, 2: 180, 3: 90 };

// ---------------------------------------------------------------- utilities

function kindOf(tile) { return tile >> 2; }
function isAka(tile) { return tile === 16 || tile === 52 || tile === 88; }

/// The engine labels actions compactly and in its own notation (`E` is 東, `0p`
/// the red five, `pon5m5m5m` a call). Those strings travel to the client inside
/// hint and analysis payloads, where a player should read 東, 赤5p and 碰 5m5m5m.
/// Decision kinds the replay analyzer reports.
const ANALYSIS_KINDS = { discard: "打牌", call: "鸣牌", riichi: "立直", kan: "杠" };

const ENGINE_HONOR = { E: "东", S: "南", W: "西", N: "北", P: "白", F: "發", C: "中" };
const ENGINE_MELD = { chi: "吃", pon: "碰", ankan: "暗杠", minkan: "大明杠", kakan: "加杠" };
const ENGINE_ACTION = { tsumo: "自摸", ron: "荣和", kyuushu: "九种九牌", pass: "跳过" };

/// Render one engine action label in the same words the table uses.
function friendlyAction(label) {
  if (!label) return "";
  const raw = String(label).trim();
  if (ENGINE_ACTION[raw]) return ENGINE_ACTION[raw];

  let prefix = "";
  let rest = raw;
  if (rest.startsWith("riichi+")) {
    prefix = "立直并打出 ";
    rest = rest.slice("riichi+".length);
  }
  for (const [kind, word] of Object.entries(ENGINE_MELD)) {
    if (rest.startsWith(kind)) {
      return `${prefix}${word} ${tileList(rest.slice(kind.length))}`;
    }
  }
  if (prefix) return prefix + tileList(rest);
  return "打出 " + tileList(rest);
}

/// `3s0pE` (an engine tile list) -> `3s 赤5p 东`.
function tileList(text) {
  const out = [];
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (ENGINE_HONOR[c]) {
      out.push(ENGINE_HONOR[c]);
    } else if ((c === "0" || /[1-9]/.test(c)) && /[mps]/.test(text[i + 1] || "")) {
      const suit = text[i + 1];
      out.push(c === "0" ? "赤5" + suit : c + suit);
      i += 1;
    } else if (c !== " ") {
      out.push(c);
    }
  }
  return out.join(" ");
}

/// Escape text that is about to be interpolated into innerHTML. Every string
/// here is locally produced today (bot names, file paths, engine labels), but
/// they reach the DOM through a file picker and the file system, so they are
/// escaped rather than trusted.
function esc(value) {
  return String(value === null || value === undefined ? "" : value)
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}

/// A seat's display name, escaped for use inside HTML.
function who(seat) {
  return esc(botNames[seat] || ("座位" + seat));
}

function tileFace(tile) {
  const k = kindOf(tile);
  if (k >= 27) return { text: HONOR_FACE[k], suit: "z", aka: false };
  const n = (k % 9) + 1;
  const suit = k < 9 ? "m" : k < 18 ? "p" : "s";
  return { text: isAka(tile) ? "0" : String(n), suit, aka: isAka(tile) };
}

// Pond tile geometry, read once from the stylesheet so the rotated pond frames
// are sized from the same numbers the tiles are drawn with.
const POND_TILE_W = parseFloat(
  getComputedStyle(document.documentElement).getPropertyValue("--pond-tile-w")) || 25;
const POND_TILE_H = parseFloat(
  getComputedStyle(document.documentElement).getPropertyValue("--pond-tile-h")) || 34;
const POND_GAP = parseFloat(
  getComputedStyle(document.documentElement).getPropertyValue("--pond-gap")) || 2;

/// Like `tileName`, but a red five is spelled 赤5p rather than the engine's `0p`.
/// Used wherever a player reads a tile in prose: buttons, aria labels, the
/// record. (`0p` is the engine's notation and means nothing to most players.)
function friendlyTileName(tile) {
  if (!isAka(tile)) return tileName(tile);
  return "赤5" + ["m", "p", "s"][Math.floor(kindOf(tile) / 9)];
}

function tileName(tile) {
  const f = tileFace(tile);
  return f.suit === "z" ? f.text : f.text + f.suit;
}

/// Name a tile *kind* (0-33) rather than a physical tile. Waits are kinds, and
/// naming them through `kind * 4` would call every 5m/5p/5s wait a red five,
/// because tiles 16/52/88 are the aka fives.
function kindName(kind) {
  if (kind >= 27) return HONOR_FACE[kind] || ("kind" + kind);
  const n = (kind % 9) + 1;
  return String(n) + (kind < 9 ? "m" : kind < 18 ? "p" : "s");
}

// Tile faces are the public-domain SVG set by FluffyStuff
// (https://github.com/FluffyStuff/riichi-mahjong-tiles, CC0), served from
// web/tiles/. They are vector, so one file serves the 22 px pond tile and the
// 62 px hand tile. A text face is kept as a fallback so the table still reads if
// an image ever fails to load.
// The white dragon is drawn blank, which is the standard look (白板). The pack's
// Haku.svg is a valid but empty drawing, so the tile shows the plain body. Do
// NOT substitute Blank.svg here: that file is the pack's *placeholder* face and
// its artwork is a red "?" glyph, which is exactly how a player reads a broken
// tile.
const HONOR_FILES = ["Ton", "Nan", "Shaa", "Pei", "Haku", "Hatsu", "Chun"];

// Bumped whenever the artwork under web/tiles/ changes. The files used to be
// served with a day-long max-age, so a browser that had seen the old art kept
// showing it; a new query string makes the difference visible immediately.
const TILE_REVISION = "2";

function tileFile(tile) {
  const k = kindOf(tile);
  if (k >= 27) return HONOR_FILES[k - 27];
  const suit = k < 9 ? "Man" : k < 18 ? "Pin" : "Sou";
  const n = (k % 9) + 1;
  return suit + n + (isAka(tile) ? "-Dora" : "");
}

function tileEl(tile, opts = {}) {
  const k = kindOf(tile);
  const aka = isAka(tile);
  const el = document.createElement("div");
  el.className = "tile"
    + (opts.small ? " small" : "")
    + (opts.clickable ? " clickable" : "")
    + (opts.disabled ? " disabled" : "")
    + (aka ? " aka" : "")
    + (opts.extra ? " " + opts.extra : "");
  el.dataset.kind = String(k);
  el.dataset.tile = String(tile);

  // A tile turned face down (暗槓's outer two, or an opponent's concealed tile).
  // The back is a plain image with no per-tile face, so there is nothing to
  // probe and nothing that can render half-drawn: `el.dataset.tile` still
  // records which tile it is, which matters because an 暗槓's tiles are public.
  if (opts.down) {
    el.classList.add("down");
    el.dataset.down = "1";
    if (opts.label) el.setAttribute("aria-label", opts.label);
    return el;
  }

  // The face is a *background* image, not an <img>.
  //
  // Two reasons, both learned the hard way. A background never paints the
  // browser's broken-image glyph (a "?" in some browsers) when a request fails,
  // and there is no "hide it until it loads" race to lose — an earlier attempt
  // at hiding an <img> until its load event left the tiles in the settlement
  // panel permanently invisible, because the event had already fired before the
  // listener was attached.
  const face = document.createElement("div");
  face.className = "tile-face";
  const url = "/tiles/" + tileFile(tile) + ".svg?v=" + TILE_REVISION;

  // Probe the artwork with a detached image: the tile shows its plain body
  // until the probe succeeds, so nothing half-drawn is ever visible.
  const probe = new Image();
  let retried = false;
  probe.addEventListener("load", () => {
    face.style.backgroundImage = 'url("' + url + '")';
  });
  probe.addEventListener("error", () => {
    if (!retried) {
      // One retry past any cache: a stale entry recovers, and the tile keeps
      // showing its body while that happens.
      retried = true;
      probe.src = url + "&r=" + Date.now();
      return;
    }
    // Still nothing: a text face keeps the table readable.
    el.classList.add("no-asset");
    face.remove();
    el.appendChild(textFace(k));
  });
  probe.src = url;
  el.appendChild(face);

  if (opts.onClick && !opts.disabled) {
    el.addEventListener("click", opts.onClick);
    // A playable tile is a button: reachable by Tab, activated by Enter/Space,
    // and announced with its face rather than as an empty box.
    el.tabIndex = 0;
    el.setAttribute("role", "button");
    el.setAttribute("aria-label", opts.label || tileName(tile));
    el.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === " ") {
        ev.preventDefault();
        opts.onClick();
      }
    });
  } else if (opts.label) {
    el.setAttribute("aria-label", opts.label);
  }
  return el;
}

/// The fallback face, used only when a tile image cannot be loaded.
// Numerals for the man suit's fallback face. (The image is the real face; this
// only appears if the artwork cannot be loaded at all.)
const NUMERAL = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

function textFace(k) {
  const face = document.createElement("span");
  face.className = "face";
  if (k >= 27) {
    face.classList.add("honor");
    face.textContent = HONOR_FACE[k];
  } else {
    const suit = k < 9 ? "m" : k < 18 ? "p" : "s";
    const n = (k % 9) + 1;
    face.classList.add(suit);
    face.textContent = suit === "m" ? NUMERAL[n] + "萬" : String(n) + suit;
  }
  return face;
}

// Opponents' hands: a row of tile backs. Kept separate from `tileEl` because a
// back has no face and should not pretend to.
function backRow(count, opts = {}) {
  const wrap = document.createElement("div");
  wrap.className = "backs" + (opts.vertical ? " vertical" : "");
  for (let i = 0; i < Math.max(0, count); i++) {
    const b = document.createElement("div");
    // The back face is painted by CSS on .back itself. A child <img> would lay
    // out at the SVG's intrinsic 300x400 size and drag the page width open.
    b.className = "back";
    wrap.appendChild(b);
  }
  return wrap;
}

// ---------------------------------------------------------------- networking

// The last game the player asked for, so a reconnect can rebuild it instead of
// silently starting a different one.
let lastRequest = null;
let reconnectTimer = null;

/// Warm the browser's cache with every tile face at boot.
///
/// Without this the first hand renders with blank bodies for a frame or two: the
/// face probes resolve asynchronously and a tile shows its plain body until they
/// do. The whole set is a few hundred KB of vector art and it is cached, so the
/// cost is paid once, before the first hand is dealt.
function preloadTileArt() {
  const names = new Set();
  for (let k = 0; k < 34; k++) names.add(tileFile(k * 4));
  // The red fives have their own files.
  for (const k of [4, 13, 22]) names.add(tileFile(k * 4 + 0).replace(/^(Man|Pin|Sou)5$/, "$15-Dora"));
  for (const name of names) {
    const img = new Image();
    img.src = "/tiles/" + name + ".svg?v=" + TILE_REVISION;
  }
}

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${proto}://${location.host}/ws`);
  socket.onmessage = (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch (e) { return; }
    handle(msg);
  };
  socket.onopen = () => {
    setConnected(true);
    // The server hands every new socket a *new* game; re-ask for the one the
    // player was in. The seed is what makes it the same match rather than a
    // fresh one, so it is sent back with the request.
    if (lastRequest) {
      send(Object.assign({}, lastRequest, lastSeed === null ? {} : { seed: lastSeed }));
    }
  };
  socket.onclose = () => {
    setConnected(false);
    toast("与服务端的连接断开，正在重连…");
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(); }, 1500);
  };
}

/// Turn an engine error into something a player can act on.
function friendlyError(message) {
  const m = String(message || "");
  if (/no pending decision/i.test(m)) return "这一步已经过时了，请按当前牌面重新操作";
  if (/illegal action/i.test(m)) return "这个操作现在不合法";
  if (/已结束/.test(m)) return "本局已结束，请开新局";
  return "操作无效：" + m;
}

/// True while the socket is open. A dropped action is otherwise completely
/// silent: the player clicks, nothing happens, and nothing says why.
let connected = false;

function send(obj) {
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify(obj));
    return true;
  }
  toast("和服务器断开了连接，正在重连…");
  return false;
}

/// Show or hide the "not connected" chip and stop the hand from looking playable.
function setConnected(up) {
  connected = up;
  const chip = document.getElementById("conn-chip");
  if (chip) chip.classList.toggle("hidden", up);
  document.body.classList.toggle("offline", !up);
}

function handle(msg) {
  switch (msg.type) {
    case "state":
      botNames = msg.botNames || botNames;
      // A new game (or a reconnect onto a fresh one) invalidates the scores the
      // delta display was comparing against, or the first round would show the
      // whole starting score as a swing.
      if (msg.seed !== null && msg.seed !== undefined && msg.seed !== lastSeed) {
        lastSeed = msg.seed;
        lastScores = null;
        lastRoundKey = null;
        deltaScores = null;
        riichiMode = false;
        for (const seat of [0, 1, 2, 3]) shownDiscards[seat] = 0;
        logs = [];
        const logEl = document.getElementById("log");
        if (logEl) logEl.innerHTML = "";
        const latest = document.getElementById("log-latest");
        if (latest) latest.textContent = "";
      }
      // A settlement panel is modal: keep the finished hand on the board behind
      // it instead of redrawing the next hand underneath the player while they
      // are still reading. The state is applied when the panel is dismissed.
      //
      // `boardHold` on its own is no longer a reason to park a state: the table
      // stops at the end of a hand, so the state that arrives with the settlement
      // *is* the finished hand the panel is about — and it has to be rendered, or
      // the ronned tile would never reach the table.
      if (settlementOpen()) {
        pendingState = msg;
        break;
      }
      state = msg;
      render();
      break;
    case "events":
      absorbEvents(msg.events || []);
      break;
    case "game_end":
      showGameEnd(msg);
      break;
    case "hint":
      showHint(msg);
      break;
    case "error":
      // Engine errors are English and phrased for a developer; a player needs
      // to know what to do about it.
      toast(friendlyError(msg.message));
      // The server re-sends the state with an error, so the board cannot stay
      // frozen on a decision it has already moved past.
      break;
    default:
      break;
  }
}

// ---------------------------------------------------------------- rendering

// Scores as of the previous render, so a round result can be shown as a delta
// rather than a jump.
let lastScores = null;
let lastRoundKey = null;
let lastSeed = null;
let deltaTimer = null;

function render() {
  if (!state) return;
  const view = state.view;
  const human = state.human;
  // The batch this state belongs to, taken here and cleared at once: a later
  // render (the score-delta timer, a panel closing) must not play it twice.
  const batch = lastBatch;
  lastBatch = [];
  // Which melds in this view are new, per seat, so a call stays hidden until its
  // own beat. A 加杠 *replaces* its 碰 rather than adding one, so it is not
  // counted and the new shape shows a beat early — the rarest form, and the seat
  // it came from never moves, so nothing is misread.
  for (const e of batch) {
    const m = e.Meld || (e.Kan && String(e.Kan.meld.kind) === "Ankan" ? e.Kan : null);
    if (m) freshMelds[m.seat] = (freshMelds[m.seat] || 0) + 1;
  }
  const roundKey = view.round_wind + ":" + view.round_number + ":" + view.honba;
  if (lastRoundKey !== null && roundKey !== lastRoundKey && lastScores) {
    // A round just ended: keep the deltas on screen for a few seconds. The
    // previous timer is cancelled so a fast round cannot clear a newer delta.
    deltaScores = view.players.map((p, i) => p.score - (lastScores[i] ?? p.score));
    if (deltaTimer) clearTimeout(deltaTimer);
    deltaTimer = setTimeout(() => { deltaScores = null; deltaTimer = null; render(); }, 6000);
  }
  lastScores = view.players.map((p) => p.score);
  lastRoundKey = roundKey;

  document.getElementById("round-name").textContent =
    (ROUND_WIND_FACE[view.round_wind] || "?") + view.round_number + "局";
  document.getElementById("honba").textContent = view.honba + " 本场";
  document.getElementById("sticks").textContent = "供托 " + view.riichi_sticks;
  document.getElementById("wall").textContent = "余 " + view.wall_remaining;
  document.getElementById("centre-wind").textContent = ROUND_WIND_FACE[view.round_wind] || "?";
  document.getElementById("centre-round").textContent =
    `${view.round_number} 局 · ${view.honba} 本场`;
  const wallText = document.getElementById("centre-wall-text");
  if (wallText) wallText.textContent = `余 ${view.wall_remaining} 张`;
  const fill = document.getElementById("wall-fill");
  if (fill) fill.style.width = Math.max(0, Math.min(100, (view.wall_remaining / 70) * 100)) + "%";
  const stickBox = document.getElementById("stick-box");
  stickBox.innerHTML = "";
  for (let i = 0; i < Math.min(view.riichi_sticks, 12); i++) {
    const st = document.createElement("span");
    st.className = "riichi-stick";
    stickBox.appendChild(st);
  }
  if (view.riichi_sticks > 12) {
    const more = document.createElement("span");
    more.className = "stick-more";
    more.textContent = "+" + (view.riichi_sticks - 12);
    stickBox.appendChild(more);
  }

  const doraBox = document.getElementById("dora-tiles");
  doraBox.innerHTML = "";
  view.dora_indicators.forEach((t) => doraBox.appendChild(tileEl(t, { small: true })));

  // Relative seat: 0 self, 1 right (plays next), 2 across, 3 left. Each seat owns
  // a box (for self, only the hand area) and a pond inside the centre ring.
  const rel = (s) => (s - human + 4) % 4;
  const seatSlotFor = { 0: "seat-self", 1: "seat-right", 2: "seat-across", 3: "seat-left" };
  const pondFor = { 0: "pond-self", 1: "pond-right", 2: "pond-across", 3: "pond-left" };
  const labelFor = { 0: "label-self", 1: "label-right", 2: "label-across", 3: "label-left" };
  const actingSeat = view.phase && view.phase.Turn ? view.phase.Turn.seat : -1;
  for (let s = 0; s < 4; s++) {
    const p = view.players[s];
    const r = rel(s);
    const seatSlot = document.getElementById(seatSlotFor[r]);
    if (seatSlot) seatSlot.classList.toggle("turn", s === actingSeat);
    if (r === 0) {
      renderSelf(document.getElementById("seat-self"), p, view);
    } else if (seatSlot) {
      renderOpponent(seatSlot, p, view, r);
    }
    const pond = document.getElementById(pondFor[r]);
    if (pond) renderPond(pond, p.discards, POND_ROT[r], s);
    const label = document.getElementById(labelFor[r]);
    if (label) {
      label.textContent = (s === human ? "你" : seatName(s)) +
        (p.riichi ? " · 立直" : "");
    }
  }
  // Everything that answers this batch waits for the batch to be on screen. The
  // hold has to start *before* the hand is drawn, because the hand decides
  // whether its tiles are clickable and whether the drawn tile is visible at all.
  const { plan, lastAt } = planBatch(batch, PENDING_DISCARDS.splice(0));
  if (lastAt > 0) holdControls(lastAt + 60);
  renderHand(view, human);
  if (lastAt > 0) {
    // The plan draws them once the last beat has landed.
    const bar = document.getElementById("action-bar");
    if (bar) bar.innerHTML = "";
    clearCallMarks();
  } else {
    releaseControls();
  }
  runPlan(plan);
}

/// True while the controls are deliberately held back: either the table is still
/// playing discards out, or a settlement is on screen.
let controlsHeld = false;
/// True only while the table is playing a batch out and the player's own turn has
/// not come round yet. This is what hides the freshly drawn tile: the state
/// already contains it (the server played the three bots instantly), but showing
/// it now would put the player's next decision on screen while the opponents are
/// still discarding. It is *not* the same as `controlsHeld`: when a hand ends the
/// board is held too, and then the winner's drawn tile — the tile they just won on
/// — must stay visible.
let awaitingTurn = false;
/// The pending hold, so a newer render replaces an older wait instead of
/// stacking two timers that would each redraw the bar.
let controlTimer = null;

function holdControls(ms) {
  controlsHeld = true;
  awaitingTurn = true;
  // A forced discard that is already counting down must not fire while the table
  // is still playing this batch out, and its "自动打出…" note must not sit there
  // pointing at a decision that is on hold: cancel both, and let
  // `releaseControls` arm it again.
  stopForcedDecision();
  clearTimeout(controlTimer);
  controlTimer = setTimeout(() => {
    controlTimer = null;
    releaseControls();
  }, ms);
}

/// Cancel a forced discard that is waiting to be played. Clearing `forcedFor` is
/// what lets it be armed again for the same decision.
function stopForcedDecision() {
  clearTimeout(forcedTimer);
  forcedTimer = null;
  forcedFor = null;
  const info = document.getElementById("hand-info");
  if (info) info.classList.remove("auto-note");
}

/// Draw the controls now, unless a hold still owns them.
function releaseControls() {
  if (controlTimer !== null) return;
  controlsHeld = false;
  awaitingTurn = false;
  // A settlement that started during the wait owns the screen now; its own path
  // releases the controls once the player closes the last panel.
  if (boardHold || panelQueue.length || pendingSettlement.length || settlementOpen()) return;
  // The hand was drawn *while the hold was on*, so it came out unclickable. It
  // has to be drawn again here: without this the player sees the discards land,
  // gets no buttons, and cannot act at all — the table simply stops.
  if (state && state.view) renderHand(state.view, state.human);
  renderActions();
  autoPlayForcedDecision();
}

/// Drop the controls without letting the hold redraw them: used when the board
/// itself is being held on a finished hand.
function cancelControls() {
  clearTimeout(controlTimer);
  controlTimer = null;
  controlsHeld = true;
  // The turn has arrived (or the hand is over), so the hand is drawn as it
  // stands; only the controls stay away.
  awaitingTurn = false;
  // Same reasoning as `holdControls`: a forced discard counting down belongs to
  // the hand that just ended, and playing it would send an action into the next
  // one.
  stopForcedDecision();
  const bar = document.getElementById("action-bar");
  if (bar) bar.innerHTML = "";
  clearCallMarks();
}

/// A seat label short enough for a narrow side box.
///
/// "神经网络 AI 1" and "神经网络 AI 2" both truncate to the same ellipsis in the
/// side seats, which makes the table unreadable exactly where the player needs
/// to tell the opponents apart. Keep the distinguishing part; the full name is
/// still there as a tooltip.
function seatName(seat) {
  const full = botNames[seat] || ("座位" + seat);
  const m = full.match(/^(.*?)\s*(\d+)$/);
  if (!m) return full;
  const kind = m[1].trim().split(/\s+/).pop() || m[1].trim();
  return kind + " " + m[2];
}

function seatHead(p, view) {
  const head = document.createElement("div");
  const acting = view.phase && view.phase.Turn && view.phase.Turn.seat === p.seat;
  head.className = "seat-head"
    + (p.is_dealer ? " dealer" : "")
    + (p.riichi ? " riichi" : "")
    + (acting ? " acting" : "");
  const wind = document.createElement("span");
  wind.className = "wind";
  wind.textContent = WIND_FACE[p.wind] || "?";
  head.appendChild(wind);
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = seatName(p.seat);
  name.title = botNames[p.seat] || "";
  head.appendChild(name);
  if (p.furiten) {
    const f = document.createElement("span");
    f.className = "furiten";
    f.textContent = "振听";
    head.appendChild(f);
  }
  const score = document.createElement("span");
  score.className = "score";
  score.textContent = p.score;
  if (deltaScores && deltaScores[p.seat]) {
    const d = document.createElement("span");
    d.className = "delta " + (deltaScores[p.seat] > 0 ? "up" : "down");
    d.textContent = (deltaScores[p.seat] > 0 ? "+" : "") + deltaScores[p.seat];
    score.textContent = p.score + " ";
    score.appendChild(d);
  }
  head.appendChild(score);
  return head;
}

function renderOpponent(slot, p, view, rel) {
  slot.innerHTML = "";
  slot.appendChild(seatHead(p, view));

  // `hand_count` is already the number of concealed tiles: a called set has left
  // the hand, so subtracting for melds again would show three tiles too few.
  const vertical = rel === 1 || rel === 3;
  slot.appendChild(backRow(p.hand_count, { vertical }));
  if (p.melds && p.melds.length) {
    slot.appendChild(meldRow(p.melds, true, p.seat, freshMelds[p.seat]));
  }
}

/// The observer's own seat has no box of its own: the hand area at the bottom
/// already shows the concealed tiles, so only the score, the wind and the melds
/// need a place here, and the discards go into the ring like everyone else's.
function renderSelf(slot, p, view) {
  if (!slot) return;
  slot.innerHTML = "";
  const info = document.createElement("div");
  info.className = "self-badge";
  const wind = document.createElement("span");
  wind.className = "wind";
  wind.textContent = WIND_FACE[p.wind] || "?";
  info.appendChild(wind);
  const name = document.createElement("span");
  name.textContent = "你";
  info.appendChild(name);
  if (p.is_dealer) {
    const d = document.createElement("span");
    d.className = "dealer-tag";
    d.textContent = "亲";
    info.appendChild(d);
  }
  if (p.riichi) {
    const r = document.createElement("span");
    r.className = "riichi-tag";
    r.textContent = "立直";
    info.appendChild(r);
  }
  if (p.ippatsu) {
    // 一発 only lasts this go-around, so it is worth shouting about.
    const i = document.createElement("span");
    i.className = "ippatsu-tag";
    i.textContent = "一发";
    i.title = "一発：这一巡内和了会加一番";
    info.appendChild(i);
  }
  const score = document.createElement("span");
  score.className = "score";
  score.textContent = p.score;
  if (deltaScores && deltaScores[p.seat]) {
    const d = document.createElement("span");
    d.className = "delta " + (deltaScores[p.seat] > 0 ? "up" : "down");
    d.textContent = (deltaScores[p.seat] > 0 ? "+" : "") + deltaScores[p.seat];
    score.textContent = p.score + " ";
    score.appendChild(d);
  }
  info.appendChild(score);
  slot.appendChild(info);
  // The observer's melds are rendered next to the hand, not here; `renderHand`
  // owns that box so a call can never leave tiles invisible.
}

// ---------------------------------------------------------------- called sets
//
// 副露 layout. Called sets sit to the right of the hand, oldest first, and one
// tile of each lies sideways to record where it came from. The layout below
// follows the reference client 電脳麻将 (kobalab/majiang-ui, `lib/mianzi.js`),
// whose code agrees with the Japanese rules write-ups on every point:
//
//   吃    the called tile lies sideways at the left end, whichever seat it came
//         from;
//   碰    the sideways tile is first for 上家, second for 対面, third for 下家;
//   大明杠 all four tiles face up, the sideways tile first / second / fourth;
//   暗杠  the two *outer* tiles are face down — [back][face][face][back] — so a
//         concealed quad still shows the table which tile it is;
//   加杠  the fourth tile is stacked on the sideways tile.
//
// Only the *slot* holding the sideways tile records the source; the direction of
// the tilt records nothing, so every sideways tile is turned the same way.
// https://github.com/kobalab/majiang-ui/blob/master/lib/mianzi.js
const MELD_SOURCE_NAME = { 0: "自家", 1: "下家", 2: "対面", 3: "上家" };

/// The source's offset from the melder: 1 for the player on their right (下家),
/// 2 for 対面, 3 for the player on their left (上家), 0 for the melder's own
/// concealed quad.
///
/// A 加杠 is the one case where the two seats differ: its own fourth tile came
/// from the melder (they drew it), while the sideways tile on the table is the
/// one left over from the ポン, so the seat that matters is the ポン's —
/// `pon_from`. Getting this wrong would show a 加杠 as coming from 下家 whoever
/// the ポン was taken from.
function meldSourceSeat(meld) {
  if (String(meld.kind) === "Kakan" && meld.pon_from !== null
      && meld.pon_from !== undefined) {
    return meld.pon_from;
  }
  return meld.from;
}

function meldSourceOffset(seat, from) {
  if (from === null || from === undefined) return 0;
  const melder = seat === null || seat === undefined ? 0 : seat;
  return (((from - melder) % 4) + 4) % 4;
}

/// Which slot of the meld the sideways tile occupies. 加杠 reuses the 碰 slot:
/// the added tile is stacked on that tile, and moving it elsewhere would hide
/// which player the 碰 came from — the reason real rules insist on the stack.
function meldSidewaysSlot(kind, offset) {
  if (kind === "Chi") return 0;
  if (kind === "Minkan") {
    // Four slots: 上家 at the far left, 対面 second, 下家 at the far right.
    if (offset === 3) return 0;
    return offset === 2 ? 1 : 3;
  }
  // 碰 / 加杠: 上家 left, 対面 middle, 下家 right.
  if (offset === 3) return 0;
  return offset === 2 ? 1 : 2;
}

function meldSourceLabel(kind, offset) {
  if (kind === "Ankan") return "自家手牌";
  return "来自" + (MELD_SOURCE_NAME[offset] || "?");
}

/// Read one called set at a glance: 碰 5m5m5m（来自下家）.
function meldTitle(meld, offset) {
  const kind = String(meld.kind || "");
  const tiles = (meld.tiles || []).slice(0, meld.len || 0).map(friendlyTileName).join(" ");
  const name = ENGINE_MELD[kind.toLowerCase()] || kind;
  return `${name} ${tiles}（${meldSourceLabel(kind, offset)}）`;
}

/// One called set, laid out the way a table lays it out.
function meldGroup(meld, seat, small) {
  const g = document.createElement("div");
  const kind = String(meld.kind || "");
  const offset = meldSourceOffset(seat, meldSourceSeat(meld));
  const tiles = (meld.tiles || []).slice(0, meld.len || 0);
  g.className = "meld " + kind.toLowerCase();
  // `data-*` so the layout is assertable from a test instead of by eye.
  g.dataset.meld = kind.toLowerCase();
  g.dataset.source = String(offset);
  const label = meldTitle(meld, offset);
  g.title = label;
  g.setAttribute("aria-label", label);

  if (kind === "Ankan") {
    // 両端2枚を裏返す: the outer pair lies face down, the middle pair still
    // names the tile. (Some clubs mirror it — the two inner tiles down — but the
    // outer pair is the common reading and the one the pro rules spell out.)
    g.dataset.sideways = "0";
    g.appendChild(tileEl(tiles[0], { small, down: true, label: label + " 扣放" }));
    g.appendChild(tileEl(tiles[1], { small, label: label + " " + friendlyTileName(tiles[1]) }));
    g.appendChild(tileEl(tiles[2], { small, label: label + " " + friendlyTileName(tiles[2]) }));
    g.appendChild(tileEl(tiles[3], { small, down: true, label: label + " 扣放" }));
    return g;
  }

  // The tiles in reading order, plus the one that gets stacked (加杠 only).
  //
  // `called` is always in the payload. If it ever stops being there, the layout
  // must not fall over: a run still has a fixed order, and a quad's four tiles
  // are one kind, so the first tile stands in for the missing one.
  const called = (meld.called === null || meld.called === undefined) ? tiles[0] : meld.called;
  let row;
  let stacked = null;
  if (kind === "Chi") {
    const rest = tiles.filter((t) => t !== called).sort((a, b) => a - b);
    row = [called, ...rest];
  } else if (kind === "Kakan") {
    stacked = called;
    row = tiles.filter((t) => t !== stacked);
  } else {
    row = tiles.slice();
  }

  const slot = meldSidewaysSlot(kind, offset);
  g.dataset.sideways = String(slot + 1);
  const source = meldSourceLabel(kind, offset);
  row.slice(0, 4).forEach((t, i) => {
    if (i !== slot) {
      g.appendChild(tileEl(t, { small, label: label + " " + friendlyTileName(t) }));
      return;
    }
    const rot = tileEl(t, {
      small, extra: "rot",
      label: `${label} ${friendlyTileName(t)}（横向，${source}）`,
    });
    if (stacked !== null) {
      // 加杠: the fourth tile lies *on top of* the sideways one. It is rendered
      // inside the sideways tile and counter-rotated, so it reads upright while
      // the tile under it stays sideways.
      rot.appendChild(tileEl(stacked, {
        small, extra: "stacked",
        label: label + " " + friendlyTileName(stacked) + "（加杠）",
      }));
    }
    g.appendChild(rot);
  });
  return g;
}

/// `hidden` is how many of the *last* sets are too new to show yet: a call is an
/// action like any other and gets its own beat, so the set that a batch just made
/// stays out of sight until the playback reaches it.
function meldRow(melds, small, seat, hidden) {
  const wrap = document.createElement("div");
  wrap.className = "melds";
  const first = melds.length - Math.min(hidden || 0, melds.length);
  melds.forEach((m, i) => {
    const g = meldGroup(m, seat, small);
    if (i >= first) g.classList.add("queued");
    wrap.appendChild(g);
  });
  return wrap;
}

/// Fill one pond. The grid is built in the owner's own frame — six discards to a
/// row, left to right, each new row nearer the owner — and the frame rotates it
/// into place, so every pond reads the way that player threw it.
///
/// The riichi declaration tile lies sideways; if that tile is called, the next
/// discard takes the sideways spot instead, which is what the competition rules
/// ask for (the marker has to stay in the pond of whoever declared).
/// Per seat, how many discards are already on screen. New ones are revealed on a
/// timer so a whole round does not appear at once.
const shownDiscards = { 0: 0, 1: 0, 2: 0, 3: 0 };

/// How many discards of each seat the table is allowed to be showing.
///
/// This is the *only* thing that decides whether a pond tile is visible, and it
/// is deliberately a number rather than a class on an element: every render
/// rebuilds the ponds, so a class would be thrown away with the element it was on
/// and the whole batch would flash into view at the next render. A number
/// survives, and `renderPond` re-derives the hidden tiles from it.
const visibleDiscards = { 0: 0, 1: 0, 2: 0, 3: 0 };

/// The pond a seat owns. The ponds are keyed by position on *this* player's
/// screen, so the seat has to be turned into a relative one first.
function pondForSeat(seat) {
  return document.getElementById(pondForRel(relativeSeat(seat)));
}

/// Show every discard each seat's clock has reached, and hide the rest.
function applyDiscardVisibility() {
  for (const seat of [0, 1, 2, 3]) {
    const pond = pondForSeat(seat);
    if (!pond) continue;
    const shown = visibleDiscards[seat] || 0;
    [...pond.querySelectorAll(".pond-grid .tile")].forEach((el, i) => {
      el.classList.toggle("queued", i >= shown);
    });
  }
}

/// Reveal one more discard of `seat`, with its landing animation.
function revealDiscard(seat) {
  visibleDiscards[seat] = (visibleDiscards[seat] || 0) + 1;
  const pond = pondForSeat(seat);
  const el = pond && [...pond.querySelectorAll(".pond-grid .tile")][visibleDiscards[seat] - 1];
  applyDiscardVisibility();
  if (!el) return;
  el.classList.add("arriving");
  setTimeout(() => el.classList.remove("arriving"), 220);
}

function pace() {
  return PACE_STEPS[paceIndex].ms;
}

/// The melds each seat gains in the batch being played back, so a call is shown
/// on its own beat instead of the instant the state arrives.
const freshMelds = { 0: 0, 1: 0, 2: 0, 3: 0 };

/// The seat box on this player's screen, by relative position.
const SEAT_SLOT_IDS = ["seat-self", "seat-right", "seat-across", "seat-left"];

function meldBoxForSeat(seat) {
  if (state && seat === state.human) return document.getElementById("melds-self");
  const slot = document.getElementById(SEAT_SLOT_IDS[relativeSeat(seat)]);
  return slot ? slot.querySelector(".melds") : null;
}

/// Show the oldest meld this seat is still hiding, on the beat its call happened.
function revealMeld(seat) {
  const box = meldBoxForSeat(seat);
  if (!box) return;
  const g = box.querySelector(".meld.queued");
  if (!g) return;
  g.classList.remove("queued");
  g.classList.add("landing");
  setTimeout(() => g.classList.remove("landing"), 260);
}

/// Work out how a batch plays out, without touching the DOM.
///
/// One beat per discard and one per call: the three bots answer instantly, and
/// playing their whole turn inside a frame is what made the table unreadable —
/// the player's next tile was in their hand before the opponents had discarded.
/// Two things are timed relative to those beats rather than fired on arrival:
///
///   * a 荣和 is announced once the discard it happened on has landed — never
///     before, or the shout names a tile the player cannot see yet;
///   * a 自摸 costs one extra beat, because it follows that player's draw, so the
///     shout comes when the turn has actually reached them.
///
/// Returns the plan and the time the last *visible* step lands, which is when the
/// player's own turn may start.
/// When the table last advanced, on `performance.now()`'s clock — both the beat
/// that has *fired* and the last beat the running plan still owes. The beat is
/// global rather than per batch: a new state can arrive the instant the previous
/// plan finished (the player answers a call window in a few hundred
/// milliseconds), and the next discard must still get its own beat instead of
/// landing on top of the last one.
///
/// Two values are needed, not one. A plan whose steps are still queued would be
/// walked over by the next plan if only the fired time counted; and a step that
/// fired *late* (a busy frame, a repaint) would let the next plan start a beat
/// early if only the scheduled time counted. Both were visible as two discards
/// landing ~100 ms apart.
let lastBeatAt = 0;
let lastPlannedBeatAt = 0;

function planBatch(batch, pending) {
  const step = pace();
  const now = performance.now();
  // Start the plan no earlier than one beat after the previous batch's last
  // step, so "a second per player" holds across batch boundaries too.
  const lead = Math.max(0, Math.max(lastBeatAt, lastPlannedBeatAt) + step - now);
  const events = (batch && batch.length)
    ? batch
    : ((state && state.view && state.view.events) || []);
  const queue = pending.slice();
  const plan = [];
  let clock = 0;
  const add = (at, what, seat, kind) => {
    plan.push({ at: at + lead, what, seat, kind });
  };

  for (const e of events) {
    if (e.Discard) {
      const d = e.Discard;
      const i = queue.findIndex((p) => p.seat === d.seat && p.tile === d.tile);
      if (i >= 0) {
        add(clock, "discard", d.seat);
        queue.splice(i, 1);
      }
      clock += step;
    } else if (e.Meld || e.Kan) {
      // A call is two steps, the way it is at a table: the shout, then the tiles
      // assembled. 電脳麻将's replay does the same (`say()` first, tiles on the
      // next entry).
      const m = e.Meld || e.Kan;
      add(clock, "shout", m.seat, String(m.meld ? m.meld.kind : m.kind));
      clock += step;
      add(clock, "call", m.seat);
      clock += step;
    } else if (e.Win) {
      const ron = e.Win.from !== null && e.Win.from !== undefined;
      if (!ron) clock += step;          // the turn has to reach the winner first
      add(clock, "headline", e.Win.seat);
    } else if (e.Riichi) {
      // 「リーチ」 comes *before* the tile goes down — that is the order at a real
      // table, and the sideways tile is only the proof of it. Shout in the beat
      // the declarer's discard was going to take, and push that discard (with
      // everything queued behind it) one beat later.
      const at = lastDiscardAt(plan, e.Riichi.seat);
      if (at === null) {
        add(clock, "headline", e.Riichi.seat);
      } else {
        for (const s of plan) if (s.at >= at) s.at += step;
        plan.push({ at, what: "headline", seat: e.Riichi.seat });
        clock += step;
      }
    } else if (e.Ryuukyoku) {
      add(clock, "headline", undefined);
    }
    // A 加杠's dora indicator (`DoraRevealed`) is not staged: the tile is already
    // drawn in the centre panel, and it arrives with the kan that turned it.
  }
  // Anything the batch did not account for — a state with no events to pair it
  // with, a resumed game — still has to be revealed, and on its own beats *after*
  // everything the batch does describe. Guessing "first" would put an unknown
  // tile on the same beat as a known one, and two tiles appearing together is the
  // exact thing this clock exists to prevent.
  for (const p of queue) {
    add(clock, "discard", p.seat);
    clock += step;
  }
  // The beats are read off the finished plan, because the 立直 shift above moves
  // steps: `lastBeat` is when the table may act again, and the player's own turn
  // starts after the last step that is not a shout.
  let lastBeat = 0;
  let turnAt = 0;
  for (const s of plan) {
    lastBeat = Math.max(lastBeat, s.at);
    if (s.what !== "headline") turnAt = Math.max(turnAt, s.at);
  }
  lastPlannedBeatAt = now + lastBeat;
  return { plan, lastAt: turnAt };
}

/// The beat a seat's most recent discard sits on, or null if it has none.
function lastDiscardAt(plan, seat) {
  let at = null;
  for (const s of plan) {
    if (s.what === "discard" && s.seat === seat) at = s.at;
  }
  return at;
}

/// Start a plan running. Steps address elements by seat and index rather than
/// closing over them, because a render can replace every tile on the table
/// between one step and the next.
function runPlan(plan) {
  plan.forEach((s) => {
    if (s.at <= 0) { runStep(s); return; }
    setTimeout(() => runStep(s), s.at);
  });
}

function runStep(s) {
  if (s.what === "discard") revealDiscard(s.seat);
  else if (s.what === "call") revealMeld(s.seat);
  else if (s.what === "shout") announceCall(s.seat, s.kind);
  else if (s.what === "headline") showHeadline();
  // The table advanced here, *now* — not when the step was planned. A step that
  // ran late must push everything after it back, or the next batch lands on top
  // of it.
  if (s.what !== "headline") lastBeatAt = performance.now();
}

/// One short shout over the seat that called, a beat before its tiles are
/// assembled: the same two steps a real table takes, and the same two steps
/// 電脳麻将's replay takes (`say()` first, tiles on the next entry).
function announceCall(seat, kind) {
  const word = ENGINE_MELD[String(kind || "").toLowerCase()];
  if (!word) return;
  announce(word, seatName(seat), 760, seat);
}

function renderPond(frame, discards, rotDeg, seat) {
  const grid = frame.querySelector(".pond-grid");
  if (!grid) return;
  grid.innerHTML = "";
  const n = discards.length;
  const rows = Math.max(1, Math.min(5, Math.ceil(n / 6)));
  const rotated = rotDeg === 90 || rotDeg === 270;

  // Six to a row in the owner's frame, so the standard six-per-row pond grows
  // away from the centre. The frame is sized to the *rotated* footprint.
  const gridW = 6 * POND_TILE_W + 5 * POND_GAP;
  const gridH = rows * POND_TILE_H + (rows - 1) * POND_GAP;
  frame.style.width = (rotated ? gridH : gridW) + "px";
  frame.style.height = (rotated ? gridW : gridH) + "px";
  grid.style.setProperty("--rot", rotDeg + "deg");

  const sideways = new Set();
  discards.forEach((d, i) => {
    if (!d.riichi) return;
    sideways.add(i);
    // The declaration was called: the next discard is the sideways one.
    if (d.called_by !== null && d.called_by !== undefined && i + 1 < n) {
      sideways.delete(i);
      sideways.add(i + 1);
    }
  });

  const last = n - 1;
  // A new hand starts with fewer discards than the last one ended with. The clock
  // has to follow the pond down, or every tile of the new hand would be revealed
  // the moment it appeared.
  if (seat !== undefined && n < (visibleDiscards[seat] || 0)) visibleDiscards[seat] = n;
  const shown = seat === undefined ? n : (visibleDiscards[seat] || 0);

  discards.forEach((d, i) => {
    let extra = "";
    if (d.called_by !== null && d.called_by !== undefined) extra += " called";
    if (sideways.has(i)) extra += " rot";
    // ツモ切り: the tile was the one just drawn, so it was never a choice. Every
    // client shades it, and a real table gives it away for free — anyone watching
    // sees the tile go straight from the wall to the pond. Shade only, no motion:
    // movement would read as a fresh discard.
    if (d.tsumogiri) extra += " tsumogiri";
    if (i === last) extra += " fresh";
    // Not played yet as far as the table is concerned: laid out, so the pond does
    // not reflow when it lands, but not visible.
    if (i >= shown) extra += " queued";
    grid.appendChild(tileEl(d.tile, { small: true, extra }));
  });
  if (seat !== undefined) {
    const seen = shownDiscards[seat] || 0;
    const tiles = [...grid.querySelectorAll(".tile")];
    for (let i = seen; i < tiles.length; i++) {
      PENDING_DISCARDS.push({ seat, el: tiles[i],
                              tile: Number(tiles[i].dataset.tile) });
    }
    shownDiscards[seat] = tiles.length;
  }
}

/// New discards gathered during the current render, revealed in order at the end
/// of it.
const PENDING_DISCARDS = [];

function renderHand(view, human) {
  const me = view.players[human];
  const handEl = document.getElementById("hand");
  handEl.innerHTML = "";
  const meldsEl = document.getElementById("melds-self");
  meldsEl.innerHTML = "";
  // Called sets are gone from `me.hand`, so without this the tiles a call took
  // would simply vanish from the board.
  if (me.melds && me.melds.length) {
    meldsEl.appendChild(meldRow(me.melds, true, human, freshMelds[human]));
  }

  const decision = state.decision;
  // `controlsHeld` covers the paced hold too: while the table is still playing
  // this batch's discards out, the hand must not take a click any more than the
  // action bar takes one — the player is watching the other seats, and a discard
  // sent now would be answered by a table that has already moved on.
  const discardable = !controlsHeld && !boardHold && !panelQueue.length && decision
    ? decision.actions.some((a) => a.Discard)
    : false;
  // After 立直 the hand is locked: the engine offers the drawn tile and nothing
  // else, and the UI must not suggest otherwise. Falling back to "same kind"
  // here would light up concealed copies of the drawn tile.
  const locked = !!me.riichi;

  // The drawn tile is rendered separately, slightly offset.
  //
  // While the table is still playing this batch out, it is not rendered at all.
  // The state says the player has already drawn — the server played the three
  // bots instantly — but showing it here is what made the table feel like it was
  // skipping the opponents' turns: their tiles were still landing in the ponds
  // while the player's next tile, and the decision that goes with it, were
  // already on screen. The hand waits for its own turn, like every other client.
  let hand = (me.hand || []).slice();
  let drawn = me.drawn;
  if (drawn !== null && drawn !== undefined) {
    const idx = hand.indexOf(drawn);
    if (idx >= 0) hand.splice(idx, 1);
  }
  if (awaitingTurn) drawn = null;

  const clickTile = (tile) => () => {
    const act = findDiscardAction(tile, riichiMode);
    if (!act) {
      toast(locked
        ? "立直后只能打出刚摸到的这张"
        : (riichiMode ? "这张牌不能立直" : "这张牌现在不能打出"));
      return;
    }
    riichiMode = false;
    send({ type: "action", action: act });
  };

  hand.forEach((t) => {
    const canPlay = !locked && discardable && !!findDiscardAction(t, riichiMode);
    handEl.appendChild(tileEl(t, {
      clickable: canPlay,
      disabled: discardable && !canPlay,
      label: (riichiMode ? "立直并打出 " : "打出 ") + friendlyTileName(t),
      onClick: clickTile(t),
    }));
  });
  if (drawn !== null && drawn !== undefined) {
    const canPlay = discardable && !!findDiscardAction(drawn, riichiMode);
    handEl.appendChild(tileEl(drawn, {
      clickable: canPlay,
      disabled: discardable && !canPlay,
      // The pulse means "this is about to be played for you", so it belongs
      // together with the note `autoPlayForcedDecision` writes — and neither may
      // appear while the controls are held back for the table to finish.
      extra: "drawn" + (locked && !controlsHeld && state.decision
        && state.decision.actions && state.decision.actions.length === 1
        ? " auto-target" : ""),
      label: (riichiMode ? "立直并打出刚摸到的 " : "打出刚摸到的 ") + friendlyTileName(drawn),
      onClick: clickTile(drawn),
    }));
  }

  const info = document.getElementById("shanten-info");
  if (me.shanten !== null && me.shanten !== undefined && (me.hand || []).length) {
    let text = me.shanten < 0 ? "已和牌" : `向听 ${me.shanten}`;
    if (me.waits && me.waits.length) {
      text += " · 听 " + me.waits.map(kindName).join(" ");
    }
    info.textContent = text;
  } else {
    info.textContent = "";
  }
  // 振听 has three flavours and they end differently: 同巡振聴 clears on your next
  // draw, 立直振聴 lasts the round, and 捨て牌振聴 until the wait changes.
  const furitenFlag = document.getElementById("furiten-flag");
  furitenFlag.classList.toggle("hidden", !me.furiten);
  if (me.furiten) {
    furitenFlag.textContent = me.furiten_riichi
      ? "立直振听" : (me.furiten_temp ? "同巡振听" : "舍牌振听");
    furitenFlag.title = me.furiten_riichi
      ? "立直振听：立直期间放弃过和牌，本局不能再荣和"
      : (me.furiten_temp
        ? "同巡振听：这一巡放弃过和牌，下次摸牌后解除"
        : "舍牌振听：自己的弃牌里有听的牌，换听前不能荣和");
  }
}

/// Turn the riichi declaration on or off, refusing when the engine has not
/// offered one (the button is only rendered when it has, but the keyboard
/// shortcut can be pressed at any time).
function toggleRiichi() {
  if (!state || !state.decision) return;
  const acts = state.decision.actions || [];
  if (!acts.some((a) => a.Discard && a.Discard.riichi)) {
    toast("现在不能立直");
    return;
  }
  riichiMode = !riichiMode;
  render();
}

function findDiscardAction(tile, wantRiichi) {
  if (!state || !state.decision) return null;
  const acts = state.decision.actions || [];
  const exact = acts.find(
    (a) => a.Discard && !!a.Discard.riichi === wantRiichi && a.Discard.tile === tile
  );
  if (exact) return exact;
  // The engine offers one action per tile *kind* and accepts any physical copy
  // of it, so clicking the second of two identical normal 5m must map to the
  // offered copy. It must not map across the aka distinction though: the red
  // five is a different tile, and quietly throwing the normal one instead would
  // change what the pond and the dora count show.
  return acts.find(
    (a) => a.Discard && !!a.Discard.riichi === wantRiichi
      && kindOf(a.Discard.tile) === kindOf(tile)
      && isAka(a.Discard.tile) === isAka(tile)
  ) || null;
}

function actKind(a) {
  if (typeof a === "string") return a;
  if (a.Discard) return "Discard";
  if (a.Meld) return a.Meld.meld.kind;
  return "?";
}

/// What the pending decision is about, when it is about somebody else's tile:
/// `{ seat, tile }` for a discard that can be called, or a kan that can be
/// robbed. Returns null for the player's own turn.
function callTarget() {
  if (!state || !state.decision) return null;
  const t = state.decision.trigger || {};
  if (t.Discard) return { seat: t.Discard.from, tile: t.Discard.tile };
  if (t.Chankan) return { seat: t.Chankan.from, tile: t.Chankan.tile, kan: true };
  return null;
}

/// Drop the call marks. They are cleared whenever the buttons that go with them
/// are cleared, so the pond never points at a tile the player can no longer call.
function clearCallMarks() {
  document.querySelectorAll(".tile.callable").forEach((e) => e.classList.remove("callable"));
  document.querySelectorAll(".pond-slot.callable").forEach((e) => e.classList.remove("callable"));
}

/// Mark the tile a call window is about, in the pond it came from, and tint that
/// pond. Without this the player has to hunt through four ponds for the tile.
function markCallTarget() {
  clearCallMarks();
  const target = callTarget();
  if (!target || target.seat === null || target.seat === undefined) return;
  const rel = (target.seat - state.human + 4) % 4;
  const pond = document.getElementById(pondForRel(rel));
  if (!pond) return;
  const tiles = [...pond.querySelectorAll(".tile")];
  for (let i = tiles.length - 1; i >= 0; i--) {
    if (Number(tiles[i].dataset.tile) === target.tile) {
      tiles[i].classList.add("callable");
      break;
    }
  }
  const slot = pond.closest(".pond-slot");
  if (slot) slot.classList.add("callable");
}

function pondForRel(rel) {
  return ["pond-self", "pond-right", "pond-across", "pond-left"][rel] || "pond-self";
}

/// Play a decision that has exactly one legal action after a short beat.
///
/// The only case in practice is the forced tsumogiri of a riichi hand. The server
/// used to play it silently, so the player never saw the draw or the discard;
/// showing the drawn tile for a moment and then throwing it is the whole point of
/// that phase of the hand.
let forcedTimer = null;
let forcedFor = null;

function autoPlayForcedDecision() {
  // `controlsHeld` keeps the forced 摸切 in step with the table: it is played by
  // the hold timer once this batch's discards have landed.
  if (!state || !state.decision || controlsHeld || boardHold || panelQueue.length) return;
  const acts = state.decision.actions || [];
  if (acts.length !== 1) return;
  const only = acts[0];
  if (!only || typeof only === "string") return;   // never auto-declare a win
  const signature = JSON.stringify(only) + ":" + (state.view ? state.view.wall_remaining : "");
  if (forcedFor === signature) return;
  forcedFor = signature;
  clearTimeout(forcedTimer);
  const box = document.getElementById("hand-info");
  if (box) box.classList.add("auto-note");
  forcedTimer = setTimeout(() => {
    if (box) box.classList.remove("auto-note");
    if (!state || !state.decision) return;
    const a = (state.decision.actions || [])[0];
    if (!a || typeof a === "string") return;
    send({ type: "action", action: a });
  }, Math.max(420, pace() + 260));
}

function renderActions() {
  const bar = document.getElementById("action-bar");
  bar.innerHTML = "";
  markCallTarget();
  if (!state || !state.decision) return;
  // A finished game has no decisions left: re-showing the last ones would offer
  // buttons the server can only reject.
  if (state.view && state.view.finished) return;
  // Nor while a settlement is on screen: those buttons belong to a decision the
  // table has already moved past.
  if (boardHold || panelQueue.length) return;
  const acts = state.decision.actions || [];

  const add = (label, action, primary, extraClass) => {
    const b = document.createElement("button");
    b.textContent = label;
    if (primary) b.className = "primary";
    if (extraClass) b.className = (b.className ? b.className + " " : "") + extraClass;
    b.addEventListener("click", () => {
      riichiMode = false;
      send({ type: "action", action });
    });
    bar.appendChild(b);
  };

  // Name the tile and its owner next to the buttons: "可鸣：AI 2 打出的 5m".
  const target = callTarget();
  if (target && target.seat !== null && target.seat !== undefined && target.seat !== state.human) {
    const hint = document.createElement("span");
    hint.className = "call-hint";
    hint.textContent = (target.kan ? "可抢杠：" : "可鸣：") + who(target.seat)
      + (target.kan ? " 加杠的 " : " 打出的 ") + friendlyTileName(target.tile);
    bar.appendChild(hint);
  }

  if (acts.some((a) => a === "Tsumo")) add("自摸", "Tsumo", true);
  if (acts.some((a) => a === "Ron")) add("荣和", "Ron", true);
  if (acts.some((a) => a === "Kyuushu")) add("九种九牌", "Kyuushu");

  acts.forEach((a) => {
    const k = actKind(a);
    if (k === "Pon") add("碰", a);
    if (k === "Minkan") add("大明杠", a);
    if (k === "Ankan") add("暗杠 " + friendlyTileName(a.Meld.meld.tiles[0]), a);
    if (k === "Kakan") add("加杠 " + friendlyTileName(a.Meld.meld.tiles[0]), a);
    if (k === "Chi") {
      const m = a.Meld.meld;
      const names = m.tiles.slice(0, m.len).map(friendlyTileName).join("");
      add("吃 " + names, a);
    }
  });

  const hasRiichi = acts.some((a) => a.Discard && a.Discard.riichi);
  if (hasRiichi) {
    const b = document.createElement("button");
    b.textContent = riichiMode ? "立直中（点击手牌）" : "立直";
    b.className = "riichi-toggle" + (riichiMode ? " on" : "");
    b.addEventListener("click", toggleRiichi);
    bar.appendChild(b);
  }

  if (acts.some((a) => a === "Pass")) add("跳过", "Pass", false, "pass");
}

// ---------------------------------------------------------------- log / events

/// The batch of events the state we are about to render belongs to. The server
/// sends it immediately before the state, and it is the only reliable record of
/// what happened in what order — including the things that leave no trace in the
/// view (a draw, a pass) and the things that must not be shown before their cause
/// (a 荣和, a 自摸).
let lastBatch = [];

/// The shout and settlement this batch owes the player, staged rather than fired:
/// it is shown when the playback reaches the beat it belongs to.
let pendingHeadline = null;

function absorbEvents(events) {
  if (!events.length) return;
  lastBatch = events;
  const logEl = document.getElementById("log");
  events.forEach((e) => {
    const line = describeEvent(e);
    if (line) {
      logs.push(line);
      if (logs.length > 300) logs.shift();
    }
  });
  logEl.innerHTML = logs.slice(-60).map((l) => `<div class="ev">${l}</div>`).join("");
  logEl.scrollTop = logEl.scrollHeight;
  // the newest line stays visible in the header while the list is collapsed
  const latest = document.getElementById("log-latest");
  if (latest) {
    latest.innerHTML = logs.length ? logs[logs.length - 1] : "";
    latest.title = latest.textContent || "";
  }

  // 立直 is easy to miss — a tile that quietly lies sideways — so it gets the
  // same shout as a win. Only when nobody won or drew in this very batch: the
  // hand's ending is the more important news.
  const wins = events.filter((e) => e.Win).map((e) => e.Win);
  const draw = [...events].reverse().find((e) => e.Ryuukyoku);
  if (!wins.length && !draw) {
    const riichi = events.filter((e) => e.Riichi);
    if (riichi.length) {
      const who1 = riichi.map((e) => botNames[e.Riichi.seat] || "对手").join("、");
      // The seat of the (first) declarer is where the banner belongs.
      pendingHeadline = { shout: { text: "立直", sub: who1, seat: riichi[0].Riichi.seat } };
    }
    return;
  }

  // A hand ended. The board is held on the finished hand — no controls, no
  // decisions — but the state that goes with it is still rendered: with the table
  // paused at the hand's end (see the server's `awaiting_ack`), that state *is*
  // the finished hand, so the winning tile is on the table before the panel
  // covers it. The shout and the panel are staged onto the playback below.
  holdBoard();
  const queue = [];
  // The announcement comes first and the settlement follows it: a big 自摸 in
  // the middle of the table, then the panel with the hand and the yaku. Showing
  // both at once would bury the announcement behind the panel.
  //
  // The wait is one beat of the table's own pace plus 400 ms — the reference
  // client's rule — with a floor, because the panel covers the middle of the
  // table and a shout nobody managed to read is worse than a pause.
  let announceMs = Math.max(900, pace() + 400);
  let shout = null;
  if (wins.length) {
    // Settle winners in play order from the discarder: that is counter-clockwise
    // at the table, and it is the order every ruleset describes.
    const from = wins[0].from;
    const order = (w) => (from === null || from === undefined ? w.seat : (w.seat - from + 4) % 4);
    // Keep the sorted result: `.sort()` on a copy left `wins[0]` as whatever the
    // engine happened to list first, which put the announcement on the wrong
    // seat (the panels were ordered correctly, the banner was not).
    const settled = wins.slice().sort((a, b) => order(a) - order(b));
    settled.forEach((w) => queue.push({ kind: "win", data: w }));
    if (wins.length > 1) {
      // Two ron is the common case; three is normally aborted by the engine as
      // 三家和了, so the third panel only appears if the rules allow it. Two
      // settlements need longer than one before the first panel covers the shout.
      announceMs = Math.max(1300, pace() + 900);
      shout = { text: wins.length === 2 ? "双响" : "三响",
                sub: settled.map((w) => botNames[w.seat]).join("、"),
                seat: settled[0].seat, ms: announceMs };
    } else {
      const w = wins[0];
      const ron = w.from !== null && w.from !== undefined;
      if (w.nagashi) {
        shout = { text: "流局满贯", sub: botNames[w.seat] || "", seat: w.seat };
      } else {
        shout = { text: ron ? "荣和" : "自摸",
                  sub: `${botNames[w.seat] || ""} ${friendlyTileName(w.tile)}`, seat: w.seat };
      }
    }
  } else if (draw) {
    const dd = draw.Ryuukyoku;
    queue.push({ kind: "draw", data: dd });
    const sub = (DRAW_REASONS[dd.reason] || "")
      + (dd.by !== null && dd.by !== undefined ? ` · ${botNames[dd.by] || ""} 宣布` : "");
    shout = { text: dd.reason === "Exhaustive" ? "流局" : "途中流局", sub, ms: announceMs };
  }
  pendingHeadline = { shout, queue, announceMs };

  // Watchdog. The playback normally shows this within a few beats, but the table
  // is *paused* until the player has read the settlement (see the server's
  // `awaiting_ack`), so a shout that never reached the screen would leave the
  // match stopped with nothing to click — for instance if the state message that
  // carries the batch's discards never arrives. This is the net under that.
  clearTimeout(headlineWatchdog);
  headlineWatchdog = setTimeout(() => {
    if (pendingHeadline) showHeadline();
  }, Math.max(6000, pace() * 8 + 2000));
}

let headlineWatchdog = null;

/// Play the staged shout and settlement, once the playback has reached the beat
/// they belong to. A 荣和 waits for the tile it happened on; a 自摸 for the turn.
function showHeadline() {
  const h = pendingHeadline;
  if (!h) return;
  pendingHeadline = null;
  clearTimeout(headlineWatchdog);
  if (h.shout) announce(h.shout.text, h.shout.sub, h.shout.ms, h.shout.seat);
  if (!h.queue || !h.queue.length) return;
  clearTimeout(settleTimer);
  pendingSettlement = h.queue;
  settleTimer = setTimeout(() => {
    pendingSettlement = [];
    enqueueSettlements(h.queue);
  }, h.announceMs || 1000);
}

let settleTimer = null;
/// Settlements waiting for their announcement to finish. Kept so a match ending
/// in that window cannot swallow them.
let pendingSettlement = [];

/// The player has read the last panel of a finished hand: tell the server it may
/// deal the next one. The table is paused at the hand's end (the server plays no
/// further until this arrives), which is what keeps the *finished* hand on the
/// board instead of the next round's opening.
function askContinue() {
  send({ type: "continue" });
}

// ------------------------------------------------------- settlement queue

/// True while a hand's panels (or the match result) are being read.
/// True from the moment a hand ends until its last panel is dismissed. While it
/// holds, the next round's state is kept back so the board stays on the hand
/// the player is still reading about.
let boardHold = false;

function enqueueSettlements(panels) {
  panelQueue = panelQueue.concat(panels);
  showNextPanel();
}

/// The player acknowledged the panel on screen: show the next one, or release
/// the board if that was the last.
function dismissPanel() {
  const wasSettlement = panelQueue.length > 0;
  if (wasSettlement) panelQueue.shift();
  document.getElementById("overlay").classList.add("hidden");
  if (!showNextPanel()) {
    // Nothing left to read: release the board, and ask the table for the next
    // hand. Nothing moves on the server until it is asked — it is paused at the
    // hand's end — so this is what starts the next round, and it starts it from
    // the deal rather than from wherever the bots had already got to.
    boardHold = false;
    applyPendingState();
    if (wasSettlement) askContinue();
  }
}

function describeEvent(e) {
  if (e.RoundStart) {
    const r = e.RoundStart;
    return `── ${ROUND_WIND_FACE[r.round_wind] || "?"}${r.round_number}局 ${r.honba}本场 开始`
      + `（宝牌指示牌 ${tileName(r.dora_indicator)}）──`;
  }
  if (e.Draw) {
    const d = e.Draw;
    return `${who(d.seat)} 摸牌${d.rinshan ? "（岭上）" : ""}`;
  }
  if (e.Discard) {
    const d = e.Discard;
    return `${who(d.seat)} 打出 <strong>${friendlyTileName(d.tile)}</strong>`
      + (d.riichi ? " 并立直" : "") + (d.tsumogiri ? "（摸切）" : "（手切）");
  }
  if (e.Riichi) return `<strong>${who(e.Riichi.seat)} 立直！</strong>`;
  if (e.Meld) {
    const m = e.Meld;
    const tiles = m.meld.tiles.slice(0, m.meld.len).map(friendlyTileName).join("");
    return `${who(m.seat)} ${meldKindName(m.meld.kind)} ${tiles}`;
  }
  if (e.Kan) {
    const k = e.Kan;
    // Name which of the three kans it was. "杠 5m" leaves the player guessing
    // whether a concealed quad just went down, and the three mean different
    // things (喰い下がり, the 搶槓 window, where the dora comes from).
    return `${who(k.seat)} ${meldKindName(k.meld.kind)} ${friendlyTileName(k.meld.tiles[0])}`
      + (k.dora_indicator !== null && k.dora_indicator !== undefined
        ? `（新宝牌指示牌 ${tileName(k.dora_indicator)}）` : "");
  }
  if (e.DoraRevealed) {
    // 加槓 turns its indicator only after the 搶槓 window closes, so it arrives
    // on its own instead of with the kan.
    return `新宝牌指示牌 ${friendlyTileName(e.DoraRevealed.indicator)}（加杠）`;
  }
  if (e.Win) {
    const w = e.Win;
    if (w.nagashi) {
      return `<strong>${who(w.seat)} 流局满贯</strong> · ${w.score.han}番`;
    }
    const how = w.from === null || w.from === undefined
      ? "自摸" : `荣和（放铳：${who(w.from)}）`;
    return `<strong>${who(w.seat)} ${how} ${tileName(w.tile)}</strong>`
      + ` · ${w.score.han}番${w.score.fu}符`;
  }
  if (e.Ryuukyoku) {
    const r = e.Ryuukyoku;
    // An abortive draw (九种九牌 and friends) pays nobody, so a tenpai list
    // there would invent a result the round never had.
    const tenpai = r.tenpai.map((t, i) => (t ? botNames[i] : null)).filter(Boolean);
    const showTenpai = r.reason === "Exhaustive" && tenpai.length;
    return `<strong>${DRAW_REASONS[r.reason] || "流局"}</strong>`
      + (r.by !== null && r.by !== undefined ? `（${who(r.by)} 宣布）` : "")
      + (typeof r.wall_remaining === "number" ? ` · 余 ${r.wall_remaining} 张` : "")
      + (showTenpai ? ` · 听牌：${tenpai.map(esc).join("、")}` : "");
  }
  if (e.RoundEnd) {
    const r = e.RoundEnd;
    const next = r.next_honba === null || r.next_honba === undefined ? r.honba : r.next_honba;
    return `── 本局结束 · 下一局 ${next} 本场（庄家：${who(r.next_dealer)}）`;
  }
  if (e.GameEnd) return "对局结束";
  return "";
}

function meldKindName(kind) {
  return {
    Chi: "吃", Pon: "碰", Ankan: "暗杠", Minkan: "大明杠", Kakan: "加杠",
  }[kind] || kind;
}

function scoreLine(score) {
  const parts = (score.yaku || []).map(([y, h]) =>
    `${esc(YAKU_NAMES[y] || y)}${h > 0 ? " " + h + "番" : ""}`);
  return parts.join("、");
}

/// The winning hand, melds included, with the winning tile marked. A settlement
/// that only prints a number is not a settlement: this is 報番.
///
/// `seat` is the winner's, so the called sets keep the same sideways-tile
/// convention the table uses. A settlement that redrew them in another order
/// would make the hand impossible to check against the table it was won on.
function handRow(hand, melds, winTile, seat) {
  const row = document.createElement("div");
  row.className = "settle-hand";
  const winKind = winTile === null || winTile === undefined ? -1 : kindOf(winTile);
  let marked = false;
  (hand || []).forEach((t) => {
    const extra = (!marked && kindOf(t) === winKind) ? " winning" : "";
    if (extra) marked = true;
    row.appendChild(tileEl(t, { small: true, extra }));
  });
  (melds || []).forEach((m) => row.appendChild(meldGroup(m, seat, true)));
  return row;
}

function showWin(w) {
  const ron = w.from !== null && w.from !== undefined;
  const nagashi = !!w.nagashi;
  const title = nagashi ? "流局满贯！" : (ron ? "荣和！" : "自摸！");
  const body = document.createElement("div");

  const head = document.createElement("p");
  head.innerHTML = nagashi
    ? `<span class="win">${who(w.seat)}</span> 流局满贯（弃牌全为幺九，且无人鸣牌）`
    : `<span class="win">${who(w.seat)}</span> `
      + (ron ? `荣和 <strong>${friendlyTileName(w.tile)}</strong>（放铳：${who(w.from)}）`
             : `自摸 <strong>${friendlyTileName(w.tile)}</strong>`);
  body.appendChild(head);

  // The hand that won, so the yaku below can be checked by eye. 流し満貫 has no
  // winning tile, so nothing is marked.
  if (w.hand && w.hand.length) {
    body.appendChild(handRow(w.hand, w.melds, nagashi ? null : w.tile, w.seat));
  }

  const yaku = document.createElement("p");
  yaku.className = "settle-yaku";
  yaku.textContent = scoreLine(w.score) || "（无役，仅宝牌）";
  body.appendChild(yaku);

  // Dora is not a yaku, so it is listed apart from the yaku line: without this
  // the panel says "5 番" and leaves the player guessing where they came from.
  const bonus = [["宝牌", w.score.dora_han], ["里宝牌", w.score.ura_han], ["赤宝牌", w.score.aka_han]]
    .filter(([, h]) => h > 0)
    .map(([name, h]) => `${name} +${h}`);
  if (bonus.length) {
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = bonus.join("　");
    body.appendChild(p);
  }

  const total = document.createElement("p");
  total.className = "settle-total";
  total.textContent = w.score.yakuman
    ? `役满 ×${w.score.yakuman}` + (w.score.is_dealer ? " · 庄家" : "")
    : `${w.score.han} 番 ${w.score.fu} 符` + (w.score.is_dealer ? " · 庄家" : "");
  body.appendChild(total);

  // What each seat actually paid, then the resulting totals.
  const paid = document.createElement("p");
  paid.className = "muted";
  if (w.pao_payer !== null && w.pao_payer !== undefined) {
    // 責任払い: the pao payer covers the whole hand, whoever discarded the tile.
    paid.textContent = `責任払い：${who(w.pao_payer)} 支付全部 ${w.paid} 点`;
  } else if (typeof w.paid === "number" && w.paid > 0 && w.from !== null && w.from !== undefined) {
    // What *this* winner was paid. In a double ron the hand-wide `deltas` table
    // includes the other winner's money, which is not this winner's to claim.
    paid.textContent = `${who(w.from)} 支付 ${w.paid} 点`;
  } else if (ron) {
    paid.textContent = `${who(w.from)} 支付 ${-Math.min(0, ...w.deltas)} 点`;
  } else {
    const others = [0, 1, 2, 3].filter((s) => s !== w.seat)
      .map((s) => -w.deltas[s]);
    paid.textContent = `每家支付 ${[...new Set(others)].sort((a, b) => a - b).join(" / ")} 点`;
  }
  if (w.riichi_sticks_taken) {
    paid.textContent += `　·　立直棒 +${w.riichi_sticks_taken * 1000}`;
  }
  body.appendChild(paid);
  body.appendChild(tableNode(scoreTable(w.deltas, true)));

  overlay(title, body);
}

/// One plain sentence per draw reason. A 途中流局 ends the hand before the wall
/// runs out, which looks like a bug unless the table says why it happened.
const DRAW_NOTES = {
  Exhaustive: "牌山摸完，比较听牌：不听者支付罚符",
  NineTerminals: "途中流局：某家在第一次摸牌时手中有九种以上的幺九牌，宣布流局",
  FourWinds: "途中流局：四家第一次出牌都是同一种风牌，且无人鸣牌",
  FourRiichi: "途中流局：四家全部立直",
  FourKans: "途中流局：四家合计开了四个杠，且不是同一人所开",
  TripleRon: "途中流局：三家同时荣和",
};

function showRyuukyoku(r) {
  const body = document.createElement("div");
  const head = document.createElement("p");
  head.innerHTML = `<strong>${esc(DRAW_REASONS[r.reason] || "流局")}</strong>`
    + (r.by !== null && r.by !== undefined ? ` · ${who(r.by)} 宣布` : "");
  body.appendChild(head);

  const why = document.createElement("p");
  why.className = "muted";
  why.textContent = DRAW_NOTES[r.reason] || "本局作废";
  body.appendChild(why);

  // The wall reading lets the player check the draw against the table: an abort
  // leaves the wall nearly full, an exhaustive draw leaves it empty.
  if (typeof r.wall_remaining === "number") {
    const wall = document.createElement("p");
    wall.className = "muted";
    wall.textContent = `本局结束时牌山还剩 ${r.wall_remaining} 张`
      + (r.reason === "Exhaustive" ? "" : "（途中流局：不计点数，庄家连庄并加一本场）");
    body.appendChild(wall);
  } else if (r.reason !== "Exhaustive") {
    // Recorded before the wall reading existed: say what matters, skip the number.
    const wall = document.createElement("p");
    wall.className = "muted";
    wall.textContent = "（途中流局：不计点数，庄家连庄并加一本场）";
    body.appendChild(wall);
  }

  // Only an exhaustive draw compares hands; the abortive draws pay nobody, so
  // printing tenpai there would invent a settlement that never happened.
  const exhaustive = r.reason === "Exhaustive";
  if (exhaustive) {
    const list = document.createElement("p");
    list.textContent = "听牌：" + r.tenpai
      .map((t, i) => `${botNames[i]}${t ? " ○" : " ×"}`).join("　");
    body.appendChild(list);
  }
  const paying = r.deltas && r.deltas.some((d) => d !== 0);
  if (paying) {
    const note = document.createElement("p");
    note.className = "muted";
    const tenpai = r.tenpai.filter(Boolean).length;
    if (tenpai === 0) {
      note.textContent = "全員不听：不支付罚符";
    } else if (tenpai === 4) {
      // Nobody is noten, so nobody pays — that is not "nobody is tenpai".
      note.textContent = "四家全部听牌：不支付罚符";
    } else {
      // 3000 points in total, split between the tenpai hands — which is only
      // "1000 each" when all three of the others are noten.
      const noten = 4 - tenpai;
      note.textContent = `不听罚符：${noten} 家不听各 -1000（合计 ${noten * 1000} 点），`
        + `${tenpai} 家听牌者均分（每家 +${Math.round((noten * 1000) / tenpai)} 点）`;
    }
    body.appendChild(note);
    body.appendChild(tableNode(scoreTable(r.deltas, true)));
  } else if (exhaustive) {
    const note = document.createElement("p");
    note.className = "muted";
    note.textContent = r.tenpai.every(Boolean)
      ? "四家全部听牌：不支付罚符"
      : "全員不听：不支付罚符";
    body.appendChild(note);
  }
  overlay("流局", body);
}

/// Wrap an HTML string (the settlement table) as a node, so it can be appended
/// to a panel that is built as DOM rather than as one big innerHTML string.
function tableNode(html) {
  const wrap = document.createElement("div");
  wrap.innerHTML = html;
  return wrap;
}

/// A settlement table: what each seat gained or lost, and where they stand now.
/// The scores shown are the totals *after* the hand, which is what a player
/// actually wants to read at the end of a hand.
function scoreTable(deltas, withTotals) {
  if (!deltas) return "";
  const before = (state && state.view && state.view.players)
    ? state.view.players.map((p) => p.score)
    : null;
  const rows = deltas.map((d, i) => {
    const total = before && before[i] !== undefined ? `<td>${before[i] + d}</td>` : "";
    return `<tr><td>${who(i)}</td>`
      + `<td class="${d > 0 ? "up" : (d < 0 ? "down" : "")}">${d > 0 ? "+" : ""}${d}</td>`
      + (withTotals ? total : "") + "</tr>";
  }).join("");
  const head = withTotals ? "<tr><th>玩家</th><th>本局增减</th><th>结算后点数</th></tr>"
                          : "<tr><th>玩家</th><th>点数增减</th></tr>";
  return `<table>${head}${rows}</table>`;
}

function showGameEnd(msg) {
  // The match is over, but the hand that decided it still has to be settled.
  // The server sends events → game_end → state back to back, so the settlement
  // of that last hand is usually still waiting on its announcement timer: cancel
  // the timer and the 報番 panel is lost for good. Flush it into the queue
  // instead, then put the result behind it.
  if (pendingSettlement.length) {
    panelQueue = panelQueue.concat(pendingSettlement);
    pendingSettlement = [];
  }
  clearTimeout(settleTimer);
  holdBoard();
  const rows = msg.ranking.map((seat, place) =>
    `<tr><td>${place + 1} 位</td><td>${who(seat)}</td><td>${msg.scores[seat]}</td></tr>`).join("");
  let body = `<p>共 ${msg.rounds} 局</p>`;
  body += `<table><tr><th>名次</th><th>玩家</th><th>终局点数</th></tr>${rows}</table>`;
  if (msg.replay) body += `<p style="opacity:.7">牌谱已保存：${esc(msg.replay)}</p>`;
  panelQueue.push({ kind: "end", title: "对局结束", body });
  showNextPanel();
}

/// Panels that are not settlements — the match result — are queued on the same
/// list so they can never overtake a hand that has not been read yet.
let panelQueue = [];

function showNextPanel() {
  if (!panelQueue.length) return false;
  const p = panelQueue[0];
  if (p.kind === "win") showWin(p.data);
  else if (p.kind === "draw") showRyuukyoku(p.data);
  else overlay(p.title, p.body, p.dismiss);
  return true;
}

// ---------------------------------------------------------------- ui bits

/// A short, loud announcement in the middle of the table: 立直, 自摸, 荣和,
/// 流局. It never blocks a click and it clears itself.
/// How a seat sits relative to the observer: 0 self, 1 right, 2 across, 3 left.
function relativeSeat(seat) {
  return (seat - (state && state.human !== undefined ? state.human : 0) + 4) % 4;
}

/// An announcement, shown **at the seat it is about** rather than in the middle
/// of the table: a 立直 banner in the centre still leaves the player hunting for
/// who declared. `seat` positions it; without one it goes to the centre.
function announce(text, sub, ms, seat) {
  const el = document.getElementById("banner");
  if (!el) return;
  el.innerHTML = `<span class="banner-text">${esc(text)}</span>`
    + (sub ? `<span class="banner-sub">${esc(sub)}</span>` : "");
  el.classList.remove("at-self", "at-right", "at-across", "at-left");
  el.classList.add(["at-self", "at-right", "at-across", "at-left"][relativeSeat(
    seat === null || seat === undefined ? (state && state.human) || 0 : seat)]);
  el.classList.remove("hidden");
  // restart the animation so two announcements in a row both animate
  el.classList.remove("pop");
  void el.offsetWidth;
  el.classList.add("pop");
  clearTimeout(bannerTimer);
  bannerTimer = setTimeout(() => el.classList.add("hidden"), ms || 1250);
}

let bannerTimer = null;

/// Hold the board on the hand just finished: stop showing live controls and put
/// the hint away.
///
/// The render that would normally clear the action bar is deferred (the new
/// state is parked in `pendingState`), so the buttons would otherwise stay on
/// screen and clicking one would send an action the table has already left.
function holdBoard() {
  boardHold = true;
  // A paced hold still waiting must not draw the bar back over a finished hand
  // when its timer fires.
  cancelControls();
  const hintBox = document.getElementById("hint-box");
  if (hintBox) hintBox.classList.add("hidden");
}

function settlementOpen() {
  const o = document.getElementById("overlay");
  return !!o && !o.classList.contains("hidden");
}

/// Show the state that was held back while a settlement was on screen.
function applyPendingState() {
  if (!pendingState) return;
  state = pendingState;
  pendingState = null;
  render();
}

/// Show a plain dialog. `transient` dialogs (the shortcut help) are not part of
/// the settlement queue, so closing one must not consume a queued panel.
/// Show a dialog. `body` is either HTML text or a DOM node.
///
/// It must accept a node: a settlement panel is built as DOM because it contains
/// tiles, and pushing it through `innerHTML` re-parses it into *new* elements —
/// the tile faces, whose images are still loading, then attach to the discarded
/// originals and the panel shows blank tiles.
function overlay(title, body, dismiss, transient) {
  const host = document.getElementById("overlay-body");
  document.getElementById("overlay-title").textContent = title;
  host.innerHTML = "";
  if (typeof body === "string") host.innerHTML = body;
  else if (body) host.appendChild(body);
  document.getElementById("overlay-close").textContent = dismiss || "继续";
  overlayIsTransient = !!transient;
  overlayIsSettlement = !transient;
  document.getElementById("overlay").classList.remove("hidden");
}

let overlayIsTransient = false;
let overlayIsSettlement = false;

// The hint panel shows three things at once: the network's own ranking with its
// probabilities, the tile-efficiency baseline's pick, and the hand's shape. They
// disagree often, and seeing both is more useful than being told one answer.
function showHint(msg) {
  const box = document.getElementById("hint-box");
  box.innerHTML = "";

  const title = document.createElement("div");
  title.className = "hint-title";
  title.textContent = msg.text
    ? msg.text.split("\n")[0].replace(/推荐：\s*(.*)$/, (_, a) => "推荐：" + friendlyAction(a))
    : "提示";
  box.appendChild(title);

  if (msg.baseline) {
    const sec = document.createElement("div");
    sec.className = "hint-sec";
    sec.textContent = "基线推荐：" + friendlyAction(msg.baseline);
    box.appendChild(sec);
  }

  const net = msg.net;
  if (net && net.top && net.top.length) {
    const sec = document.createElement("div");
    sec.className = "hint-sec";
    const h = document.createElement("div");
    h.className = "hint-head";
    h.innerHTML = '<span>神经网络</span><span class="mono">' + esc(net.checkpoint || "") + "</span>";
    sec.appendChild(h);
    const best = net.top[0].prob || 1;
    net.top.forEach((row) => {
      const line = document.createElement("div");
      line.className = "hint-row" + (row === net.top[0] ? " best" : "");
      const bar = document.createElement("span");
      bar.className = "hint-bar";
      bar.style.width = Math.max(4, Math.round((row.prob / best) * 100)) + "%";
      const label = document.createElement("span");
      label.className = "hint-label";
      label.textContent = friendlyAction(row.label);
      const pct = document.createElement("span");
      pct.className = "hint-pct mono";
      pct.textContent = (row.prob * 100).toFixed(1) + "%";
      line.appendChild(label);
      line.appendChild(bar);
      line.appendChild(pct);
      sec.appendChild(line);
    });
    if (typeof net.value === "number") {
      const v = document.createElement("div");
      v.className = "hint-foot";
      v.textContent = "期望得失 ≈ " + (net.value > 0 ? "+" : "") + Math.round(net.value) +
        " 分（价值头 R²≈0.11，只看方向）";
      sec.appendChild(v);
    }
    box.appendChild(sec);
  } else if (net && net.error) {
    const sec = document.createElement("div");
    sec.className = "hint-sec muted";
    sec.textContent = "神经网络不可用：" + net.error;
    box.appendChild(sec);
  }

  if (msg.shape) {
    const sec = document.createElement("div");
    sec.className = "hint-sec";
    let line = "向听 " + msg.shape.before + " → " + msg.shape.after + "，进张 " + msg.shape.ukeire + " 张";
    if (msg.shape.waits) line += "，听：" + msg.shape.waits;
    sec.textContent = line;
    box.appendChild(sec);
  } else if (msg.text) {
    const sec = document.createElement("div");
    sec.className = "hint-sec";
    sec.textContent = msg.text.split("\n").slice(1).join("  ");
    box.appendChild(sec);
  }

  const close = document.createElement("button");
  close.className = "hint-close";
  close.textContent = "关闭";
  close.addEventListener("click", () => box.classList.add("hidden"));
  box.appendChild(close);

  box.classList.remove("hidden");
  placeHint();
  clearTimeout(box._timer);
  box._timer = setTimeout(() => box.classList.add("hidden"), 16000);
}

/// Sit the hint panel above the controls rather than on top of them. Both the
/// call buttons and the hand are things the player has to reach, and either can
/// be the lowest thing on screen depending on the window height.
function placeHint() {
  const box = document.getElementById("hint-box");
  if (!box || box.classList.contains("hidden")) return;
  const tops = ["action-bar", "hand-area"]
    .map((id) => document.getElementById(id))
    .filter((e) => e && e.getBoundingClientRect().height > 0)
    .map((e) => e.getBoundingClientRect().top);
  const limit = tops.length ? Math.min(...tops) : window.innerHeight - 120;
  box.style.top = Math.round(Math.max(58, limit - box.offsetHeight - 10)) + "px";
  box.style.bottom = "auto";
}

function toast(text) {
  const t = document.getElementById("toast");
  t.textContent = text;
  t.classList.remove("hidden");
  clearTimeout(t._timer);
  t._timer = setTimeout(() => t.classList.add("hidden"), 3000);
}

// ---------------------------------------------------------------- replay analysis

async function openReplayPanel() {
  const overlay = document.getElementById("replay-overlay");
  overlay.classList.remove("hidden");
  const select = document.getElementById("replay-select");
  const status = document.getElementById("replay-status");
  status.textContent = "读取牌谱列表…";
  document.getElementById("replay-body").innerHTML = "";
  try {
    const res = await fetch("/api/replays");
    const data = await res.json();
    const replays = data.replays || [];
    select.innerHTML = "";
    if (!replays.length) {
      status.textContent = "还没有牌谱：先打完一场对局吧。";
      return;
    }
    replays.forEach((r) => {
      const opt = document.createElement("option");
      opt.value = r.name;
      opt.textContent = `${r.name}  (${(r.bytes / 1024).toFixed(0)} KB)`;
      select.appendChild(opt);
    });
    status.textContent = `共 ${replays.length} 份牌谱`;
    runReplayAnalysis(replays[0].name);
  } catch (e) {
    status.textContent = "无法读取牌谱列表：" + e;
  }
}

async function runReplayAnalysis(name) {
  const status = document.getElementById("replay-status");
  const body = document.getElementById("replay-body");
  status.textContent = "分析中（重建对局 + 逐步评估，约 1-3 秒）…";
  body.innerHTML = "";
  try {
    const res = await fetch(`/api/analyze?name=${encodeURIComponent(name)}`);
    if (!res.ok) {
      status.textContent = "分析失败：" + (await res.text());
      return;
    }
    const a = await res.json();
    status.textContent = a.verified ? "对局已精确重建" : "注意：未能精确重建，结果仅供参考";
    body.innerHTML = renderAnalysis(a);
  } catch (e) {
    status.textContent = "分析失败：" + e;
  }
}

function renderAnalysis(a) {
  const names = a.players.map((p) => (p.seat === a.analyzed_seat ? "你" : "AI" + p.seat));
  const out = [];

  out.push("<h3>终局</h3>");
  out.push('<table class="rep"><tr><th>名次</th><th>玩家</th><th>点数</th></tr>');
  a.ranking.forEach((seat, i) => {
    out.push(
      `<tr class="${seat === a.analyzed_seat ? "me" : ""}"><td>${i + 1}</td><td>${names[seat]}</td><td class="num">${a.scores[seat]}</td></tr>`
    );
  });
  out.push("</table>");

  out.push("<h3>座位统计</h3>");
  out.push(
    '<table class="rep"><tr><th>玩家</th><th>和牌</th><th>自摸</th><th>荣和</th><th>放铳</th><th>立直</th><th>听牌流局</th><th>未听流局</th></tr>'
  );
  a.players.forEach((p) => {
    out.push(
      `<tr class="${p.seat === a.analyzed_seat ? "me" : ""}"><td>${names[p.seat]}</td>` +
        `<td class="num">${p.wins}</td><td class="num">${p.tsumo}</td><td class="num">${p.ron}</td>` +
        `<td class="num">${p.deal_ins}</td><td class="num">${p.riichi}</td>` +
        `<td class="num">${p.tenpai}</td><td class="num">${p.noten}</td></tr>`
    );
  });
  out.push("</table>");

  out.push("<h3>每局结果</h3>");
  out.push(
    '<table class="rep"><tr><th>局</th><th>结果</th><th>和牌</th><th>役</th><th>番/符</th><th>点数</th></tr>'
  );
  a.hands.forEach((h) => {
    let result = h.result === "win" ? "和了" : h.reason || "流局";
    let who = "";
    let yaku = "";
    let hanfu = "";
    let pts = h.deltas.map((d, i) => `${names[i]} ${d > 0 ? "+" : ""}${d}`).join("　");
    if (h.winner !== null && h.winner !== undefined) {
      const how = h.from === null || h.from === undefined ? "自摸" : `荣和 ← ${names[h.from]}`;
      who = `${names[h.winner]} ${how} ${h.tile_name || ""}`;
      yaku = (h.yaku || []).map(([n, v]) => (v > 0 ? `${n}${v}` : n)).join(" ");
      hanfu = `${h.han}番 ${h.fu}符`;
    }
    out.push(
      `<tr><td>${h.round}${h.honba ? h.honba + "本场" : ""}</td><td>${result}</td><td>${who}</td>` +
        `<td>${yaku}</td><td>${hanfu}</td><td class="num">${pts}</td></tr>`
    );
  });
  out.push("</table>");

  const s = a.summary;
  out.push("<h3>AI 视角的决策评估</h3>");
  out.push(
    `<p class="muted">共 ${s.decisions} 个决策；网络给实际打法的平均概率 ` +
      `<strong>${(s.mean_agreement * 100).toFixed(1)}%</strong>，` +
      `与网络首选不同 <strong>${s.disagreements}</strong> 次，` +
      `状态价值均值 ${s.mean_value.toFixed(2)}（千点）</p>`
  );
  const interesting = (a.decisions || []).filter((d) => d.options && d.options.length > 1).slice(0, 40);
  if (!interesting.length) {
    out.push('<p class="muted">没有可用于评估的决策（缺少神经网络权重？）</p>');
    return out.join("");
  }
  out.push("<h3>分歧最大的决策</h3>");
  out.push(
    '<table class="rep"><tr><th>局</th><th>手</th><th>类型</th><th>你打</th><th>网络倾向</th>' +
      "<th>实际概率</th><th>首选概率</th><th>选项分布</th></tr>"
  );
  interesting.forEach((d) => {
    const gap = d.chosen_prob - d.top_prob;
    const opts = d.options
      .slice()
      .sort((x, y) => y.prob - x.prob)
      .slice(0, 6)
      .map(
        (o) =>
          `<span class="opt${o.chosen ? " chosen" : ""}">${esc(friendlyAction(o.label))} ${(o.prob * 100).toFixed(0)}%` +
          (o.value !== null && o.value !== undefined ? ` <span class="muted">v${o.value >= 0 ? "+" : ""}${o.value.toFixed(2)}</span>` : "") +
          "</span>"
      )
      .join("");
    out.push(
      `<tr><td>${esc(d.round)}</td><td class="num">${esc(d.turn)}</td>` +
        `<td>${esc(ANALYSIS_KINDS[d.kind] || d.kind)}</td>` +
        `<td>${esc(friendlyAction(d.chosen))}</td>` +
        `<td>${esc(friendlyAction(d.top))}</td>` +
        `<td class="num">${(d.chosen_prob * 100).toFixed(0)}%</td>` +
        `<td class="num">${(d.top_prob * 100).toFixed(0)}%</td><td>${opts}</td></tr>`
    );
  });
  out.push("</table>");
  out.push(
    '<p class="muted">说明：概率是「训练后的策略网络会这么打的可能性」；' +
      "v 是把该选项往后推一步、再由价值网络给出的期望得失（千点），只对打牌决策计算。" +
      "网络越强，这些数字越可信。</p>"
  );
  return out.join("");
}

// ---------------------------------------------------------------- boot

document.addEventListener("DOMContentLoaded", () => {
  document.getElementById("btn-new").addEventListener("click", () => {
    logs = [];
    clearTimeout(settleTimer);
    panelQueue = [];
    boardHold = false;
    pendingState = null;
    document.getElementById("log").innerHTML = "";
    document.getElementById("log-latest").textContent = "";
    document.getElementById("banner").classList.add("hidden");
    // Remembered so a reconnect rebuilds this game rather than the default one.
    lastRequest = {
      type: "new_game",
      seat: parseInt(document.getElementById("sel-seat").value, 10),
      length: document.getElementById("sel-length").value,
      bot: document.getElementById("sel-bot").value,
    };
    // The seed changes with the game, so `handle` clears the score deltas.
    send(lastRequest);
  });
  // How fast discards appear. Kept in localStorage so a player who prefers a
  // slower table does not have to set it every session. A stored value that the
  // current table of steps no longer covers falls back to the default, because
  // the step list itself changed once (four steps -> five) and an index out of
  // range would otherwise leave the selector showing one thing and the table
  // playing another.
  document.getElementById("sel-pace").addEventListener("change", (ev) => {
    paceIndex = Math.max(0, Math.min(PACE_STEPS.length - 1, Number(ev.target.value) || 0));
    localStorage.setItem("mmj-pace", String(paceIndex));
  });
  const savedPace = Number(localStorage.getItem("mmj-pace"));
  if (Number.isInteger(savedPace) && savedPace >= 0 && savedPace < PACE_STEPS.length) {
    paceIndex = savedPace;
  }
  document.getElementById("sel-pace").value = String(paceIndex);

  // Motion on or off. "Off" removes the animation, not the beat: the table still
  // plays one discard at a time, because that pacing is what makes the hand
  // readable — only the fades and drops go away. The first value comes from the
  // system preference, so a player who asked their OS for less motion gets it
  // without looking for the switch. (SEGA NET MJ, しらぎく麻雀 both ship an
  // effects toggle; this is the same idea for a browser table.)
  const animBox = document.getElementById("chk-anim");
  const setAnim = (on) => {
    document.body.classList.toggle("no-anim", !on);
    if (animBox) animBox.checked = on;
    localStorage.setItem("mmj-anim", on ? "1" : "0");
  };
  const savedAnim = localStorage.getItem("mmj-anim");
  const prefersLess = window.matchMedia
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  setAnim(savedAnim === null ? !prefersLess : savedAnim === "1");
  if (animBox) animBox.addEventListener("change", () => setAnim(animBox.checked));
  // The hint costs a network evaluation and a baseline search on the server, so
  // ignore repeat presses instead of queueing them.
  let hintAskedAt = 0;
  document.getElementById("btn-hint").addEventListener("click", () => {
    const now = Date.now();
    if (now - hintAskedAt < 400) return;
    hintAskedAt = now;
    send({ type: "hint" });
  });
  // The panel is advice; a call button underneath it is a decision. Keep the two
  // apart at every window size. Registered once, not per hint.
  window.addEventListener("resize", placeHint);
  document.getElementById("btn-replays").addEventListener("click", openReplayPanel);
  document.getElementById("log-toggle").addEventListener("click", () => {
    const log = document.getElementById("log");
    const collapsed = log.classList.toggle("collapsed");
    document.getElementById("log-toggle").textContent = collapsed ? "展开" : "收起";
  });
  document.getElementById("keys-btn").addEventListener("click", () => {
    overlay("操作方式", [
      "<table>",
      "<tr><th>操作</th><th>作用</th></tr>",
      "<tr><td>点击手牌</td><td>打出这张牌</td></tr>",
      "<tr><td>点击「立直」再点牌</td><td>立直宣言（只能打出能听牌的牌）</td></tr>",
      "<tr><td>回车</td><td>打出刚摸到的牌（摸切）</td></tr>",
      "<tr><td>R</td><td>开关立直宣言</td></tr>",
      "<tr><td>H</td><td>让基线 AI 给出建议</td></tr>",
      "<tr><td>N</td><td>开新对局</td></tr>",
      "<tr><td>Esc</td><td>关闭弹窗与提示面板</td></tr>",
      "<tr><td>Tab / 回车</td><td>纯键盘：Tab 选中按钮或手牌，回车确认</td></tr>",
      "</table>",
      "<p class=\"muted\">立直之后手牌会锁住，只能打出刚摸到的那张；此时也不能再吃碰杠。</p>",
    ].join(""), "知道了", true);
  });
  document.getElementById("replay-close").addEventListener("click", () => {
    document.getElementById("replay-overlay").classList.add("hidden");
  });
  document.getElementById("replay-run").addEventListener("click", () => {
    const name = document.getElementById("replay-select").value;
    if (name) runReplayAnalysis(name);
  });
  document.getElementById("overlay-close").addEventListener("click", () => {
    if (overlayIsTransient) {
      document.getElementById("overlay").classList.add("hidden");
      overlayIsTransient = false;
      return;
    }
    dismissPanel();
  });

  // Keyboard play, because clicking fourteen tiles with a mouse is worse than it
  // sounds: Enter discards the tile you drew, R toggles the riichi declaration,
  // H asks for a hint, N starts a new hand and Esc closes whatever is open.
  document.addEventListener("keydown", (ev) => {
    if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
    const tag = (ev.target && ev.target.tagName) || "";
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
    // A focused tile handles Enter itself; without this the same press threw
    // two tiles, and the second one came back as a server error.
    if (ev.defaultPrevented) return;
    if (ev.target && ev.target.classList && ev.target.classList.contains("tile")) return;
    const key = ev.key.toLowerCase();
    if (key === "enter") {
      ev.preventDefault();
      const me = state && state.view && state.view.players ? state.view.players[state.human] : null;
      if (me && me.drawn !== null && me.drawn !== undefined) {
        const act = findDiscardAction(me.drawn, riichiMode);
        if (act) { riichiMode = false; send({ type: "action", action: act }); }
        else toast("摸到的这张现在不能打");
      }
    } else if (key === "r") {
      toggleRiichi();
    } else if (key === "h") {
      document.getElementById("btn-hint").click();
    } else if (key === "n") {
      document.getElementById("btn-new").click();
    } else if (key === "escape") {
      document.getElementById("hint-box").classList.add("hidden");
      document.getElementById("replay-overlay").classList.add("hidden");
      document.getElementById("banner").classList.add("hidden");
      // Esc means "next": it must not drop the rest of a double ron, and while
      // an announcement is still counting down it must not release the board
      // early either.
      if (overlayIsTransient) {
        document.getElementById("overlay").classList.add("hidden");
        overlayIsTransient = false;
      } else if (settlementOpen() || boardHold || panelQueue.length) {
        dismissPanel();
      }
    }
  });

  preloadTileArt();
  connect();
  // Read by the inline guard in index.html: the table is only usable once this
  // script has actually booted.
  window.__mmjBooted = true;
});
