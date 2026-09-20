// mmoyager training dashboard.
//
// Polls /api/training and renders it. Nothing here computes training numbers —
// the loop writes data/training_state.json and this page only draws it, so the
// dashboard and the terminal can never tell different stories.

const POLL_MS = 4000;

const $ = (id) => document.getElementById(id);

let timer = null;
let lastIteration = null;

/// Escape data that is about to go into innerHTML: recipes, notes and file
/// paths are written by the loop rather than by this page.
function esc(value) {
  return String(value === null || value === undefined ? "" : value)
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}

// ---------------------------------------------------------------- formatting

function fmtScore(v) {
  if (v === null || v === undefined) return "—";
  const sign = v > 0 ? "+" : "";
  return sign + Math.round(v);
}

function fmtDuration(secs) {
  if (!secs && secs !== 0) return "—";
  const s = Math.round(secs);
  if (s < 90) return s + " s";
  const m = Math.floor(s / 60);
  if (m < 90) return m + " 分 " + (s % 60) + " s";
  const h = Math.floor(m / 60);
  return h + " 时 " + (m % 60) + " 分";
}

function fmtQuiet(secs) {
  if (secs === null || secs === undefined) return "—";
  if (secs < 60) return secs + " 秒前";
  if (secs < 3600) return Math.floor(secs / 60) + " 分钟前";
  return Math.floor(secs / 3600) + " 小时前";
}

function fmtTime(unix) {
  if (!unix) return "";
  const d = new Date(unix * 1000);
  return d.toLocaleTimeString();
}

function shortPath(p) {
  if (!p) return "—";
  const parts = String(p).split("/");
  return parts[parts.length - 1];
}

// ---------------------------------------------------------------- rendering

function renderStatus(data) {
  const st = data.state || {};
  const running = !!data.running;
  const quiet = data.quiet_secs;

  const badge = $("run-badge");
  badge.textContent = running ? "训练中" : "已停止";
  badge.className = "badge " + (running ? "running" : "stopped");

  $("k-pids").textContent = running ? (data.pids || []).join(", ") : "无";
  $("k-iter").textContent = st.iteration === null || st.iteration === undefined
    ? "—" : "第 " + st.iteration + " 轮";
  $("k-quiet").textContent = running ? fmtQuiet(quiet) : "—";
  $("k-log").textContent = data.log_file || "—";
  $("k-log").title = data.log_file || "";

  $("k-best").textContent = st.best || "（还没有权重）";
  $("k-abs").textContent = fmtScore(st.best_absolute);
  const guards = st.guards || {};
  const g = guards.efficiency;
  $("k-guard").textContent = fmtScore(g);
  const n = (st.iterations || []).length;
  const promos = (st.iterations || []).filter((i) => i.promoted).length;
  $("k-promos").textContent = n + " / " + promos;

  $("btn-start").disabled = running;
  $("btn-stop").disabled = !running;
  $("cmd").textContent = data.command || "";
  $("log-path").textContent = data.log_file || "";

  // The history mixes two scales: the current loop stores a margin over the
  // incumbent (a few hundred points at most), older rows stored an absolute
  // average near 25 000. Plotting both together flattened the real trend into a
  // single line, so each row is labelled with its own scale.
  const rows = (st.iterations || []).map((r) =>
    Object.assign({}, r, { scale: rowScale(r.score) }));
  const scored = rows.filter((r) => r.scale);
  const scale = scored.length && scored[scored.length - 1].scale === "absolute"
    ? "absolute" : "margin";
  renderChart(rows, scale);
  renderHistory(st.iterations || []);
  renderLog(data.log_file, data.log_tail || []);

  // A loop that has stopped writing for a while is the interesting case: say so
  // loudly rather than showing a comforting "训练中".
  if (running && quiet !== null && quiet !== undefined && quiet > 900) {
    badge.textContent = "训练中（日志静默）";
    badge.className = "badge stale";
  }
}

function renderHistory(rows) {
  const body = $("hist-body");
  body.innerHTML = "";
  const recent = rows.slice(-60).reverse();
  for (const r of recent) {
    const tr = document.createElement("tr");
    if (r.promoted) tr.className = "promoted";

    // A row with no score was never evaluated (older protocols recorded a
    // stage without one); calling that "拒绝" reads as a measured rejection.
    const verdict = typeof r.score !== "number"
      ? '<span class="pill wait">未评测</span>'
      : (r.promoted
        ? '<span class="pill ok">提升</span>'
        : (r.partial
          ? '<span class="pill early">提前停止</span>'
          : '<span class="pill no">拒绝</span>'));

    const replicas = (r.replicas || []).map((x) => Math.round(x)).join(" / ");
    // Recipes, notes and paths come from the loop's state file, so they are
    // escaped rather than trusted as markup.
    tr.innerHTML =
      "<td class='num'>" + esc(r.iteration ?? "") + "</td>" +
      "<td>" + esc(r.recipe || "—") + "</td>" +
      "<td class='num'>" + fmtScore(r.score) + "</td>" +
      "<td class='num'>" + esc(replicas || "—") + "</td>" +
      "<td class='num'>" + fmtDuration(r.seconds) + "</td>" +
      "<td>" + verdict + "</td>" +
      "<td class='mono'>" + esc(shortPath(r.checkpoint)) + "</td>" +
      "<td class='note'>" + esc(r.note || "") + "</td>";
    body.appendChild(tr);
  }
  $("hist-note").textContent = rows.length
    ? "共 " + rows.length + " 轮，显示最近 " + Math.min(60, rows.length) + " 轮"
    : "还没有迭代记录";
}

function renderLog(path, lines) {
  const el = $("log");
  const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 40;
  el.textContent = lines.length ? lines.join("\n") : "（暂无日志）";
  if (atBottom) el.scrollTop = el.scrollHeight;
}

/// Which scale a history row is on: `null` when it was never evaluated,
/// `"absolute"` for an average score near the starting 25 000, `"margin"` for
/// the current protocol's difference against the incumbent.
function rowScale(score) {
  if (typeof score !== "number") return null;
  return Math.abs(score) > 1000 ? "absolute" : "margin";
}

/// A sparkline of the per-iteration score, with the accept bar drawn in.
///
/// The history mixes two protocols: early rows stored an *absolute* average
/// score (about 24 700-25 600) and the current loop stores a *margin* over the
/// incumbent (-400..+400). Plotting both on one axis squashed the real trend
/// into a single pixel, so only the rows that share the current scale are
/// plotted, and the dropped ones are named on the panel.
function renderChart(rows, scale) {
  const host = $("chart");
  const all = rows.filter((r) => typeof r.score === "number");
  const pts = all.filter((r) => r.scale === scale);
  const dropped = all.length - pts.length;
  if (pts.length < 2) {
    host.innerHTML = '<p class="muted">至少需要两轮同尺度的迭代才能画趋势（当前尺度：'
      + esc(scale) + "）。</p>";
    return;
  }
  const W = 1000, H = 220, PAD = 34;
  const scores = pts.map((p) => p.score);
  let lo = Math.min(...scores, 0), hi = Math.max(...scores, 0);
  const pad = Math.max(30, (hi - lo) * 0.15);
  lo -= pad; hi += pad;
  const x = (i) => PAD + (i * (W - 2 * PAD)) / Math.max(1, pts.length - 1);
  const y = (v) => H - PAD - ((v - lo) * (H - 2 * PAD)) / Math.max(1e-9, hi - lo);

  let svg = '<svg viewBox="0 0 ' + W + " " + H + '" preserveAspectRatio="xMidYMid meet">';
  // horizontal grid
  for (let k = 0; k <= 4; k++) {
    const v = lo + ((hi - lo) * k) / 4;
    svg += '<line class="grid-line" x1="' + PAD + '" x2="' + (W - PAD) + '" y1="' + y(v) + '" y2="' + y(v) + '"/>';
    svg += '<text class="axis-text" x="4" y="' + (y(v) + 4) + '">' + Math.round(v) + "</text>";
  }
  if (scale === "margin") {
    // The accept bar: a candidate needs a margin of at least +60 to be kept.
    svg += '<line class="zero-line" x1="' + PAD + '" x2="' + (W - PAD) + '" y1="' + y(60) + '" y2="' + y(60) + '"/>';
    svg += '<text class="axis-text" x="' + (W - PAD - 62) + '" y="' + (y(60) - 5) + '">门槛 +60</text>';
  }
  svg += '<line class="zero-line" x1="' + PAD + '" x2="' + (W - PAD) + '" y1="' + y(0) + '" y2="' + y(0) + '"/>';

  const path = pts.map((p, i) => (i ? "L" : "M") + x(i) + " " + y(p.score)).join(" ");
  svg += '<path d="' + path + '" fill="none" stroke="#9dc4ae" stroke-width="2" opacity="0.85"/>';

  pts.forEach((p, i) => {
    const color = p.promoted ? "#57d68a" : (p.partial ? "transparent" : "#ff8f8f");
    const stroke = p.partial ? "#ffc46b" : "#0b3221";
    svg += '<circle class="pt" cx="' + x(i) + '" cy="' + y(p.score) + '" r="4.5" fill="' + color +
      '" stroke="' + stroke + '" stroke-width="2"><title>第 ' + esc(p.iteration ?? i) + " 轮 · " +
      esc(p.recipe || "") + " · " + fmtScore(p.score) + (p.promoted ? " · 提升" : "") + "</title></circle>";
  });
  svg += "</svg>";
  host.innerHTML = svg;
  const note = $("trend-note");
  if (note) {
    note.textContent = `每点是一轮已评测的迭代（绿=被采纳，灰=未过门槛，空心=提前停止）`
      + `；尺度：${scale === "margin" ? "相对在位者的分差（--accept-on direct）" : "绝对平均分"}`
      + `，共 ${pts.length} 轮`
      + (dropped ? `，另有 ${dropped} 轮属于旧协议的另一种尺度，未画入` : "");
  }
}

// ---------------------------------------------------------------- networking

async function refresh() {
  try {
    const res = await fetch("/api/training", { cache: "no-store" });
    const data = await res.json();
    renderStatus(data);
    // A control message is the answer to something the operator just did, so it
    // stays until it is replaced or dismissed rather than being wiped by the
    // next poll.
    expireCtlMessage();
  } catch (e) {
    $("run-badge").textContent = "连不上服务";
    $("run-badge").className = "badge stopped";
  }
}

// Control feedback lives for a minute, so a fast poll cannot swallow it.
let ctlShownAt = 0;

function setCtlMessage(text) {
  $("ctl-msg").textContent = text;
  ctlShownAt = text ? Date.now() : 0;
}

function expireCtlMessage() {
  const el = $("ctl-msg");
  if (!el.textContent) return;
  if (Date.now() - ctlShownAt > 60000) {
    el.textContent = "";
    ctlShownAt = 0;
  }
}

async function control(action) {
  const labels = { start: "启动", stop: "停止", restart: "重启" };
  setCtlMessage((labels[action] || action) + "中…");
  try {
    const res = await fetch("/api/training/control", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action }),
    });
    const data = await res.json();
    setCtlMessage(data.message || (data.ok ? "完成" : "失败"));
  } catch (e) {
    setCtlMessage("请求失败：" + e);
  }
  setTimeout(refresh, 800);
}

function setAuto(on) {
  if (timer) { clearInterval(timer); timer = null; }
  if (on) timer = setInterval(refresh, POLL_MS);
}

$("btn-refresh").addEventListener("click", refresh);
$("btn-start").addEventListener("click", () => control("start"));
$("btn-stop").addEventListener("click", () => control("stop"));
$("btn-restart").addEventListener("click", () => control("restart"));
$("auto").addEventListener("change", (e) => setAuto(e.target.checked));

refresh();
setAuto(true);
