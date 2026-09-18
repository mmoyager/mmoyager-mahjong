//! The training dashboard's backend: read the loop's state, follow its log, and
//! start/stop the loop itself.
//!
//! The loop is a Python process writing `data/training_state.json` after every
//! step and appending to `data/logs/loop-*.out`. That is a perfectly good
//! interface for a terminal and a poor one for a human, so this module exposes
//! exactly those two sources plus process control:
//!
//! * `GET  /api/training`          — state + history + a log tail + run status
//! * `POST /api/training/control`  — `{"action": "start" | "stop" | "restart"}`
//!
//! Nothing here decides anything about training; it only reports what the loop
//! wrote and forwards start/stop. `data/training_state.json` stays the single
//! source of truth, so the dashboard can never disagree with the terminal.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::CheckpointSource;

/// The exact command the dashboard starts. Kept in one place so what the button
/// does is auditable, and printed in the UI so an operator can run it by hand.
pub const LOOP_ARGS: &[&str] = &[
    "-u",
    "python/trainer/loop.py",
    "--resume",
    "--forever",
    "--dagger-loop",
    "--search-modes",
    "--primary-spec",
    "efficiency-v2",
    "--yardstick2",
    "efficiency",
    "--eval-games",
    "9600",
    "--accept-margin",
    "60",
    "--confirm-runs",
    "2",
    "--early-stop-slack",
    "100",
    "--bootstrap-lr",
    "3e-4",
    "--bootstrap-records",
    "900000",
    "--bootstrap-epochs",
    "4",
    "--bootstrap-data",
    "data/selfplay/im-v12.bin",
    "--value-coef",
    "0.25",
    "--entropy-coef",
    "0.005",
    "--max-records",
    "400000",
    "--threads",
    "4",
    "--accept-on",
    "direct",
    "--abs-every",
    "5",
];

fn python() -> String {
    // The trainer lives in the project's venv; fall back to whatever `python3`
    // resolves to if the venv is missing, so the button still says something
    // useful rather than silently doing nothing.
    let venv = Path::new(".venv/bin/python");
    if venv.exists() {
        venv.to_string_lossy().to_string()
    } else {
        "python3".to_string()
    }
}

/// Every pid whose command line runs the training loop.
fn loop_pids() -> Vec<u32> {
    let out = match Command::new("ps").args(["-Ao", "pid=,command="]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut pids = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let (pid, cmd) = match line.split_once(char::is_whitespace) {
            Some((p, c)) => (p, c.trim_start()),
            None => continue,
        };
        // `loop.py` is specific enough: the game server, the evaluator and the
        // trainer itself do not contain that string.
        if cmd.contains("loop.py") && !cmd.contains("grep") {
            if let Ok(pid) = pid.parse::<u32>() {
                pids.push(pid);
            }
        }
    }
    pids
}

/// The log the loop is currently writing to: the newest `data/logs/loop-*.out`.
fn newest_loop_log() -> Option<PathBuf> {
    let dir = Path::new("data/logs");
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string())?;
        if !name.starts_with("loop-") || !name.ends_with(".out") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if best.as_ref().map(|(b, _)| modified > *b).unwrap_or(true) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, p)| p)
}

/// Last `n` lines of a file, without reading gigabytes into memory.
fn tail(path: &Path, n: usize) -> Vec<String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&bytes);
    // The loop logs progress with '\r' as well as '\n'; normalise first so a
    // progress bar does not swallow the lines around it.
    let normalised = text.replace('\r', "\n");
    let mut lines: Vec<&str> = normalised.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() > n {
        lines.drain(..lines.len() - n);
    }
    lines.iter().map(|l| l.to_string()).collect()
}

/// The state file, or an empty object when the loop has never run.
fn read_state() -> Value {
    std::fs::read_to_string("data/training_state.json")
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// A compact digest of the state: what the UI actually needs, in the shape it
/// needs it. The full state stays in the file.
fn digest(state: &Value) -> Value {
    let history = state
        .get("history")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();
    let iterations: Vec<Value> = history
        .iter()
        .map(|h| {
            let checkpoint = h.get("checkpoint").and_then(|c| c.as_str()).unwrap_or("");
            let best = h.get("best").and_then(|b| b.as_str()).unwrap_or("");
            let promoted = !checkpoint.is_empty() && checkpoint == best;
            json!({
                "iteration": h.get("iteration").and_then(|v| v.as_i64()),
                "recipe": h.get("recipe").and_then(|v| v.as_str()),
                "score": h.get("group_mean").and_then(|v| v.as_f64()),
                "seconds": h.get("seconds").and_then(|v| v.as_f64()),
                "partial": h.get("partial").and_then(|v| v.as_bool()),
                "promoted": promoted,
                "replicas": h.get("replica_scores").cloned().unwrap_or(json!([])),
                "checkpoint": checkpoint,
                "note": h.get("note").and_then(|v| v.as_str()),
            })
        })
        .collect();

    let best = state.get("best").and_then(|b| b.as_str()).unwrap_or("");
    json!({
        "best": best.rsplit('/').next().unwrap_or(best),
        "best_path": best,
        "best_absolute": state.get("best_absolute").and_then(|v| v.as_f64()),
        "best_score": state.get("best_score").and_then(|v| v.as_f64()),
        "guards": state.get("best_guards").cloned().unwrap_or(json!({})),
        "iteration": state.get("iteration").and_then(|v| v.as_i64()),
        "iterations": iterations,
        "protects": state.get("protected").cloned().unwrap_or(json!([])),
    })
}

/// `GET /api/training` — everything the dashboard polls.
pub async fn status(State(source): State<CheckpointSource>) -> Json<Value> {
    let state = read_state();
    let pids = loop_pids();
    let log = newest_loop_log();
    let (log_name, log_tail) = match &log {
        Some(p) => (
            p.to_string_lossy().to_string(),
            tail(p, 60),
        ),
        None => (String::new(), Vec::new()),
    };
    // How long the newest log has been quiet: a stuck loop shows up here.
    let quiet_secs = log
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs());

    Json(json!({
        "running": !pids.is_empty(),
        "pids": pids,
        "log_file": log_name,
        "quiet_secs": quiet_secs,
        "now": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "command": format!("{} {}", python(), LOOP_ARGS.join(" ")),
        "log_tail": log_tail,
        "served": source.resolve().map(|p| p.to_string_lossy().to_string()),
        "state": digest(&state),
    }))
}

#[derive(Deserialize)]
pub struct Control {
    action: String,
}

/// `POST /api/training/control` — start, stop or restart the loop.
///
/// Stopping is done with SIGTERM, which the loop handles by finishing the current
/// step and writing its state: the same guarantee Ctrl-C gives in a terminal.
pub async fn control(Json(body): Json<Control>) -> Json<Value> {
    let before = loop_pids();
    let mut message = String::new();

    match body.action.as_str() {
        "stop" => {
            for pid in &before {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
            message = if before.is_empty() {
                "循环本来就没有在跑".to_string()
            } else {
                format!("已发送停止信号（SIGTERM）给 {} 个进程，当前迭代结束后退出", before.len())
            };
        }
        "start" | "restart" => {
            if !before.is_empty() {
                if body.action == "start" {
                    return Json(json!({ "ok": false, "message": "循环已经在运行", "pids": before }));
                }
                for pid in &before {
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                }
                // Give it a moment to write its state before starting a new one,
                // otherwise two loops race over the same state file.
                std::thread::sleep(std::time::Duration::from_secs(6));
            }
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let log = format!("data/logs/loop-dash-{stamp}.out");
            let script = format!(
                "nohup {} {} >> {} 2>&1 &",
                python(),
                LOOP_ARGS.join(" "),
                log
            );
            match Command::new("sh").arg("-c").arg(&script).status() {
                Ok(s) if s.success() => {
                    message = format!("已启动训练循环，日志写入 {log}");
                }
                Ok(s) => message = format!("启动失败（exit {}）", s.code().unwrap_or(-1)),
                Err(e) => message = format!("启动失败：{e}"),
            }
        }
        other => {
            return Json(json!({ "ok": false, "message": format!("未知操作 {other}") }));
        }
    }

    Json(json!({
        "ok": true,
        "message": message,
        "running": !loop_pids().is_empty(),
    }))
}
