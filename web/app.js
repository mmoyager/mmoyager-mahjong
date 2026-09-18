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
let lastRoundSeen = -1;

// ---------------------------------------------------------------- utilities

function kindOf(tile) { return tile >> 2; }
function isAka(tile) { return tile === 16 || tile === 52 || tile === 88; }

function tileFace(tile) {
  const k = kindOf(tile);
  if (k >= 27) return { text: HONOR_FACE[k], suit: "z", aka: false };
  const n = (k % 9) + 1;
  const suit = k < 9 ? "m" : k < 18 ? "p" : "s";
  return { text: isAka(tile) ? "0" : String(n), suit, aka: isAka(tile) };
}

function tileName(tile) {
  const f = tileFace(tile);
  return f.suit === "z" ? f.text : f.text + f.suit;
}

// The tile faces are drawn with CSS rather than images: 萬 shows its numeral over
// 萬, 筒 is that many dots, 索 that many bamboo sticks, and the honours carry
// their own character. Drawn this way they stay crisp at any size, need no
// assets, and scale from a 26 px pond tile to a 52 px hand tile from one set of
// rules.
const NUMERAL = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

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

  const face = document.createElement("span");
  face.className = "face";

  if (k >= 27) {
    face.classList.add("honor", "h" + k);
    face.textContent = HONOR_FACE[k];
  } else {
    const suit = k < 9 ? "m" : k < 18 ? "p" : "s";
    const n = (k % 9) + 1;
    face.classList.add(suit);
    if (suit === "m") {
      // 萬: the numeral above the character, as on a real tile.
      face.innerHTML = '<span class="num">' + NUMERAL[n] + '</span><span class="kanji">萬</span>';
    } else {
      // 筒 (dots) and 索 (bamboo) are drawn as repeated marks; the container
      // arranges them, and a single mark gets its own larger style in CSS.
      const marks = new Array(n).fill("<i></i>").join("");
      face.innerHTML = '<span class="' + (suit === "p" ? "pips" : "sticks") +
        '" data-n="' + n + '">' + marks + "</span>";
    }
  }
  el.appendChild(face);

  if (opts.onClick && !opts.disabled) {
    el.addEventListener("click", opts.onClick);
  }
  return el;
}

// Opponents' hands: a row of tile backs. Kept separate from `tileEl` because a
// back has no face and should not pretend to.
function backRow(count, opts = {}) {
  const wrap = document.createElement("div");
  wrap.className = "backs" + (opts.vertical ? " vertical" : "");
  for (let i = 0; i < Math.max(0, count); i++) {
    const b = document.createElement("div");
    b.className = "back" + (opts.small ? " small" : "");
    wrap.appendChild(b);
  }
  return wrap;
}

function mulberry(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ---------------------------------------------------------------- networking

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${proto}://${location.host}/ws`);
  socket.onmessage = (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch (e) { return; }
    handle(msg);
  };
  socket.onclose = () => {
    toast("与服务端的连接断开，正在重连…");
    setTimeout(connect, 1500);
  };
}

function send(obj) {
  if (socket && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(obj));
}

function handle(msg) {
  switch (msg.type) {
    case "state":
      botNames = msg.botNames || botNames;
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
      showHint(msg.text);
      break;
    case "error":
      toast(msg.message);
      break;
    default:
      break;
  }
}

// ---------------------------------------------------------------- rendering

function render() {
  if (!state) return;
  const view = state.view;
  const human = state.human;

  document.getElementById("round-name").textContent =
    (ROUND_WIND_FACE[view.round_wind] || "?") + view.round_number + "局";
  document.getElementById("honba").textContent = view.honba + " 本场";
  document.getElementById("sticks").textContent = "供托 " + view.riichi_sticks;
  document.getElementById("wall").textContent = "余 " + view.wall_remaining;
  document.getElementById("center-wind").textContent = ROUND_WIND_FACE[view.round_wind] || "?";
  document.getElementById("center-meta").textContent =
    `${view.round_number}局 ${view.honba}本场 · 剩余 ${view.wall_remaining} 张`;

  const doraBox = document.getElementById("dora-tiles");
  doraBox.innerHTML = "";
  view.dora_indicators.forEach((t) => doraBox.appendChild(tileEl(t, { small: true })));

  // Relative seat: 0 self, 1 right (plays next), 2 across, 3 left.
  const rel = (s) => (s - human + 4) % 4;
  const slotFor = { 0: "seat-bottom", 1: "seat-right", 2: "seat-top", 3: "seat-left" };
  for (let s = 0; s < 4; s++) {
    const p = view.players[s];
    const slot = document.getElementById(slotFor[rel(s)]);
    if (!slot) continue;
    if (rel(s) === 0) { renderSelf(slot, p, view); continue; }
    renderOpponent(slot, p, view, rel(s));
  }
  renderHand(view, human);
  renderActions();
}

function seatHead(p, view) {
  const head = document.createElement("div");
  head.className = "seat-head"
    + (p.is_dealer ? " dealer" : "")
    + (p.riichi ? " riichi" : "");
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
  head.appendChild(score);
  return head;
}

function renderOpponent(slot, p, view, rel) {
  slot.innerHTML = "";
  slot.appendChild(seatHead(p, view));

  const concealed = p.hand_count - 3 * (p.melds ? p.melds.length : 0);
  slot.appendChild(backRow(concealed, { vertical: view === "left" || view === "right" }));

  if (p.melds && p.melds.length) slot.appendChild(meldRow(p.melds, true));
  slot.appendChild(pond(p.discards, view));
}

function renderSelf(slot, p, view) {
  slot.innerHTML = "";
  slot.appendChild(seatHead(p, view));
  if (p.melds && p.melds.length) slot.appendChild(meldRow(p.melds, true));
  slot.appendChild(pond(p.discards, view));
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

function pond(discards, view) {
  const wrap = document.createElement("div");
  wrap.className = "pond";
  const last = discards.length - 1;
  discards.forEach((d, i) => {
    let extra = "";
    if (d.called_by !== null && d.called_by !== undefined) extra += " called";
    // 立直宣言牌 is laid sideways; that is the one convention a player reads the
    // table by, so it is worth the rotation.
    if (d.riichi) extra += " rot";
    if (i === last && last >= 0) extra += " fresh";
    wrap.appendChild(tileEl(d.tile, { small: true, extra }));
  });
  return wrap;
}

function renderHand(view, human) {
  const me = view.players[human];
  const handEl = document.getElementById("hand");
  handEl.innerHTML = "";
  const meldsEl = document.getElementById("melds-self");
  meldsEl.innerHTML = "";

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
      text += " · 听 " + me.waits.map((k) => tileName(k * 4)).join(" ");
    }
    info.textContent = text;
  } else {
    info.textContent = "";
  }
  document.getElementById("furiten-flag").classList.toggle("hidden", !me.furiten);
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

  const kanKinds = ["Ankan", "Kakan", "Minkan"];
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
    b.addEventListener("click", () => {
      riichiMode = !riichiMode;
      render();
    });
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
    return `${botNames[d.seat]} 摸牌${d.rinshan ? "（岭上）" : ""}`;
  }
  if (e.Discard) {
    const d = e.Discard;
    return `${botNames[d.seat]} 打出 <strong>${tileName(d.tile)}</strong>`
      + (d.riichi ? " 并立直" : "") + (d.tsumogiri ? "（摸切）" : "（手切）");
  }
  if (e.Riichi) return `<strong>${botNames[e.Riichi.seat]} 立直！</strong>`;
  if (e.Meld) {
    const m = e.Meld;
    const tiles = m.meld.tiles.slice(0, m.meld.len).map(tileName).join("");
    return `${botNames[m.seat]} ${meldKindName(m.meld.kind)} ${tiles}`;
  }
  if (e.Kan) {
    const k = e.Kan;
    return `${botNames[k.seat]} 杠 ${tileName(k.meld.tiles[0])}`
      + (k.dora_indicator !== null && k.dora_indicator !== undefined
        ? `（新宝牌指示牌 ${tileName(k.dora_indicator)}）` : "");
  }
  if (e.Win) {
    const w = e.Win;
    const how = w.from === null || w.from === undefined ? "自摸" : `荣和（放铳：${botNames[w.from]}）`;
    return `<strong>${botNames[w.seat]} ${how} ${tileName(w.tile)}</strong>`
      + ` · ${w.score.han}番${w.score.fu}符`;
  }
  if (e.Ryuukyoku) {
    const r = e.Ryuukyoku;
    const tenpai = r.tenpai.map((t, i) => (t ? botNames[i] : null)).filter(Boolean);
    return `<strong>${DRAW_REASONS[r.reason] || "流局"}</strong>`
      + (tenpai.length ? ` · 听牌：${tenpai.join("、")}` : "");
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
  const parts = (score.yaku || []).map(([y, h]) => `${YAKU_NAMES[y] || y}${h > 0 ? " " + h + "番" : ""}`);
  return parts.join("、");
}

function showWin(w) {
  const title = w.from === null || w.from === undefined ? "自摸！" : "荣和！";
  let body = `<p><span class="win">${botNames[w.seat]}</span> 和了 ${tileName(w.tile)}</p>`;
  if (scoreLine(w.score)) body += `<p>${scoreLine(w.score)}</p>`;
  body += `<p>${w.score.han} 番 ${w.score.fu} 符`
    + (w.score.yakuman ? ` · 役满 ×${w.score.yakuman}` : "")
    + (w.score.is_dealer ? " · 庄家" : "") + `</p>`;
  body += scoreTable(w.deltas, w.seat);
  overlay(title, body);
}

function showRyuukyoku(r) {
  let body = `<p>${DRAW_REASONS[r.reason] || "流局"}</p>`;
  body += `<p>听牌：${r.tenpai.map((t, i) => `${botNames[i]}${t ? " ○" : " ×"}`).join("　")}</p>`;
  if (r.deltas && r.deltas.some((d) => d !== 0)) body += scoreTable(r.deltas, -1);
  overlay("流局", body);
}

function scoreTable(deltas, winner) {
  if (!deltas) return "";
  const rows = deltas.map((d, i) =>
    `<tr><td>${botNames[i]}</td><td>${d > 0 ? "+" : ""}${d}</td></tr>`).join("");
  return `<table><tr><th>玩家</th><th>点数增减</th></tr>${rows}</table>`;
}

function showGameEnd(msg) {
  const rows = msg.ranking.map((seat, place) =>
    `<tr><td>${place + 1} 位</td><td>${botNames[seat]}</td><td>${msg.scores[seat]}</td></tr>`).join("");
  let body = `<p>共 ${msg.rounds} 局</p>`;
  body += `<table><tr><th>名次</th><th>玩家</th><th>终局点数</th></tr>${rows}</table>`;
  if (msg.replay) body += `<p style="opacity:.7">牌谱已保存：${msg.replay}</p>`;
  overlay("对局结束", body);
}

// ---------------------------------------------------------------- ui bits

function overlay(title, bodyHtml) {
  document.getElementById("overlay-title").textContent = title;
  document.getElementById("overlay-body").innerHTML = bodyHtml;
  document.getElementById("overlay").classList.remove("hidden");
}

function showHint(text) {
  const box = document.getElementById("hint-box");
  box.textContent = text;
  box.classList.remove("hidden");
  clearTimeout(box._timer);
  box._timer = setTimeout(() => box.classList.add("hidden"), 12000);
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
    send({
      type: "new_game",
      seat: parseInt(document.getElementById("sel-seat").value, 10),
      length: document.getElementById("sel-length").value,
      bot: document.getElementById("sel-bot").value,
    });
  });
  document.getElementById("btn-hint").addEventListener("click", () => send({ type: "hint" }));
  document.getElementById("btn-replays").addEventListener("click", openReplayPanel);
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
  connect();
});
