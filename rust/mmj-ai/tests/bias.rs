use mmj_ai::{Agent, EfficiencyAgent};
use mmj_core::rules::Rules;
use mmj_core::state::TableConfig;

fn run(games: u64, eager: u8) -> ([f64; 4], [f64; 4], [f64; 4], [f64; 4]) {
    let mut scores = [0f64; 4];
    let mut wins = [0f64; 4];
    let mut deal_ins = [0f64; 4];
    let mut riichi = [0f64; 4];
    for seed in 0..games {
        let mk = |l: &str| -> Box<dyn Agent> {
            let mut a = EfficiencyAgent::new(l);
            a.config.call_eagerness = eager;
            Box::new(a)
        };
        let mut agents = [mk("a"), mk("b"), mk("c"), mk("d")];
        let config = TableConfig {
            rules: Rules::tenhou().single_round(),
            seed,
        };
        let r = mmj_ai::play_game(&mut agents, config);
        for s in 0..4 {
            scores[s] += r.scores[s] as f64;
            wins[s] += r.wins[s] as f64;
            deal_ins[s] += r.deal_ins[s] as f64;
            riichi[s] += r.riichi[s] as f64;
        }
    }
    let n = games as f64;
    (
        scores.map(|x| x / n),
        wins.map(|x| x / n),
        deal_ins.map(|x| x / n),
        riichi.map(|x| x / n),
    )
}

#[test]
fn seat_bias_check() {
    let (scores, wins, di, ri) = run(400, 1);
    println!("eager=1 scores {:?}", scores.map(|x| x.round()));
    println!("        wins {:?} deal-ins {:?} riichi {:?}",
        wins.map(|x| (x * 100.0).round() / 100.0),
        di.map(|x| (x * 100.0).round() / 100.0),
        ri.map(|x| (x * 100.0).round() / 100.0));
    let (scores0, wins0, di0, ri0) = run(400, 0);
    println!("eager=0 scores {:?}", scores0.map(|x| x.round()));
    println!("        wins {:?} deal-ins {:?} riichi {:?}",
        wins0.map(|x| (x * 100.0).round() / 100.0),
        di0.map(|x| (x * 100.0).round() / 100.0),
        ri0.map(|x| (x * 100.0).round() / 100.0));
    let spread = scores.iter().cloned().fold(f64::MIN, f64::max)
        - scores.iter().cloned().fold(f64::MAX, f64::min);
    println!("score spread across seats: {:.0}", spread);
}
