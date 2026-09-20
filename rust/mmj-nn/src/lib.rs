//! `mmj-nn` — observation encoding and a tiny dependency-free inference engine
//! for the mahjong policy/value network.
//!
//! # Why a hand-written network
//!
//! Training happens in Python (PyTorch), but *playing* happens here: self-play
//! generates millions of games, and the browser needs to answer in
//! milliseconds. Inference is therefore a plain multi-layer perceptron
//! (`y = act(W·x + b)`) evaluated directly in Rust, with the weights read from a
//! checkpoint written by the trainer.
//!
//! # Checkpoint format
//!
//! ```text
//! line 1  "MMJNN1\n"
//! line 2  JSON header: {"layers":[{"in":545,"out":512,"act":"relu"}, ...]}
//! then    little-endian f32 weights, layer by layer, each layer `out*in`
//!         weights in row-major order followed by `out` biases
//! ```
//!
//! Everything about the input layout lives in [`encode`] on this side, so the
//! Python trainer never has to reproduce it: it only needs the same layer
//! shapes, which it reads from the header it wrote itself.

pub mod data;

use mmj_core::action::{ACTION_SPACE, Action};
use mmj_core::danger::danger_table_full;
use mmj_core::hand::{
    hand_strength, shanten, tile_keep_value, ukeire_count, winning_kinds, yaku_score,
};
use mmj_core::meld::MeldKind;
use mmj_core::state::{Decision, Table, Trigger};
use mmj_core::tile::{Kind, NUM_KINDS, kind_of};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::Path;

/// Number of policy outputs.
pub const POLICY_DIM: usize = ACTION_SPACE;

// ---------------------------------------------------------------------------
// Observation encoding
// ---------------------------------------------------------------------------

/// Per-player block: discard counts, meld counts, meld type counters.
const PER_PLAYER: usize = NUM_KINDS + NUM_KINDS + 6;
/// Self-only block: concealed counts and red-five flags.
const SELF_BLOCK: usize = NUM_KINDS + 3;
/// Round/turn context block.
const CONTEXT: usize = 4 + 4 + 1 + 1;
/// Derived "what happens if I discard each kind" block (see [`encode`]).
const DISCARD_LOOKAHEAD: usize = NUM_KINDS;
/// Tile acceptance (進張) for the most promising discards.
const UKEIRE_LOOKAHEAD: usize = NUM_KINDS;
/// The rule-based teacher's final tie-break, exposed per discard candidate.
const KEEP_VALUE: usize = NUM_KINDS;
/// How dangerous each tile is right now (現物 / 同巡 / 筋 / 壁).
const DANGER: usize = NUM_KINDS;
/// Plausible yaku count if each tile is discarded.
const YAKU_AFTER: usize = NUM_KINDS;
/// The rule teacher's own preference for each discard, exposed so the network can
/// learn its combination rule instead of approximating it.
const TEACHER_RANK: usize = NUM_KINDS;
/// Decision context: trigger kind (3), called tile (34), relative discarder (4).
const DECISION_BLOCK: usize = 3 + NUM_KINDS + 4;
// Round 31 appended a "threat read" block here (what a riichi player has thrown
// since declaring, 16 features, 737 -> 753). It measured *worse* than the
// baseline both against the rule bot (+16 ± 54 is zero, and the paired comparison
// against a zero-block control arm was −72.8 ± 72) and was not adopted, so the
// encoder is back to 737 dimensions and the block is gone rather than left in the
// tree: a build with it silently produces data the served lineage cannot train
// on. See docs/TRAINING.md section 42.

/// Total number of input features. See [`encode`] for the layout.
pub const FEATURE_DIM: usize = SELF_BLOCK
    + 4 * PER_PLAYER
    + NUM_KINDS
    + NUM_KINDS
    + 2 * NUM_KINDS
    + 4
    + CONTEXT
    + 4
    + 5
    + DISCARD_LOOKAHEAD
    + UKEIRE_LOOKAHEAD
    + KEEP_VALUE
    + DANGER
    + YAKU_AFTER
    + TEACHER_RANK
    + DECISION_BLOCK;

/// Meld type counters inside each per-player block.
const MELD_TYPES: [MeldKind; 5] = [
    MeldKind::Chi,
    MeldKind::Pon,
    MeldKind::Ankan,
    MeldKind::Minkan,
    MeldKind::Kakan,
];

/// Scratch buffer for one observation.
#[derive(Clone, Debug)]
pub struct Obs {
    pub features: Vec<f32>,
    /// Legal action mask over [`POLICY_DIM`] slots.
    pub mask: Vec<u8>,
    /// Slot chosen for each legal action, aligned with [`Obs::slots`].
    pub slots: Vec<usize>,
    pub actions: Vec<Action>,
}

impl Default for Obs {
    fn default() -> Self {
        Obs {
            features: vec![0.0; FEATURE_DIM],
            mask: vec![0; POLICY_DIM],
            slots: Vec::new(),
            actions: Vec::new(),
        }
    }
}

impl Obs {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Encode the state from `seat`'s point of view.
///
/// The layout, in order:
///
/// | block | dims | contents |
/// |---|---|---|
/// | self | 34 | concealed counts / 4 |
/// | self | 3 | red fives held |
/// | 4× player | 34 | discard counts / 4 |
/// | 4× player | 34 | meld tile counts / 4 |
/// | 4× player | 6 | chi/pon/ankan/minkan/kakan counts, riichi flag |
/// | all | 34 | visible counts (discards + melds + own hand) / 4 |
/// | all | 34 | live copies remaining / 4 |
/// | all | 68 | dora indicator one-hot, dora multiplicity |
/// | scores | 4 | (score − 25000) / 25000 |
/// | context | 10 | round wind, seat wind, dealer, wall, honba, sticks, round no. |
/// | flags | 4 | this seat's riichi, ippatsu, furiten, tenpai |
/// | misc | 5 | handheld count, shanten/4, aka total, turn index, kan count |
/// | decision | 41 | trigger kind, called tile one-hot, relative discarder |
///
/// Players are rotated so that index 0 is always the acting seat.
pub fn encode(table: &Table, seat: u8, decision: &Decision, obs: &mut Obs) {
    // The teacher's riichi quirk needs the action list, which `encode_state` does
    // not see, so it is resolved here and handed down.
    let melds = table.players[seat as usize].melds.len() as u8;
    let riichi_kind = decision.actions.iter().find_map(|a| match a {
        Action::Discard { tile, riichi: true } => Some(kind_of(*tile)),
        _ => None,
    });
    encode_state(table, seat, decision.trigger, obs, riichi_kind.filter(|&k| wait_is_live(table, seat, k, melds)));
    // ---- action mask ----
    obs.mask.iter_mut().for_each(|m| *m = 0);
    obs.slots.clear();
    obs.actions.clear();
    for a in &decision.actions {
        if let Some(slot) = a.index() {
            if slot < POLICY_DIM {
                obs.mask[slot] = 1;
                obs.slots.push(slot);
                obs.actions.push(*a);
            }
        }
    }
}

/// Encode a state without a decision, for value-only lookahead.
///
/// The action mask is left empty, so only the value head is meaningful.
pub fn encode_state(
    table: &Table,
    seat: u8,
    trigger: Trigger,
    obs: &mut Obs,
    riichi_kind: Option<Kind>,
) {
    let f = &mut obs.features;
    f.fill(0.0);
    let me = &table.players[seat as usize];

    let mut at = 0usize;
    let mut push = |v: f32| {
        f[at] = v;
        at += 1;
    };

    // ---- self block ----
    for k in 0..NUM_KINDS {
        push(me.hand[k] as f32 / 4.0);
    }
    let aka = me.hand_tiles.iter().filter(|&&t| table.rules.is_aka(t)).count();
    let meld_aka: u8 = me.melds.iter().map(|m| m.aka_count(&table.rules)).sum();
    push(aka as f32 / 3.0);
    push(meld_aka as f32 / 3.0);
    push(if me.is_closed() { 1.0 } else { 0.0 });

    // ---- per player blocks, rotated so that relative seat 0 is self ----
    for rel in 0..4u8 {
        let s = (seat + rel) % 4;
        let p = &table.players[s as usize];
        let mut discards = [0u8; NUM_KINDS];
        for d in &p.discards {
            discards[kind_of(d.tile) as usize] += 1;
        }
        for k in 0..NUM_KINDS {
            push(discards[k] as f32 / 4.0);
        }
        let mut meld_tiles = [0u8; NUM_KINDS];
        for m in &p.melds {
            for &t in m.as_slice() {
                meld_tiles[kind_of(t) as usize] += 1;
            }
        }
        for k in 0..NUM_KINDS {
            push(meld_tiles[k] as f32 / 4.0);
        }
        for kind in MELD_TYPES {
            let n = p.melds.iter().filter(|m| m.kind == kind).count();
            push(n as f32 / 4.0);
        }
        push(if p.riichi { 1.0 } else { 0.0 });
    }

    // ---- global visible / live ----
    let visible = table.all_visible_counts();
    for k in 0..NUM_KINDS {
        push(visible[k] as f32 / 4.0);
    }
    for k in 0..NUM_KINDS {
        let own = me.hand[k];
        let live = 4u8.saturating_sub(visible[k].saturating_add(own));
        push(live as f32 / 4.0);
    }
    let mut indicators = [0u8; NUM_KINDS];
    for n in 0..table.wall.revealed_indicators() {
        if let Some(t) = table.wall.dora_indicator(n) {
            indicators[kind_of(t) as usize] += 1;
        }
    }
    for k in 0..NUM_KINDS {
        push(indicators[k] as f32 / 4.0);
    }
    let dora_kinds = table.wall.dora_kinds();
    let mut dora_count = [0u8; NUM_KINDS];
    for k in dora_kinds {
        dora_count[k as usize] += 1;
    }
    for k in 0..NUM_KINDS {
        push(dora_count[k] as f32 / 4.0);
    }

    // ---- scores (mapped into [0, 1]; 0.5 means "at the starting score") ----
    for s in 0..4 {
        push((table.players[s].score as f32 - 25000.0) / 100_000.0 + 0.5);
    }

    // ---- round context ----
    for w in 0..4 {
        push(if table.round_wind == mmj_core::tile::EAST + w { 1.0 } else { 0.0 });
    }
    let seat_wind = table.seat_wind(seat);
    for w in 0..4 {
        push(if seat_wind == mmj_core::tile::EAST + w { 1.0 } else { 0.0 });
    }
    push(if table.dealer == seat { 1.0 } else { 0.0 });
    push(table.wall.remaining() as f32 / 70.0);

    // ---- self flags ----
    push(if me.riichi { 1.0 } else { 0.0 });
    push(if me.ippatsu { 1.0 } else { 0.0 });
    push(if table.is_furiten(seat) { 1.0 } else { 0.0 });
    let sh = shanten(&me.hand, me.melds.len() as u8);
    push(if sh <= 0 { 1.0 } else { 0.0 });

    // ---- misc ----
    push(me.hand_len() as f32 / 14.0);
    push((sh as f32 + 1.0) / 9.0);
    push(aka as f32 / 3.0);
    push(table.round_number as f32 / 4.0);
    push(table.wall.kan_count() as f32 / 4.0);

    // ---- discard lookahead ----
    //
    // Tile efficiency is the backbone of every mahjong policy, and "what is my
    // shanten if I drop this tile" is the single most informative function of
    // the hand. Handing it to the network directly costs one shanten scan per
    // decision and makes both imitation and reinforcement learning far easier;
    // it is information the player may legally compute.
    //
    // The second block is tile acceptance: how many tiles are still live that
    // would improve the hand after discarding this kind. That is the tie-break
    // a tile-efficiency player uses among discards that leave the same shanten,
    // and without it the network has no way to tell those apart — which is
    // exactly where its remaining errors were concentrated.
    {
        let melds_count = me.melds.len() as u8;
        let on_turn = me.hand_len() % 3 == 2;
        let mut after = me.hand;
        let mut sh_after = [8i8; NUM_KINDS];
        if on_turn {
            for k in 0..NUM_KINDS {
                if after[k] > 0 {
                    after[k] -= 1;
                    sh_after[k] = shanten(&after, melds_count);
                    after[k] += 1;
                }
            }
        }
        for k in 0..NUM_KINDS {
            let v = if on_turn && me.hand[k] > 0 {
                // -1 (already winning) maps to 1.0, 8 shanten to 0.0.
                ((8.0 - sh_after[k] as f32) / 9.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            push(v);
        }
        let best_shanten = if on_turn {
            (0..NUM_KINDS)
                .filter(|&k| me.hand[k] > 0)
                .map(|k| sh_after[k])
                .min()
                .unwrap_or(8)
        } else {
            8
        };
        let seen = table.public_counts();
        // Tile acceptance is by far the most expensive thing in this function:
        // one `ukeire_count` is itself a 34-tile shanten scan (~30 us), so
        // measuring all ~13 in-hand kinds costs about 400 us per decision --
        // more than ten times the network forward. Only the kinds that *tie*
        // the best shanten are ever read: the feature block below requires
        // `sh_after[k] == best_shanten`, and `pref::discard_ranking` skips every
        // worse kind before it touches `acceptance`, while its folding branch
        // reads no acceptance at all. Computing the others was pure waste, and
        // skipping them leaves every stored value (and therefore every label)
        // identical. `keep_vals` and `yaku_vals` are consumed for *all* in-hand
        // kinds by their own feature blocks, so they stay unconditional.
        let mut accept = [0u32; NUM_KINDS];
        let mut keep_vals = [0i32; NUM_KINDS];
        let mut yaku_vals = [0u8; NUM_KINDS];
        if on_turn {
            for k in 0..NUM_KINDS {
                if me.hand[k] == 0 {
                    continue;
                }
                let mut rest = me.hand;
                rest[k] -= 1;
                if sh_after[k] == best_shanten {
                    accept[k] = ukeire_count(&rest, melds_count, &seen);
                }
                keep_vals[k] = tile_keep_value(&rest, k as Kind);
                yaku_vals[k] = yaku_score(&rest, &me.melds, table.round_wind, table.seat_wind(seat));
            }
        }
        let mut computed = 0;
        for k in 0..NUM_KINDS {
            let v = if on_turn
                && me.hand[k] > 0
                && sh_after[k] == best_shanten
                && computed < 8
            {
                computed += 1;
                (accept[k] as f32 / 24.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            push(v);
        }
        // Third and fourth blocks: the tie-break the teacher applies when two
        // discards leave the same shanten *and* the same tile acceptance, and
        // how many yaku the hand would still be heading for.
        //
        // The last two blocks are what let the network imitate the *improved*
        // teacher at all: that teacher folds on a safety model and pushes for
        // yaku, and a network that cannot see tile danger or yaku potential
        // simply cannot reproduce those decisions (measured: imitation fidelity
        // dropped from 89% to 85% without them, and the teacher's strength did
        // not transfer).
        let mut rest = me.hand;
        for k in 0..NUM_KINDS {
            let v = if on_turn && rest[k] > 0 {
                (keep_vals[k] as f32 / 16.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            push(v);
        }
        // Danger of each tile against the opponents in riichi.
        //
        // Configuration history, because this block has moved twice:
        //   * round 14: open-hand threats made the *teacher* worse (-31 to -169)
        //     when used as a criterion;
        //   * round 22: a discard-*sequence* read looked like +198 as a feature;
        //   * round 25: a properly powered A/B (three replicas per arm) refuted
        //     that -- the legacy block measured +271.5 against the new one's
        //     +204.2. So the legacy configuration is back, which also restores
        //     consistency with the data the current best checkpoint was trained
        //     on (im-v12).
        let danger = danger_table_full(table, seat, true, true);
        for k in 0..NUM_KINDS {
            push(danger[k].clamp(0.0, 1.0));
        }
        // Yaku still available after discarding each kind.
        for k in 0..NUM_KINDS {
            let v = if on_turn && rest[k] > 0 {
                yaku_vals[k] as f32 / 3.0
            } else {
                0.0
            };
            push(v);
        }

        // ---- the teacher's own ranking (see `mmj_core::pref`) ----
        //
        // Every criterion above was already visible; this block exposes the
        // *combination* the teacher applies, which is what a smooth network
        // approximates badly (fidelity saturated at ~0.89 no matter how much
        // data it saw). The values are shifted so the teacher's own choice reads
        // 1.0 and alternatives decay with their score gap.
        {
            use mmj_core::pref::{RankingInputs, discard_ranking};
            // The teacher of the imitation labels uses its own safety settings,
            // which differ from the danger block above (that one is deliberately
            // richer: open threats and strict suji).
            let teacher_danger = danger_table_full(table, seat, false, false);
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
            let dora_in_hand: u8 = dora_kinds
                .iter()
                .map(|&d| me.hand[d as usize])
                .sum();
            let strength = hand_strength(
                &me.hand,
                &me.melds,
                shanten(&me.hand, melds_count),
                dora_in_hand,
                table.round_wind,
                table.seat_wind(seat),
            );
            let threatened = teacher_danger.iter().any(|&d| d > 0.0);
            // Mirrors `EfficiencyAgent::smart()`: fold_strength 2.5, push_fold on,
            // defense weight 1, no placement or turn awareness.
            let folding = threatened && strength < 2.5;
            let caution = if !threatened {
                0.0
            } else if strength >= 2.5 {
                0.2
            } else {
                0.7
            };
            let ranking = discard_ranking(&RankingInputs {
                shanten_after: &sh_after,
                acceptance: &accept,
                keep: &keep_vals,
                danger: &teacher_danger,
                yaku: &yaku_vals,
                dora: &dora_kept,
                best_shanten,
                folding,
                caution,
                yaku_aware: true,
                riichi_kind,
            });
            let top = ranking.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            for k in 0..NUM_KINDS {
                let v = if top.is_finite() && ranking[k].is_finite() {
                    ((ranking[k] - top) / 4.0).clamp(-20.0, 0.0).exp()
                } else {
                    0.0
                };
                push(v);
            }
        }
    }
    // ---- decision context ----
    let (kind_index, called, from) = match trigger {
        Trigger::SelfTurn => (0usize, None, seat),
        Trigger::Discard { from, tile } => (1, Some(tile), from),
        Trigger::Chankan { from, tile } => (2, Some(tile), from),
    };
    for i in 0..3 {
        push(if i == kind_index { 1.0 } else { 0.0 });
    }
    let called_kind = called.map(kind_of);
    for k in 0..NUM_KINDS {
        push(if called_kind == Some(k as Kind) { 1.0 } else { 0.0 });
    }
    let rel_from = (from + 4 - seat) % 4;
    for r in 0..4 {
        push(if r == rel_from { 1.0 } else { 0.0 });
    }

    debug_assert_eq!(at, FEATURE_DIM, "feature layout drifted");
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// Activation function of a dense layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Act {
    Relu,
    Tanh,
    /// No activation (used by output heads).
    Linear,
}

/// A dense layer.
///
/// The checkpoint format stores weights row-major as `out × in`, but inference
/// keeps them **transposed** (`in × out`). That turns the layer into a sequence
/// of scaled vector adds instead of many dot products: there is no reduction
/// and therefore no dependency chain, so the compiler emits full-width vector
/// FMAs, and an input feature that is zero can be skipped entirely (many are —
/// one-hot wind blocks, empty discard piles). Measured on the target machine
/// this is several times faster than the row-major dot product.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub r#in: usize,
    pub out: usize,
    pub act: Act,
    /// Weights transposed to `in × out`, row-major.
    #[serde(skip)]
    pub wt: Vec<f32>,
    /// The same weights rounded to half precision, used by the inference
    /// kernel when the CPU has F16C.
    ///
    /// The forward is bound by weight *bytes* (a 64x64 network whose weights fit
    /// entirely in cache moves no more bytes per second than the 4.85 MB body
    /// does), so halving the width of every weight is the one lever that does
    /// not need a smaller network. Rebuilt by [`Layer::rebuild_half`] whenever
    /// the weights change; if it is ever out of step the kernel falls back to
    /// the f32 path rather than reading stale values.
    #[serde(skip)]
    pub wt_h: Vec<u16>,
    #[serde(skip)]
    pub b: Vec<f32>,
}

/// Would discarding `kind` leave a wait with at least one live tile?
fn wait_is_live(table: &Table, seat: u8, kind: Kind, melds: u8) -> bool {
    let me = &table.players[seat as usize];
    if me.hand[kind as usize] == 0 {
        return false;
    }
    let mut rest = me.hand;
    rest[kind as usize] -= 1;
    let visible = table.public_counts();
    winning_kinds(&rest, melds)
        .iter()
        .map(|&w| 4u32.saturating_sub(visible[w as usize] as u32))
        .sum::<u32>()
        >= 1
}

/// Round an `f32` to IEEE binary16 (round-half-to-even), as a bit pattern.
///
/// Software rather than `_cvtss_sh` so that building the half-precision copy of
/// a checkpoint never depends on the CPU that happens to run it: the quantised
/// weights, and therefore the numbers the service and the evaluator report, must
/// be identical on every machine.
pub fn f32_to_half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let raw_exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x007f_ffff;
    if raw_exp == 0xff {
        // Infinity, or a NaN: keep it a NaN (the payload is not preserved).
        return sign | 0x7c00 | if man != 0 { 0x0200 } else { 0 };
    }
    let exp = raw_exp - 127 + 15;
    if exp >= 0x1f {
        return sign | 0x7c00; // overflows binary16
    }
    if exp <= 0 {
        if exp < -10 {
            return sign; // underflows to zero
        }
        let man = man | 0x0080_0000;
        let shift = (14 - exp) as u32;
        let half = (man >> shift) as u16;
        let rem = man & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        let round_up = rem > halfway || (rem == halfway && half & 1 == 1);
        return sign | (half + round_up as u16);
    }
    let half = (man >> 13) as u16;
    let rem = man & 0x1fff;
    let mut out = sign | ((exp as u16) << 10) | half;
    if rem > 0x1000 || (rem == 0x1000 && half & 1 == 1) {
        out = out.wrapping_add(1); // a carry into the exponent is correct here
    }
    out
}

/// The exact inverse of [`f32_to_half`] for every finite input.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = ((h as u32) & 0x8000) << 16;
    let exp = ((h >> 10) & 0x1f) as u32;
    let man = (h & 0x3ff) as u32;
    let bits = match exp {
        0 => {
            if man == 0 {
                sign
            } else {
                // Subnormal half: the value is exactly `man * 2^-24`, so the
                // float exponent comes from the position of the top set bit and
                // the mantissa is that bit normalised to bit 23.
                let k = 31 - man.leading_zeros(); // top set bit, 0..=9
                let exp = (k as i32 - 24 + 127) as u32;
                sign | (exp << 23) | ((man << (23 - k)) & 0x007f_ffff)
            }
        }
        0x1f => sign | 0x7f80_0000 | (man << 13),
        _ => sign | ((exp + 127 - 15) << 23) | (man << 13),
    };
    f32::from_bits(bits)
}

/// `acc += w * scale` for half-precision weights, eight lanes per pass.
///
/// F16C converts eight halves to eight floats in one instruction, so the byte
/// saving is not given back in conversions. Weights are rounded to 10 mantissa
/// bits, i.e. ~5e-4 relative, which is far below the uncertainty the network
/// itself operates with; the effect on play is measured, not assumed.
///
/// # Safety
///
/// Requires AVX2, FMA and F16C; the caller checks with `is_x86_feature_detected!`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma,f16c")]
unsafe fn add_scaled_half_avx2(acc: &mut [f32], w: &[u16], scale: f32) {
    use std::arch::x86_64::*;
    debug_assert_eq!(acc.len(), w.len());
    let n = acc.len();
    let s = _mm256_set1_ps(scale);
    let mut i = 0usize;
    while i + 8 <= n {
        let p = acc.as_mut_ptr().add(i);
        let a = _mm256_loadu_ps(p);
        let h = _mm_loadu_si128(w.as_ptr().add(i) as *const __m128i);
        let wv = _mm256_cvtph_ps(h);
        _mm256_storeu_ps(p, _mm256_fmadd_ps(wv, s, a));
        i += 8;
    }
    while i < n {
        *acc.get_unchecked_mut(i) += half_to_f32(*w.get_unchecked(i)) * scale;
        i += 1;
    }
}

/// `acc += w * scale`, eight lanes at a time.
///
/// The obvious `acc.iter_mut().zip(w).for_each(|(a, b)| *a += b * scale)` form
/// leaves the compiler at 4.2 GFLOP/s on a 768x768 block because it does not
/// vectorise the iterator chain; this shape reaches 6.4 GFLOP/s, which is the
/// memory bandwidth limit for a kernel that reads four bytes per FMA.
/// Accumulation order per element is unchanged, so results are bit-identical.
#[inline]
fn add_scaled(acc: &mut [f32], w: &[f32], scale: f32) {
    debug_assert_eq!(acc.len(), w.len());
    let n = acc.len();
    let body = n - n % 8;
    let (acc_body, acc_tail) = acc.split_at_mut(body);
    let (w_body, w_tail) = w.split_at(body);
    for (a, b) in acc_body.chunks_exact_mut(8).zip(w_body.chunks_exact(8)) {
        for k in 0..8 {
            a[k] += b[k] * scale;
        }
    }
    for (a, b) in acc_tail.iter_mut().zip(w_tail.iter()) {
        *a += b * scale;
    }
}

impl Layer {
    pub fn new(r#in: usize, out: usize, act: Act) -> Self {
        Layer {
            r#in,
            out,
            act,
            wt: vec![0.0; r#in * out],
            wt_h: Vec::new(),
            b: vec![0.0; out],
        }
    }

    /// Weight at `(output, input)` when the layer is seen row-major.
    pub fn weight(&self, o: usize, i: usize) -> f32 {
        self.wt[i * self.out + o]
    }

    /// Set a weight given row-major coordinates.
    pub fn set_weight(&mut self, o: usize, i: usize, v: f32) {
        self.wt[i * self.out + o] = v;
        self.rebuild_half();
    }

    /// Recompute the half-precision copy of the weights.
    ///
    /// Cheap (one pass over the layer) and idempotent, so every mutator calls
    /// it rather than trying to patch single entries.
    pub fn rebuild_half(&mut self) {
        self.wt_h.clear();
        self.wt_h.reserve(self.wt.len());
        for &v in &self.wt {
            self.wt_h.push(f32_to_half(v));
        }
    }

    /// Load row-major `out × in` weights from a checkpoint.
    pub fn load_row_major(&mut self, rows: &[f32]) {
        debug_assert_eq!(rows.len(), self.r#in * self.out);
        for o in 0..self.out {
            for i in 0..self.r#in {
                self.wt[i * self.out + o] = rows[o * self.r#in + i];
            }
        }
        self.rebuild_half();
    }

    /// Weights in the checkpoint's row-major layout.
    pub fn row_major_weights(&self) -> Vec<f32> {
        let mut out = vec![0.0f32; self.r#in * self.out];
        for o in 0..self.out {
            for i in 0..self.r#in {
                out[o * self.r#in + i] = self.wt[i * self.out + o];
            }
        }
        out
    }

    /// Deterministic initialization (Xavier uniform), used when no checkpoint
    /// exists yet so self-play can start from a well-conditioned random policy.
    pub fn init_xavier(&mut self, seed: u64) {
        let mut state = seed | 1;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f32 / (1u64 << 53) as f32
        };
        let limit = (6.0 / (self.r#in + self.out) as f32).sqrt();
        for w in self.wt.iter_mut() {
            *w = (next() * 2.0 - 1.0) * limit;
        }
        for b in self.b.iter_mut() {
            *b = 0.0;
        }
        self.rebuild_half();
    }

    /// Batched form of [`Layer::forward`].
    ///
    /// `x` is `n` inputs of `in` values each, laid out contiguously, and `out`
    /// is `n * out`. Each weight row is loaded once and applied to every row of
    /// the batch that has that feature set, so the weight traffic per decision
    /// falls towards `1/n`. The per-row accumulation order is exactly the same
    /// as in the single-input kernel, so a batch of one is bit-identical to
    /// [`Layer::forward`].
    fn forward_batch(&self, x: &[f32], n: usize, out: &mut [f32]) {
        let (ni, no) = (self.r#in, self.out);
        for r in 0..n {
            out[r * no..(r + 1) * no].copy_from_slice(&self.b[..no]);
        }
        for j in 0..ni {
            let mut any = false;
            for r in 0..n {
                if x[r * ni + j] != 0.0 {
                    any = true;
                    break;
                }
            }
            if !any {
                continue;
            }
            let col = &self.wt[j * no..(j + 1) * no];
            for r in 0..n {
                let xj = x[r * ni + j];
                if xj == 0.0 {
                    continue;
                }
                add_scaled(&mut out[r * no..(r + 1) * no], col, xj);
            }
        }
        match self.act {
            Act::Relu => out[..n * no].iter_mut().for_each(|v| *v = v.max(0.0)),
            Act::Tanh => out[..n * no].iter_mut().for_each(|v| *v = v.tanh()),
            Act::Linear => {}
        }
    }

    /// Scatter the layer's zero inputs out of the hot loop.
    ///
    /// About three quarters of an observation is zero (whole blocks of the
    /// encoding are inactive depending on the decision), so skipping them is
    /// essential: the same loop run densely costs 294.6 us per forward against
    /// 72.6 us when zeros are skipped.
    ///
    /// Collecting the non-zero indices first makes the hot loop branch-free, but
    /// it measured *equal*, not faster (72.5 us vs 72.6 us): the kernel is not
    /// branch-bound. Timings across input sparsities show why — with only 32 of
    /// 703 features set the forward still costs 77.8 us, because the second
    /// layer's hidden activations are dense regardless. The kernel moves weight
    /// rows at about 24 GB/s, i.e. it is bound by weight traffic (4.5 MB of f32
    /// weights per forward, one FMA per 4 bytes loaded). The only real lever left
    /// is fewer or narrower weights: halving the width, or quantising them.
    ///
    /// Round 28 tested two of those shapes explicitly and kept this one:
    /// a register-blocked GEMV (accumulator held in registers, columns streamed
    /// inside each block of 32 outputs) measured *slower* — 102-107 us against
    /// this shape's 96-98 us — because the 3 KB stride between consecutive
    /// columns defeats the prefetcher, which is worth more here than the
    /// accumulator traffic it saves; and an explicit AVX2/FMA version of this
    /// same shape measured equal to the compiler's own vectorisation
    /// (37.5 GB/s against 35.4 GB/s, inside run-to-run noise). What the
    /// measurements do show is that the kernel is near this machine's memory
    /// bandwidth: a 64x64 network whose weights fit entirely in cache moves no
    /// more bytes per second than the 4.85 MB body does.
    fn forward(&self, x: &[f32], out: &mut [f32], idx: &mut Vec<u32>) {
        let n = self.out;
        idx.clear();
        for (j, &xj) in x[..self.r#in].iter().enumerate() {
            if xj != 0.0 {
                idx.push(j as u32);
            }
        }
        out[..n].copy_from_slice(&self.b[..n]);
        self.gemv(x, out, idx);
        match self.act {
            Act::Relu => out[..n].iter_mut().for_each(|v| *v = v.max(0.0)),
            Act::Tanh => out[..n].iter_mut().for_each(|v| *v = v.tanh()),
            Act::Linear => {}
        }
    }

    /// `out += W·x` over the non-zero columns in `idx`.
    ///
    /// Two implementations of the same sum: half-precision weights with F16C
    /// (half the bytes to move, ~5e-4 relative error per weight) when the CPU
    /// supports it, and the f32 vector adds otherwise. The half path is used
    /// only when the layer's half copy is exactly the right length, so a layer
    /// whose weights were replaced without [`Layer::rebuild_half`] silently
    /// takes the exact path instead of reading stale values.
    fn gemv(&self, x: &[f32], out: &mut [f32], idx: &[u32]) {
        let n = self.out;
        #[cfg(target_arch = "x86_64")]
        {
            if self.wt_h.len() == self.wt.len()
                && std::is_x86_feature_detected!("avx2")
                && std::is_x86_feature_detected!("fma")
                && std::is_x86_feature_detected!("f16c")
            {
                for &j in idx {
                    let j = j as usize;
                    // SAFETY: the features were just detected at runtime, and
                    // the slice is `out` long in both directions.
                    unsafe {
                        add_scaled_half_avx2(
                            &mut out[..n],
                            &self.wt_h[j * n..(j + 1) * n],
                            x[j],
                        )
                    };
                }
                return;
            }
        }
        self.gemv_f32(x, out, idx);
    }

    /// Exact f32 path, also used by the tests as the reference.
    fn gemv_f32(&self, x: &[f32], out: &mut [f32], idx: &[u32]) {
        let n = self.out;
        for &j in idx {
            let j = j as usize;
            add_scaled(&mut out[..n], &self.wt[j * n..(j + 1) * n], x[j]);
        }
    }
}

/// The policy / value network.
///
/// The body is a stack of dense layers; the last body activation feeds two
/// linear heads: `policy` (one logit per action slot) and `value` (a single
/// scalar estimating the acting player's final score change).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Net {
    pub body: Vec<Layer>,
    pub policy: Layer,
    pub value: Layer,
    /// Feature dimension this network was built for.
    pub feature_dim: usize,
    /// Extra value heads, in the order the checkpoint listed them.
    #[serde(skip)]
    pub extra: Vec<(String, Layer)>,
    #[serde(skip)]
    scratch: Vec<Vec<f32>>,
    /// Reusable buffer of non-zero input indices (see [`Layer::forward`]).
    #[serde(skip)]
    idx: Vec<u32>,
    /// Scratch for [`Net::forward_batch`].
    #[serde(skip, default)]
    batch_scratch: Vec<f32>,
    #[serde(skip, default)]
    policy_batch: Vec<f32>,
    #[serde(skip, default)]
    value_batch: Vec<f32>,
}

/// JSON part of a checkpoint file.
#[derive(Serialize, Deserialize)]
struct Header {
    layers: Vec<Layer>,
    policy: Layer,
    value: Layer,
    /// Extra heads written after `value` (round 19's decomposed value:
    /// `p_win`, `v_win`, `p_deal`, `v_deal`, `v_other`). Every one of them is a
    /// single linear output on the last body activation, so only their names are
    /// stored.
    #[serde(default)]
    decomposed: Vec<String>,
    feature_dim: usize,
    /// Free-form notes written by the trainer (step, dataset, metrics…).
    #[serde(default)]
    info: serde_json::Value,
}

/// Magic string at the top of a checkpoint.
pub const MAGIC: &str = "MMJNN1";

impl Net {
    /// Build a network with the given hidden widths.
    pub fn new(hidden: &[usize], feature_dim: usize, seed: u64) -> Self {
        let mut body = Vec::new();
        let mut prev = feature_dim;
        for (i, &h) in hidden.iter().enumerate() {
            let mut l = Layer::new(prev, h, Act::Relu);
            l.init_xavier(seed.wrapping_add(i as u64 * 7919));
            body.push(l);
            prev = h;
        }
        let mut policy = Layer::new(prev, POLICY_DIM, Act::Linear);
        let mut value = Layer::new(prev, 1, Act::Linear);
        policy.init_xavier(seed.wrapping_add(104_729));
        value.init_xavier(seed.wrapping_add(154_858));
        let scratch: Vec<Vec<f32>> = body.iter().map(|l| vec![0.0; l.out]).collect();
        let idx = Vec::with_capacity(feature_dim);
        Net {
            extra: Vec::new(),
            body,
            policy,
            value,
            feature_dim,
            scratch,
            idx,
            batch_scratch: Vec::new(),
            policy_batch: Vec::new(),
            value_batch: Vec::new(),
        }
    }

    /// The default architecture: two hidden layers of 512.
    pub fn default_shape() -> Vec<usize> {
        vec![512, 512]
    }

    /// Run the network on one observation.
    ///
    /// Intermediates live in `self.scratch`, so inference allocates nothing.
    pub fn forward(&mut self, features: &[f32], policy_out: &mut [f32], value_out: &mut f32) {
        debug_assert_eq!(features.len(), self.feature_dim);
        let mut idx = std::mem::take(&mut self.idx);
        for i in 0..self.body.len() {
            if i == 0 {
                self.body[0].forward(features, &mut self.scratch[0], &mut idx);
            } else {
                let (prev, cur) = self.scratch.split_at_mut(i);
                self.body[i].forward(&prev[i - 1], &mut cur[0], &mut idx);
            }
        }
        let hidden: &[f32] = if self.body.is_empty() {
            features
        } else {
            &self.scratch[self.body.len() - 1]
        };
        self.policy.forward(hidden, policy_out, &mut idx);
        let mut v = [0.0f32];
        self.value.forward(hidden, &mut v, &mut idx);
        *value_out = v[0];
        self.idx = idx;
    }

    /// Batched evaluation of `n` observations.
    ///
    /// `inputs` holds `n * feature_dim` values, `policy_out` `n * POLICY_DIM`
    /// and `value_out` `n`. Results for a given row are bit-identical to calling
    /// [`Net::forward`] on that row alone.
    pub fn forward_batch(
        &mut self,
        inputs: &[f32],
        n: usize,
        policy_out: &mut [f32],
        value_out: &mut [f32],
    ) {
        if n == 0 {
            return;
        }
        let fd = self.feature_dim;
        assert_eq!(inputs.len(), n * fd);
        let total: usize = self.body.iter().map(|l| l.out * n).sum();
        self.batch_scratch.resize(total, 0.0);
        let mut offset = 0usize;
        let mut prev_offset = 0usize;
        for i in 0..self.body.len() {
            let w = self.body[i].out;
            // Layer i writes into the block that starts at `offset`; everything
            // before that is the previous layers' activations.
            let (before, rest) = self.batch_scratch.split_at_mut(offset);
            let src: &[f32] = if i == 0 {
                inputs
            } else {
                &before[prev_offset..offset]
            };
            self.body[i].forward_batch(src, n, &mut rest[..w * n]);
            prev_offset = offset;
            offset += w * n;
        }
        let hidden: &[f32] = if self.body.is_empty() {
            &inputs[..n * fd]
        } else {
            let last = self.body.last().unwrap().out;
            &self.batch_scratch[offset - last * n..offset]
        };
        let mut policy_scratch = std::mem::take(&mut self.policy_batch);
        policy_scratch.resize(n * self.policy.out, 0.0);
        self.policy.forward_batch(hidden, n, &mut policy_scratch);
        policy_out[..n * self.policy.out].copy_from_slice(&policy_scratch);
        self.policy_batch = policy_scratch;

        let mut value_scratch = std::mem::take(&mut self.value_batch);
        value_scratch.resize(n, 0.0);
        self.value.forward_batch(hidden, n, &mut value_scratch);
        value_out[..n].copy_from_slice(&value_scratch);
        self.value_batch = value_scratch;
    }

    /// Win probability from the `p_win` head, when the checkpoint carries the
    /// decomposed value. Round 19 measured that head at AUC 0.787 while the
    /// scalar value head explains only ~14% of the hand swing, so this is the
    /// reliable learned signal available to a rule agent.
    pub fn win_probability(&mut self, features: &[f32]) -> Option<f32> {
        if !self.extra.iter().any(|(n, _)| n == "p_win") {
            return None;
        }
        debug_assert_eq!(features.len(), self.feature_dim);
        for i in 0..self.body.len() {
            if i == 0 {
                self.body[0].forward(features, &mut self.scratch[0], &mut self.idx);
            } else {
                let mut idx = std::mem::take(&mut self.idx);
                let (prev, cur) = self.scratch.split_at_mut(i);
                self.body[i].forward(&prev[i - 1], &mut cur[0], &mut idx);
                self.idx = idx;
            }
        }
        let hidden: &[f32] = &self.scratch[self.body.len() - 1];
        let mut out = [0.0f32];
        for (name, layer) in self.extra.iter() {
            if name == "p_win" {
                let mut scratch = [0.0f32];
                layer.forward(hidden, &mut scratch, &mut Vec::new());
                out[0] = 1.0 / (1.0 + (-scratch[0]).exp());
                return Some(out[0]);
            }
        }
        None
    }

    /// Convenience wrapper that also applies the legal-action mask.
    pub fn policy(&mut self, features: &[f32], mask: &[u8]) -> (Vec<f32>, f32) {
        let mut logits = vec![0.0f32; POLICY_DIM];
        let mut value = 0.0f32;
        self.forward(features, &mut logits, &mut value);
        masked_softmax_in_place(&mut logits, mask);
        (logits, value)
    }

    /// Save to the checkpoint format.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let header = Header {
            layers: self.body.clone(),
            policy: self.policy.clone(),
            value: self.value.clone(),
            decomposed: self.extra.iter().map(|(n, _)| n.clone()).collect(),
            feature_dim: self.feature_dim,
            info: serde_json::Value::Null,
        };
        let mut file = io::BufWriter::new(std::fs::File::create(path)?);
        writeln!(file, "{}", MAGIC)?;
        writeln!(file, "{}", serde_json::to_string(&header).unwrap())?;
        let extras: Vec<&Layer> = self.extra.iter().map(|(_, l)| l).collect();
        for layer in self
            .body
            .iter()
            .chain([&self.policy, &self.value])
            .chain(extras)
        {
            for w in layer.row_major_weights() {
                file.write_all(&w.to_le_bytes())?;
            }
            for b in &layer.b {
                file.write_all(&b.to_le_bytes())?;
            }
        }
        file.flush()?;
        Ok(())
    }

    /// Load a checkpoint.
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        io::BufReader::new(std::fs::File::open(path)?).read_to_end(&mut bytes)?;
        // The file is two text lines followed by raw f32 data, so the header is
        // parsed from the byte stream rather than as UTF-8 text.
        let mut offset = 0usize;
        let line = |bytes: &[u8], offset: &mut usize| -> io::Result<String> {
            let start = *offset;
            while *offset < bytes.len() && bytes[*offset] != b'\n' {
                *offset += 1;
            }
            let s = std::str::from_utf8(&bytes[start..*offset])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                .to_string();
            *offset += 1;
            Ok(s)
        };
        let magic = line(&bytes, &mut offset)?;
        if magic.trim() != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("not a mmj checkpoint (magic {:?})", magic),
            ));
        }
        let header: Header = serde_json::from_str(&line(&bytes, &mut offset)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        // An *older* checkpoint is accepted by a build with more features: its
        // first layer has `header.feature_dim` inputs and `Layer::forward` only
        // reads `x[..self.r#in]`, so the newer features are simply not visible to
        // it and its behaviour is bit-for-bit what it was. That is what makes a
        // feature block bootstrappable: the incumbent can play with the new
        // encoder and generate labelled data that *has* the new block, so the
        // first model that can see it does not have to be trained from nothing.
        //
        // The reverse (a checkpoint with more features than this build) cannot
        // work, because the missing inputs would have to be invented.
        if header.feature_dim > FEATURE_DIM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "checkpoint was trained on {} features, this build encodes {}",
                    header.feature_dim, FEATURE_DIM
                ),
            ));
        }
        let hidden_out = header.layers.last().map(|l| l.out).unwrap_or(0);
        let extra_specs: Vec<Layer> = header
            .decomposed
            .iter()
            .map(|_| Layer::new(hidden_out, 1, Act::Linear))
            .collect();
        let extra_names = header.decomposed.clone();
        let mut layers = header.layers;
        layers.push(header.policy);
        layers.push(header.value);
        layers.extend(extra_specs);
        for layer in layers.iter_mut() {
            let n_w = layer.r#in * layer.out;
            if offset + 4 * (n_w + layer.out) > bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "checkpoint is truncated",
                ));
            }
            let rows: Vec<f32> = (0..n_w)
                .map(|i| {
                    let o = offset + 4 * i;
                    f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
                })
                .collect();
            // `wt` is skipped by serde, so it must be sized before loading.
            layer.wt = vec![0.0; n_w];
            layer.load_row_major(&rows);
            offset += 4 * n_w;
            layer.b = (0..layer.out)
                .map(|i| {
                    let o = offset + 4 * i;
                    f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
                })
                .collect();
            offset += 4 * layer.out;
        }
        let mut extra: Vec<(String, Layer)> = Vec::new();
        for name in extra_names.iter().rev() {
            extra.push((name.clone(), layers.pop().unwrap()));
        }
        extra.reverse();
        let value = layers.pop().unwrap();
        let policy = layers.pop().unwrap();
        let body = layers;
        let scratch: Vec<Vec<f32>> = body.iter().map(|l| vec![0.0; l.out]).collect();
        let idx = Vec::with_capacity(header.feature_dim);
        Ok(Net {
            body,
            policy,
            value,
            feature_dim: header.feature_dim,
            extra,
            scratch,
            idx,
            batch_scratch: Vec::new(),
            policy_batch: Vec::new(),
            value_batch: Vec::new(),
        })
    }

    /// Total parameter count.
    pub fn params(&self) -> usize {
        let body: usize = self.body.iter().map(|l| l.r#in * l.out + l.out).sum();
        body + self.policy.r#in * self.policy.out
            + self.policy.out
            + self.value.r#in * self.value.out
            + self.value.out
    }
}

/// Softmax over the legal slots only; illegal slots are set to `-inf` (0 after
/// exponentiation) and the result sums to 1 over the mask.
pub fn masked_softmax_in_place(logits: &mut [f32], mask: &[u8]) {
    let mut max = f32::NEG_INFINITY;
    for i in 0..logits.len().min(mask.len()) {
        if mask[i] == 1 && logits[i] > max {
            max = logits[i];
        }
    }
    if !max.is_finite() {
        // No legal action: leave a uniform distribution.
        let n = logits.len() as f32;
        logits.iter_mut().for_each(|l| *l = 1.0 / n);
        return;
    }
    let mut sum = 0.0f32;
    for i in 0..logits.len() {
        if i < mask.len() && mask[i] == 1 {
            let e = (logits[i] - max).exp();
            logits[i] = e;
            sum += e;
        } else {
            logits[i] = 0.0;
        }
    }
    if sum > 0.0 {
        logits.iter_mut().for_each(|l| *l /= sum);
    }
}

/// Sample an index from a probability vector.
pub fn sample_from(probs: &[f32], rng_value: f64) -> usize {
    let mut acc = 0.0f64;
    let target = rng_value;
    for (i, &p) in probs.iter().enumerate() {
        acc += p as f64;
        if acc >= target {
            return i;
        }
    }
    probs.iter().rposition(|&p| p > 0.0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmj_core::state::{Table, TableConfig};

    #[test]
    fn feature_dim_matches_encoding() {
        let table = Table::new(TableConfig::new(1));
        let decision = table.decisions()[0].clone();
        let mut obs = Obs::new();
        encode(&table, 0, &decision, &mut obs);
        assert_eq!(obs.features.len(), FEATURE_DIM);
        assert!(obs.mask.iter().any(|&m| m == 1));
        assert_eq!(obs.slots.len(), obs.actions.len());
    }

    #[test]
    fn roundtrip_checkpoint() {
        let dir = std::env::temp_dir().join("mmj-nn-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("net.bin");
        let net = Net::new(&[32, 16], FEATURE_DIM, 7);
        net.save(&path).unwrap();
        let mut loaded = Net::load(&path).unwrap();
        let x = vec![0.01f32; FEATURE_DIM];
        let mut a = vec![0.0; POLICY_DIM];
        let mut va = 0.0;
        let mut b = vec![0.0; POLICY_DIM];
        let mut vb = 0.0;
        let mut net2 = net.clone();
        net2.forward(&x, &mut a, &mut va);
        loaded.forward(&x, &mut b, &mut vb);
        assert_eq!(va, vb);
        for i in 0..POLICY_DIM {
            assert!((a[i] - b[i]).abs() < 1e-6);
        }
        assert_eq!(net.params(), loaded.params());
    }

    #[test]
    fn masked_softmax_only_uses_legal_slots() {
        let mut logits = vec![0.0f32; POLICY_DIM];
        let mut mask = vec![0u8; POLICY_DIM];
        mask[3] = 1;
        mask[7] = 1;
        logits[3] = 5.0;
        logits[7] = 5.0;
        logits[99] = 100.0;
        masked_softmax_in_place(&mut logits, &mask);
        assert!((logits[3] - 0.5).abs() < 1e-6);
        assert!((logits[7] - 0.5).abs() < 1e-6);
        assert_eq!(logits[99], 0.0);
        assert!((logits.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sampling_respects_probabilities() {
        let mut probs = vec![0.0f32; 4];
        probs[2] = 1.0;
        assert_eq!(sample_from(&probs, 0.5), 2);
        probs[2] = 0.0;
        probs[1] = 0.5;
        probs[3] = 0.5;
        assert_eq!(sample_from(&probs, 0.1), 1);
        assert_eq!(sample_from(&probs, 0.9), 3);
    }

    /// Batched evaluation must agree with one call per row, bit for bit: the
    /// accumulation order within a row is unchanged, only the loop nesting moves.
    #[test]
    fn batched_forward_matches_single() {
        let mut net = Net::new(&[24, 17], 40, 7);
        let n = 5;
        let mut inputs = vec![0.0f32; n * 40];
        let mut seed = 12345u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32 - 0.5
        };
        for r in 0..n {
            for j in 0..40 {
                // A mix of zeros and values, so the sparse paths are exercised.
                inputs[r * 40 + j] = if j % 3 == 0 || (r + j) % 5 == 0 { 0.0 } else { next() };
            }
        }
        let mut batch_policy = vec![0.0f32; n * POLICY_DIM];
        let mut batch_value = vec![0.0f32; n];
        net.forward_batch(&inputs, n, &mut batch_policy, &mut batch_value);

        let mut policy = vec![0.0f32; POLICY_DIM];
        let mut value = 0.0f32;
        for r in 0..n {
            net.forward(&inputs[r * 40..(r + 1) * 40], &mut policy, &mut value);
            // The single-input path rounds weights to half precision, so the two
            // paths agree to within that quantisation (~5e-4 relative) rather
            // than bit for bit. `sparse_forward_matches_dense` pins the exact
            // f32 kernel down separately.
            for (o, (b, p)) in batch_policy[r * POLICY_DIM..(r + 1) * POLICY_DIM]
                .iter()
                .zip(policy.iter())
                .enumerate()
            {
                assert!(
                    (b - p).abs() <= 2e-3 * (1.0 + p.abs()),
                    "policy row {} element {}: batch {} vs single {}",
                    r,
                    o,
                    b,
                    p
                );
            }
            assert!(
                (batch_value[r] - value).abs() <= 2e-3 * (1.0 + value.abs()),
                "value row {} differs: {} vs {}",
                r,
                batch_value[r],
                value
            );
        }
    }

    /// The hot loop skips zero inputs by collecting their indices first. That is
    /// an exact optimization (adding `w * 0.0` changes nothing), and this test
    /// pins it down: the sparse kernel must reproduce the naive dense evaluation
    /// bit for bit, including on an all-zero and an all-non-zero input.
    /// The half-precision round trip has to be exact for everything binary16
    /// can represent, and within one ulp of binary16 everywhere else.
    #[test]
    fn half_precision_conversions_round_trip() {
        let mut exact = 0;
        for &v in &[
            0.0f32, 1.0, -1.0, 0.5, -0.25, 0.125, 2.0, 1024.0, -2048.0, 0.0625, 3.0, 0.1,
            -0.333_333_34, 1.0e-5, 6.103_515_6e-5, 5.960_464_5e-8,
        ] {
            let h = f32_to_half(v);
            let back = half_to_f32(h);
            if back == v {
                exact += 1;
            } else if v.abs() < 6.104e-5 {
                // Subnormal halves are spaced 2^-24 apart, so below 2^-14 the
                // bound is absolute (half a step) rather than relative.
                assert!(
                    (back - v).abs() <= 2.98e-8,
                    "{} -> {:04x} -> {} (abs {})",
                    v,
                    h,
                    back,
                    (back - v).abs()
                );
            } else {
                // Everywhere else binary16 has 11 significant bits, so a
                // round-to-nearest conversion is within half an ulp = 2^-12.
                let rel = ((back - v) / v).abs();
                assert!(rel <= 2.5e-4, "{} -> {:04x} -> {} (rel {})", v, h, back, rel);
            }
        }
        assert!(exact > 6, "only {} of the sample round-tripped exactly", exact);
        // Saturation and non-finite handling.
        assert_eq!(f32_to_half(1.0e30), 0x7c00);
        assert_eq!(f32_to_half(-1.0e30), 0xfc00);
        assert_eq!(f32_to_half(1.0e-30), 0x0000);
        assert!(f32_to_half(f32::NAN) & 0x7c00 == 0x7c00);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x8000), -0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        // Every one of the 65536 patterns must convert to a finite-or-infinite
        // float and back to the same pattern (binary16 is exact in binary32).
        for bits in 0..=u16::MAX {
            let f = half_to_f32(bits);
            if f.is_nan() {
                continue;
            }
            assert_eq!(f32_to_half(f), bits, "pattern {:04x} did not survive", bits);
        }
    }

    /// What can one core actually read? Calibrates the GEMV numbers above: if a
    /// plain streaming read of the same number of bytes is no faster than the
    /// forward is, the kernel is at the memory wall and only narrower weights
    /// can help. Run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn memory_bandwidth_probe() {
        // Eight independent accumulators: one chain would be latency-bound and
        // would understate what the hardware can stream.
        fn stream(data: &[f32]) -> f32 {
            let mut acc = [0f32; 8];
            for chunk in data.chunks_exact(8) {
                for k in 0..8 {
                    acc[k] += chunk[k];
                }
            }
            acc.iter().sum()
        }
        for (label, bytes) in [
            ("L3-ish 4.8 MB", 4_850_000usize),
            ("DRAM  64 MB", 64_000_000usize),
        ] {
            let data: Vec<f32> = (0..bytes / 4).map(|i| (i % 977) as f32 * 0.001).collect();
            let mut sink = stream(&data);
            let reps = 8;
            let start = std::time::Instant::now();
            for _ in 0..reps {
                sink += stream(&data);
            }
            let secs = start.elapsed().as_secs_f64();
            println!(
                "streaming f32 read, {}: {:.1} GB/s single thread (sink {:.0})",
                label,
                bytes as f64 * reps as f64 / secs / 1e9,
                sink
            );
        }
    }

    #[test]
    fn sparse_forward_matches_dense() {
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32 - 0.5
        };
        let layer = Layer::new(37, 11, Act::Linear);

        for case in 0..6 {
            let x: Vec<f32> = (0..37)
                .map(|j| match case {
                    0 => 0.0,
                    1 => 0.3,
                    2 => if j % 3 == 0 { 0.0 } else { 0.25 },
                    3 => if j % 7 == 0 { -0.4 } else { 0.0 },
                    4 => if j < 4 { 0.5 } else { 0.0 },
                    _ => next(),
                })
                .collect();
            let mut sparse = vec![0.0f32; 11];
            let mut dense = vec![0.0f32; 11];
            let mut idx = Vec::new();
            layer.forward(&x, &mut sparse, &mut idx);
            // Naive: every input, including the zeros, without the index list.
            dense.copy_from_slice(&layer.b);
            for (j, &xj) in x.iter().enumerate() {
                let col = &layer.wt[j * 11..(j + 1) * 11];
                for (o, w) in dense.iter_mut().zip(col.iter()) {
                    *o += w * xj;
                }
            }
            assert_eq!(sparse, dense, "case {} diverged", case);
            // The production path rounds the weights to binary16 first; that is
            // a *quantisation*, so it is checked against the same reference with
            // a tolerance instead of for equality.
            let mut half = vec![0.0f32; 11];
            half.copy_from_slice(&layer.b);
            layer.gemv(&x, &mut half, &idx);
            for (o, (h, d)) in half.iter().zip(dense.iter()).enumerate() {
                assert!(
                    (h - d).abs() <= 2e-3 * (1.0 + d.abs()),
                    "case {} element {}: half {} vs dense {}",
                    case,
                    o,
                    h,
                    d
                );
            }
        }
    }
}
