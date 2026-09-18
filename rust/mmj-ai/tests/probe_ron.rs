use mmj_ai::{Agent, EfficiencyAgent};
use mmj_core::action::{Action, ActionKind};
use mmj_core::hand::is_agari;
use mmj_core::rules::Rules;
use mmj_core::state::{Event, Table, TableConfig};
use mmj_core::tile::kind_of;

#[test]
fn probe_ron_availability() {
    let mut ron_offered = 0u32;
    let mut agari_but_no_ron = 0u32;
    let mut furiten_blocks = 0u32;
    let mut ron_taken = 0u32;
    let mut tsumo_taken = 0u32;

    for seed in 0..10u64 {
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
        let mut t = Table::new(config);
        let mut guard = 0;
        while !t.finished && guard < 100_000 {
            let ds = t.decisions().to_vec();
            if ds.is_empty() {
                break;
            }
            for d in ds {
                for a in &d.actions {
                    if a.kind() == ActionKind::Ron {
                        ron_offered += 1;
                    }
                }
                // Inspect whether this seat's hand would be complete on the discard.
                if let mmj_core::state::Trigger::Discard { from, tile } = d.trigger {
                    let p = &t.players[d.seat as usize];
                    if p.hand.iter().map(|&c| c as u32).sum::<u32>() % 3 == 1 {
                        let mut h = p.hand;
                        h[kind_of(tile) as usize] += 1;
                        if is_agari(&h, p.melds.len() as u8) {
                            if t.is_furiten(d.seat) {
                                furiten_blocks += 1;
                            } else if !d.actions.iter().any(|a| a.kind() == ActionKind::Ron) {
                                agari_but_no_ron += 1;
                            }
                        }
                    }
                    let _ = from;
                }
                let a = agents[d.seat as usize].act(&t, d.seat, &d);
                if a.kind() == ActionKind::Ron {
                    ron_taken += 1;
                }
                if a.kind() == ActionKind::Tsumo {
                    tsumo_taken += 1;
                }
                t.submit(d.seat, a).unwrap();
                guard += 1;
            }
        }
        let _ = Event::GameEnd {
            scores: [0; 4],
            ranking: [0; 4],
        };
    }
    println!(
        "ron offered={} taken={} | agari-but-no-ron-action={} | furiten blocked={} | tsumo taken={}",
        ron_offered, ron_taken, agari_but_no_ron, furiten_blocks, tsumo_taken
    );
}
