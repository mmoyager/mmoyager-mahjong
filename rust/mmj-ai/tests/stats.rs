use mmj_ai::{Agent, play_game};
use mmj_core::rules::Rules;
use mmj_core::state::TableConfig;
use std::time::Instant;

#[test]
fn bot_match_statistics() {
    let mut totals = [0i64; 4];
    let mut wins = [0u32; 4];
    let mut deal_ins = [0u32; 4];
    let mut riichi = [0u32; 4];
    let rounds = 20u32;
    let start = Instant::now();
    for seed in 0..rounds as u64 {
        let mut agents: [Box<dyn Agent>; 4] = [
            Box::new(mmj_ai::EfficiencyAgent::new("a")),
            Box::new(mmj_ai::EfficiencyAgent::new("b")),
            Box::new(mmj_ai::EfficiencyAgent::new("c")),
            Box::new(mmj_ai::EfficiencyAgent::new("d")),
        ];
        let config = TableConfig {
            rules: Rules::tenhou().single_round(),
            seed,
        };
        let r = play_game(&mut agents, config);
        for s in 0..4 {
            totals[s] += r.scores[s] as i64;
            wins[s] += r.wins[s];
            deal_ins[s] += r.deal_ins[s];
            riichi[s] += r.riichi[s];
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "games={} elapsed={:.2}s ({:.2} games/s, {:.3}s per game)",
        rounds,
        secs,
        rounds as f64 / secs,
        secs / rounds as f64
    );
    println!("avg score per seat: {:?}", totals.map(|t| t as f64 / rounds as f64));
    println!("wins {:?} deal-ins {:?} riichi {:?}", wins, deal_ins, riichi);
    println!(
        "per game: wins {:.2}, riichi {:.2}, deal-ins {:.2}",
        wins.iter().sum::<u32>() as f64 / rounds as f64,
        riichi.iter().sum::<u32>() as f64 / rounds as f64,
        deal_ins.iter().sum::<u32>() as f64 / rounds as f64
    );
}
