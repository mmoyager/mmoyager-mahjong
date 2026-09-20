//! `mmj-eval` — measure one checkpoint against a baseline.
//!
//! ```text
//! mmj-eval --a data/checkpoints/ck-0003.bin --b efficiency --games 400
//! mmj-eval --a ck-0003.bin --b ck-0002.bin --games 400 --hanchan
//! ```
//!
//! Four seats alternate between A and B over the run, so seating luck cancels
//! out. The headline number is the average final score of seat A, which starts
//! at 25000 for a break-even agent.

use mmj_ai::{
    ActionFilter, Agent, AssistRegion, AssistedAgent, CallBiasAgent, EfficiencyAgent, FilterAgent,
    NnAgent, RandomAgent,
};
use mmj_core::rules::Rules;
use mmj_nn::Net;
use mmj_core::state::{Table, TableConfig};
use rayon::prelude::*;
use serde_json::json;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: mmj-eval --a PATH|efficiency|random [--b ...] [--games N] [--seed N] \\
         [--hanchan] [--sample] [--json] [--search-a K] [--search-b K]"
    );
    std::process::exit(2);
}

/// Build a teacher whose parameters can be overridden inline, so a parameter
/// sweep needs no rebuild: `efficiency-v2?fold=1.5&push=0.3&care=0.9`.
fn tuned_teacher(spec: &str) -> Option<Box<dyn Agent>> {
    let (base, params) = match spec.split_once('?') {
        Some((b, p)) => (b, p),
        None => return None,
    };
    let mut agent = match base {
        "efficiency" => EfficiencyAgent::new("tuned"),
        "efficiency-v2" => EfficiencyAgent::smart("tuned"),
        "efficiency-v3" => EfficiencyAgent::smarter("tuned"),
        _ => return None,
    };
    // The learned-value variant: `efficiency-v2?value=CKPT.bin&vthr=0.2` replaces
    // the handcrafted strength score in the push/fold decision with the
    // checkpoint's value head.
    let mut value_net: Option<PathBuf> = None;
    let mut value_threshold: f32 = 0.0;
    let mut use_win_probability = false;
    for kv in params.split('&') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        let f: f32 = v.parse().unwrap_or(0.0);
        match k {
            "value" => {
                value_net = Some(PathBuf::from(v));
                continue;
            }
            "vthr" => {
                value_threshold = f;
                continue;
            }
            "pwin" => {
                use_win_probability = true;
                value_threshold = f;
                continue;
            }
            "fold" => agent.config.fold_strength = f,
            "push" => agent.config.push_caution = f,
            "care" => agent.config.careful_caution = f,
            "weight" => agent.config.defense_weight = f,
            "eager" => agent.config.call_eagerness = f as u8,
            "yaku_w" => agent.config.yaku_weight = f,
            "dora_w" => agent.config.dora_weight = f,
            "def" => agent.config.defense = f != 0.0,
            "yaku" => agent.config.yaku_aware = f != 0.0,
            "pf" => agent.config.push_fold = f != 0.0,
            "open" => agent.config.open_threats = f != 0.0,
            "suitread" => agent.config.suit_reading = f != 0.0,
            "readtime" => agent.config.read_timing = f != 0.0,
            "readmeld" => agent.config.read_melds = f != 0.0,
            "riichi" => agent.config.riichi_min_live = f.max(1.0) as u32,
            "suji" => agent.config.strict_suji = f != 0.0,
            "place" => agent.config.placement_aware = f != 0.0,
            "turn" => agent.config.turn_aware = f,
            other => {
                eprintln!("unknown teacher parameter: {}", other);
                std::process::exit(2);
            }
        }
    }
    if let Some(path) = value_net {
        match Net::load(&path) {
            Ok(net) => {
                agent = if use_win_probability {
                    agent.with_win_probability(net, value_threshold)
                } else {
                    agent.with_value(net, value_threshold)
                }
            }
            Err(e) => {
                eprintln!("cannot load value net {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
    }
    Some(Box::new(agent))
}

/// `assisted:PATH?sh=2&calls=1&v2=1` — the checkpoint plays everywhere except in
/// the named region, where the rule-based teacher takes over. Used to find out
/// which decisions actually cost the network points.
fn assisted(spec: &str) -> Option<Box<dyn Agent>> {
    let rest = spec.strip_prefix("assisted:")?;
    let (path, params) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    let mut region = AssistRegion::default();
    let mut v2 = true;
    for item in params.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = item.split_once('=').unwrap_or((item, "1"));
        match k {
            "sh" => {
                region.min_shanten = if v == "off" { None } else { Some(v.parse().unwrap_or(2)) };
            }
            "calls" => region.calls = v != "0",
            "v2" => v2 = v != "0",
            other => {
                eprintln!("unknown assisted parameter: {}", other);
                std::process::exit(2);
            }
        }
    }
    let label = format!("assisted {}", path);
    let nn = match NnAgent::from_checkpoints(&[PathBuf::from(path)], 0, false, Some(label)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cannot load {}: {}", path, e);
            std::process::exit(1);
        }
    };
    let teacher = if v2 {
        EfficiencyAgent::smart("assist")
    } else {
        EfficiencyAgent::new("assist")
    };
    Some(Box::new(AssistedAgent::new(nn, teacher, region)))
}

/// `filter:MODE:PATH` — play the checkpoint with one strategic option removed
/// or forced (`no-riichi`, `force-riichi`, `no-call`, `force-call`).
fn filtered(spec: &str) -> Option<Box<dyn Agent>> {
    let rest = spec.strip_prefix("filter:")?;
    let (mode, path) = rest.split_once(':')?;
    let filter = match mode {
        "no-riichi" => ActionFilter::NoRiichi,
        "force-riichi" => ActionFilter::ForceRiichi,
        "no-call" => ActionFilter::NoCall,
        "force-call" => ActionFilter::ForceCall,
        other => {
            eprintln!("unknown filter mode: {}", other);
            std::process::exit(2);
        }
    };
    let nn = match NnAgent::from_checkpoints(&[PathBuf::from(path)], 0, false, None) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cannot load {}: {}", path, e);
            std::process::exit(1);
        }
    };
    Some(Box::new(FilterAgent::new(nn, filter)))
}

/// `bias:PATH?calls=3` — the checkpoint with its call probabilities scaled.
fn biased(spec: &str) -> Option<Box<dyn Agent>> {
    let rest = spec.strip_prefix("bias:")?;
    let (path, params) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    let mut bias = 1.0f32;
    for item in params.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = item.split_once('=').unwrap_or((item, "1"));
        if k == "calls" {
            bias = v.parse().unwrap_or(1.0);
        } else {
            eprintln!("unknown bias parameter: {}", k);
            std::process::exit(2);
        }
    }
    let nn = match NnAgent::from_checkpoints(&[PathBuf::from(path)], 0, false, None) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cannot load {}: {}", path, e);
            std::process::exit(1);
        }
    };
    Some(Box::new(CallBiasAgent::new(nn, bias)))
}

fn build(spec: &str, seed: u64, search_k: usize, sample: bool) -> Box<dyn Agent> {
    if let Some(a) = biased(spec) {
        return a;
    }
    if let Some(a) = filtered(spec) {
        return a;
    }
    if let Some(a) = assisted(spec) {
        return a;
    }
    if let Some(a) = tuned_teacher(spec) {
        return a;
    }
    match spec {
        "efficiency" => Box::new(EfficiencyAgent::new("efficiency")),
        "efficiency-v3" => Box::new(EfficiencyAgent::smarter("smarter")),
        "efficiency-v2" => Box::new(EfficiencyAgent::smart("smart")),
        "efficiency-yaku" => {
            let mut a = EfficiencyAgent::new("yaku");
            a.config.yaku_aware = true;
            Box::new(a)
        }
        "efficiency-pushfold" => {
            let mut a = EfficiencyAgent::new("pushfold");
            a.config.defense = true;
            a.config.push_fold = true;
            Box::new(a)
        }
        "efficiency-legacy" => {
            let mut a = EfficiencyAgent::new("legacy");
            a.config.legacy_acceptance = true;
            Box::new(a)
        }
        "efficiency-attack" => {
            // The old, defence-free baseline, for A/B comparisons.
            let mut a = EfficiencyAgent::new("attack");
            a.config.defense = false;
            Box::new(a)
        }
        "random" => Box::new(RandomAgent::new(seed)),
        path => {
            // A comma-separated list plays their averaged policy (an ensemble).
            let paths: Vec<PathBuf> = path.split(',').map(PathBuf::from).collect();
            let label = if paths.len() > 1 {
                Some(format!("ensemble x{}", paths.len()))
            } else {
                None
            };
            match NnAgent::from_checkpoints(&paths, seed, sample, label) {
                Ok(a) => Box::new(a.with_search(search_k)),
                Err(e) => {
                    eprintln!("cannot load {}: {}", path, e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn main() {
    let mut a = "efficiency".to_string();
    let mut b = "efficiency".to_string();
    let mut games = 200u64;
    let mut seed = 1u64;
    let mut hanchan = false;
    let mut as_json = false;
    let mut sample = false;
    let mut search_a = 0usize;
    let mut search_b = 0usize;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        macro_rules! val {
            () => {
                args.next().unwrap_or_else(|| usage())
            };
        }
        match arg.as_str() {
            "--a" => a = val!(),
            "--b" => b = val!(),
            "--games" => games = val!().parse().unwrap_or(200),
            "--seed" => seed = val!().parse().unwrap_or(1),
            "--hanchan" => hanchan = true,
            "--sample" => sample = true,
            "--search-a" => search_a = val!().parse().unwrap_or(0),
            "--search-b" => search_b = val!().parse().unwrap_or(0),
            "--json" => as_json = true,
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {}", other);
                usage()
            }
        }
    }

    let mut rules = Rules::tenhou();
    if !hanchan {
        rules = rules.single_round();
    }

    // Games are independent, so run them across all cores: the evaluator is
    // called after every training iteration and used to be a large share of the
    // loop's wall clock.
    #[derive(Default, Clone, Copy)]
    struct Tally {
        a_scores: f64,
        b_scores: f64,
        a_firsts: u64,
        a_wins: u64,
        b_wins: u64,
        rounds: u64,
        /// Sum of placements (1..4) over A's seats, for the average rank.
        a_rank_sum: u64,
        a_seats: u64,
    }

    let tallies: Vec<Tally> = (0..games)
        .into_par_iter()
        .map_init(
            || {
                [
                    build(&a, seed, search_a, sample),
                    build(&a, seed.wrapping_add(1), search_a, sample),
                    build(&b, seed.wrapping_add(1000), search_b, sample),
                    build(&b, seed.wrapping_add(1001), search_b, sample),
                ]
            },
            |pool, game| {
                let mut tally = Tally::default();
                // Alternate which seats A takes so position luck cancels out.
                let a_seats: [bool; 4] = if game % 2 == 0 {
                    [true, false, true, false]
                } else {
                    [false, true, false, true]
                };
                let mut slot = [0usize; 4];
                let (mut ai, mut bi) = (0usize, 2usize);
                for s in 0..4 {
                    if a_seats[s] {
                        slot[s] = ai;
                        ai += 1;
                    } else {
                        slot[s] = bi;
                        bi += 1;
                    }
                }
                let [a1, a2, b1, b2] = pool;
                let mut table = Table::new(TableConfig {
                    rules,
                    seed: seed.wrapping_add(game),
                });
                let mut guard = 0usize;
                while !table.finished {
                    let decisions = table.decisions().to_vec();
                    if decisions.is_empty() {
                        break;
                    }
                    for d in decisions {
                        let agent: &mut Box<dyn Agent> = match slot[d.seat as usize] {
                            0 => a1,
                            1 => a2,
                            2 => b1,
                            _ => b2,
                        };
                        let action = agent.act(&table, d.seat, &d);
                        table
                            .submit(d.seat, action)
                            .unwrap_or_else(|e| panic!("illegal move from {}: {}", agent.name(), e));
                        guard += 1;
                        if guard > 500_000 || table.finished {
                            break;
                        }
                    }
                }
                for e in &table.history {
                    if let mmj_core::state::Event::Win { seat, .. } = e {
                        if a_seats[*seat as usize] {
                            tally.a_wins += 1;
                        } else {
                            tally.b_wins += 1;
                        }
                    }
                }
                let scores = table.scores();
                let mut order: Vec<(i32, u8)> = (0..4).map(|s| (scores[s], s as u8)).collect();
                order.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
                if a_seats[order[0].1 as usize] {
                    tally.a_firsts += 1;
                }
                for (place, &(_, seat)) in order.iter().enumerate() {
                    if a_seats[seat as usize] {
                        tally.a_rank_sum += place as u64 + 1;
                        tally.a_seats += 1;
                    }
                }
                tally.rounds += table.rounds_played as u64;
                for s in 0..4 {
                    if a_seats[s] {
                        tally.a_scores += scores[s] as f64;
                    } else {
                        tally.b_scores += scores[s] as f64;
                    }
                }
                tally
            },
        )
        .collect();

    let mut a_scores = 0f64;
    let mut b_scores = 0f64;
    let mut a_firsts = 0u64;
    let mut a_wins = 0u64;
    let mut b_wins = 0u64;
    let mut total_rounds = 0u64;
    let mut a_rank_sum = 0u64;
    let mut a_seats_total = 0u64;
    for t in &tallies {
        a_scores += t.a_scores;
        b_scores += t.b_scores;
        a_firsts += t.a_firsts;
        a_wins += t.a_wins;
        b_wins += t.b_wins;
        total_rounds += t.rounds;
        a_rank_sum += t.a_rank_sum;
        a_seats_total += t.a_seats;
    }

    let per_seat = games as f64 * 2.0;
    let avg_a = a_scores / per_seat;
    let avg_b = b_scores / per_seat;
    // Average placement is the metric mahjong players actually care about, and
    // it is far less noisy than the average score.
    let avg_rank_a = a_rank_sum as f64 / a_seats_total.max(1) as f64;
    let uma_a = (4.0 - avg_rank_a) * 10.0;
    if as_json {
        println!(
            "{}",
            json!({
                "a": a,
                "b": b,
                "games": games,
                "avg_a": avg_a,
                "avg_b": avg_b,
                "first_rate_a": a_firsts as f64 / games as f64,
                "avg_rank_a": (avg_rank_a * 10000.0).round() / 10000.0,
                "uma_a": (uma_a * 100.0).round() / 100.0,
                "wins_a": a_wins,
                "wins_b": b_wins,
                "rounds": total_rounds,
            })
        );
    } else {
        println!("A = {}   B = {}", a, b);
        println!("games {}   rounds {}", games, total_rounds);
        println!("average score   A {:.0}   B {:.0}   (break-even {:.0})", avg_a, avg_b, 25000.0);
        println!(
            "average rank    A {:.3}   (random 2.500; lower is better)",
            avg_rank_a
        );
        println!(
            "hands won       A {}   B {}",
            a_wins, b_wins
        );
        println!("first-place rate for A: {:.3}", a_firsts as f64 / games as f64);
    }
}
