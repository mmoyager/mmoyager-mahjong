//! The tile-efficiency teacher's *final* ranking, exposed as a value.
//!
//! Every criterion the teacher uses is already visible to the network as a
//! feature block (shanten after each discard, tile acceptance, keep value,
//! danger, yaku). What was never exposed is the step that *combines* them into
//! one choice — a lexicographic rule with thresholds, which a smooth network
//! approximates to about 89% and then saturates (measured: adding data or
//! epochs does not move it). Round 14 showed that residual disagreement is what
//! costs roughly 500 points against the stronger rule bot, so this module
//! computes the teacher's own preference for each discard kind:
//!
//! * when the teacher is folding — the safe tile wins, ties broken by keep value;
//! * otherwise — among discards that preserve the best shanten, the largest
//!   `acceptance + yaku/dora bonus - caution * danger * 40` wins.
//!
//! The teacher in `mmj-ai` keeps its own implementation (so that its behaviour
//! cannot change by accident and invalidate every recorded measurement); a test
//! in that crate checks that this ranking's argmax reproduces the teacher's
//! actual choice on real positions.

use crate::tile::Kind;

/// Inputs of the teacher's ranking, all of which the observation encoder already
/// computes for other blocks.
#[derive(Clone, Copy, Debug)]
pub struct RankingInputs<'a> {
    /// Shanten after discarding each kind (8 for kinds not in hand).
    pub shanten_after: &'a [i8],
    /// Tile acceptance after discarding each kind (0 when not computed).
    pub acceptance: &'a [u32; crate::tile::NUM_KINDS],
    /// Keep value of the remaining hand for each discarded kind.
    pub keep: &'a [i32; crate::tile::NUM_KINDS],
    /// Danger of each kind against the current threats.
    pub danger: &'a [f32; crate::tile::NUM_KINDS],
    /// Plausible yaku count after discarding each kind.
    pub yaku: &'a [u8; crate::tile::NUM_KINDS],
    /// Dora retained after discarding each kind.
    pub dora: &'a [f32; crate::tile::NUM_KINDS],
    /// Best shanten reachable by a discard.
    pub best_shanten: i8,
    /// Whether the teacher is in its folding branch.
    pub folding: bool,
    /// Safety weight while pushing (`0` when no threat is present).
    pub caution: f32,
    /// Whether the value-aware bonus is enabled (`yaku_aware` in the teacher).
    pub yaku_aware: bool,
    /// The kind the teacher would declare riichi on, when it can.
    ///
    /// The teacher does *not* apply its ranking in that case: it walks its action
    /// list and takes the first riichi-capable discard ("a riichi declaration is
    /// a separate action for the same tile: prefer it whenever it is available").
    /// The labels come from the teacher, quirk included, so the exposure has to
    /// reproduce it — otherwise the feature and the label disagree on ~2% of
    /// decisions, exactly the cases a network most needs to get right.
    pub riichi_kind: Option<Kind>,
}

/// Preference per discard kind: larger is better. `f32::NEG_INFINITY` marks a
/// kind the teacher would not consider at all.
pub fn discard_ranking(inputs: &RankingInputs) -> [f32; crate::tile::NUM_KINDS] {
    use crate::tile::NUM_KINDS;
    let mut out = [f32::NEG_INFINITY; NUM_KINDS];
    if let Some(kind) = inputs.riichi_kind {
        // Riichi wins outright, as it does in the teacher.
        out[kind as usize] = f32::MAX;
        return out;
    }
    for k in 0..NUM_KINDS {
        if inputs.shanten_after[k] >= 8 {
            continue; // not in hand: the encoder leaves the 8 sentinel in place
        }
        if inputs.folding {
            // Lexicographic: least dangerous first, then the least useful tile.
            // Scaled so that danger always dominates keep value.
            out[k] = -inputs.danger[k] * 1.0e3 - inputs.keep[k] as f32 * 1.0e-3;
            continue;
        }
        if inputs.shanten_after[k] > inputs.best_shanten {
            continue;
        }
        let value_bonus = if inputs.yaku_aware {
            2.5 * inputs.yaku[k] as f32 + 0.8 * inputs.dora[k]
        } else {
            0.0
        };
        out[k] = inputs.acceptance[k] as f32 + value_bonus
            - inputs.caution * inputs.danger[k] * 40.0;
    }
    out
}

/// The kind the teacher's ranking prefers, if any.
pub fn preferred_kind(ranking: &[f32; crate::tile::NUM_KINDS]) -> Option<Kind> {
    let mut best: Option<(usize, f32)> = None;
    for (k, &v) in ranking.iter().enumerate() {
        if v == f32::NEG_INFINITY {
            continue;
        }
        match best {
            Some((_, bv)) if v <= bv + 1e-4 => {}
            _ => best = Some((k, v)),
        }
    }
    best.map(|(k, _)| k as Kind)
}
