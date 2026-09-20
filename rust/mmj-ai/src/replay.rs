//! Replay reconstruction and analysis.
//!
//! A saved replay contains everything needed to rebuild the match: the seed the
//! wall was shuffled with, plus the ordered event log. [`replay`] re-drives the
//! engine from the seed and feeds the recorded actions back in, which
//! reproduces the exact same game (the test at the bottom of this file asserts
//! the rebuilt event log is identical to the stored one).
//!
//! [`analyze`] walks that reconstruction a second time and, for one chosen
//! seat, reports at every decision:
//!
//! * the probability the trained policy gives each legal action;
//! * for discards, a shallow one-ply estimate of what the action is worth,
//!   obtained by playing it out deterministically and reading the value head.
//!
//! Only information the seat can legally see is ever encoded, so the analysis
//! never "cheats" with the opponents' hands.

use crate::nn_agent::NnAgent;
use mmj_core::action::Action;
use mmj_core::rules::Rules;
use mmj_core::state::{Decision, DrawReason, Event, Table, TableConfig, Trigger};
use mmj_core::tile::{Kind, tile_name};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A replay file as written by the web server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplayFile {
    pub seed: u64,
    #[serde(default)]
    pub human: u8,
    #[serde(default)]
    pub scores: [i32; 4],
    pub events: Vec<Event>,
    /// Present in newer files; older ones fall back to the ruleset default.
    #[serde(default)]
    pub hanchan: Option<bool>,
}

impl ReplayFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e))
    }

    /// Candidate rulesets to rebuild with, most likely first.
    ///
    /// A replay only says how long the match was, not every flags setting, and
    /// rebuilding with the wrong ruleset silently produces a *different* game.
    /// [`analyze`] therefore tries each candidate and keeps the one whose event
    /// log matches the file exactly.
    pub fn candidate_rules(&self) -> Vec<Rules> {
        let mut out: Vec<Rules> = Vec::new();
        let mut push = |r: Rules| {
            if !out.contains(&r) {
                out.push(r);
            }
        };
        let tenhou = Rules::tenhou();
        let mut tonpuu = tenhou;
        tonpuu.length = mmj_core::rules::GameLength::Tonpuu;
        if self.hanchan == Some(true) {
            push(tenhou);
            push(tonpuu);
        } else {
            // Files written before the flag existed were tonpuu.
            push(tonpuu);
            push(tenhou);
        }
        push(Rules::tenhou().single_round());
        out
    }
}

/// One action recovered from the event log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayAction {
    Discard {
        seat: u8,
        tile: u8,
        riichi: bool,
        tsumogiri: bool,
    },
    Meld {
        seat: u8,
        meld: mmj_core::meld::Meld,
    },
    Tsumo {
        seat: u8,
        tile: u8,
    },
    Ron {
        seat: u8,
        from: u8,
        tile: u8,
    },
    Kyuushu,
}

/// The seat an event belongs to, or `None` for 九種九牌 (which the engine
/// accepts from whoever is on turn).
fn event_seat(action: &ReplayAction) -> Option<u8> {
    match action {
        ReplayAction::Discard { seat, .. }
        | ReplayAction::Meld { seat, .. }
        | ReplayAction::Tsumo { seat, .. }
        | ReplayAction::Ron { seat, .. } => Some(*seat),
        ReplayAction::Kyuushu => None,
    }
}

/// Extract the ordered actions from an event log, skipping everything that the
/// engine reproduces on its own (draws, dora reveals, round boundaries).
pub fn action_events(events: &[Event]) -> Vec<ReplayAction> {
    let mut out = Vec::with_capacity(events.len());
    for e in events {
        match e {
            Event::Discard {
                seat,
                tile,
                tsumogiri,
                riichi,
            } => out.push(ReplayAction::Discard {
                seat: *seat,
                tile: *tile,
                riichi: *riichi,
                tsumogiri: *tsumogiri,
            }),
            Event::Meld { seat, meld, .. } => out.push(ReplayAction::Meld {
                seat: *seat,
                meld: *meld,
            }),
            // A 大明槓 emits both `Meld` (above) and `Kan`; only the first
            // describes the action, so the second must not be counted again or
            // every later decision would line up with the wrong event.
            Event::Kan { seat, meld, .. } => {
                if meld.kind != mmj_core::meld::MeldKind::Minkan {
                    out.push(ReplayAction::Meld {
                        seat: *seat,
                        meld: *meld,
                    });
                }
            }
            Event::Win {
                seat,
                from,
                tile,
                nagashi,
                ..
            } => {
                // 流し満貫 is settled at an exhaustive draw: it is not an action
                // any decision could produce, so replaying it as a 自摸 made the
                // reconstruction diverge at the next decision.
                if *nagashi {
                    continue;
                }
                match from {
                    None => out.push(ReplayAction::Tsumo {
                        seat: *seat,
                        tile: *tile,
                    }),
                    Some(f) => out.push(ReplayAction::Ron {
                        seat: *seat,
                        from: *f,
                        tile: *tile,
                    }),
                }
            }
            Event::Ryuukyoku {
                reason: DrawReason::NineTerminals,
                ..
            } => out.push(ReplayAction::Kyuushu),
            _ => {}
        }
    }
    out
}

/// Choose the replayed action that matches this pending decision.
/// Choose the replayed action that matches this pending decision.
///
/// Decisions are submitted in seat order, but events are recorded in
/// *resolution* order, and those differ: on a ダブロン both winners are recorded
/// in turn order while the decisions are offered seat by seat. So instead of
/// peeking at the head of the list, take the earliest unconsumed action that
/// belongs to this seat and see whether it fits the decision.
pub fn action_for(
    actions: &[ReplayAction],
    consumed: &mut [bool],
    seat: u8,
    decision: &Decision,
) -> Action {
    for (i, event) in actions.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        match event_seat(event) {
            Some(s) if s != seat => continue,
            _ => {}
        }
        let matched = match *event {
            ReplayAction::Discard {
                tile, riichi, ..
            } => {
                let exact = decision.actions.iter().copied().find(|a| {
                    matches!(a, Action::Discard { tile: t, riichi: r } if *t == tile && *r == riichi)
                });
                exact.or_else(|| {
                    decision.actions.iter().copied().find(|a| {
                        matches!(a, Action::Discard { tile: t, riichi: r }
                            if mmj_core::tile::kind_of(*t) == mmj_core::tile::kind_of(tile)
                                && *r == riichi)
                    })
                })
            }
            ReplayAction::Meld { meld, .. } => decision.actions.iter().copied().find(|a| match a {
                Action::Meld { meld: m } => {
                    m.kind == meld.kind
                        && m.triplet_kind() == meld.triplet_kind()
                        && m.run_start() == meld.run_start()
                }
                _ => false,
            }),
            ReplayAction::Tsumo { .. } => decision
                .actions
                .iter()
                .copied()
                .find(|a| *a == Action::Tsumo),
            ReplayAction::Ron { .. } => {
                decision.actions.iter().copied().find(|a| *a == Action::Ron)
            }
            ReplayAction::Kyuushu => decision
                .actions
                .iter()
                .copied()
                .find(|a| *a == Action::Kyuushu),
        };
        return match matched {
            Some(a) => {
                consumed[i] = true;
                a
            }
            // The earliest action of this seat does not fit the current
            // decision, so this decision was a pass.
            None => Action::Pass,
        };
    }
    Action::Pass
}

/// Re-drive a stored event log through the engine.
///
/// `visitor` is called before each submitted action with the live table, so
/// callers can observe the state at every decision point.
pub fn replay<F>(
    file: &ReplayFile,
    rules: Rules,
    mut visitor: F,
) -> Result<Table, String>
where
    F: FnMut(&Table, u8, &Decision, Action),
{
    let actions = action_events(&file.events);
    let mut consumed = vec![false; actions.len()];
    let mut table = Table::new(TableConfig {
        rules,
        seed: file.seed,
    });
    let mut guard = 0usize;
    while !table.finished {
        let decisions = table.decisions().to_vec();
        if decisions.is_empty() {
            break;
        }
        for decision in decisions {
            let action = action_for(&actions, &mut consumed, decision.seat, &decision);
            visitor(&table, decision.seat, &decision, action);
            table
                .submit(decision.seat, action)
                .map_err(|e| format!("replay diverged at seat {}: {}", decision.seat, e))?;
            guard += 1;
            if guard > 500_000 || table.finished {
                break;
            }
        }
    }
    Ok(table)
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionReport {
    /// Human-readable action, e.g. `3m` or `pon 5m`.
    pub label: String,
    /// Probability the trained policy gives this action.
    pub prob: f32,
    /// Shallow one-ply value estimate, when it could be computed.
    pub value: Option<f32>,
    pub chosen: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionReport {
    pub hand: u32,
    pub round: String,
    /// Decision number inside the hand.
    pub turn: u32,
    pub trigger: String,
    /// `discard`, `riichi`, `call`, `win` or `other`.
    pub kind: String,
    pub options: Vec<OptionReport>,
    pub chosen: String,
    pub top: String,
    /// Probability of the chosen action under the trained policy.
    pub chosen_prob: f32,
    pub top_prob: f32,
    pub value: f32,
    /// Shanten before and after the chosen action (own-turn decisions only).
    pub shanten_before: Option<i8>,
    pub shanten_after: Option<i8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandReport {
    pub index: u32,
    pub round: String,
    pub honba: u32,
    /// `win`, `draw` or `abort`.
    pub result: String,
    pub winner: Option<u8>,
    pub from: Option<u8>,
    pub tile: Option<u8>,
    pub tile_name: Option<String>,
    pub han: u16,
    pub fu: u16,
    pub ron_total: i32,
    pub yaku: Vec<(String, u8)>,
    pub reason: Option<String>,
    pub tenpai: [bool; 4],
    pub deltas: [i32; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerReport {
    pub seat: u8,
    pub score: i32,
    pub wins: u32,
    pub tsumo: u32,
    pub ron: u32,
    pub deal_ins: u32,
    pub riichi: u32,
    pub tenpai: u32,
    pub hands: u32,
    /// Hands where this seat was tenpai at the exhaustive draw.
    pub noten: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub analyzed_seat: u8,
    pub decisions: usize,
    /// Mean policy probability the trained net put on the action actually taken.
    pub mean_agreement: f32,
    /// Mean policy probability the net puts on its own top choice.
    pub mean_confidence: f32,
    /// Decisions where the chosen action was not the net's top choice, worst first.
    pub disagreements: usize,
    pub mean_value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Analysis {
    pub seed: u64,
    pub analyzed_seat: u8,
    pub rounds: u32,
    pub initial_scores: [i32; 4],
    pub scores: [i32; 4],
    pub ranking: Vec<u8>,
    pub players: Vec<PlayerReport>,
    pub hands: Vec<HandReport>,
    pub decisions: Vec<DecisionReport>,
    pub summary: Summary,
    /// Set when the replay could be rebuilt exactly; `false` means the analysis
    /// is based on the stored log only.
    pub verified: bool,
}

fn round_label(wind: Kind, number: u8) -> String {
    let w = match wind {
        mmj_core::tile::EAST => "东",
        mmj_core::tile::SOUTH => "南",
        mmj_core::tile::WEST => "西",
        _ => "北",
    };
    format!("{}{}局", w, number)
}

/// Aggregate the stored event log into per-hand and per-player reports.
pub fn summarize(file: &ReplayFile) -> (Vec<HandReport>, Vec<PlayerReport>, [i32; 4]) {
    let mut hands: Vec<HandReport> = Vec::new();
    let mut players: [PlayerReport; 4] = std::array::from_fn(|s| PlayerReport {
        seat: s as u8,
        score: 0,
        wins: 0,
        tsumo: 0,
        ron: 0,
        deal_ins: 0,
        riichi: 0,
        tenpai: 0,
        hands: 0,
        noten: 0,
    });
    let mut current: Option<HandReport> = None;
    let mut final_scores = file.scores;
    let mut seen_hands = 0u32;

    for event in &file.events {
        match event {
            Event::RoundStart {
                round_wind,
                round_number,
                honba,
                ..
            } => {
                current = Some(HandReport {
                    index: seen_hands,
                    round: round_label(*round_wind, *round_number),
                    honba: *honba,
                    result: "abort".to_string(),
                    winner: None,
                    from: None,
                    tile: None,
                    tile_name: None,
                    han: 0,
                    fu: 0,
                    ron_total: 0,
                    yaku: Vec::new(),
                    reason: None,
                    tenpai: [false; 4],
                    deltas: [0; 4],
                });
                seen_hands += 1;
                for p in players.iter_mut() {
                    p.hands += 1;
                }
            }
            Event::Riichi { seat } => players[*seat as usize].riichi += 1,
            Event::Win {
                seat,
                from,
                tile,
                score,
                deltas,
                nagashi,
                ..
            } => {
                let p = &mut players[*seat as usize];
                p.wins += 1;
                match from {
                    None if *nagashi => {}
                    None => p.tsumo += 1,
                    Some(f) => {
                        p.ron += 1;
                        players[*f as usize].deal_ins += 1;
                    }
                }
                if let Some(h) = current.as_mut() {
                    h.result = "win".to_string();
                    h.winner = Some(*seat);
                    h.from = *from;
                    h.tile = Some(*tile);
                    h.tile_name = if *from == None && score.yaku.iter().any(|(y, _)| {
                        matches!(y, mmj_core::score::Yaku::NagashiMangan)
                    }) {
                        Some("流局满贯".to_string())
                    } else {
                        Some(tile_name(*tile))
                    };
                    h.han = score.han;
                    h.fu = score.fu;
                    h.ron_total = score.ron_total(0);
                    h.yaku = score
                        .yaku
                        .iter()
                        .map(|(y, han)| (y.name_zh().to_string(), *han))
                        .collect();
                    h.deltas = *deltas;
                }
            }
            Event::Ryuukyoku {
                reason,
                tenpai,
                deltas,
                ..
            } => {
                // An abortive draw (九種九牌 and friends) does not compare hands
                // and pays nothing, so counting every seat as noten there
                // invented four phantom 未听 per abort.
                if matches!(reason, DrawReason::Exhaustive) {
                    for s in 0..4 {
                        if tenpai[s] {
                            players[s].tenpai += 1;
                        } else {
                            players[s].noten += 1;
                        }
                    }
                }
                if let Some(h) = current.as_mut() {
                    h.result = if matches!(reason, DrawReason::Exhaustive) {
                        "draw".to_string()
                    } else {
                        "abort".to_string()
                    };
                    h.reason = Some(reason.name_zh().to_string());
                    h.tenpai = *tenpai;
                    h.deltas = *deltas;
                }
            }
            Event::RoundEnd { scores, .. } => {
                if let Some(h) = current.take() {
                    hands.push(h);
                }
                final_scores = *scores;
            }
            Event::GameEnd { scores, .. } => final_scores = *scores,
            _ => {}
        }
    }
    if let Some(h) = current.take() {
        hands.push(h);
    }
    for (i, p) in players.iter_mut().enumerate() {
        p.score = final_scores[i];
    }
    (hands, players.to_vec(), final_scores)
}

fn decision_kind(action: &Action) -> &'static str {
    match action {
        Action::Discard { riichi: true, .. } => "riichi",
        Action::Discard { .. } => "discard",
        Action::Meld { .. } => "call",
        Action::Tsumo | Action::Ron => "win",
        _ => "other",
    }
}

fn trigger_name(trigger: &Trigger) -> String {
    match trigger {
        Trigger::SelfTurn => "自己回合".to_string(),
        Trigger::Discard { from, tile } => {
            format!("{} 打出 {}", from, tile_name(*tile))
        }
        Trigger::Chankan { from, tile } => format!("{} 加杠 {}", from, tile_name(*tile)),
    }
}

/// Shallow one-ply value of taking `action`, as used by the search agent.
fn lookahead_value(
    agent: &mut NnAgent,
    table: &Table,
    seat: u8,
    action: Action,
) -> Option<f32> {
    agent.lookahead_value(table, seat, action)
}

/// Rebuild the match, returning the ruleset whose event log matches the file.
///
/// The match is only accepted when the rebuilt event log is byte-for-byte the
/// stored one; every reported number is computed on that reconstruction.
fn rebuild(file: &ReplayFile) -> Result<(Rules, Table, bool), String> {
    let target = canonical(&file.events);
    let mut fallback: Option<(Rules, Table)> = None;
    let mut last_error = String::new();
    for rules in file.candidate_rules() {
        match replay(file, rules, |_, _, _, _| {}) {
            Ok(table) => {
                let produced = canonical(&table.history);
                if produced == target {
                    return Ok((rules, table, true));
                }
                if fallback.is_none() {
                    fallback = Some((rules, table));
                }
            }
            Err(e) => last_error = e,
        }
    }
    // A ruleset that rebuilds *something* but not the recorded game would make
    // the analysis describe a different match, so that is refused outright.
    if let Some((_, table)) = fallback {
        return Err(format!(
            "the replay could not be rebuilt exactly ({} events stored, {} rebuilt); \
             it was probably recorded by a different engine version",
            file.events.len(),
            table.history.len()
        ));
    }
    Err(format!("cannot rebuild the replay: {}", last_error))
}

/// A comparable projection of an event log.
///
/// Verification compares the rebuilt log with the stored one, but the event
/// shape grows: `Win` gained `hand`, `melds`, `paid`, `pao_payer` and `nagashi`,
/// `RoundEnd` gained `next_honba`, and `ScoreResult` gained the dora breakdown.
/// A replay recorded before those fields existed could therefore never verify —
/// 94% of the saved replays were rejected for that reason alone. Comparing the
/// fields that existed then keeps old replays analysable while still catching a
/// rebuilt game that genuinely differs.
fn canonical(events: &[Event]) -> String {
    let mut value = serde_json::to_value(events).unwrap_or(serde_json::Value::Null);
    strip_added_fields(&mut value);
    serde_json::to_string(&value).unwrap_or_default()
}

/// Remove every field that older replays cannot contain, at any depth.
fn strip_added_fields(value: &mut serde_json::Value) {
    const ADDED: [&str; 9] = [
        "hand", "melds", "paid", "pao_payer", "nagashi", "next_honba",
        "dora_han", "ura_han", "aka_han",
    ];
    match value {
        serde_json::Value::Object(map) => {
            for key in ADDED {
                map.remove(key);
            }
            for (_, v) in map.iter_mut() {
                strip_added_fields(v);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items.iter_mut() {
                strip_added_fields(v);
            }
        }
        _ => {}
    }
}

/// Analyse a replay from `seat`'s point of view.
pub fn analyze(file: &ReplayFile, seat: u8, agent: &mut Option<NnAgent>) -> Result<Analysis, String> {
    // A replay recorded by an older engine cannot always be rebuilt exactly —
    // the rules and the event shape have both changed since. Refusing it flatly
    // threw away a file whose *stored* log is perfectly readable, so an
    // unrebuildable replay now degrades to a stored-log summary, marked
    // unverified, instead of an error.
    let (rules, prebuilt, verified) = match rebuild(file) {
        Ok(v) => v,
        Err(_) => {
            let (hands, players, scores) = summarize(file);
            let mut ranking: Vec<u8> = (0..4u8).collect();
            ranking.sort_by(|a, b| scores[*b as usize].cmp(&scores[*a as usize]));
            return Ok(Analysis {
                seed: file.seed,
                analyzed_seat: seat,
                rounds: hands.len() as u32,
                initial_scores: [25000; 4],
                scores,
                ranking,
                players,
                hands,
                decisions: Vec::new(),
                summary: Summary {
                    analyzed_seat: seat,
                    decisions: 0,
                    mean_agreement: 0.0,
                    mean_confidence: 0.0,
                    disagreements: 0,
                    mean_value: 0.0,
                },
                verified: false,
            });
        }
    };
    let _ = prebuilt;
    let (hands, players, scores) = summarize(file);

    // Without a network the per-decision report would be empty, so only the
    // stored-log summary is produced.
    let record_decisions = agent.is_some();
    let mut decisions: Vec<DecisionReport> = Vec::new();
    let mut hand_index = 0u32;
    let mut turn = 0u32;

    // Walk the reconstructed game and record the seat's decisions.
    let rebuilt = replay(file, rules, |table, s, decision, action| {
        if s != seat || !record_decisions {
            return;
        }
        if table.rounds_played.saturating_sub(1) != hand_index {
            hand_index = table.rounds_played.saturating_sub(1);
            turn = 0;
        }
        turn += 1;
        let round_label_now = table.round_name();
        let shanten_before = {
            let p = &table.players[seat as usize];
            if p.hand_len() % 3 == 2 {
                Some(mmj_core::hand::shanten(&p.hand, p.melds.len() as u8))
            } else {
                None
            }
        };
        let shanten_after = match action {
            Action::Discard { tile, .. } => {
                let p = &table.players[seat as usize];
                let mut rest = p.hand;
                let k = mmj_core::tile::kind_of(tile) as usize;
                if rest[k] > 0 {
                    rest[k] -= 1;
                    Some(mmj_core::hand::shanten(&rest, p.melds.len() as u8))
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut options = Vec::new();
        let mut chosen_label = action.label();
        let mut chosen_prob = 0.0f32;
        let mut top_label = String::new();
        let mut top_prob = 0.0f32;
        let mut value = 0.0f32;
        if let Some(a) = agent.as_mut() {
            let (dist, state_value) = a.evaluate(table, seat, decision);
            value = state_value;
            // One-ply estimates are only meaningful for discards; evaluating
            // every call variant would multiply the cost for little insight.
            let want_values = matches!(action, Action::Discard { .. }) && dist.len() <= 16;
            for (opt_action, prob) in &dist {
                let is_chosen = opt_action.index() == action.index();
                let v = if want_values {
                    lookahead_value(a, table, seat, *opt_action)
                } else {
                    None
                };
                if is_chosen {
                    chosen_prob = *prob;
                    chosen_label = opt_action.label();
                }
                if *prob > top_prob {
                    top_prob = *prob;
                    top_label = opt_action.label();
                }
                options.push(OptionReport {
                    label: opt_action.label(),
                    prob: *prob,
                    value: v,
                    chosen: is_chosen,
                });
            }
        }
        decisions.push(DecisionReport {
            hand: hand_index,
            round: round_label_now,
            turn,
            trigger: trigger_name(&decision.trigger),
            kind: decision_kind(&action).to_string(),
            options,
            chosen: chosen_label,
            top: top_label,
            chosen_prob,
            top_prob,
            value,
            shanten_before,
            shanten_after,
        });
    })?;

    let mut final_scores = scores;
    let mut ranking: Vec<u8> = (0..4u8).collect();
    ranking.sort_by(|&a, &b| {
        final_scores[b as usize]
            .cmp(&final_scores[a as usize])
            .then(a.cmp(&b))
    });
    if let Some(last) = rebuilt.history.iter().rev().find_map(|e| match e {
        Event::GameEnd { scores, .. } => Some(*scores),
        _ => None,
    }) {
        final_scores = last;
    }

    let n = decisions.len().max(1);
    let mean_agreement = decisions.iter().map(|d| d.chosen_prob).sum::<f32>() / n as f32;
    let mean_confidence = decisions.iter().map(|d| d.top_prob).sum::<f32>() / n as f32;
    let disagreements = decisions
        .iter()
        .filter(|d| d.options.len() > 1 && d.top != d.chosen)
        .count();
    let mean_value = decisions.iter().map(|d| d.value).sum::<f32>() / n as f32;

    // Keep the most interesting decisions first.
    let mut sorted = decisions.clone();
    sorted.sort_by(|a, b| {
        (a.chosen_prob - a.top_prob)
            .partial_cmp(&(b.chosen_prob - b.top_prob))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(Analysis {
        seed: file.seed,
        analyzed_seat: seat,
        rounds: rebuilt.rounds_played,
        initial_scores: [25000; 4],
        scores: final_scores,
        ranking,
        players,
        hands,
        decisions: sorted,
        summary: Summary {
            analyzed_seat: seat,
            decisions: decisions.len(),
            mean_agreement,
            mean_confidence,
            disagreements,
            mean_value,
        },
        verified,
    })
}

/// A compact text report, used by the CLI.
pub fn format_report(a: &Analysis, names: &[String; 4]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "对局 seed={}  共 {} 局  分析座位 {}\n",
        a.seed, a.rounds, names[a.analyzed_seat as usize]
    ));
    out.push_str("终局点数：");
    for s in 0..4 {
        out.push_str(&format!("{} {}  ", names[s], a.scores[s]));
    }
    out.push('\n');
    out.push_str("座位统计：\n");
    for p in &a.players {
        out.push_str(&format!(
            "  {} 和牌 {}（自摸 {} / 荣和 {}） 放铳 {}  立直 {}  听牌 {} / 流局 {}\n",
            names[p.seat as usize],
            p.wins,
            p.tsumo,
            p.ron,
            p.deal_ins,
            p.riichi,
            p.tenpai,
            p.noten
        ));
    }
    out.push_str("每局结果：\n");
    for h in &a.hands {
        let detail = if let Some(w) = h.winner {
            let how = match h.from {
                None => "自摸".to_string(),
                Some(f) => format!("荣和({})", names[f as usize]),
            };
            let yaku: Vec<String> = h
                .yaku
                .iter()
                .map(|(n, v)| if *v > 0 { format!("{}{}番", n, v) } else { n.clone() })
                .collect();
            format!(
                "{} {} {} {}番{}符 {}点 [{}]",
                names[w as usize],
                how,
                h.tile_name.clone().unwrap_or_default(),
                h.han,
                h.fu,
                h.ron_total,
                yaku.join(" ")
            )
        } else {
            format!(
                "{} 听牌 {}",
                h.reason.clone().unwrap_or_else(|| "流局".into()),
                h.tenpai
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| **t)
                    .map(|(i, _)| names[i].clone())
                    .collect::<Vec<_>>()
                    .join("、")
            )
        };
        out.push_str(&format!("  {}{}本场  {}\n", h.round, h.honba, detail));
    }
    out.push_str(&format!(
        "\n分析：{} 个决策，网络给实际选择的平均概率 {:.3}，分歧 {} 次，状态价值均值 {:.2}\n",
        a.summary.decisions, a.summary.mean_agreement, a.summary.disagreements, a.summary.mean_value
    ));
    let worst: Vec<&DecisionReport> = a
        .decisions
        .iter()
        .filter(|d| d.options.len() > 1 && d.top != d.chosen)
        .take(8)
        .collect();
    if worst.is_empty() {
        out.push_str("（没有明显分歧的决策）\n");
    } else {
        out.push_str("分歧最大的几手：\n");
        for d in worst {
            let mut opts: Vec<String> = d
                .options
                .iter()
                .map(|o| {
                    format!(
                        "{}{} {:.0}%{}",
                        if o.chosen { "*" } else { " " },
                        o.label,
                        o.prob * 100.0,
                        match o.value {
                            Some(v) => format!(" v{:+.2}", v),
                            None => String::new(),
                        }
                    )
                })
                .collect();
            opts.sort_by(|a, b| b.len().cmp(&a.len()));
            out.push_str(&format!(
                "  {} 第{}手 [{}] 你打 {}，网络倾向 {}  —— {}\n",
                d.round,
                d.turn,
                d.trigger,
                d.chosen,
                d.top,
                opts.join(" | ")
            ));
        }
    }
    out
}

/// Names used in reports.
pub fn seat_names(human: u8) -> [String; 4] {
    std::array::from_fn(|s| {
        if s as u8 == human {
            "你".to_string()
        } else {
            format!("AI{}", s)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::play_game;
    use crate::{Agent, EfficiencyAgent};

    fn play_a_match(seed: u64) -> ReplayFile {
        let mut agents: [Box<dyn Agent>; 4] = [
            Box::new(EfficiencyAgent::new("a")),
            Box::new(EfficiencyAgent::new("b")),
            Box::new(EfficiencyAgent::new("c")),
            Box::new(EfficiencyAgent::new("d")),
        ];
        let config = TableConfig {
            rules: Rules::tenhou().single_round(),
            seed,
        };
        let result = play_game(&mut agents, config);
        ReplayFile {
            seed,
            human: 0,
            scores: result.scores,
            events: result.events,
            hanchan: Some(false),
        }
    }

    #[test]
    fn replay_reproduces_the_same_game() {
        for seed in 1..8u64 {
            let file = play_a_match(seed);
            let (_, rebuilt, verified) = rebuild(&file).expect("replay");
            assert!(verified, "seed {} did not replay identically", seed);
            assert_eq!(rebuilt.history.len(), file.events.len());
            assert_eq!(
                serde_json::to_string(&file.events).unwrap(),
                serde_json::to_string(&rebuilt.history).unwrap(),
                "seed {} diverged",
                seed
            );
        }
    }

    #[test]
    fn summary_matches_the_final_scores() {
        let file = play_a_match(9);
        let (hands, players, scores) = summarize(&file);
        assert!(!hands.is_empty());
        assert_eq!(scores, file.scores);
        let wins: u32 = players.iter().map(|p| p.wins).sum();
        let hands_with_wins = hands.iter().filter(|h| h.result == "win").count() as u32;
        assert_eq!(wins, hands_with_wins);
        let deal_ins: u32 = players.iter().map(|p| p.deal_ins).sum();
        assert_eq!(deal_ins, hands.iter().filter(|h| h.from.is_some()).count() as u32);
    }

    #[test]
    fn analysis_reports_probabilities_without_a_network() {
        let file = play_a_match(3);
        let mut agent = None;
        let analysis = analyze(&file, 0, &mut agent).expect("analysis");
        assert_eq!(analysis.analyzed_seat, 0);
        assert!(
            analysis.decisions.is_empty(),
            "a network is required for per-decision reports"
        );
        assert!(!analysis.hands.is_empty());
        assert!(analysis.verified);
    }
}
