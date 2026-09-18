//! Game driver: run a whole match with four agents.
//!
//! The loop is deliberately thin — ask every pending decision, submit, repeat —
//! so that the same code drives the web server, self-play and evaluation.

use crate::Agent;
use mmj_core::state::{Event, Table, TableConfig};

/// Outcome of one match.
#[derive(Clone, Debug)]
pub struct GameResult {
    pub scores: [i32; 4],
    /// Seat order by final placement: `ranking[0]` is first place.
    pub ranking: [u8; 4],
    pub rounds: u32,
    /// Number of hands where each seat won (including 流し満貫).
    pub wins: [u32; 4],
    /// Number of hands where each seat dealt into a ron.
    pub deal_ins: [u32; 4],
    /// Number of riichi declarations per seat.
    pub riichi: [u32; 4],
    pub events: Vec<Event>,
}

/// Play one match to completion.
pub fn play_game(agents: &mut [Box<dyn Agent>; 4], config: TableConfig) -> GameResult {
    let mut table = Table::new(config);
    let mut wins = [0u32; 4];
    let mut deal_ins = [0u32; 4];
    let mut riichi = [0u32; 4];
    let mut current_round = table.rounds_played;
    let mut guard = 0usize;

    while !table.finished {
        if table.rounds_played != current_round {
            current_round = table.rounds_played;
            for (seat, agent) in agents.iter_mut().enumerate() {
                agent.on_round_start(&table, seat as u8);
            }
        }
        let decisions = table.decisions().to_vec();
        if decisions.is_empty() {
            break;
        }
        for decision in decisions {
            let seat = decision.seat as usize;
            let action = agents[seat].act(&table, decision.seat, &decision);
            match table.submit(decision.seat, action) {
                Ok(events) => {
                    for e in events {
                        match e {
                            Event::Win { seat, from, .. } => {
                                wins[seat as usize] += 1;
                                if let Some(f) = from {
                                    deal_ins[f as usize] += 1;
                                }
                            }
                            Event::Riichi { seat } => riichi[seat as usize] += 1,
                            _ => {}
                        }
                    }
                }
                Err(e) => panic!("agent {} produced an illegal action: {}", agents[seat].name(), e),
            }
            guard += 1;
            if guard > 2_000_000 {
                panic!("game did not terminate");
            }
            if table.finished {
                break;
            }
        }
    }

    let scores = table.scores();
    let mut order: Vec<(i32, u8)> = (0..4).map(|s| (scores[s as usize], s as u8)).collect();
    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ranking = [0u8; 4];
    for (place, &(_, seat)) in order.iter().enumerate() {
        ranking[place] = seat;
    }

    GameResult {
        scores,
        ranking,
        rounds: table.rounds_played,
        wins,
        deal_ins,
        riichi,
        events: table.history,
    }
}

/// Play one match, returning the table so callers can inspect the final state.
pub fn play_game_table(agents: &mut [Box<dyn Agent>; 4], config: TableConfig) -> Table {
    let mut table = Table::new(config);
    let mut guard = 0usize;
    while !table.finished {
        let decisions = table.decisions().to_vec();
        if decisions.is_empty() {
            break;
        }
        for decision in decisions {
            let seat = decision.seat as usize;
            let action = agents[seat].act(&table, decision.seat, &decision);
            table
                .submit(decision.seat, action)
                .unwrap_or_else(|e| panic!("illegal action from {}: {}", agents[seat].name(), e));
            guard += 1;
            if guard > 2_000_000 {
                panic!("game did not terminate");
            }
            if table.finished {
                break;
            }
        }
    }
    table
}
