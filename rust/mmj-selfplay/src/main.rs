//! `mmj-selfplay` — experience generation for the trainer.
//!
//! ```text
//! mmj-selfplay generate --games 5000 --out data/selfplay/sp-0001.bin \
//!     --checkpoint data/checkpoints/ck-0000.bin [--opponent <path>] \
//!     [--seats learner,learner,learner,learner] [--epsilon 0.02] \
//!     [--seed 1] [--batch 256] [--hanchan]
//! mmj-selfplay imitate --games 2000 --out data/selfplay/im-0001.bin
//! ```
//!
//! `generate` plays the checkpoint against itself (or against `--opponent`) and
//! records every decision with its Monte-Carlo return. `imitate` records the
//! tile-efficiency baseline instead: that is how the first policy is
//! bootstrapped, before any reward signal exists.

use mmj_core::rules::Rules;
use mmj_nn::data::{DataWriter, RECORD_BYTES};
use mmj_nn::{FEATURE_DIM, Net, POLICY_DIM};
use mmj_selfplay::{Engine, RunnerConfig, SeatSpec};
use rayon::prelude::*;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

fn usage() -> ! {
    eprintln!(
        "usage:\n  mmj-selfplay generate --games N --out FILE [options]\n  \
         mmj-selfplay imitate  --games N --out FILE [options]\n\n\
         options:\n  --checkpoint PATH   policy network to train (omit for a fresh random policy)\n  \
         --opponent PATH     frozen opponent for 'frozen' seats\n  \
         --seats LIST        learner|frozen|efficiency|random, four entries (default all learner)\n  \
         --epsilon F         probability of a uniformly random legal action (default 0.02)\n  \
         --temperature F     policy softmax temperature (default 1.0)\n  \
         --greedy            take the argmax instead of sampling\n  \
         --dagger            label learner states with the teacher's action (DAgger)\n  \
         --dagger-shanten N  only label states with at least this many shanten\n  \
         --dagger-calls      also label every call window (chi/pon/kan/ron)\n  \
         --teacher-v2        use the improved teacher for the rule-based seats\n  \
         --teacher-v3        use the third teacher level (open hands count as threats)\n  \
         --teacher-v4        use the fourth level (v2 + discard-sequence safety read)\n  \
         --seed N            base seed (default 1)\n  \
         --batch N           games per parallel batch (default 256)\n  \
         --hanchan           play full hanchan instead of tonpuu\n  \
         --hidden LIST       hidden widths for a fresh random policy (default 512,512)\n  \
         bench-encode        time observation encoding against a forward pass\n  \
         stats               playing statistics (win / deal-in / riichi / call rates)"
    );
    std::process::exit(2);
}

struct Args {
    command: String,
    games: u64,
    out: PathBuf,
    checkpoint: Option<PathBuf>,
    opponent: Option<PathBuf>,
    seats: Vec<SeatSpec>,
    epsilon: f32,
    temperature: f32,
    sample: bool,
    dagger: bool,
    dagger_shanten: i8,
    dagger_calls: bool,
    teacher_v2: bool,
    teacher_v3: bool,
    teacher_v4: bool,
    label_kind: String,
    seed: u64,
    batch: u64,
    hanchan: bool,
    hidden: Vec<usize>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| usage());
    let mut games = 100u64;
    let mut out = PathBuf::from("data/selfplay/out.bin");
    let mut checkpoint = None;
    let mut opponent = None;
    let mut seats = vec![SeatSpec::Learner; 4];
    let mut epsilon = 0.02f32;
    let mut temperature = 1.0f32;
    let mut sample = true;
    let mut dagger = false;
    let mut dagger_shanten: i8 = -1;
    let mut dagger_calls = false;
    let mut teacher_v2 = false;
    let mut teacher_v3 = false;
    let mut teacher_v4 = false;
    let mut label_kind = "selfplay".to_string();
    let mut seed = 1u64;
    let mut batch = 256u64;
    let mut hanchan = false;
    let mut hidden = Net::default_shape();

    while let Some(a) = args.next() {
        macro_rules! val {
            () => {
                args.next().unwrap_or_else(|| usage())
            };
        }
        match a.as_str() {
            "--games" => games = val!().parse().unwrap_or_else(|_| usage()),
            "--out" => out = PathBuf::from(val!()),
            "--checkpoint" => checkpoint = Some(PathBuf::from(val!())),
            "--opponent" => opponent = Some(PathBuf::from(val!())),
            "--seats" => {
                seats = val!()
                    .split(',')
                    .map(|s| match s.trim() {
                        "learner" => SeatSpec::Learner,
                        "frozen" => SeatSpec::Frozen,
                        "efficiency" => SeatSpec::Efficiency,
                        "random" => SeatSpec::Random,
                        other => {
                            eprintln!("unknown seat spec: {}", other);
                            usage()
                        }
                    })
                    .collect();
                if seats.len() != 4 {
                    eprintln!("--seats needs exactly four entries");
                    usage();
                }
            }
            "--epsilon" => epsilon = val!().parse().unwrap_or(0.02),
            "--temperature" => temperature = val!().parse().unwrap_or(1.0),
            "--greedy" => sample = false,
            "--dagger" => dagger = true,
            "--dagger-shanten" => dagger_shanten = val!().parse().unwrap_or(-1),
            "--dagger-calls" => dagger_calls = true,
            "--teacher-v2" => teacher_v2 = true,
            "--teacher-v3" => teacher_v3 = true,
            "--teacher-v4" => teacher_v4 = true,
            "--label-kind" => label_kind = val!(),
            "--seed" => seed = val!().parse().unwrap_or(1),
            "--batch" => batch = val!().parse().unwrap_or(256),
            "--hanchan" => hanchan = true,
            "--hidden" => {
                hidden = val!()
                    .split(',')
                    .filter_map(|s| s.trim().parse::<usize>().ok())
                    .collect();
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {}", other);
                usage()
            }
        }
    }

    Args {
        command,
        games,
        out,
        checkpoint,
        opponent,
        seats,
        epsilon,
        temperature,
        sample,
        dagger,
        dagger_shanten,
        dagger_calls,
        teacher_v2,
        teacher_v3,
        teacher_v4,
        label_kind,
        seed,
        batch,
        hanchan,
        hidden,
    }
}

/// Time the raw inference cost, which is what limits self-play throughput.
/// Time observation encoding against a forward pass.
///
/// Encoding is not free: for an own-turn decision it runs a shanten search for
/// every kind in hand, tile acceptance for the best candidates and a danger
/// table, all of which compete with the network for the same core. Knowing the
/// split decides where a throughput optimization is worth writing.
fn bench_encode(args: &Args) {
    use mmj_ai::{Agent, RandomAgent};
    use mmj_nn::{Obs, encode};
    use mmj_core::state::{Table, TableConfig};

    let mut net = match &args.checkpoint {
        Some(p) => Net::load(p).expect("cannot load checkpoint"),
        None => Net::new(&args.hidden, FEATURE_DIM, 1),
    };
    let mut logits = vec![0.0f32; POLICY_DIM];
    let mut value = 0.0f32;

    // Reach a realistic mid-game position: four random players, a few turns in.
    let mut table = Table::new(TableConfig {
        rules: mmj_core::rules::Rules::tenhou(),
        seed: 20260917,
    });
    let mut rng = RandomAgent::new(7);
    let mut guard = 0;
    let mut own_turn = Vec::new();
    while !table.finished && guard < 60 {
        let decisions = table.decisions().to_vec();
        if decisions.is_empty() {
            break;
        }
        for d in decisions {
            if matches!(d.trigger, mmj_core::state::Trigger::SelfTurn) && own_turn.len() < 400 {
                own_turn.push((table.clone(), d.seat, d.clone()));
            }
            let a = rng.act(&table, d.seat, &d);
            let _ = table.submit(d.seat, a);
            guard += 1;
        }
    }
    println!("captured {} own-turn decisions to encode", own_turn.len());
    if own_turn.is_empty() {
        return;
    }
    let mut obs = Obs::new();
    // Warm up pages and branch predictors.
    for (t, seat, d) in own_turn.iter().take(64) {
        encode(t, *seat, d, &mut obs);
    }
    let reps = 20;
    let start = Instant::now();
    let mut acc = 0.0f32;
    for _ in 0..reps {
        for (t, seat, d) in &own_turn {
            encode(t, *seat, d, &mut obs);
            acc += obs.features[0];
        }
    }
    let secs = start.elapsed().as_secs_f64();
    let n = (reps * own_turn.len()) as f64;
    println!(
        "encode: {:.1} us each ({} decisions x {} reps, checksum {:.1})",
        secs / n * 1e6,
        own_turn.len(),
        reps,
        acc
    );
    // Same loop, forward pass instead of encoding.
    for _ in 0..64 {
        net.forward(&obs.features, &mut logits, &mut value);
    }
    let start = Instant::now();
    for _ in 0..reps {
        for _ in own_turn.iter() {
            net.forward(&obs.features, &mut logits, &mut value);
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!("forward: {:.1} us each", secs / n * 1e6);

    // Does the forward scale with the number of *non-zero* inputs? The axpy
    // kernel touches one weight row per non-zero feature, so time proportional
    // to that count means weight traffic; flat means fixed overhead.
    for keep in [703usize, 512, 256, 128, 32] {
        let mut dense = vec![0.0f32; FEATURE_DIM];
        for i in 0..keep {
            dense[i] = 0.25;
        }
        for _ in 0..64 {
            net.forward(&dense, &mut logits, &mut value);
        }
        let start = Instant::now();
        for _ in 0..2000 {
            net.forward(&dense, &mut logits, &mut value);
        }
        let t = start.elapsed().as_secs_f64() / 2000.0;
        let rows = keep + 384; // second layer sees ~half of its 768 ReLU outputs
        println!(
            "  forward with {:>3} non-zero features: {:>6.1} us  ({:.0} GB/s of weight traffic)",
            keep,
            t * 1e6,
            (rows * 768 * 4) as f64 / t / 1e9
        );
    }

    // Batched evaluation: does serving several decisions from one pass over the
    // weights actually pay? This decides whether restructuring the evaluator
    // into lock-stepped groups of games is worth the change.
    let fd = FEATURE_DIM;
    let mut rows = Vec::new();
    for (t, seat, d) in own_turn.iter().take(8) {
        encode(t, *seat, d, &mut obs);
        rows.push(obs.features.clone());
    }
    if rows.len() >= 2 {
        for &n in &[2usize, 4, 8] {
            if n > rows.len() {
                continue;
            }
            let mut buf = vec![0.0f32; n * fd];
            for r in 0..n {
                buf[r * fd..(r + 1) * fd].copy_from_slice(&rows[r]);
            }
            let reps = 2000 / n;
            // Batched.
            let mut bp = vec![0.0f32; n * POLICY_DIM];
            let mut bv = vec![0.0f32; n];
            for _ in 0..32 {
                net.forward_batch(&buf, n, &mut bp, &mut bv);
            }
            let start = Instant::now();
            for _ in 0..reps {
                net.forward_batch(&buf, n, &mut bp, &mut bv);
            }
            let batched = start.elapsed().as_secs_f64() / (reps * n) as f64;
            // One at a time.
            let start = Instant::now();
            for _ in 0..reps {
                for r in 0..n {
                    net.forward(&buf[r * fd..(r + 1) * fd], &mut logits, &mut value);
                }
            }
            let single = start.elapsed().as_secs_f64() / (reps * n) as f64;
            println!(
                "  batch of {}: {:>6.1} us per decision batched, {:>6.1} us one-at-a-time ({:.2}x)",
                n,
                batched * 1e6,
                single * 1e6,
                single / batched
            );
        }
    }

    // Micro-kernel: is the inner loop actually vectorised? Both variants
    // compute the same axpy, so measuring them in one run cancels machine load.
    {
        let n: usize = 768;
        let mut w = vec![0.0f32; n * n];
        for (i, v) in w.iter_mut().enumerate() {
            *v = ((i % 97) as f32 - 48.0) / 97.0;
        }
        let x = vec![0.31f32; n];
        let mut out = vec![0.0f32; n];
        let reps = 400;

        let start = Instant::now();
        for _ in 0..reps {
            for &xj in x.iter() {
                let col = &w[..];
                for (o, ww) in out.iter_mut().zip(col.iter()) {
                    *o += ww * xj;
                }
            }
        }
        let zip_t = start.elapsed().as_secs_f64() / reps as f64;

        let start = Instant::now();
        for _ in 0..reps {
            for &xj in x.iter() {
                let col = &w[..];
                for (oc, wc) in out.chunks_exact_mut(8).zip(col.chunks_exact(8)) {
                    for k in 0..8 {
                        oc[k] += wc[k] * xj;
                    }
                }
            }
        }
        let chunk_t = start.elapsed().as_secs_f64() / reps as f64;
        let flops = (n * n * 2) as f64;
        println!(
            "  axpy 768x768: zip {:.1} us ({:.1} GFLOP/s), chunks {:.1} us ({:.1} GFLOP/s) -> {:.2}x",
            zip_t * 1e6,
            flops / zip_t / 1e9,
            chunk_t * 1e6,
            flops / chunk_t / 1e9,
            zip_t / chunk_t
        );
    }

    // Quantised kernel probe: f32 FMA gives 8 MACs per instruction, AVX2
    // `vpmaddwd` (i16) gives 16 and `vpmaddubsw` (u8 x i8) gives 32. The f32
    // kernel already runs at the core's FMA peak, so the only way to a faster
    // forward is fewer instructions per MAC. Measure before wiring anything in.
    {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::*;
            let n: usize = 768;
            let reps = 400usize;
            // Reference: the f32 chunked kernel used by the network.
            let w = vec![0.31f32; n * n];
            let x = vec![0.27f32; n];
            let mut out = vec![0.0f32; n];
            let t0 = Instant::now();
            for _ in 0..reps {
                for &xj in x.iter() {
                    let col = &w[..];
                    for (oc, wc) in out.chunks_exact_mut(8).zip(col.chunks_exact(8)) {
                        for k in 0..8 {
                            oc[k] += wc[k] * xj;
                        }
                    }
                }
            }
            let f32_t = t0.elapsed().as_secs_f64() / reps as f64;

            // i16 weights, i16 inputs, `vpmaddwd` (16 MACs per instruction).
            let w16: Vec<i16> = (0..n * n)
                .map(|i| (((i % 61) as i32 - 30) * 400) as i16)
                .collect();
            let x16 = vec![3000i16; n];
            let mut acc = vec![0i32; n];
            let t0 = Instant::now();
            for _ in 0..reps {
                acc.iter_mut().for_each(|a| *a = 0);
                for &xj in x16.iter() {
                    let xv = _mm256_set1_epi16(xj);
                    let wv = w16.as_ptr();
                    let ap = acc.as_mut_ptr();
                    let mut k = 0usize;
                    while k + 16 <= n {
                        let ww = _mm256_loadu_si256(wv.add(k) as *const __m256i);
                        let prod = _mm256_madd_epi16(ww, xv);
                        let cur = _mm256_loadu_si256(ap.add(k) as *const __m256i);
                        _mm256_storeu_si256(ap.add(k) as *mut __m256i, _mm256_add_epi32(cur, prod));
                        k += 16;
                    }
                }
            }
            let i16_t = t0.elapsed().as_secs_f64() / reps as f64;
            let checksum: i64 = acc.iter().map(|&v| v as i64).sum();

            println!(
                "  axpy 768x768: f32 {:.1} us ({:.1} GFLOP/s), i16 vpmaddwd {:.1} us ({:.1} GMAC/s x2) -> {:.2}x  [checksum {}]",
                f32_t * 1e6,
                (n * n * 2) as f64 / f32_t / 1e9,
                i16_t * 1e6,
                (n * n) as f64 / i16_t / 1e9,
                f32_t / i16_t,
                checksum
            );
        }
    }

    // Which part of the encoder costs what.
    use mmj_core::danger::danger_table_full;
    use mmj_core::hand::{shanten, tile_keep_value, ukeire_count, yaku_score};
    use mmj_core::tile::{Kind, NUM_KINDS};
    // Straight-line timings, one block at a time.
    let mut checksum = 0.0f32;
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let me = &t.players[*seat as usize];
            checksum += shanten(&me.hand, me.melds.len() as u8) as f32;
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "shanten (whole hand)", start.elapsed().as_secs_f64() / n * 1e6);
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let me = &t.players[*seat as usize];
            let melds = me.melds.len() as u8;
            let mut after = me.hand;
            let mut best = 8i8;
            for k in 0..NUM_KINDS {
                if after[k] > 0 {
                    after[k] -= 1;
                    let s = shanten(&after, melds);
                    if s < best {
                        best = s;
                    }
                    after[k] += 1;
                }
            }
            checksum += best as f32;
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "34x shanten (lookahead)", start.elapsed().as_secs_f64() / n * 1e6);
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let me = &t.players[*seat as usize];
            let seen = t.public_counts();
            let melds = me.melds.len() as u8;
            let mut rest = me.hand;
            let mut total = 0u32;
            let mut done = 0;
            for k in 0..NUM_KINDS {
                if rest[k] > 0 && done < 8 {
                    done += 1;
                    rest[k] -= 1;
                    total += ukeire_count(&rest, melds, &seen);
                    rest[k] += 1;
                }
            }
            checksum += total as f32;
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "8x ukeire_count", start.elapsed().as_secs_f64() / n * 1e6);
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let me = &t.players[*seat as usize];
            let mut rest = me.hand;
            for k in 0..NUM_KINDS {
                if rest[k] > 0 {
                    rest[k] -= 1;
                    checksum += tile_keep_value(&rest, k as Kind) as f32;
                    rest[k] += 1;
                }
            }
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "34x tile_keep_value", start.elapsed().as_secs_f64() / n * 1e6);
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let me = &t.players[*seat as usize];
            let mut rest = me.hand;
            for k in 0..NUM_KINDS {
                if rest[k] > 0 {
                    rest[k] -= 1;
                    checksum += yaku_score(&rest, &me.melds, t.round_wind, t.seat_wind(*seat)) as f32;
                    rest[k] += 1;
                }
            }
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "34x yaku_score", start.elapsed().as_secs_f64() / n * 1e6);
    let start = Instant::now();
    for _ in 0..reps {
        for (t, seat, _) in &own_turn {
            let d = danger_table_full(t, *seat, true, true);
            checksum += d[0];
        }
    }
    println!("  {:<22} {:>7.1} us per decision", "danger_table_full", start.elapsed().as_secs_f64() / n * 1e6);
    println!("  checksum {:.1}", checksum);
}

/// Aggregate playing statistics for a seat configuration.
///
/// The project's yardstick (paired score against a fixed bot) says how much
/// better the AI is, but nothing about *what kind of player* it is. Human
/// strength is charted with per-hand rates — 和了率, 放銃率, 立直率, 副露率 —
/// so measuring the same rates is the only way to ask "how far from an ordinary
/// human is this?" with numbers instead of adjectives.
fn stats(args: &Args) {
    use mmj_ai::{Agent, EfficiencyAgent, NnAgent, RandomAgent, play_game};
    use mmj_core::state::{Event, TableConfig};
    use rayon::prelude::*;

    #[derive(Default, Clone, Copy)]
    struct Tally {
        rounds: u64,
        wins: u64,
        tsumo: u64,
        ron: u64,
        deal_ins: u64,
        riichi: u64,
        melds: u64,
        kans: u64,
        draws: u64,
        tenpai_draws: u64,
        win_points: i64,
        score: i64,
        placement: u64,
        value_below_2000: u64,
        value_2000_3999: u64,
        value_4000_7999: u64,
        value_8000_plus: u64,
    }
    let seats = args.seats.clone();
    let checkpoint = args.checkpoint.clone();
    let opponent = args.opponent.clone();
    let teacher_v2 = args.teacher_v2;
    let _ = (&opponent, teacher_v2);

    let tallies: Vec<Tally> = (0..args.games)
        .into_par_iter()
        .map(|g| {
            let mut agents: Vec<Box<dyn Agent>> = Vec::with_capacity(4);
            for spec in &seats {
                agents.push(match spec {
                    SeatSpec::Learner | SeatSpec::Frozen => {
                        let p = checkpoint.clone().expect("--checkpoint is required for learner seats");
                        Box::new(NnAgent::from_checkpoints(&[p], g, false, None).expect("load"))
                    }
                    SeatSpec::Efficiency => {
                        if teacher_v2 {
                            Box::new(EfficiencyAgent::smart("efficiency-v2"))
                        } else {
                            Box::new(EfficiencyAgent::new("efficiency"))
                        }
                    }
                    SeatSpec::Random => Box::new(RandomAgent::new(g)),
                });
            }
            let mut agents: [Box<dyn Agent>; 4] = [
                agents.remove(0),
                agents.remove(0),
                agents.remove(0),
                agents.remove(0),
            ];
            let result = play_game(
                &mut agents,
                TableConfig {
                    rules: if args.hanchan { Rules::tenhou() } else { Rules::tenhou().single_round() },
                    seed: args.seed.wrapping_add(g),
                },
            );
            let mut t = Tally::default();
            t.rounds = result.rounds as u64;
            let rounds = result.rounds as u64;
            for s in 0..4 {
                t.wins += result.wins[s] as u64;
                t.deal_ins += result.deal_ins[s] as u64;
                t.riichi += result.riichi[s] as u64;
                t.score += result.scores[s] as i64;
                t.placement += result.ranking.iter().position(|&x| x == s as u8).unwrap_or(3) as u64 + 1;
            }
            for e in &result.events {
                match e {
                    Event::Win { from, deltas, .. } => {
                        t.win_points += deltas.iter().copied().max().unwrap_or(0) as i64;
                        let v = deltas.iter().copied().max().unwrap_or(0);
                        if v < 2000 {
                            t.value_below_2000 += 1;
                        } else if v < 4000 {
                            t.value_2000_3999 += 1;
                        } else if v < 8000 {
                            t.value_4000_7999 += 1;
                        } else {
                            t.value_8000_plus += 1;
                        }
                        if from.is_none() {
                            t.tsumo += 1;
                        } else {
                            t.ron += 1;
                        }
                    }
                    Event::Meld { .. } => t.melds += 1,
                    Event::Kan { .. } => t.kans += 1,
                    Event::Ryuukyoku { tenpai, .. } => {
                        t.draws += 1;
                        t.tenpai_draws += tenpai.iter().filter(|&&x| x).count() as u64;
                    }
                    _ => {}
                }
            }
            if rounds == 0 {
                t.rounds = 1;
            }
            t
        })
        .collect();

    let mut total = Tally::default();
    for t in &tallies {
        total.rounds += t.rounds;
        total.wins += t.wins;
        total.tsumo += t.tsumo;
        total.ron += t.ron;
        total.deal_ins += t.deal_ins;
        total.riichi += t.riichi;
        total.melds += t.melds;
        total.kans += t.kans;
        total.draws += t.draws;
        total.tenpai_draws += t.tenpai_draws;
        total.win_points += t.win_points;
        total.score += t.score;
        total.placement += t.placement;
        total.value_below_2000 += t.value_below_2000;
        total.value_2000_3999 += t.value_2000_3999;
        total.value_4000_7999 += t.value_4000_7999;
        total.value_8000_plus += t.value_8000_plus;
    }
    let rounds = total.rounds.max(1) as f64;
    let seat_hands = rounds * 4.0;
    let seats_n = (args.games * 4) as f64;
    let json = serde_json::json!({
        "games": args.games,
        "rounds_per_game": total.rounds as f64 / args.games as f64,
        "seat_hands": total.rounds * 4,
        "win_rate": total.wins as f64 / seat_hands,
        "deal_in_rate": total.deal_ins as f64 / seat_hands,
        "riichi_rate": total.riichi as f64 / seat_hands,
        "call_rate": total.melds as f64 / seat_hands,
        "kan_rate": total.kans as f64 / seat_hands,
        "draw_rate": total.draws as f64 / rounds,
        "tsumo_share": total.tsumo as f64 / total.wins.max(1) as f64,
        "avg_win_points": total.win_points as f64 / total.wins.max(1) as f64,
        "avg_score": total.score as f64 / seats_n,
        "avg_placement": total.placement as f64 / seats_n,
        "draw_tenpai_rate": total.tenpai_draws as f64 / (4.0 * total.draws.max(1) as f64),
        "win_value_mix": {
            "lt2000": total.value_below_2000 as f64 / total.wins.max(1) as f64,
            "2000_3999": total.value_2000_3999 as f64 / total.wins.max(1) as f64,
            "4000_7999": total.value_4000_7999 as f64 / total.wins.max(1) as f64,
            "ge8000": total.value_8000_plus as f64 / total.wins.max(1) as f64,
        },
    });
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}

fn bench(args: &Args) {
    use mmj_nn::POLICY_DIM;
    let net = match &args.checkpoint {
        Some(p) => Net::load(p).expect("cannot load checkpoint"),
        None => Net::new(&args.hidden, FEATURE_DIM, 1),
    };
    let mut net = net;
    let features = vec![0.3f32; FEATURE_DIM];
    let mut logits = vec![0.0f32; POLICY_DIM];
    let mut value = 0.0f32;
    // Warm up.
    for _ in 0..200 {
        net.forward(&features, &mut logits, &mut value);
    }
    let n = 20_000;
    let start = Instant::now();
    for _ in 0..n {
        net.forward(&features, &mut logits, &mut value);
    }
    let secs = start.elapsed().as_secs_f64();
    let per = secs / n as f64;
    println!(
        "network {} params, {:.0} KB of weights",
        net.params(),
        net.params() as f64 * 4.0 / 1024.0
    );
    println!(
        "forward: {:.1} us each, {:.0} forwards/s single thread, {:.1} GB/s of weight traffic",
        per * 1e6,
        1.0 / per,
        net.params() as f64 * 4.0 / per / 1e9
    );
}

/// Time hand-shape analysis, which decides whether richer features (tile
/// acceptance) are affordable inside self-play.
fn bench_shanten() {
    use mmj_core::hand::{shanten, useful_kinds};
    use mmj_core::tile::NUM_KINDS;
    let mut state = 0x9E3779B97F4A7C15u64;
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut hands: Vec<[u8; NUM_KINDS]> = Vec::new();
    for _ in 0..2000 {
        let mut c = [0u8; NUM_KINDS];
        let mut left = 13;
        while left > 0 {
            let k = (rnd() % NUM_KINDS as u64) as usize;
            if c[k] < 4 {
                c[k] += 1;
                left -= 1;
            }
        }
        hands.push(c);
    }
    let visible = [0u8; NUM_KINDS];
    let n = 20_000;
    let start = Instant::now();
    let mut sink = 0i32;
    for i in 0..n {
        sink += shanten(&hands[i % hands.len()], 0) as i32;
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "shanten: {:.2} us each (cache-warm), {} calls in {:.2}s",
        secs / n as f64 * 1e6,
        n,
        secs
    );
    let n2 = 2_000;
    let start = Instant::now();
    for i in 0..n2 {
        sink += useful_kinds(&hands[i % hands.len()], 0, &visible).len() as i32;
    }
    let secs2 = start.elapsed().as_secs_f64();
    println!(
        "useful_kinds (34 shanten + overhead): {:.1} us each",
        secs2 / n2 as f64 * 1e6
    );
    println!("sink {}", sink);
}

fn main() {
    let args = parse_args();
    if args.command == "stats" {
        stats(&args);
        return;
    }
    if args.command == "bench-encode" {
        bench_encode(&args);
        return;
    }
    if args.command == "bench-shanten" {
        bench_shanten();
        return;
    }
    if args.command == "bench" {
        bench(&args);
        return;
    }
    let mut rules = Rules::tenhou();
    if !args.hanchan {
        rules = rules.single_round();
    }
    let cfg = RunnerConfig {
        rules,
        sample: args.sample,
        temperature: args.temperature,
        epsilon: args.epsilon,
        dagger_labels: args.dagger,
        dagger_min_shanten: args.dagger_shanten,
        dagger_calls: args.dagger_calls,
        teacher_v2: args.teacher_v2,
        teacher_v3: args.teacher_v3,
        teacher_v4: args.teacher_v4,
    };

    let seat_array: [SeatSpec; 4] = [args.seats[0], args.seats[1], args.seats[2], args.seats[3]];
    let (seats, record, kind): ([SeatSpec; 4], [bool; 4], &str) = match args.command.as_str() {
        "generate" => (
            seat_array,
            [
                seat_array[0] == SeatSpec::Learner,
                seat_array[1] == SeatSpec::Learner,
                seat_array[2] == SeatSpec::Learner,
                seat_array[3] == SeatSpec::Learner,
            ],
            if args.label_kind == "imitation" { "imitation" } else { "selfplay" },
        ),
        "imitate" => (
            [
                SeatSpec::Efficiency,
                SeatSpec::Efficiency,
                SeatSpec::Efficiency,
                SeatSpec::Efficiency,
            ],
            [true; 4],
            "imitation",
        ),
        _ => usage(),
    };

    let meta = json!({
        "games": args.games,
        "seed": args.seed,
        "seats": format!("{:?}", seats),
        "epsilon": args.epsilon,
        "temperature": args.temperature,
        "sample": args.sample,
        "dagger": args.dagger,
        "teacher_v2": args.teacher_v2,
        "checkpoint": args.checkpoint.as_ref().map(|p| p.display().to_string()),
        "opponent": args.opponent.as_ref().map(|p| p.display().to_string()),
        "hanchan": args.hanchan,
        "feature_dim": FEATURE_DIM,
        "policy_dim": POLICY_DIM,
    });

    let mut writer = DataWriter::create(&args.out, kind, meta).expect("cannot create output");
    let start = Instant::now();
    let mut done = 0u64;
    let mut decisions = 0u64;

    while done < args.games {
        let n = args.batch.min(args.games - done);
        let base = args.seed.wrapping_add(done);
        let checkpoint = args.checkpoint.clone();
        let opponent = args.opponent.clone();
        let hidden = args.hidden.clone();
        let cfg = cfg.clone();

        let blobs: Vec<Vec<u8>> = (0..n)
            .into_par_iter()
            .map_init(
                || {
                    let net = match &checkpoint {
                        Some(p) => Net::load(p).expect("cannot load checkpoint"),
                        None => Net::new(&hidden, FEATURE_DIM, 12345),
                    };
                    let opp = opponent
                        .as_ref()
                        .map(|p| Net::load(p).expect("cannot load opponent"));
                    Engine::new(net, opp, 0xC0FFEE)
                },
                |engine, i| {
                    let (_, blob) = engine.play_game(&cfg, seats, record, base.wrapping_add(i));
                    blob
                },
            )
            .collect();

        for blob in &blobs {
            decisions += (blob.len() / RECORD_BYTES) as u64;
            writer.write_records(blob).expect("cannot write records");
        }
        done += n;
        let secs = start.elapsed().as_secs_f64().max(1e-9);
        eprintln!(
            "  {:>7}/{} games  {} decisions  {:.1} games/s  {:.0} decisions/s",
            done,
            args.games,
            decisions,
            done as f64 / secs,
            decisions as f64 / secs
        );
    }

    let count = writer.finish().expect("cannot finalise output");
    let secs = start.elapsed().as_secs_f64();
    println!(
        "wrote {} records ({:.0} MB) to {} in {:.1}s",
        count,
        count as f64 * RECORD_BYTES as f64 / 1e6,
        args.out.display(),
        secs
    );
}
