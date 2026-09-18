//! Is the exposed teacher ranking faithful?
//!
//! `mmj_nn` gives the network a feature block containing the *rule teacher's own
//! preference* for every discard (`mmj_core::pref`). The whole point of that
//! block is that its argmax is the action the teacher would take, so the network
//! can read the answer off instead of approximating a thresholded rule. If the
//! replication in `pref.rs` drifts from the teacher in `efficiency.rs`, the
//! feature silently teaches a different policy — this test is the alarm.
//!
//! It drives real positions with the teacher playing every seat, recomputes the
//! ranking inputs independently, and compares the ranking's argmax with the
//! action the teacher actually chose.

use mmj_ai::{Agent, EfficiencyAgent};
use mmj_core::action::Action;
use mmj_core::danger::danger_table_full;
use mmj_core::hand::{hand_strength, shanten, tile_keep_value, ukeire_count, yaku_score};
use mmj_core::pref::{RankingInputs, discard_ranking, preferred_kind};
use mmj_core::rules::Rules;
use mmj_core::state::{Table, TableConfig, Trigger};
use mmj_core::tile::{Kind, NUM_KINDS};

#[test]
fn exposed_ranking_reproduces_the_teachers_choices() {
    let mut teacher = EfficiencyAgent::smart("teacher");
    let mut games = 0usize;
    let (mut agree, mut total) = (0usize, 0usize);
    let (mut riichi_agree, mut riichi_total) = (0usize, 0usize);
    let mut printed = 0usize;
    let (mut folded, mut folded_total) = (0usize, 0usize);

    for seed in 0..60u64 {
        let mut table = Table::new(TableConfig {
            rules: Rules::tenhou().single_round(),
            seed,
        });
        let mut guard = 0usize;
        games += 1;
        while !table.finished && guard < 200_000 {
            let decisions = table.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            for decision in decisions {
                let seat = decision.seat;
                // Reproduce the ranking for own-turn discard decisions, then check
                // that the teacher (acting blind of it) agrees with its argmax.
                if matches!(decision.trigger, Trigger::SelfTurn)
                    && decision
                        .actions
                        .iter()
                        .any(|a| matches!(a, Action::Discard { .. }))
                    && decision.actions.len() > 1
                {
                    if let Some((ranking, folding, sh_after, acc, dng, best_sh, cau)) =
                        ranking_for(&table, seat, &decision)
                    {
                        let teacher_action = teacher.act(&table, seat, &decision);
                        if let Some(chosen) = preferred_kind(&ranking) {
                            let teacher_kind = decision.actions.iter().find_map(|a| match a {
                                Action::Discard { tile, .. } if *a == teacher_action => {
                                    Some(mmj_core::tile::kind_of(*tile))
                                }
                                _ => None,
                            });
                            if let Some(tk) = teacher_kind {
                                total += 1;
                                let riichi_open = decision
                                    .actions
                                    .iter()
                                    .any(|a| matches!(a, Action::Discard { riichi: true, .. }));
                                if riichi_open {
                                    riichi_total += 1;
                                }
                                if tk == chosen {
                                    agree += 1;
                                    if riichi_open {
                                        riichi_agree += 1;
                                    }
                                } else if printed < 4 {
                                    printed += 1;
                                    let hand: Vec<String> = (0..NUM_KINDS)
                                        .filter(|&k| table.players[seat as usize].hand[k] > 0)
                                        .map(|k| format!("{}x{}", mmj_core::tile::kind_name(k as Kind), table.players[seat as usize].hand[k]))
                                        .collect();
                                    let tk_i = tk as usize;
                                    let ch_i = chosen as usize;
                                    println!(
                                        "MISMATCH seed={} riichi={} folding={} best_sh={} caution={:.2}",
                                        seed, riichi_open, folding, best_sh, cau
                                    );
                                    println!(
                                        "  teacher {}: sh={} uke={} danger={:.3} | ranking {}: sh={} uke={} danger={:.3}",
                                        mmj_core::tile::kind_name(tk),
                                        sh_after[tk_i],
                                        acc[tk_i],
                                        dng[tk_i],
                                        mmj_core::tile::kind_name(chosen),
                                        sh_after[ch_i],
                                        acc[ch_i],
                                        dng[ch_i]
                                    );
                                    println!("  hand={:?}", hand);
                                }
                                if folding {
                                    folded_total += 1;
                                    if tk == chosen {
                                        folded += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                let action = teacher.act(&table, seat, &decision);
                if table.submit(seat, action).is_err() {
                    break;
                }
                guard += 1;
                if table.finished {
                    break;
                }
            }
        }
    }

    let rate = agree as f64 / total.max(1) as f64;
    println!(
        "games {}: ranking reproduced the teacher's pick in {}/{} decisions ({:.4}); \
         folding branch {}/{}; riichi-available {}/{} ({:.4})",
        games,
        agree,
        total,
        rate,
        folded,
        folded_total.max(1),
        riichi_agree,
        riichi_total.max(1),
        riichi_agree as f64 / riichi_total.max(1) as f64
    );
    assert!(total > 500, "not enough decisions sampled");
    // The folding branch and the riichi branch reproduce the teacher exactly.
    assert_eq!(
        folded, folded_total,
        "the folding branch must match the teacher exactly"
    );
    assert_eq!(
        riichi_agree, riichi_total,
        "the riichi branch must match the teacher exactly"
    );
    // The pushing branch still disagrees on ~2% of decisions: the mismatches are
    // always ties in acceptance where the teacher takes a different kind, so the
    // remaining gap is in a tie-break or value-bonus detail of `pref.rs` that has
    // not been identified yet. It is recorded here rather than papered over: the
    // exposed block is a *ceiling* on how faithful a student can become.
    assert!(
        rate > 0.975,
        "ranking disagrees with the teacher too often: {rate}"
    );
}

/// Recompute what the encoder feeds `discard_ranking`, from the table alone.
#[allow(clippy::type_complexity)]
fn ranking_for(
    table: &Table,
    seat: u8,
    decision: &mmj_core::state::Decision,
) -> Option<(
    [f32; NUM_KINDS],
    bool,
    [i8; NUM_KINDS],
    [u32; NUM_KINDS],
    [f32; NUM_KINDS],
    i8,
    f32,
)> {
    let me = &table.players[seat as usize];
    let melds_count = me.melds.len() as u8;
    if me.hand_len() % 3 != 2 {
        return None;
    }
    let mut sh_after = [8i8; NUM_KINDS];
    let mut acceptance = [0u32; NUM_KINDS];
    let mut keep_vals = [0i32; NUM_KINDS];
    let mut yaku_vals = [0u8; NUM_KINDS];
    let seen = table.public_counts();
    let mut best_shanten = 8i8;
    let mut rest = me.hand;
    for k in 0..NUM_KINDS {
        if rest[k] == 0 {
            continue;
        }
        rest[k] -= 1;
        sh_after[k] = shanten(&rest, melds_count);
        rest[k] += 1;
        best_shanten = best_shanten.min(sh_after[k]);
    }
    // Every in-hand kind, exactly as the encoder does it for the ranking block.
    for k in 0..NUM_KINDS {
        if rest[k] == 0 {
            continue;
        }
        rest[k] -= 1;
        acceptance[k] = ukeire_count(&rest, melds_count, &seen);
        keep_vals[k] = tile_keep_value(&rest, k as Kind);
        yaku_vals[k] = yaku_score(&rest, &me.melds, table.round_wind, table.seat_wind(seat));
        rest[k] += 1;
    }
    let danger = danger_table_full(table, seat, false, false);
    let dora_kinds = table.wall.dora_kinds();
    let mut dora_kept = [0f32; NUM_KINDS];
    for k in 0..NUM_KINDS {
        if me.hand[k] == 0 {
            continue;
        }
        rest[k] -= 1;
        dora_kept[k] = dora_kinds
            .iter()
            .filter(|&&d| rest[d as usize] > 0)
            .count() as f32;
        rest[k] += 1;
    }
    let dora_in_hand: u8 = dora_kinds.iter().map(|&d| me.hand[d as usize]).sum();
    let strength = hand_strength(
        &me.hand,
        &me.melds,
        shanten(&me.hand, melds_count),
        dora_in_hand,
        table.round_wind,
        table.seat_wind(seat),
    );
    let threatened = danger.iter().any(|&d| d > 0.0);
    let folding = threatened && strength < 2.5;
    let caution = if !threatened {
        0.0
    } else if strength >= 2.5 {
        0.2
    } else {
        0.7
    };
    // Mirror `encode`: the teacher takes the first riichi-capable discard whose
    // wait is not dead, and never applies the ranking in that case.
    let riichi_kind = decision
        .actions
        .iter()
        .find_map(|a| match a {
            Action::Discard { tile, riichi: true } => Some(mmj_core::tile::kind_of(*tile)),
            _ => None,
        })
        .filter(|&k| {
            let mut r2 = me.hand;
            if r2[k as usize] == 0 {
                return false;
            }
            r2[k as usize] -= 1;
            let visible = table.public_counts();
            mmj_core::hand::winning_kinds(&r2, melds_count)
                .iter()
                .map(|&w| 4u32.saturating_sub(visible[w as usize] as u32))
                .sum::<u32>()
                >= 1
        });
    let ranking = discard_ranking(&RankingInputs {
        shanten_after: &sh_after,
        acceptance: &acceptance,
        keep: &keep_vals,
        danger: &danger,
        yaku: &yaku_vals,
        dora: &dora_kept,
        best_shanten,
        folding,
        caution,
        yaku_aware: true,
        riichi_kind,
    });
    Some((ranking, folding, sh_after, acceptance, danger, best_shanten, caution))
}
