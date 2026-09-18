//! Tile safety against opponents who have declared riichi.
//!
//! All three parts of the standard model matter and are easy to get wrong:
//!
//! * **現物** — a tile the threat has already discarded can never be ronned by
//!   them, including everything they discarded before declaring.
//! * **同巡の安全牌** — a tile somebody discarded since the threat's last draw
//!   is safe for the rest of this go-around: the threat passed on it and is in
//!   同巡振聴 until they draw again.
//! * **筋 and 壁** — a ryanmen wait on a tile needs one of two specific pairs of
//!   neighbours, so a broken 筋 is much less likely to be waited on and an
//!   exhausted neighbour pair rules the shape out entirely.
//!
//! [`danger_table`] returns a `0..1` danger score per tile kind, and is used
//! both by the rule-based agent (to fold) and by the observation encoder (so a
//! network imitating that agent can see what it sees).

use crate::meld::MeldKind;
use crate::state::Table;
use crate::tile::{Kind, NUM_KINDS, is_honor, kind_of};

/// Danger of discarding each tile kind, `0` meaning "safe against every
/// opponent currently in riichi".
pub fn danger_table(table: &Table, seat: u8) -> [f32; NUM_KINDS] {
    danger_table_with(table, seat, false)
}

/// As [`danger_table`], but optionally treating well-developed *open* hands as
/// threats too.
///
/// A player who has called three melds is very likely to be waiting even
/// without riichi, and two melds late in the hand is suspicious as well. 現物
/// and 同巡 safety apply to them exactly as they do to a riichi player, so only
/// the weighting changes.
pub fn danger_table_with(table: &Table, seat: u8, open_threats: bool) -> [f32; NUM_KINDS] {
    danger_table_full(table, seat, open_threats, false)
}

/// As [`danger_table_full`] with sequence reading disabled: the historical model.
pub fn danger_table_shape(table: &Table, seat: u8, open_threats: bool, strict_suji: bool) -> [f32; NUM_KINDS] {
    danger_table_full_with(table, seat, open_threats, strict_suji, false)
}

/// As [`danger_table_with`], optionally distinguishing 片筋 from 両筋: a tile
/// whose *both* three-away neighbours have been discarded cannot be completed
/// into a ryanmen from either side, which makes it far safer than a tile that
/// has only one side broken.
pub fn danger_table_full(
    table: &Table,
    seat: u8,
    open_threats: bool,
    strict_suji: bool,
) -> [f32; NUM_KINDS] {
    danger_table_full_with(table, seat, open_threats, strict_suji, false)
}

/// As [`danger_table_full`], with an optional **sequence** read.
///
/// Everything above this point treats a threat's discards as a *set*: 現物 and
/// 筋 both ask which tiles have appeared. A human also reads the *order*: a
/// player who has already thrown away several tiles of one suit is unlikely to be
/// waiting in that suit, so its remaining tiles are safer than the shape-based
/// model says. `suit_reading` counts the threat's discards of each suit up to
/// their riichi declaration and discounts the whole suit accordingly. It is the
/// first criterion in the teacher that uses discard order at all.
pub fn danger_table_full_with(
    table: &Table,
    seat: u8,
    open_threats: bool,
    strict_suji: bool,
    suit_reading: bool,
) -> [f32; NUM_KINDS] {
    danger_table_reading(
        table,
        seat,
        open_threats,
        strict_suji,
        DangerReads {
            abandoned_suit: suit_reading,
            ..DangerReads::none()
        },
    )
}

/// Which sequence reads the safety model applies.
#[derive(Clone, Copy, Debug, Default)]
pub struct DangerReads {
    /// Discount a suit the threat has largely thrown away before claiming.
    pub abandoned_suit: bool,
    /// Weight a threat's *late* discards more heavily than early ones: a tile
    /// thrown at turn 12 says far more about the finished hand than one thrown
    /// at turn 2.
    pub timing: bool,
    /// Raise the danger of suits the threat has called melds in: an open hand
    /// with three tiles of one suit is collecting that suit.
    pub melded_suit: bool,
}

impl DangerReads {
    pub const fn none() -> Self {
        DangerReads {
            abandoned_suit: false,
            timing: false,
            melded_suit: false,
        }
    }
}

/// As [`danger_table_full_with`], with explicit control over each sequence read.
pub fn danger_table_reading(
    table: &Table,
    seat: u8,
    open_threats: bool,
    strict_suji: bool,
    reads: DangerReads,
) -> [f32; NUM_KINDS] {
    let mut danger = [0.0f32; NUM_KINDS];
    let mut weighted: Vec<(usize, f32)> = Vec::new();
    for s in 0..4usize {
        if s == seat as usize {
            continue;
        }
        let p = &table.players[s];
        // A player who has declined a legal ron cannot ron *anything* until
        // their next draw (同巡振聴), and a riichi player who does it once is
        // furiten for the rest of the round (立直後見逃し). Against such a
        // player every tile on the table is completely safe, so they leave the
        // threat set entirely. Note this is not the same as 捨て牌振聴: a
        // discard-furiten player may still ron tiles they have not discarded,
        // and those are exactly the tiles 現物 already marks as safe below.
        if p.temp_furiten || p.riichi_furiten {
            continue;
        }
        if p.riichi {
            weighted.push((s, 1.0));
        } else if open_threats {
            let melds = p.melds.iter().filter(|m| m.kind.is_open()).count();
            if melds >= 3 {
                weighted.push((s, 0.6));
            } else if melds == 2 {
                weighted.push((s, 0.35));
            }
        }
    }
    if weighted.is_empty() {
        return danger;
    }
    let threats: Vec<usize> = weighted.iter().map(|&(s, _)| s).collect();

    // Public tiles: every discard plus every exposed meld.
    let mut public = [0u8; NUM_KINDS];
    for p in table.players.iter() {
        for d in &p.discards {
            public[kind_of(d.tile) as usize] += 1;
        }
        for m in &p.melds {
            if m.kind == MeldKind::Ankan {
                continue; // still concealed
            }
            for &t in m.as_slice() {
                public[kind_of(t) as usize] += 1;
            }
        }
    }

    // Sequence read: how many tiles of each suit each threat threw away before
    // declaring riichi (or before now, for a threat that has not declared).
    let mut suit_discards = [[0u8; 3]; 4];
    let mut meld_tiles = [[0u8; 3]; 4];
    if reads.abandoned_suit || reads.timing || reads.melded_suit {
        for &t in &threats {
            let p = &table.players[t];
            let upto = if p.riichi {
                p.discards.iter().position(|d| d.riichi).unwrap_or(p.discards.len())
            } else {
                p.discards.len()
            };
            for (i, d) in p.discards.iter().take(upto).enumerate() {
                let k = kind_of(d.tile);
                if is_honor(k) {
                    continue;
                }
                let suit = (k / 9) as usize;
                // Late discards are worth twice an early one.
                let weight = if reads.timing && i >= 6 { 2u8 } else { 1u8 };
                suit_discards[t][suit] = suit_discards[t][suit].saturating_add(weight);
            }
            if reads.melded_suit {
                for m in &p.melds {
                    if m.kind == MeldKind::Ankan {
                        continue; // still concealed
                    }
                    for &tile in m.as_slice() {
                        let k = kind_of(tile);
                        if !is_honor(k) {
                            let suit = (k / 9) as usize;
                            meld_tiles[t][suit] = meld_tiles[t][suit].saturating_add(1);
                        }
                    }
                }
            }
        }
    }

    // One mask per threat instead of rescanning their discards per tile kind.
    let mut genbutsu = [0u64; 4];
    let mut same_turn = [0u64; 4];
    for &t in &threats {
        for d in &table.players[t].discards {
            genbutsu[t] |= 1 << kind_of(d.tile);
        }
        let passed = table.players[t].discards.len();
        for (i, q) in table.players.iter().enumerate() {
            if i == t {
                continue;
            }
            for d in q.discards.iter().skip(passed) {
                same_turn[t] |= 1 << kind_of(d.tile);
            }
        }
    }

    for k in 0..NUM_KINDS {
        let kind = k as Kind;
        let bit = 1u64 << k;
        let mut worst = 0.0f32;
        for &t in &threats {
            if genbutsu[t] & bit != 0 || same_turn[t] & bit != 0 {
                continue;
            }
            let mut d = if is_honor(kind) {
                0.30
            } else {
                match k % 9 {
                    0 | 8 => 0.45,
                    1 | 7 => 0.62,
                    2 | 6 => 0.82,
                    _ => 1.0,
                }
            };
            if !is_honor(kind) {
                let base = (k / 9) * 9;
                let n = k % 9;
                let low_suji = n >= 3 && genbutsu[t] & (1u64 << (base + n - 3)) != 0;
                let high_suji = n + 3 <= 8 && genbutsu[t] & (1u64 << (base + n + 3)) != 0;
                if strict_suji {
                    match (low_suji, high_suji) {
                        (true, true) => d *= 0.30, // 両筋
                        (true, false) | (false, true) => d *= 0.55, // 片筋
                        (false, false) => {}
                    }
                } else if low_suji || high_suji {
                    d *= 0.55;
                }
                let exhausted = |x: i32| -> bool {
                    if !(0..9).contains(&x) {
                        return true;
                    }
                    public[base + x as usize] >= 4
                };
                let ni = n as i32;
                let mut live_sides = 0;
                if !(exhausted(ni - 2) || exhausted(ni - 1)) {
                    live_sides += 1;
                }
                if !(exhausted(ni + 1) || exhausted(ni + 2)) {
                    live_sides += 1;
                }
                if live_sides == 0 {
                    d *= 0.45;
                } else if live_sides == 1 {
                    d *= 0.8;
                }
            }
            if reads.abandoned_suit && !is_honor(kind) {
                // An abandoned suit: enough tiles thrown away before the threat's
                // claim means they are building elsewhere.
                let dropped = suit_discards[t][(k / 9) as usize];
                d *= match dropped {
                    0..=1 => 1.0,
                    2..=5 => 0.88,
                    _ => 0.72,
                };
            }
            if reads.melded_suit && !is_honor(kind) {
                // An open hand holding several tiles of a suit is collecting it.
                let held = meld_tiles[t][(k / 9) as usize];
                if held >= 3 {
                    d *= 1.2;
                } else if held >= 2 {
                    d *= 1.1;
                }
            }
            let live = 4u32.saturating_sub(public[k] as u32) as f32 / 4.0;
            d *= 0.35 + 0.65 * live;
            let weight = weighted
                .iter()
                .find(|&&(s, _)| s == t)
                .map(|&(_, w)| w)
                .unwrap_or(1.0);
            worst = worst.max(d * weight);
        }
        danger[k] = worst;
    }
    danger
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;
    use crate::state::{Table, TableConfig};

    fn table() -> Table {
        Table::new(TableConfig {
            rules: Rules::tenhou(),
            seed: 4242,
        })
    }

    /// A riichi player who has declined a ron cannot ron again this round, so
    /// discarding against them is free — a rule the safety model used to miss.
    #[test]
    fn furiten_riichi_player_is_not_a_threat() {
        let mut t = table();
        t.players[1].riichi = true;
        let before = danger_table(&t, 0);
        assert!(
            before.iter().any(|&d| d > 0.0),
            "a riichi player should make some tiles dangerous"
        );

        t.players[1].riichi_furiten = true;
        let after = danger_table(&t, 0);
        assert_eq!(
            after,
            [0.0f32; NUM_KINDS],
            "a furiten riichi player cannot ron anything, so nothing is dangerous"
        );

        // 同巡振聴 expires on their next draw, so the threat has to come back.
        t.players[1].riichi_furiten = false;
        t.players[1].temp_furiten = true;
        assert_eq!(danger_table(&t, 0), [0.0f32; NUM_KINDS]);
        t.players[1].temp_furiten = false;
        assert_eq!(danger_table(&t, 0), before);
    }

    /// An open hand treated as a threat must obey the same rule.
    #[test]
    fn furiten_open_hand_is_not_a_threat() {
        let mut t = table();
        t.players[2].temp_furiten = true;
        let with_threats = danger_table_with(&t, 0, true);
        assert_eq!(with_threats, [0.0f32; NUM_KINDS]);
    }
}
