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
async fn tile_svg(
    axum::extract::Path(name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    // These assets are baked into the binary and change whenever the artwork
    // does. A long max-age meant a rebuilt server kept serving the *old* tiles
    // out of the browser cache for a day — the new artwork was on the server and
    // invisible on screen. Revalidate instead: a content hash as the ETag, and
    // `no-cache` so a refresh always asks.
    let Some((_, bytes)) = TILE_SVGS.iter().find(|(n, _)| *n == name) else {
        return (axum::http::StatusCode::NOT_FOUND, "no such tile").into_response();
    };
    let etag = format!("\"{:016x}\"", hash_bytes(bytes));
    if headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains(etag.trim_matches('"')))
        .unwrap_or(false)
    {
        return (
            axum::http::StatusCode::NOT_MODIFIED,
            [(axum::http::header::ETAG, etag)],
        )
            .into_response();
    }
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml".to_string()),
            (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
            (axum::http::header::ETAG, etag),
        ],
        *bytes,
    )
        .into_response()
}

/// A cheap content hash, used only to tell one asset revision from another.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

async fn dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

async fn dashboard_js() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/javascript"),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
        ],
        DASHBOARD_JS,
    )
}

async fn dashboard_css() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css"),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
        ],
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

// The page's own assets are embedded too, so they change with every build:
// revalidate rather than let a browser serve a stale table from cache.
async fn app_js() -> impl IntoResponse {
    (
        [
            ("content-type", "application/javascript; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        APP_JS,
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        STYLE_CSS,
    )
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
    /// A hand has ended and the client has not said it is done reading the
    /// settlement. Nothing advances until it does.
    ///
    /// Bots answer instantly, so without this the table played the next round
    /// before the player had seen the end of the last one: the state the client
    /// got back was the *next* round, and the board behind the settlement panel
    /// could not show the tile that had just won the hand. Pausing here is what
    /// lets the client stage the ending — the ronned tile lands in the pond, the
    /// shout follows it, and only then does the panel cover the table.
    awaiting_ack: bool,
    /// How long one beat lasts on this client: the client's animation pace, told
    /// to us by the client itself. The table never runs ahead of it.
    pace_ms: u64,
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
        let mut table = Table::new(TableConfig { rules, seed });
        // Stop at the end of every hand. The players read a settlement on top of
        // the finished hand, and the next round would replace it; the client asks
        // for the next hand with `ClientMsg::Continue`.
        table.set_pause_at_round_end(true);
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
            awaiting_ack: false,
            pace_ms: 1000,
        }
    }

    /// Let every bot act until the human must decide, the hand ends, or the
    /// match ends.
    /// Is a bot the one who has to move? The player's own turn is not a beat —
    /// the table waits for them however long they take.
    fn bot_to_move(&self) -> bool {
        self.table
            .decisions()
            .iter()
            .any(|d| d.seat != self.human)
    }

    /// Play bots forward until **one** action has been taken, and return the events
    /// it produced.
    ///
    /// One beat, not "until the player has something to do". The old version ran
    /// the whole table forward in a single call and handed the client a batch
    /// covering many turns, so the client was left animating a past that had
    /// already happened — which is why a 立直 could be announced before its owner's
    /// turn looked like it had arrived, and why a 鳴き window could open before the
    /// tile it concerned was on the table. Bots that pass produce no events and are
    /// not a beat, so the loop walks through them.
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
                    // Always wait for the player, even when there is only one
                    // legal action. The forced tsumogiri of a riichi hand used to
                    // be played here, which meant the player never saw the draw
                    // and the discard: the hand appeared to play itself. The
                    // client now plays that single action after a visible beat.
                    continue;
                } else {
                    self.agents[d.seat as usize].act(&self.table, d.seat, &d)
                };
                match self.table.submit(d.seat, action) {
                    Ok(ev) => {
                        let visible = !ev.is_empty();
                        events.extend(ev);
                        acted = true;
                        // The beat ends here: anything already in `events` is this
                        // beat's, and the caller sends it before asking for more.
                        if visible && !self.table.at_round_end() {
                            return events;
                        }
                        // The hand ended: the table is sitting on `Phase::RoundEnd`
                        // and the next round is only dealt when the player asks.
                        // (Checking the phase rather than scanning the events is
                        // what makes this work at all — the engine used to pump
                        // straight through the round transition and into the next
                        // hand before `submit` even returned.)
                        if self.table.at_round_end() {
                            self.awaiting_ack = true;
                            return events;
                        }
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

    /// One beat: the events and the state as of their end, in a single message.
    ///
    /// Two messages (events, then state) made the client redraw its whole table
    /// twice per beat, which reads as the screen flashing. The 鳴き window opened by
    /// a discard belongs to *that* discard's beat — the client holds its buttons
    /// until the tile has landed, which is what "立刻出现" means in practice.
    fn beat_message(&self, events: &[Event]) -> Value {
        let mut msg = self.state_message();
        msg["events"] = json!(events);
        msg
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
    /// "I have read the settlement; deal the next hand." Sent by the client when
    /// the last panel of a finished hand is dismissed. A no-op when the table is
    /// not waiting, so a stale client cannot skip anything.
    Continue,
    /// "This is how long one beat lasts here." The client's animation pace drives
    /// the table: the server waits this long between beats so the state can never
    /// describe something the player has not been shown yet.
    Pace {
        ms: u64,
    },
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

    /// Play the table forward **one beat at a time**, pausing a beat between beats
    /// so the client's animation of each action has finished before the next one is
    /// computed.
    ///
    /// This is the whole point: the ordering the player sees is now the ordering the
    /// engine took, because the engine is not allowed to run ahead. Each beat sends
    /// the events plus the board *without* the decision it creates; the decision
    /// follows after the pause, which is what stops a 鳴き window appearing before
    /// the tile that opened it has landed.
    /// Send one batch as one or two beats, each followed by exactly one pause.
    ///
    /// A batch from the engine is "what one action produced": usually a discard
    /// *and* the next player's draw, because the engine draws as part of resolving
    /// the discard. Those are two different things to watch — but only when the
    /// drawing player is the observer. A bot's draw is invisible (their row of backs
    /// does not change), so giving it its own beat would put a pause in the middle
    /// of a bot's turn with nothing happening, which is the "why did it stutter"
    /// the player notices. The observer's own draw is a tile rising into their hand,
    /// so it gets its own beat: 上家打出宣言牌 — 我摸的下一张牌出现.
    macro_rules! send_beats {
        ($s:expr, $events:expr) => {{
            let events: Vec<Event> = $events;
            let own_draw = events
                .iter()
                .position(|e| matches!(e, Event::Draw { seat, .. } if *seat == $s.human))
                .filter(|&i| i > 0);
            let parts: Vec<&[Event]> = match own_draw {
                Some(i) => vec![&events[..i], &events[i..]],
                None => vec![&events[..]],
            };
            for part in parts {
                send_json!($s.beat_message(part));
                tokio::time::sleep(std::time::Duration::from_millis($s.pace_ms)).await;
            }
        }};
    }

    macro_rules! play_beats {
        ($s:expr) => {{
            while $s.bot_to_move() && !$s.awaiting_ack && !$s.table.finished {
                // The table's clock: the previous action has had its beat by the
                // time we get here, so this one may be computed.
                let events = $s.advance();
                if events.is_empty() {
                    break;
                }
                send_beats!($s, events);
            }
        }};
    }

    // Start a default game immediately so the page is usable on load.
    let mut session: Option<Session> = {
        let seed = rand::thread_rng().gen::<u64>();
        let mut s = Session::new(0, GameLength::Tonpuu, BotKind::Efficiency, seed, &checkpoints);
        // The opening events matter too: without them the record starts empty
        // and the first discards never appear in it.
        let events = s.advance();
        if !events.is_empty() {
            send_json!(json!({ "type": "events", "events": events }));
        }
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
                let events = s.advance();
                session = Some(s);
                if !events.is_empty() {
                    send_json!(json!({ "type": "events", "events": events }));
                }
                let s = session.as_mut().unwrap();
                send_json!(s.state_message());
                // The deal is a beat of its own before anyone plays.
                tokio::time::sleep(std::time::Duration::from_millis(s.pace_ms)).await;
                play_beats!(s);
            }
            ClientMsg::Action { action } => {
                let Some(s) = session.as_mut() else { continue };
                if s.table.finished {
                    send_json!(json!({ "type": "error", "message": "本局已结束，请开新局。" }));
                    send_json!(s.state_message());
                    continue;
                }
                // The player's own action produces events too, and they matter:
                // a tsumo, a ron, or a discard that exhausts the wall ends the
                // hand *here*, so dropping these events meant the client never
                // saw the Win / Ryuukyoku and could not show a settlement — and
                // the round record was missing every one of the player's own
                // moves.
                let events = match s.table.submit(s.human, action) {
                    Ok(ev) => ev,
                    Err(e) => {
                        // The client may be showing a decision the table has
                        // already moved past; re-send the state so it cannot get
                        // stuck.
                        send_json!(json!({ "type": "error", "message": e }));
                        send_json!(s.state_message());
                        continue;
                    }
                };
                // The player's own move can end the hand (their tsumo, their ron,
                // their last discard exhausting the wall). Bots then must not play
                // on into the next round: see `awaiting_ack`.
                if s.table.at_round_end() {
                    s.awaiting_ack = true;
                }
                // The player's own action is a beat like any other, and the draw it
                // produces for the next seat is its own beat when that seat is the
                // player.
                send_beats!(s, events);
                play_beats!(s);
                // The result of the match waits for the settlement too: sending
                // it here would put "对局结束" on top of the last hand's panel.
                if s.table.finished && !s.awaiting_ack {
                    let replay = s.save_replay();
                    let mut result = s.result_message();
                    if let Some(p) = replay {
                        result["replay"] = json!(p.display().to_string());
                    }
                    send_json!(result);
                }
                // The board behind the final overlay is the last state the beats
                // already sent — a second copy here would make the client rebuild
                // the same table twice, which the player sees as a flash.
                if s.table.finished && !s.awaiting_ack {
                    send_json!(s.state_message());
                }
            }
            ClientMsg::Pace { ms } => {
                if let Some(s) = session.as_mut() {
                    // Clamped: a client that asks for 0 would turn the table back
                    // into the instant-advance it used to be, and one that asks for
                    // a minute would look hung.
                    s.pace_ms = ms.clamp(120, 5000);
                }
            }
            ClientMsg::Continue => {
                let Some(s) = session.as_mut() else { continue };
                if !s.awaiting_ack {
                    // Nothing is waiting (a stale click, or the player dismissed
                    // a panel that was not a settlement). Re-send the state so
                    // the client is never left guessing.
                    send_json!(s.state_message());
                    continue;
                }
                s.awaiting_ack = false;
                // Deal the next hand (or end the match), then let the bots play one
                // beat at a time.
                let events = s.table.resume_round_end();
                send_beats!(s, events);
                play_beats!(s);
                if s.table.finished {
                    let replay = s.save_replay();
                    let mut result = s.result_message();
                    if let Some(p) = replay {
                        result["replay"] = json!(p.display().to_string());
                    }
                    send_json!(result);
                }
            }
            ClientMsg::Hint => {
                let Some(s) = session.as_ref() else { continue };
                let h = hint_message(s, &checkpoints);
                send_json!(h);
            }
        }
    }
}
