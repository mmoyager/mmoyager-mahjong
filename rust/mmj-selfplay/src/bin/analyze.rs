//! `mmj-analyze` — analyse a saved replay.
//!
//! ```text
//! mmj-analyze --replay data/replays/1789543167-1000.json \
//!             [--checkpoint data/checkpoints/ck-0007.bin] [--seat 0] [--json]
//! ```
//!
//! The replay is first rebuilt through the engine from its seed (which must
//! reproduce the stored event log exactly), then every decision of the chosen
//! seat is scored by the policy/value network: what the network would play, how
//! confident it is, and — for discards — what the action looks to be worth one
//! ply ahead.

use mmj_ai::replay::{analyze, format_report, seat_names, ReplayFile};
use mmj_ai::NnAgent;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: mmj-analyze --replay FILE [--checkpoint FILE] [--seat N] [--json]"
    );
    std::process::exit(2);
}

/// Resolve the checkpoint the training loop considers best.
fn default_checkpoint() -> Option<PathBuf> {
    let text = std::fs::read_to_string("data/training_state.json").ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let path = PathBuf::from(value.get("best")?.as_str()?);
    if path.exists() {
        return Some(path);
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir("data/checkpoints")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "bin").unwrap_or(false))
        .collect();
    candidates.sort();
    candidates.pop()
}

/// Re-drive the log and report the first place the rebuild stops matching.
fn debug_replay(path: &PathBuf) -> i32 {
    use mmj_ai::replay::{action_events, action_for};
    use mmj_core::state::{Table, TableConfig};
    let file = match ReplayFile::load(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot read replay: {}", e);
            return 1;
        }
    };
    println!("stored events: {}", file.events.len());
    let actions = action_events(&file.events);
    println!("action events: {}", actions.len());

    for rules in file.candidate_rules() {
        let mut consumed = vec![false; actions.len()];
        let mut table = Table::new(TableConfig {
            rules,
            seed: file.seed,
        });
        let mut step = 0usize;
        let mut failure: Option<String> = None;
        let mut trace: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        while !table.finished {
            let decisions = table.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            for d in decisions {
                let action = action_for(&actions, &mut consumed, d.seat, &d);
                if trace.len() >= 22 {
                    trace.pop_front();
                }
                let left = consumed.iter().filter(|c| !**c).count();
                trace.push_back(format!(
                    "step {:>4} events left {:>3} hist {:>3} seat {} {:?} -> {}",
                    step,
                    left,
                    table.history.len(),
                    d.seat,
                    table.phase,
                    action.label()
                ));
                if let Err(e) = table.submit(d.seat, action) {
                    let window: Vec<String> = actions
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !consumed[*i])
                        .take(5)
                        .map(|(_, a)| format!("{:?}", a))
                        .collect();
                    let offered: Vec<String> =
                        d.actions.iter().map(|a| a.label()).collect();
                    let recent: Vec<String> = table
                        .history
                        .iter()
                        .rev()
                        .take(4)
                        .map(|e| {
                            serde_json::to_string(e)
                                .unwrap_or_default()
                                .chars()
                                .take(120)
                                .collect()
                        })
                        .collect();
                    failure = Some(format!(
                        "step {} seat {} phase {:?}\n  error: {}\n  next events: {:#?}\n  offered: {:?}\n  recent: {:#?}",
                        step, d.seat, table.phase, e, window, offered, recent
                    ));
                    break;
                }
                step += 1;
                if table.finished {
                    break;
                }
            }
            if failure.is_some() {
                break;
            }
        }
        println!("rules hanchan={:?}: {} steps", rules.length, step);
        if let Some(f) = failure {
            println!("{}", f);
            println!("--- last steps ---");
            for line in &trace {
                println!("  {}", line);
            }
            return 1;
        }
    }
    0
}

fn main() {
    let mut replay_path: Option<PathBuf> = None;
    let mut checkpoint: Option<PathBuf> = None;
    let mut seat: Option<u8> = None;
    let mut as_json = false;
    let mut debug = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        macro_rules! val {
            () => {
                args.next().unwrap_or_else(|| usage())
            };
        }
        match a.as_str() {
            "--replay" | "-r" => replay_path = Some(PathBuf::from(val!())),
            "--checkpoint" | "-c" => checkpoint = Some(PathBuf::from(val!())),
            "--seat" | "-s" => seat = val!().parse().ok(),
            "--json" => as_json = true,
            "--debug" => debug = true,
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {}", other);
                usage()
            }
        }
    }
    let Some(replay_path) = replay_path else { usage() };
    if debug {
        std::process::exit(debug_replay(&replay_path));
    }

    let file = match ReplayFile::load(&replay_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot read replay: {}", e);
            std::process::exit(1);
        }
    };
    let seat = seat.unwrap_or(file.human).min(3);

    let checkpoint = checkpoint.or_else(default_checkpoint);
    let mut agent = match &checkpoint {
        Some(path) => match NnAgent::from_checkpoint(path, 7, false) {
            Ok(a) => {
                eprintln!("using checkpoint {}", path.display());
                Some(a)
            }
            Err(e) => {
                eprintln!("cannot load {}: {} (continuing without a network)", path.display(), e);
                None
            }
        },
        None => {
            eprintln!("no checkpoint found; reporting the stored log only");
            None
        }
    };

    match analyze(&file, seat, &mut agent) {
        Ok(analysis) => {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&analysis).unwrap());
            } else {
                print!("{}", format_report(&analysis, &seat_names(file.human)));
            }
        }
        Err(e) => {
            eprintln!("analysis failed: {}", e);
            std::process::exit(1);
        }
    }
}
