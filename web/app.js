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

// How each pond is turned to face its owner, indexed by relative seat
// (0 self, 1 right, 2 across, 3 left). This is what the established clients do:
// the side ponds read down the screen and the opposite one reads upside down,
// because that is the direction those players threw their tiles.
const POND_ROT = { 0: 0, 1: 270, 2: 180, 3: 90 };

// ---------------------------------------------------------------- utilities

function kindOf(tile) { return tile >> 2; }
function isAka(tile) { return tile === 16 || tile === 52 || tile === 88; }

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
const HONOR_FILES = ["Ton", "Nan", "Shaa", "Pei", "Haku", "Hatsu", "Chun"];

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
    // The server hands every new socket a default game; if the player had
    // chosen seat, length or opponent, ask for it again.
    if (lastRequest) send(lastRequest);
  };
  socket.onclose = () => {
    toast("与服务端的连接断开，正在重连…");
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(); }, 1500);
  };
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
      toast(msg.message);
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
      toast(riichiMode ? "这张牌不能立直" : "这张牌现在不能打出");
      return;
    }
    riichiMode = false;
    send({ type: "action", action: act });
  };

  hand.forEach((t) => {
    const canPlay = discardable && !!findDiscardAction(t, riichiMode);
    handEl.appendChild(tileEl(t, {
      clickable: canPlay,
      disabled: discardable && !canPlay,
      onClick: clickTile(t),
    }));
  });
  if (drawn !== null && drawn !== undefined) {
    const canPlay = discardable && !!findDiscardAction(drawn, riichiMode);
    handEl.appendChild(tileEl(drawn, {
      clickable: canPlay,
      disabled: discardable && !canPlay,
      extra: "drawn",
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
  document.getElementById("furiten-flag").classList.toggle("hidden", !me.furiten);
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
  // The engine accepts any physical copy of the same kind.
  return acts.find(
    (a) => a.Discard && !!a.Discard.riichi === wantRiichi && kindOf(a.Discard.tile) === kindOf(tile)
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

  const add = (label, action, primary) => {
    const b = document.createElement("button");
    b.textContent = label;
    if (primary) b.className = "primary";
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

  if (acts.some((a) => a === "Pass")) add("跳过", "Pass");
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

  const lastWin = [...events].reverse().find((e) => e.Win);
  if (lastWin) showWin(lastWin.Win);
  const lastDraw = [...events].reverse().find((e) => e.Ryuukyoku);
  if (lastDraw && !lastWin) showRyuukyoku(lastDraw.Ryuukyoku);
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
  if (e.RoundEnd) return "";
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

function showWin(w) {
  const title = w.from === null || w.from === undefined ? "自摸！" : "荣和！";
  let body = `<p><span class="win">${who(w.seat)}</span> 和了 ${tileName(w.tile)}</p>`;
  if (scoreLine(w.score)) body += `<p>${scoreLine(w.score)}</p>`;
  body += `<p>${w.score.han} 番 ${w.score.fu} 符`
    + (w.score.yakuman ? ` · 役满 ×${w.score.yakuman}` : "")
    + (w.score.is_dealer ? " · 庄家" : "") + `</p>`;
  body += scoreTable(w.deltas);
  overlay(title, body);
}

function showRyuukyoku(r) {
  let body = `<p>${esc(DRAW_REASONS[r.reason] || "流局")}</p>`;
  // Only an exhaustive draw compares hands; the abortive draws pay nobody.
  if (r.reason === "Exhaustive") {
    body += `<p>听牌：${r.tenpai.map((t, i) => `${who(i)}${t ? " ○" : " ×"}`).join("　")}</p>`;
  }
  if (r.deltas && r.deltas.some((d) => d !== 0)) body += scoreTable(r.deltas);
  overlay("流局", body);
}

function scoreTable(deltas) {
  if (!deltas) return "";
  const rows = deltas.map((d, i) =>
    `<tr><td>${who(i)}</td><td>${d > 0 ? "+" : ""}${d}</td></tr>`).join("");
  return `<table><tr><th>玩家</th><th>点数增减</th></tr>${rows}</table>`;
}

function showGameEnd(msg) {
  const rows = msg.ranking.map((seat, place) =>
    `<tr><td>${place + 1} 位</td><td>${who(seat)}</td><td>${msg.scores[seat]}</td></tr>`).join("");
  let body = `<p>共 ${msg.rounds} 局</p>`;
  body += `<table><tr><th>名次</th><th>玩家</th><th>终局点数</th></tr>${rows}</table>`;
  if (msg.replay) body += `<p style="opacity:.7">牌谱已保存：${esc(msg.replay)}</p>`;
  overlay("对局结束", body);
}

// ---------------------------------------------------------------- ui bits

function overlay(title, bodyHtml) {
  document.getElementById("overlay-title").textContent = title;
  document.getElementById("overlay-body").innerHTML = bodyHtml;
  document.getElementById("overlay").classList.remove("hidden");
}

// The hint panel shows three things at once: the network's own ranking with its
// probabilities, the tile-efficiency baseline's pick, and the hand's shape. They
// disagree often, and seeing both is more useful than being told one answer.
function showHint(msg) {
  const box = document.getElementById("hint-box");
  box.innerHTML = "";

  const title = document.createElement("div");
  title.className = "hint-title";
  title.textContent = msg.text ? msg.text.split("\n")[0] : "提示";
  box.appendChild(title);

  const net = msg.net;
  if (net && net.top && net.top.length) {
    const sec = document.createElement("div");
    sec.className = "hint-sec";
    const h = document.createElement("div");
    h.className = "hint-head";
    h.innerHTML = '<span>神经网络</span><span class="mono">' + (net.checkpoint || "") + "</span>";
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
      label.textContent = row.label;
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
  clearTimeout(box._timer);
  box._timer = setTimeout(() => box.classList.add("hidden"), 16000);
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
          `<span class="opt${o.chosen ? " chosen" : ""}">${o.label} ${(o.prob * 100).toFixed(0)}%` +
          (o.value !== null && o.value !== undefined ? ` <span class="muted">v${o.value >= 0 ? "+" : ""}${o.value.toFixed(2)}</span>` : "") +
          "</span>"
      )
      .join("");
    out.push(
      `<tr><td>${d.round}</td><td class="num">${d.turn}</td><td>${d.kind}</td><td>${d.chosen}</td>` +
        `<td>${d.top}</td><td class="num">${(d.chosen_prob * 100).toFixed(0)}%</td>` +
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
    document.getElementById("log").innerHTML = "";
    document.getElementById("log-latest").textContent = "";
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
  document.getElementById("btn-hint").addEventListener("click", () => send({ type: "hint" }));
  document.getElementById("btn-replays").addEventListener("click", openReplayPanel);
  document.getElementById("log-toggle").addEventListener("click", () => {
    const log = document.getElementById("log");
    const collapsed = log.classList.toggle("collapsed");
    document.getElementById("log-toggle").textContent = collapsed ? "展开" : "收起";
  });
  document.getElementById("replay-close").addEventListener("click", () => {
    document.getElementById("replay-overlay").classList.add("hidden");
  });
  document.getElementById("replay-run").addEventListener("click", () => {
    const name = document.getElementById("replay-select").value;
    if (name) runReplayAnalysis(name);
  });
  document.getElementById("overlay-close").addEventListener("click", () => {
    document.getElementById("overlay").classList.add("hidden");
  });

  // Keyboard play, because clicking fourteen tiles with a mouse is worse than it
  // sounds: Enter discards the tile you drew, R toggles the riichi declaration,
  // H asks for a hint, N starts a new hand and Esc closes whatever is open.
  document.addEventListener("keydown", (ev) => {
    if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
    const tag = (ev.target && ev.target.tagName) || "";
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
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
      send({ type: "hint" });
    } else if (key === "n") {
      document.getElementById("btn-new").click();
    } else if (key === "escape") {
      document.getElementById("hint-box").classList.add("hidden");
      document.getElementById("overlay").classList.add("hidden");
      document.getElementById("replay-overlay").classList.add("hidden");
    }
  });

  connect();
});
