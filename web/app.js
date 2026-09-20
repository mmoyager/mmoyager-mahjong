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

function tileName(tile) {
  const f = tileFace(tile);
  return f.suit === "z" ? f.text : f.text + f.suit;
}

/// Name a tile *kind* (0-33) rather than a physical tile. Waits are kinds, and
/// naming them through `kind * 4` would call every 5m/5p/5s wait a red five,
/// because tiles 16/52/88 are the aka fives.
function kindName(kind) {
  if (kind >= 27) return HONOR_FACE[kind] || "?";
  const n = (kind % 9) + 1;
  return String(n) + (kind < 9 ? "m" : kind < 18 ? "p" : "s");
}

// Tile faces are the public-domain SVG set by FluffyStuff
// (https://github.com/FluffyStuff/riichi-mahjong-tiles, CC0), served from
// web/tiles/. They are vector, so one file serves the 22 px pond tile and the
// 62 px hand tile. A text face is kept as a fallback so the table still reads if
// an image ever fails to load.
// The white dragon is the set's blank, framed tile. The pack also ships a
// Haku.svg, but everything in it sits inside <defs> and it paints nothing.
const HONOR_FILES = ["Ton", "Nan", "Shaa", "Pei", "Blank", "Hatsu", "Chun"];

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

  const img = document.createElement("img");
  img.className = "tile-img";
  img.alt = tileName(tile);
  img.draggable = false;
  img.src = "/tiles/" + tileFile(tile) + ".svg";
  img.addEventListener("error", () => {
    // Keep the table readable without the asset: fall back to a text face.
    el.classList.add("no-asset");
    img.remove();
    el.appendChild(textFace(k));
  });
  el.appendChild(img);

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
    b.className = "back" + (opts.small ? " small" : "");
    wrap.appendChild(b);
  }
  return wrap;
}

// ---------------------------------------------------------------- networking

// The last game the player asked for, so a reconnect can rebuild it instead of
// silently starting a different one.
let lastRequest = null;
let reconnectTimer = null;

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${proto}://${location.host}/ws`);
  socket.onmessage = (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch (e) { return; }
    handle(msg);
  };
  socket.onopen = () => {
    // The server hands every new socket a *new* game; re-ask for the one the
    // player was in. The seed is what makes it the same match rather than a
    // fresh one, so it is sent back with the request.
    if (lastRequest) {
      send(Object.assign({}, lastRequest, lastSeed === null ? {} : { seed: lastSeed }));
    }
  };
  socket.onclose = () => {
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

function send(obj) {
  if (socket && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(obj));
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
        logs = [];
        const logEl = document.getElementById("log");
        if (logEl) logEl.innerHTML = "";
        const latest = document.getElementById("log-latest");
        if (latest) latest.textContent = "";
      }
      // A settlement panel is modal: keep the finished hand on the board behind
      // it instead of redrawing the next hand underneath the player while they
      // are still reading. The state is applied when the panel is dismissed.
      if (settlementOpen() || boardHold) {
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
    if (pond) renderPond(pond, p.discards, POND_ROT[r]);
    const label = document.getElementById(labelFor[r]);
    if (label) {
      label.textContent = (s === human ? "你" : (botNames[s] || "对手")) +
        (p.riichi ? " · 立直" : "");
    }
  }
  renderHand(view, human);
  renderActions();
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
  name.textContent = botNames[p.seat] || ("座位" + p.seat);
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
  if (p.melds && p.melds.length) slot.appendChild(meldRow(p.melds, true));
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

function meldRow(melds, small) {
  const wrap = document.createElement("div");
  wrap.className = "melds";
  melds.forEach((m) => {
    const g = document.createElement("div");
    g.className = "meld";
    m.tiles.slice(0, m.len).forEach((t) => g.appendChild(tileEl(t, { small })));
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
function renderPond(frame, discards, rotDeg) {
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
  discards.forEach((d, i) => {
    let extra = "";
    if (d.called_by !== null && d.called_by !== undefined) extra += " called";
    if (sideways.has(i)) extra += " rot";
    if (i === last) extra += " fresh";
    grid.appendChild(tileEl(d.tile, { small: true, extra }));
  });
}

function renderHand(view, human) {
  const me = view.players[human];
  const handEl = document.getElementById("hand");
  handEl.innerHTML = "";
  const meldsEl = document.getElementById("melds-self");
  meldsEl.innerHTML = "";
  // Called sets are gone from `me.hand`, so without this the tiles a call took
  // would simply vanish from the board.
  if (me.melds && me.melds.length) meldsEl.appendChild(meldRow(me.melds, true));

  const decision = state.decision;
  const discardable = decision
    ? decision.actions.some((a) => a.Discard)
    : false;
  // After 立直 the hand is locked: the engine offers the drawn tile and nothing
  // else, and the UI must not suggest otherwise. Falling back to "same kind"
  // here would light up concealed copies of the drawn tile.
  const locked = !!me.riichi;

  // The drawn tile is rendered separately, slightly offset.
  let hand = (me.hand || []).slice();
  let drawn = me.drawn;
  if (drawn !== null && drawn !== undefined) {
    const idx = hand.indexOf(drawn);
    if (idx >= 0) hand.splice(idx, 1);
  }

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
      label: (riichiMode ? "立直并打出 " : "打出 ") + tileName(t),
      onClick: clickTile(t),
    }));
  });
  if (drawn !== null && drawn !== undefined) {
    const canPlay = discardable && !!findDiscardAction(drawn, riichiMode);
    handEl.appendChild(tileEl(drawn, {
      clickable: canPlay,
      disabled: discardable && !canPlay,
      extra: "drawn",
      label: (riichiMode ? "立直并打出刚摸到的 " : "打出刚摸到的 ") + tileName(drawn),
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

function renderActions() {
  const bar = document.getElementById("action-bar");
  bar.innerHTML = "";
  if (!state || !state.decision) return;
  // A finished game has no decisions left: re-showing the last ones would offer
  // buttons the server can only reject.
  if (state.view && state.view.finished) return;
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

  if (acts.some((a) => a === "Tsumo")) add("自摸", "Tsumo", true);
  if (acts.some((a) => a === "Ron")) add("荣和", "Ron", true);
  if (acts.some((a) => a === "Kyuushu")) add("九种九牌", "Kyuushu");

  acts.forEach((a) => {
    const k = actKind(a);
    if (k === "Pon") add("碰", a);
    if (k === "Minkan") add("大明杠", a);
    if (k === "Ankan") add("暗杠 " + tileName(a.Meld.meld.tiles[0]), a);
    if (k === "Kakan") add("加杠 " + tileName(a.Meld.meld.tiles[0]), a);
    if (k === "Chi") {
      const m = a.Meld.meld;
      const names = m.tiles.slice(0, m.len).map(tileName).join("");
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

function absorbEvents(events) {
  if (!events.length) return;
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
      announce("立直", who1);
    }
    return;
  }

  // A hand ended. Hold the board on the finished hand until the last panel has
  // been read, then let the next round's state through.
  boardHold = true;
  const queue = [];
  // The announcement comes first and the settlement follows it: a big 自摸 in
  // the middle of the table, then the panel with the hand and the yaku. Showing
  // both at once would bury the announcement behind the panel.
  let announceMs = 1200;
  if (wins.length) {
    // Settle winners in play order from the discarder: that is counter-clockwise
    // at the table, and it is the order every ruleset describes.
    const from = wins[0].from;
    const order = (w) => (from === null || from === undefined ? w.seat : (w.seat - from + 4) % 4);
    wins.slice().sort((a, b) => order(a) - order(b)).forEach((w) => queue.push({ kind: "win", data: w }));
    if (wins.length > 1) {
      // Two ron is the common case; three is normally aborted by the engine as
      // 三家和了, so the third panel only appears if the rules allow it.
      announceMs = 1400;
      announce(wins.length === 2 ? "双响" : "三响", wins.map((w) => botNames[w.seat]).join("、"), announceMs);
    } else {
      const w = wins[0];
      const ron = w.from !== null && w.from !== undefined;
      if (w.nagashi) {
        announce("流局满贯", botNames[w.seat] || "");
      } else {
        announce(ron ? "荣和" : "自摸", `${botNames[w.seat] || ""} ${tileName(w.tile)}`);
      }
    }
  } else if (draw) {
    queue.push({ kind: "draw", data: draw.Ryuukyoku });
    announce("流局", DRAW_REASONS[draw.Ryuukyoku.reason] || "", announceMs);
  }
  clearTimeout(settleTimer);
  settleTimer = setTimeout(() => enqueueSettlements(queue), announceMs);
}

let settleTimer = null;

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
    // Nothing left to read: release the board so the next hand appears.
    boardHold = false;
    applyPendingState();
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
    return `${who(d.seat)} 打出 <strong>${tileName(d.tile)}</strong>`
      + (d.riichi ? " 并立直" : "") + (d.tsumogiri ? "（摸切）" : "（手切）");
  }
  if (e.Riichi) return `<strong>${who(e.Riichi.seat)} 立直！</strong>`;
  if (e.Meld) {
    const m = e.Meld;
    const tiles = m.meld.tiles.slice(0, m.meld.len).map(tileName).join("");
    return `${who(m.seat)} ${meldKindName(m.meld.kind)} ${tiles}`;
  }
  if (e.Kan) {
    const k = e.Kan;
    return `${who(k.seat)} 杠 ${tileName(k.meld.tiles[0])}`
      + (k.dora_indicator !== null && k.dora_indicator !== undefined
        ? `（新宝牌指示牌 ${tileName(k.dora_indicator)}）` : "");
  }
  if (e.DoraRevealed) {
    // 加槓 turns its indicator only after the 搶槓 window closes, so it arrives
    // on its own instead of with the kan.
    return `新宝牌指示牌 ${tileName(e.DoraRevealed.indicator)}（加杠）`;
  }
  if (e.Win) {
    const w = e.Win;
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
      + (showTenpai ? ` · 听牌：${tenpai.map(esc).join("、")}` : "");
  }
  if (e.RoundEnd) {
    const r = e.RoundEnd;
    return `── 本局结束 · 下一局 ${r.honba} 本场`;
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
function handRow(hand, melds, winTile) {
  const row = document.createElement("div");
  row.className = "settle-hand";
  const winKind = winTile === null || winTile === undefined ? -1 : kindOf(winTile);
  let marked = false;
  (hand || []).forEach((t) => {
    const extra = (!marked && kindOf(t) === winKind) ? " winning" : "";
    if (extra) marked = true;
    row.appendChild(tileEl(t, { small: true, extra }));
  });
  (melds || []).forEach((m) => {
    const g = document.createElement("div");
    g.className = "meld";
    m.tiles.slice(0, m.len).forEach((t) => g.appendChild(tileEl(t, { small: true })));
    row.appendChild(g);
  });
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
      + (ron ? `荣和 <strong>${tileName(w.tile)}</strong>（放铳：${who(w.from)}）`
             : `自摸 <strong>${tileName(w.tile)}</strong>`);
  body.appendChild(head);

  // The hand that won, so the yaku below can be checked by eye. 流し満貫 has no
  // winning tile, so nothing is marked.
  if (w.hand && w.hand.length) {
    body.appendChild(handRow(w.hand, w.melds, nagashi ? null : w.tile));
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
  if (typeof w.paid === "number" && w.paid > 0 && w.from !== null && w.from !== undefined) {
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

  overlay(title, body.innerHTML);
}

function showRyuukyoku(r) {
  const body = document.createElement("div");
  const head = document.createElement("p");
  head.innerHTML = `<strong>${esc(DRAW_REASONS[r.reason] || "流局")}</strong>`;
  body.appendChild(head);

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
    note.textContent = tenpai === 0
      ? "全員不听：不支付罚符"
      : `不听罚符：不听者每人 -1000，${tenpai} 家听牌者平分 ${tenpai * 1000} 点`;
    body.appendChild(note);
    body.appendChild(tableNode(scoreTable(r.deltas, true)));
  } else if (exhaustive) {
    const note = document.createElement("p");
    note.className = "muted";
    note.textContent = "全員不听：不支付罚符";
    body.appendChild(note);
  }
  overlay("流局", body.innerHTML);
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
  // The match is over, but the hand that decided it still has to be settled:
  // the server sends events → game_end → state back to back, so clearing the
  // queue here threw the final 報番 panel away before it was ever shown. Queue
  // the match result instead, behind whatever hand is still being read.
  clearTimeout(settleTimer);
  boardHold = true;
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
function announce(text, sub, ms) {
  const el = document.getElementById("banner");
  if (!el) return;
  el.innerHTML = `<span class="banner-text">${esc(text)}</span>`
    + (sub ? `<span class="banner-sub">${esc(sub)}</span>` : "");
  el.classList.remove("hidden");
  // restart the animation so two announcements in a row both animate
  el.classList.remove("pop");
  void el.offsetWidth;
  el.classList.add("pop");
  clearTimeout(bannerTimer);
  bannerTimer = setTimeout(() => el.classList.add("hidden"), ms || 1250);
}

let bannerTimer = null;

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
function overlay(title, bodyHtml, dismiss, transient) {
  document.getElementById("overlay-title").textContent = title;
  document.getElementById("overlay-body").innerHTML = bodyHtml;
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

  connect();
});
