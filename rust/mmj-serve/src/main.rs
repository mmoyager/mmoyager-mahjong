//! `mmj-serve` — the local web app: play against three bots in a browser.
//!
//! The whole application is one process and one binary: it serves the UI
//! (embedded at compile time, so there is nothing to install), runs the rules
//! engine and the bots, and speaks a small JSON protocol over one WebSocket per
//! game. Run it with `cargo run --release -p mmj-serve` and open the printed URL.

mod training;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::extract::State;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use mmj_ai::{Agent, EfficiencyAgent, NnAgent, RandomAgent};
use mmj_core::action::Action;
use mmj_core::rules::{GameLength, Rules};
use mmj_core::state::{Event, Table, TableConfig};
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_JS: &str = include_str!("../../../web/app.js");
const STYLE_CSS: &str = include_str!("../../../web/style.css");
const DASHBOARD_HTML: &str = include_str!("../../../web/dashboard.html");
const DASHBOARD_JS: &str = include_str!("../../../web/dashboard.js");
const DASHBOARD_CSS: &str = include_str!("../../../web/dashboard.css");
/// The public-domain tile artwork, embedded one file at a time so the binary
/// stays self-contained. Kept as a table rather than a directory scan because
/// `include_bytes!` needs a literal path.
const TILE_SVGS: &[(&str, &[u8])] = &[
    ("Back.svg", include_bytes!("../../../web/tiles/Back.svg")),
    ("Front.svg", include_bytes!("../../../web/tiles/Front.svg")),
    ("Blank.svg", include_bytes!("../../../web/tiles/Blank.svg")),
    ("Ton.svg", include_bytes!("../../../web/tiles/Ton.svg")),
    ("Nan.svg", include_bytes!("../../../web/tiles/Nan.svg")),
    ("Shaa.svg", include_bytes!("../../../web/tiles/Shaa.svg")),
    ("Pei.svg", include_bytes!("../../../web/tiles/Pei.svg")),
    ("Haku.svg", include_bytes!("../../../web/tiles/Haku.svg")),
    ("Hatsu.svg", include_bytes!("../../../web/tiles/Hatsu.svg")),
    ("Chun.svg", include_bytes!("../../../web/tiles/Chun.svg")),
    ("Man1.svg", include_bytes!("../../../web/tiles/Man1.svg")),
    ("Man2.svg", include_bytes!("../../../web/tiles/Man2.svg")),
    ("Man3.svg", include_bytes!("../../../web/tiles/Man3.svg")),
    ("Man4.svg", include_bytes!("../../../web/tiles/Man4.svg")),
    ("Man5.svg", include_bytes!("../../../web/tiles/Man5.svg")),
    ("Man5-Dora.svg", include_bytes!("../../../web/tiles/Man5-Dora.svg")),
    ("Man6.svg", include_bytes!("../../../web/tiles/Man6.svg")),
    ("Man7.svg", include_bytes!("../../../web/tiles/Man7.svg")),
    ("Man8.svg", include_bytes!("../../../web/tiles/Man8.svg")),
    ("Man9.svg", include_bytes!("../../../web/tiles/Man9.svg")),
    ("Pin1.svg", include_bytes!("../../../web/tiles/Pin1.svg")),
    ("Pin2.svg", include_bytes!("../../../web/tiles/Pin2.svg")),
    ("Pin3.svg", include_bytes!("../../../web/tiles/Pin3.svg")),
    ("Pin4.svg", include_bytes!("../../../web/tiles/Pin4.svg")),
    ("Pin5.svg", include_bytes!("../../../web/tiles/Pin5.svg")),
    ("Pin5-Dora.svg", include_bytes!("../../../web/tiles/Pin5-Dora.svg")),
    ("Pin6.svg", include_bytes!("../../../web/tiles/Pin6.svg")),
    ("Pin7.svg", include_bytes!("../../../web/tiles/Pin7.svg")),
    ("Pin8.svg", include_bytes!("../../../web/tiles/Pin8.svg")),
    ("Pin9.svg", include_bytes!("../../../web/tiles/Pin9.svg")),
    ("Sou1.svg", include_bytes!("../../../web/tiles/Sou1.svg")),
    ("Sou2.svg", include_bytes!("../../../web/tiles/Sou2.svg")),
    ("Sou3.svg", include_bytes!("../../../web/tiles/Sou3.svg")),
    ("Sou4.svg", include_bytes!("../../../web/tiles/Sou4.svg")),
    ("Sou5.svg", include_bytes!("../../../web/tiles/Sou5.svg")),
    ("Sou5-Dora.svg", include_bytes!("../../../web/tiles/Sou5-Dora.svg")),
    ("Sou6.svg", include_bytes!("../../../web/tiles/Sou6.svg")),
    ("Sou7.svg", include_bytes!("../../../web/tiles/Sou7.svg")),
    ("Sou8.svg", include_bytes!("../../../web/tiles/Sou8.svg")),
    ("Sou9.svg", include_bytes!("../../../web/tiles/Sou9.svg")),
];

/// Which kind of bot fills the three seats the human does not take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BotKind {
    /// Tile-efficiency baseline.
    Efficiency,
    /// A trained policy/value network.
    Nn,
    /// Uniformly random — useful for sanity-checking the UI.
    Random,
}

impl Default for BotKind {
    fn default() -> Self {
        BotKind::Efficiency
    }
}

/// Where the trained checkpoints live, and which one the UI should use.
#[derive(Clone, Debug, Default)]
struct CheckpointSource {
    explicit: Option<PathBuf>,
}

impl CheckpointSource {
    /// Resolve the checkpoint to play with: `--checkpoint` when given, else the
    /// best one recorded by the training loop.
    fn resolve(&self) -> Option<PathBuf> {
        if let Some(p) = &self.explicit {
            return if p.exists() { Some(p.clone()) } else { None };
        }
        let state = Path::new("data/training_state.json");
        let text = std::fs::read_to_string(state).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let best = value.get("best")?.as_str()?;
        let path = PathBuf::from(best);
        if path.exists() {
            Some(path)
        } else {
            // Fall back to the newest checkpoint on disk.
            let mut candidates: Vec<PathBuf> = std::fs::read_dir("data/checkpoints")
                .ok()?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|e| e == "bin").unwrap_or(false))
                .collect();
            candidates.sort();
            candidates.pop()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut port = 8787u16;
    let mut checkpoint: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" | "-p" => {
                if let Some(v) = args.next() {
                    port = v.parse().unwrap_or(port);
                }
            }
            "--checkpoint" | "-c" => {
                checkpoint = args.next().map(PathBuf::from);
            }
            "--help" | "-h" => {
                println!("mmj-serve [--port <port>] [--checkpoint <file>]");
                return Ok(());
            }
            other => eprintln!("unknown argument: {}", other),
        }
    }

    let checkpoints = CheckpointSource {
        explicit: checkpoint.clone(),
    };
    let resolved = checkpoints.resolve();
    match &resolved {
        Some(p) => println!("  nn bot      {}", p.display()),
        None => println!("  nn bot      (no checkpoint yet — train one with python/trainer/loop.py)"),
    }
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/tiles/{name}", get(tile_svg))
        .route("/ws", get(ws_handler))
        .route("/api/replays", get(list_replays))
        .route("/api/analyze", get(analyze_replay))
        // The training dashboard: state, history, log tail and start/stop.
        .route("/dashboard", get(dashboard))
        .route("/dashboard.js", get(dashboard_js))
        .route("/dashboard.css", get(dashboard_css))
        .route("/api/training", get(training::status))
        .route("/api/training/control", axum::routing::post(training::control))

        .with_state(checkpoints);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("mmoyager mahjong");
    println!("  playing at  http://{}", addr);
    println!("  training at http://{}/dashboard", addr);
    println!("  stop with   Ctrl-C");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

/// One tile SVG out of the embedded set.
async fn tile_svg(axum::extract::Path(name): axum::extract::Path<String>) -> Response {
    let headers = [
        (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
        (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
    ];
    match TILE_SVGS.iter().find(|(n, _)| *n == name) {
        Some((_, bytes)) => (headers, *bytes).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "no such tile").into_response(),
    }
}

async fn dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

async fn dashboard_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        DASHBOARD_JS,
    )
}

async fn dashboard_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css")],
        DASHBOARD_CSS,
    )
}

/// Saved replays, newest first.
async fn list_replays() -> impl IntoResponse {
    let mut out: Vec<Value> = Vec::new();
    if let Ok(dir) = std::fs::read_dir("data/replays") {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            let meta = entry.metadata().ok();
            out.push(json!({
                "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                "bytes": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                "modified": meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            }));
        }
    }
    out.sort_by(|a, b| b["modified"].as_u64().cmp(&a["modified"].as_u64()));
    axum::Json(json!({ "replays": out }))
}

#[derive(Deserialize)]
struct AnalyzeQuery {
    name: String,
    #[serde(default)]
    seat: Option<u8>,
}

/// Rebuild and analyse one replay with the current best checkpoint.
async fn analyze_replay(
    State(checkpoints): State<CheckpointSource>,
    axum::extract::Query(query): axum::extract::Query<AnalyzeQuery>,
) -> Response {
    // Only files inside data/replays may be read.
    let safe = !query.name.contains('/') && !query.name.contains('\\') && !query.name.contains("..");
    if !safe || !query.name.ends_with(".json") {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid replay name").into_response();
    }
    let path = Path::new("data/replays").join(&query.name);
    let checkpoint = checkpoints.resolve();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, String> {
        let file = mmj_ai::replay::ReplayFile::load(&path)?;
        let seat = query.seat.unwrap_or(file.human).min(3);
        let mut agent = match &checkpoint {
            Some(p) => mmj_ai::NnAgent::from_checkpoint(p, 7, false).ok(),
            None => None,
        };
        let analysis = mmj_ai::replay::analyze(&file, seat, &mut agent)?;
        serde_json::to_value(analysis).map_err(|e| e.to_string())
    })
    .await;
    match result {
        Ok(Ok(value)) => axum::Json(value).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("analysis task failed: {}", e),
        )
            .into_response(),
    }
}

async fn app_js() -> impl IntoResponse {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        APP_JS,
    )
}

async fn style_css() -> impl IntoResponse {
    ([("content-type", "text/css; charset=utf-8")], STYLE_CSS)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(checkpoints): State<CheckpointSource>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, checkpoints))
}

/// One game session, driven over a single WebSocket.
struct Session {
    table: Table,
    agents: [Box<dyn Agent>; 4],
    human: u8,
    seed: u64,
}

fn make_agent(
    kind: BotKind,
    index: usize,
    seed: u64,
    checkpoints: &CheckpointSource,
) -> Box<dyn Agent> {
    match kind {
        BotKind::Efficiency => Box::new(EfficiencyAgent::new(format!("牌效率 AI {}", index))),
        BotKind::Random => Box::new(RandomAgent::new(seed.wrapping_add(index as u64))),
        BotKind::Nn => match checkpoints.resolve() {
            Some(path) => match NnAgent::from_checkpoint_labeled(
                &path,
                seed.wrapping_add(index as u64),
                false,
                Some(format!("神经网络 AI {}", index)),
            ) {
                Ok(agent) => Box::new(agent),
                Err(e) => {
                    eprintln!("cannot load {}: {}", path.display(), e);
                    Box::new(EfficiencyAgent::new(format!("牌效率 AI {}", index)))
                }
            },
            None => Box::new(EfficiencyAgent::new(format!("牌效率 AI {}", index))),
        },
    }
}

impl Session {
    fn new(
        seat: u8,
        length: GameLength,
        kind: BotKind,
        seed: u64,
        checkpoints: &CheckpointSource,
    ) -> Self {
        let mut rules = Rules::tenhou();
        rules.length = length;
        let table = Table::new(TableConfig { rules, seed });
        let mut agents: [Box<dyn Agent>; 4] = [
            Box::new(RandomAgent::new(0)),
            Box::new(RandomAgent::new(1)),
            Box::new(RandomAgent::new(2)),
            Box::new(RandomAgent::new(3)),
        ];
        for (i, slot) in agents.iter_mut().enumerate() {
            if i as u8 != seat {
                *slot = make_agent(kind, i, seed, checkpoints);
            }
        }
        Session {
            table,
            agents,
            human: seat,
            seed,
        }
    }

    /// Let every bot act until the human must decide, or the match ends.
    fn advance(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > 100_000 || self.table.finished {
                break;
            }
            let decisions = self.table.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            let mut acted = false;
            for d in decisions {
                let action = if d.seat == self.human {
                    if d.actions.len() > 1 {
                        continue; // wait for the player
                    }
                    // A single legal option needs no input, e.g. the forced
                    // tsumogiri of a riichi hand.
                    d.actions[0]
                } else {
                    self.agents[d.seat as usize].act(&self.table, d.seat, &d)
                };
                match self.table.submit(d.seat, action) {
                    Ok(ev) => {
                        events.extend(ev);
                        acted = true;
                    }
                    Err(e) => {
                        eprintln!("internal error: {}", e);
                        return events;
                    }
                }
                if self.table.finished {
                    break;
                }
            }
            if !acted {
                break;
            }
        }
        events
    }

    /// The human's pending decision, if any.
    fn human_decision(&self) -> Option<Value> {
        self.table
            .decisions()
            .iter()
            .find(|d| d.seat == self.human)
            .map(|d| serde_json::to_value(d).unwrap_or(Value::Null))
    }

    fn state_message(&self) -> Value {
        let view = self.table.view(self.human);
        let bot_names: Vec<String> = (0..4)
            .map(|s| {
                if s == self.human as usize {
                    "你".to_string()
                } else {
                    self.agents[s].name()
                }
            })
            .collect();
        json!({
            "type": "state",
            "view": view,
            "decision": self.human_decision(),
            "botNames": bot_names,
            "human": self.human,
            "seed": self.seed,
        })
    }

    fn result_message(&self) -> Value {
        let scores = self.table.scores();
        let mut order: Vec<(i32, u8)> = (0..4).map(|s| (scores[s as usize], s as u8)).collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        json!({
            "type": "game_end",
            "scores": scores,
            "ranking": order.iter().map(|&(_, s)| s).collect::<Vec<u8>>(),
            "rounds": self.table.rounds_played,
        })
    }

    /// Save the finished match under `data/replays`.
    fn save_replay(&self) -> Option<PathBuf> {
        let dir = Path::new("data/replays");
        if std::fs::create_dir_all(dir).is_err() {
            return None;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("{}-{}.json", stamp, self.seed));
        let payload = json!({
            "seed": self.seed,
            "human": self.human,
            "hanchan": self.table.rules.length == GameLength::Hanchan,
            "score": self.table.scores(),
            "scores": self.table.scores(),
            "events": self.table.history,
        });
        match std::fs::write(&path, serde_json::to_vec_pretty(&payload).unwrap_or_default()) {
            Ok(()) => Some(path),
            Err(_) => None,
        }
    }
}

/// A hint request: what the network would play (with its own probabilities),
/// what the tile-efficiency baseline would play, and the hand's shape.
///
/// The two opinions are shown side by side because they disagree often and the
/// disagreement is the interesting part: the network is what the project actually
/// trained, the baseline is the yardstick every strength number is quoted
/// against. The value estimate carries an explicit caveat, because the honest
/// R² of that head is about 0.11.
fn hint_message(session: &Session, checkpoints: &CheckpointSource) -> Value {
    let seat = session.human;
    let Some(d) = session
        .table
        .decisions()
        .iter()
        .find(|d| d.seat == seat)
        .cloned()
    else {
        return json!({ "type": "hint", "text": "现在没有需要你决策的地方。" });
    };
    let mut agent = EfficiencyAgent::new("hint");
    let action = agent.act(&session.table, seat, &d);
    let player = &session.table.players[seat as usize];
    let melds = player.melds.len() as u8;
    let mut text = format!("推荐：{}", action.label());

    // What the trained network makes of the same position.
    let net_report = match checkpoints.resolve() {
        Some(path) => match NnAgent::from_checkpoint_labeled(&path, 7, false, None) {
            Ok(mut nn) => {
                let (dist, value) = nn.evaluate(&session.table, seat, &d);
                let mut ranked: Vec<(String, f32)> = dist
                    .iter()
                    .map(|(a, p)| (a.label(), *p))
                    .collect();
                ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                ranked.truncate(5);
                json!({
                    "checkpoint": path.file_name().map(|n| n.to_string_lossy().to_string()),
                    "value": value,
                    "top": ranked
                        .iter()
                        .map(|(label, p)| json!({ "label": label, "prob": p }))
                        .collect::<Vec<_>>(),
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        },
        None => Value::Null,
    };

    let mut shape = Value::Null;
    if let Action::Discard { tile, .. } = action {
        let k = mmj_core::tile::kind_of(tile);
        let mut rest = player.hand;
        rest[k as usize] = rest[k as usize].saturating_sub(1);
        let before = mmj_core::hand::shanten(&player.hand, melds);
        let after = mmj_core::hand::shanten(&rest, melds);
        let visible = session.table.visible_counts(seat);
        let uke: u32 = mmj_core::hand::useful_kinds(&rest, melds, &visible)
            .iter()
            .map(|&(_, n)| n as u32)
            .sum();
        let waits = mmj_core::hand::winning_kinds(&rest, melds);
        let mut detail = String::new();
        for w in waits {
            let live = 4u32.saturating_sub(visible[w as usize] as u32);
            detail.push_str(&format!("{}×{} ", mmj_core::tile::kind_name(w), live));
        }
        text.push_str(&format!("\n向听 {} → {}，进张 {} 张", before, after, uke));
        if !detail.is_empty() {
            text.push_str(&format!("\n听牌：{}", detail.trim_end()));
        }
        shape = json!({
            "before": before,
            "after": after,
            "ukeire": uke,
            "waits": detail.trim_end(),
        });
    }
    json!({
        "type": "hint",
        "text": text,
        "shape": shape,
        "baseline": action.label(),
        "net": net_report,
        "caveat": "价值头的真实拟合度只有 R²≈0.11（预测标准差约目标的 36%），期望得失只能当方向参考。",
    })
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    NewGame {
        seat: Option<u8>,
        #[serde(default)]
        length: Option<String>,
        #[serde(default)]
        bot: Option<BotKind>,
        #[serde(default)]
        seed: Option<u64>,
    },
    Action {
        action: Action,
    },
    Hint,
}

async fn handle_socket(socket: WebSocket, checkpoints: CheckpointSource) {
    let (mut sender, mut receiver) = socket.split();

    macro_rules! send_json {
        ($v:expr) => {{
            let text = serde_json::to_string(&$v).unwrap_or_else(|_| "{}".to_string());
            if sender.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }};
    }

    // Start a default game immediately so the page is usable on load.
    let mut session: Option<Session> = {
        let seed = rand::thread_rng().gen::<u64>();
        let mut s = Session::new(0, GameLength::Tonpuu, BotKind::Efficiency, seed, &checkpoints);
        s.advance();
        let state = s.state_message();
        send_json!(state);
        Some(s)
    };

    while let Some(Ok(msg)) = receiver.next().await {
        let Message::Text(text) = msg else {
            if matches!(msg, Message::Close(_)) {
                break;
            }
            continue;
        };
        let msg = match serde_json::from_str::<ClientMsg>(text.as_str()) {
            Ok(m) => m,
            Err(e) => {
                send_json!(json!({ "type": "error", "message": format!("无法解析消息: {}", e) }));
                continue;
            }
        };

        match msg {
            ClientMsg::NewGame {
                seat,
                length,
                bot,
                seed,
            } => {
                let length = match length.as_deref() {
                    Some("hanchan") => GameLength::Hanchan,
                    _ => GameLength::Tonpuu,
                };
                if let Some(prev) = session.take() {
                    if prev.table.rounds_played > 1 {
                        prev.save_replay();
                    }
                }
                let seat = seat.unwrap_or(0).min(3);
                let seed = seed.unwrap_or_else(|| rand::thread_rng().gen::<u64>());
                let mut s = Session::new(seat, length, bot.unwrap_or_default(), seed, &checkpoints);
                s.advance();
                let state = s.state_message();
                session = Some(s);
                send_json!(state);
            }
            ClientMsg::Action { action } => {
                let Some(s) = session.as_mut() else { continue };
                if s.table.finished {
                    send_json!(json!({ "type": "error", "message": "本局已结束，请开新局。" }));
                    send_json!(s.state_message());
                    continue;
                }
                if let Err(e) = s.table.submit(s.human, action) {
                    // The client may be showing a decision the table has already
                    // moved past; re-send the state so it cannot stay stuck.
                    send_json!(json!({ "type": "error", "message": e }));
                    send_json!(s.state_message());
                    continue;
                }
                let events = s.advance();
                if !events.is_empty() {
                    send_json!(json!({ "type": "events", "events": events }));
                }
                if s.table.finished {
                    let replay = s.save_replay();
                    let mut result = s.result_message();
                    if let Some(p) = replay {
                        result["replay"] = json!(p.display().to_string());
                    }
                    send_json!(result);
                }
                // Always re-render, including on the final hand: the result
                // overlay sits on top of the board, and the board behind it must
                // show the finished round rather than the last decision.
                send_json!(s.state_message());
            }
            ClientMsg::Hint => {
                let Some(s) = session.as_ref() else { continue };
                let h = hint_message(s, &checkpoints);
                send_json!(h);
            }
        }
    }
}
