// mmoyager training dashboard.
//
// Polls /api/training and renders it. Nothing here computes training numbers —
// the loop writes data/training_state.json and this page only draws it, so the
// dashboard and the terminal can never tell different stories.

const POLL_MS = 4000;

const $ = (id) => document.getElementById(id);

let timer = null;
let lastIteration = null;

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

  renderChart(st.iterations || []);
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

    const verdict = r.promoted
      ? '<span class="pill ok">提升</span>'
      : (r.partial
        ? '<span class="pill early">提前停止</span>'
        : '<span class="pill no">拒绝</span>');

    const replicas = (r.replicas || []).map((x) => Math.round(x)).join(" / ");
    tr.innerHTML =
      "<td class='num'>" + (r.iteration ?? "") + "</td>" +
      "<td>" + (r.recipe || "—") + "</td>" +
      "<td class='num'>" + fmtScore(r.score) + "</td>" +
      "<td class='num'>" + (replicas || "—") + "</td>" +
      "<td class='num'>" + fmtDuration(r.seconds) + "</td>" +
      "<td>" + verdict + "</td>" +
      "<td class='mono'>" + shortPath(r.checkpoint) + "</td>" +
      "<td class='note'>" + (r.note || "") + "</td>";
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

/// A sparkline of the per-iteration score, with the accept bar drawn in.
function renderChart(rows) {
  const host = $("chart");
  const pts = rows.filter((r) => typeof r.score === "number");
  if (pts.length < 2) {
    host.innerHTML = '<p class="muted">至少需要两轮迭代才能画趋势。</p>';
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
  // the accept bar (score must be >= 60 under --accept-on direct)
  svg += '<line class="zero-line" x1="' + PAD + '" x2="' + (W - PAD) + '" y1="' + y(60) + '" y2="' + y(60) + '"/>';
  svg += '<text class="axis-text" x="' + (W - PAD - 62) + '" y="' + (y(60) - 5) + '">门槛 +60</text>';
  svg += '<line class="zero-line" x1="' + PAD + '" x2="' + (W - PAD) + '" y1="' + y(0) + '" y2="' + y(0) + '"/>';

  const path = pts.map((p, i) => (i ? "L" : "M") + x(i) + " " + y(p.score)).join(" ");
  svg += '<path d="' + path + '" fill="none" stroke="#9dc4ae" stroke-width="2" opacity="0.85"/>';

  pts.forEach((p, i) => {
    const color = p.promoted ? "#57d68a" : (p.partial ? "transparent" : "#ff8f8f");
    const stroke = p.partial ? "#ffc46b" : "#0b3221";
    svg += '<circle class="pt" cx="' + x(i) + '" cy="' + y(p.score) + '" r="4.5" fill="' + color +
      '" stroke="' + stroke + '" stroke-width="2"><title>第 ' + (p.iteration ?? i) + " 轮 · " +
      (p.recipe || "") + " · " + fmtScore(p.score) + (p.promoted ? " · 提升" : "") + "</title></circle>";
  });
  svg += "</svg>";
  host.innerHTML = svg;
}

// ---------------------------------------------------------------- networking

async function refresh() {
  try {
    const res = await fetch("/api/training", { cache: "no-store" });
    const data = await res.json();
    renderStatus(data);
    $("ctl-msg").textContent = "";
  } catch (e) {
    $("run-badge").textContent = "连不上服务";
    $("run-badge").className = "badge stopped";
  }
}

async function control(action) {
  const labels = { start: "启动", stop: "停止", restart: "重启" };
  $("ctl-msg").textContent = (labels[action] || action) + "中…";
  try {
    const res = await fetch("/api/training/control", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action }),
    });
    const data = await res.json();
    $("ctl-msg").textContent = data.message || (data.ok ? "完成" : "失败");
  } catch (e) {
    $("ctl-msg").textContent = "请求失败：" + e;
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
