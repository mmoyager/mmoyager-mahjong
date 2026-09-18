//! A tile-efficiency baseline agent.
//!
//! This is the "reasonable human beginner" reference used for the first playable
//! UI and, more importantly, as the fixed opponent that measures whether
//! self-play training actually improves. It uses only information the seat can
//! legally see.
//!
//! Strategy:
//!
//! * always take a win (ron / tsumo);
//! * discard the tile that keeps the lowest shanten, breaking ties by widest
//!   tile acceptance (受け入れ) and then by discarding isolated tiles first;
//! * declare riichi on any tenpai with at least one live wait tile;
//! * call ポン/チー only when it reduces shanten on a hand that still has a
//!   plausible yaku, and never on a hand that is already tenpai (keeping the
//!   closed hand for riichi is worth more);
//! * fold to genbutsu (tiles already discarded by a riichi player) once the
//!   hand is far from tenpai, when `defense` is on.

use crate::{Agent, fallback, prefer};
use mmj_core::action::{Action, ActionKind};
use mmj_core::danger::{DangerReads, danger_table_reading};
use mmj_nn::{Net, Obs, POLICY_DIM, encode};
use mmj_core::hand::{
    Counts, hand_strength, shanten, tile_keep_value, useful_kinds, winning_kinds, yaku_score,
};
use mmj_core::meld::MeldKind;
use mmj_core::state::{Decision, Table, Trigger};
use mmj_core::tile::{
    CHUN, HAKU, HATSU, Kind, NUM_KINDS, is_honor, is_simple, kind_of, suit_of,
};
use std::collections::HashSet;

/// Baseline agent configuration.
#[derive(Clone, Copy, Debug)]
pub struct EfficiencyConfig {
    /// 0 = never call, 1 = only when it clearly helps, 2 = eager.
    pub call_eagerness: u8,
    /// Fold to safe tiles when an opponent has declared riichi.
    pub defense: bool,
    /// How strongly safety is weighed against tile acceptance (0 = ignore).
    pub defense_weight: f32,
    /// Reward keeping yaku and dora when choosing a discard, instead of
    /// optimising purely for speed.
    pub yaku_aware: bool,
    /// Decide push/fold from the hand's rough value rather than from shanten
    /// alone: a valuable hand keeps pushing through a riichi, a cheap one folds.
    pub push_fold: bool,
    /// Weight of the yaku-count term in the discard value bonus. The defaults
    /// are the historical hardcoded constants, so leaving them alone reproduces
    /// the teacher's behaviour exactly.
    pub yaku_weight: f32,
    /// Weight of the retained-dora term in the discard value bonus.
    pub dora_weight: f32,
    /// Also treat well-developed open hands as threats when folding.
    pub open_threats: bool,
    /// Read the *order* of a threat's discards: a suit they have largely thrown
    /// away is unlikely to hold their wait.
    pub suit_reading: bool,
    /// Weight late discards more than early ones in that read.
    pub read_timing: bool,
    /// Treat suits a threat has called melds in as more dangerous.
    pub read_melds: bool,
    /// Hand strength below which a threatened player folds instead of pushing.
    pub fold_strength: f32,
    /// Safety weight while pushing with a strong hand.
    pub push_caution: f32,
    /// Safety weight while pushing with a mediocre hand.
    pub careful_caution: f32,
    /// Minimum number of live tiles in the wait required to declare riichi.
    /// `1` declares on any tenpai; higher values keep the hand concealed (黙聴)
    /// when the wait is nearly dead.
    pub riichi_min_live: u32,
    /// Treat 両筋 (both sides broken) as much safer than 片筋.
    pub strict_suji: bool,
    /// Let the score situation move the push/fold threshold: push when behind
    /// late, protect a lead. This is what placement-aware human play does, and
    /// it shows up in the average placement rather than in raw points.
    pub placement_aware: bool,
    /// Let the fold threshold grow as the hand progresses: opponents are far
    /// more likely to be waiting late than in the first few turns, so the same
    /// hand value justifies folding later on. `0` disables it, `1` raises the
    /// threshold by up to half at the very end of a hand.
    pub turn_aware: f32,
    /// Reproduce the pre-2026-09 tile-acceptance counting, which folded the
    /// player's own hand into the "already seen" counts as well as passing it
    /// separately. Kept only so the two teachers can be measured against each
    /// other; the corrected counting is used everywhere else.
    pub legacy_acceptance: bool,
    /// Declare 九種九牌 below this shanten — i.e. only when the hand is awful.
    pub kyuushu_shanten: i8,
}

impl Default for EfficiencyConfig {
    fn default() -> Self {
        EfficiencyConfig {
            call_eagerness: 1,
            // Measured break-even against the defence-free version on this
            // ruleset, so it stays off by default: the benchmark that all the
            // reported numbers use must not move underneath them.
            defense: false,
            defense_weight: 1.0,
            yaku_aware: false,
            push_fold: false,
            yaku_weight: 2.5,
            dora_weight: 0.8,
            open_threats: false,
            suit_reading: false,
            read_timing: false,
            read_melds: false,
            fold_strength: 1.2,
            push_caution: 0.2,
            careful_caution: 0.7,
            riichi_min_live: 1,
            strict_suji: false,
            // Measured worse on both points and placement (head-to-head -152)
            // with this crude formulation, so it stays off.
            placement_aware: false,
            turn_aware: 0.0,
            legacy_acceptance: false,
            kyuushu_shanten: 4,
        }
    }
}

/// The baseline agent.
/// Which learned signal drives the fold decision.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueUse {
    /// The scalar value head, in thousands of points.
    LearnedValue,
    /// The `p_win` head: probability that this hand is completed by this seat.
    WinProbability,
}

pub struct EfficiencyAgent {
    pub config: EfficiencyConfig,
    label: String,
    /// Optional learned value function.
    ///
    /// The teacher's push/fold decision has always used [`hand_strength`], a
    /// handcrafted score (shanten plus yaku and dora bonuses). When a network is
    /// attached here its value head is used instead, which is the first time a
    /// *learned* signal enters the teacher's criteria rather than another
    /// hand-written rule. Because the student now reproduces the teacher's
    /// decisions at 0.97 fidelity, a teacher improved this way can actually be
    /// inherited.
    value_net: Option<Net>,
    value_obs: Obs,
    value_logits: Vec<f32>,
    /// Fold when the learned value is below this. Same units as the value head,
    /// which predicts the hand-scoped score change in thousands of points.
    pub value_threshold: f32,
    /// Which learned signal the fold decision consults. The scalar value failed
    /// (round 18: R^2 0.14, every threshold far worse than the heuristic), but
    /// the `p_win` head of the decomposed value measured AUC 0.787, so the
    /// probability of finishing the hand is the piece worth trying.
    pub value_use: ValueUse,
}

impl EfficiencyAgent {
    /// The improved teacher: a corrected safety model (現物 / 同巡 / 筋 / 壁)
    /// plus yaku-and-dora awareness and value-based push-fold decisions.
    /// Measured at about +400 points over [`EfficiencyAgent::new`] in paired
    /// evaluation, so it is used to generate training data while the plain
    /// version stays frozen as the benchmark.
    pub fn smart(label: impl Into<String>) -> Self {
        EfficiencyAgent {
            config: EfficiencyConfig {
                defense: true,
                defense_weight: 1.0,
                yaku_aware: true,
                push_fold: true,
                open_threats: false,
                // Tuned by paired evaluation against the frozen benchmark:
                // folding at 2-shanten-without-yaku rather than 3-shanten is
                // worth about +300 on its own.
                fold_strength: 2.5,
                ..EfficiencyConfig::default()
            },
            label: label.into(),
            value_net: None,
            value_obs: Obs::default(),
            value_logits: vec![0.0; POLICY_DIM],
            value_threshold: 0.0,
            value_use: ValueUse::LearnedValue,
        }
    }

    /// Attach a learned value head, so the push/fold decision uses it instead of
    /// the handcrafted strength score.
    pub fn with_value(mut self, net: Net, threshold: f32) -> Self {
        self.value_net = Some(net);
        self.value_threshold = threshold;
        self
    }

    /// Attach a checkpoint and use its `p_win` head to decide whether to fold.
    pub fn with_win_probability(mut self, net: Net, threshold: f32) -> Self {
        self.value_net = Some(net);
        self.value_threshold = threshold;
        self.value_use = ValueUse::WinProbability;
        self
    }

    /// Probability of completing this hand, from the checkpoint's `p_win` head.
    fn learned_win_probability(
        &mut self,
        table: &Table,
        seat: u8,
        decision: &Decision,
    ) -> Option<f32> {
        let net = self.value_net.as_mut()?;
        encode(table, seat, decision, &mut self.value_obs);
        net.win_probability(&self.value_obs.features)
    }

    /// Current learned value of the acting player's position, if a net is set.
    fn learned_value(&mut self, table: &Table, seat: u8, decision: &Decision) -> Option<f32> {
        let net = self.value_net.as_mut()?;
        encode(table, seat, decision, &mut self.value_obs);
        let mut value = 0.0f32;
        net.forward(
            &self.value_obs.features,
            &mut self.value_logits,
            &mut value,
        );
        Some(value)
    }

    /// The next rung of the teacher ladder: [`EfficiencyAgent::smart`] with the
    /// discard-*sequence* read enabled in its safety model.
    ///
    /// Measured in a strong field with a same-batch control: the read is worth
    /// +136 against the standard teacher (9600 games). The student reproduces
    /// this teacher's decisions at 0.97 fidelity and its danger features already
    /// use the same read, so the labels are the last piece that was still coming
    /// from the shape-only model.
    pub fn reading(label: impl Into<String>) -> Self {
        let mut a = Self::smart(label);
        a.config.suit_reading = true;
        a
    }

    /// As [`EfficiencyAgent::smart`], and additionally treats developed open
    /// hands as threats rather than only riichi declarations.
    pub fn smarter(label: impl Into<String>) -> Self {
        let mut a = Self::smart(label);
        a.config.open_threats = true;
        a
    }

    pub fn new(label: impl Into<String>) -> Self {
        EfficiencyAgent {
            config: EfficiencyConfig::default(),
            label: label.into(),
            value_net: None,
            value_obs: Obs::default(),
            value_logits: vec![0.0; POLICY_DIM],
            value_threshold: 0.0,
            value_use: ValueUse::LearnedValue,
        }
    }

    pub fn with_config(label: impl Into<String>, config: EfficiencyConfig) -> Self {
        EfficiencyAgent {
            config,
            label: label.into(),
            value_net: None,
            value_obs: Obs::default(),
            value_logits: vec![0.0; POLICY_DIM],
            value_threshold: 0.0,
            value_use: ValueUse::LearnedValue,
        }
    }
}

/// How strongly the score situation calls for pushing rather than protecting.
///
/// Positive means "push": the seat is behind with few hands left. Negative
/// means "protect": leading late, where a deal-in costs a placement.
fn placement_urge(table: &Table, seat: u8) -> f32 {
    let scores = table.scores();
    let mine = scores[seat as usize] as f32;
    let best_other = (0..4)
        .filter(|&s| s != seat as usize)
        .map(|s| scores[s])
        .max()
        .unwrap_or(0) as f32;
    let rounds_left = table.rounds_in_match.saturating_sub(table.round_index + 1);
    // Only the closing hands change how a human plays; earlier, points are
    // worth chasing on their own merits.
    let urgency = match rounds_left {
        0 => 1.0,
        1 => 0.7,
        2 => 0.35,
        _ => 0.0,
    };
    if urgency == 0.0 {
        return 0.0;
    }
    ((best_other - mine) / 8000.0).clamp(-1.0, 1.0) * urgency
}

/// Does the open hand still have a plausible yaku?
fn yaku_potential(counts: &Counts, melds: &[mmj_core::meld::Meld], round_wind: Kind, seat_wind: Kind) -> bool {
    // 断幺九
    let mut all_simple = true;
    let mut suits = [false; 3];
    let mut honors = false;
    for k in 0..NUM_KINDS {
        if counts[k] == 0 {
            continue;
        }
        if !is_simple(k as Kind) {
            all_simple = false;
        }
        if is_honor(k as Kind) {
            honors = true;
        } else {
            suits[suit_of(k as Kind)] = true;
        }
    }
    if all_simple {
        return true;
    }
    // 役牌: a pair is enough to aim for a triplet.
    for k in [HAKU, HATSU, CHUN] {
        if counts[k as usize] >= 2 {
            return true;
        }
    }
    if counts[round_wind as usize] >= 2 || counts[seat_wind as usize] >= 2 {
        return true;
    }
    // Melds already hold a yaku.
    for m in melds {
        if let Some(k) = m.triplet_kind() {
            if k >= HAKU || k == round_wind || k == seat_wind {
                return true;
            }
        }
    }
    // 混一色 / 清一色 potential: at most two suits in play, honors allowed.
    let used = suits.iter().filter(|&&x| x).count();
    if used <= 1 {
        return true;
    }
    // 対々和 potential: two triplets already.
    let triplets = melds.iter().filter(|m| m.kind.is_triplet()).count()
        + (0..NUM_KINDS).filter(|&k| counts[k] >= 3).count();
    if triplets >= 2 {
        return true;
    }
    let _ = honors;
    false
}

impl Agent for EfficiencyAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        // ---- unconditional wins -------------------------------------------
        if let Some(a) = prefer(decision, |a| a.kind() == ActionKind::Tsumo) {
            return a;
        }
        if let Some(a) = prefer(decision, |a| a.kind() == ActionKind::Ron) {
            return a;
        }

        let player = &table.players[seat as usize];
        let melds = player.melds.len() as u8;
        let current = shanten(&player.hand, melds);
        // `useful_kinds` adds the hand itself, so the "seen" counts must be
        // public only — using `visible_counts` here counts every tile in hand
        // twice and understates tile acceptance.
        let visible = if self.config.legacy_acceptance {
            table.visible_counts(seat)
        } else {
            table.public_counts()
        };

        match decision.trigger {
            Trigger::SelfTurn => {
                // ---- 九種九牌: only with a hopeless hand
                if let Some(a) = prefer(decision, |a| a.kind() == ActionKind::Kyuushu) {
                    if current >= self.config.kyuushu_shanten {
                        return a;
                    }
                }
                // ---- kan: only when it does not hurt the shape
                if let Some(a) = self.choose_kan(table, seat, decision, current) {
                    return a;
                }
                // ---- discard
                self.choose_discard(table, seat, decision, &visible)
            }
            Trigger::Discard { .. } | Trigger::Chankan { .. } => {
                if let Some(a) = self.choose_call(table, seat, decision, current, &visible) {
                    a
                } else {
                    fallback(decision)
                }
            }
        }
    }
}

impl EfficiencyAgent {
    fn choose_kan(
        &self,
        table: &Table,
        seat: u8,
        decision: &Decision,
        current: i8,
    ) -> Option<Action> {
        let player = &table.players[seat as usize];
        for &a in decision.actions.iter() {
            let Action::Meld { meld } = a else { continue };
            if !meld.kind.is_kan() {
                continue;
            }
            if meld.kind == MeldKind::Kakan && self.config.call_eagerness == 0 {
                continue;
            }
            let kind = meld.triplet_kind()?;
            let mut rest = player.hand;
            let take = if meld.kind == MeldKind::Ankan { 4 } else { 1 };
            for _ in 0..take {
                if rest[kind as usize] == 0 {
                    return None;
                }
                rest[kind as usize] -= 1;
            }
            let after = shanten(&rest, player.melds.len() as u8 + 1);
            if after <= current {
                return Some(a);
            }
        }
        None
    }

    fn choose_discard(
        &mut self,
        table: &Table,
        seat: u8,
        decision: &Decision,
        visible: &Counts,
    ) -> Action {
        let player = &table.players[seat as usize];
        let melds = player.melds.len() as u8;
        let mut candidates: Vec<(Action, Kind)> = Vec::with_capacity(20);
        for &a in decision.actions.iter() {
            if let Action::Discard { tile, .. } = a {
                candidates.push((a, kind_of(tile)));
            }
        }
        if candidates.is_empty() {
            return fallback(decision);
        }

        // A riichi declaration is a separate action for the same tile: prefer it
        // whenever it is available and the wait is not dead.
        let riichi_available = candidates.iter().any(|(a, _)| {
            matches!(a, Action::Discard { riichi: true, .. })
        });

        let danger = if self.config.defense && !player.riichi {
            danger_table_reading(
                table,
                seat,
                self.config.open_threats,
                self.config.strict_suji,
                DangerReads {
                    abandoned_suit: self.config.suit_reading,
                    timing: self.config.read_timing,
                    melded_suit: self.config.read_melds,
                },
            )
        } else {
            [0.0f32; NUM_KINDS]
        };
        let threatened = danger.iter().any(|&d| d > 0.0);
        let round_wind = table.round_wind;
        let seat_wind = table.seat_wind(seat);
        let dora_kinds = table.wall.dora_kinds();
        let dora_here = |kind: Kind| dora_kinds.iter().filter(|&&d| d == kind).count() as u8;
        let strength = hand_strength(
            &player.hand,
            &player.melds,
            shanten(&player.hand, melds),
            player.hand_tiles.iter().map(|&t| dora_here(kind_of(t))).sum::<u8>(),
            round_wind,
            seat_wind,
        );

        // Pass 1: shanten after each discard.
        let mut scored: Vec<(Kind, i8)> = Vec::with_capacity(candidates.len());
        for &(_, kind) in &candidates {
            let mut rest = player.hand;
            rest[kind as usize] -= 1;
            scored.push((kind, shanten(&rest, melds)));
        }
        let best_shanten = scored.iter().map(|&(_, s)| s).min().unwrap_or(8);

        // Folding: when the hand is cheap and far from tenpai the only thing
        // that matters is not dealing in, so pick the safest tile on the table
        // and use `keep_value` purely to break ties. With `push_fold` the
        // decision comes from the hand's value rather than from shanten alone,
        // so a valuable hand keeps pushing through a riichi.
        let urge = if self.config.placement_aware {
            placement_urge(table, seat)
        } else {
            0.0
        };
        // Danger grows through the hand, so the same hand value justifies
        // folding later on than it does in the first few turns.
        let elapsed = 1.0 - (table.wall.remaining() as f32 / 70.0).clamp(0.0, 1.0);
        let turn_factor = 1.0 + self.config.turn_aware * 0.5 * elapsed;
        let fold_threshold = self.config.fold_strength * (1.0 - 0.5 * urge) * turn_factor;
        let should_fold_learned = match self.value_use {
            ValueUse::LearnedValue => self
                .learned_value(table, seat, decision)
                .map(|v| v < self.value_threshold),
            ValueUse::WinProbability => self
                .learned_win_probability(table, seat, decision)
                .map(|p| p < self.value_threshold),
        };
        let folding = threatened
            && if let Some(fold) = should_fold_learned {
                fold
            } else if self.config.push_fold {
                strength < fold_threshold
            } else {
                best_shanten >= 3
            };
        if folding {
            let mut best: Option<(Action, f32, i32)> = None;
            for &(action, kind) in &candidates {
                let d = danger[kind as usize];
                let mut rest = player.hand;
                rest[kind as usize] -= 1;
                let keep = tile_keep_value(&rest, kind);
                let better = match &best {
                    None => true,
                    Some((_, bd, bk)) => d < *bd - 1e-6 || ((d - *bd).abs() < 1e-6 && keep < *bk),
                };
                if better {
                    best = Some((action, d, keep));
                }
            }
            if let Some((action, _, _)) = best {
                return action;
            }
        }
        // Pushing but wary: among similarly efficient discards prefer the safer
        // one. The weight depends on how close the hand is to tenpai.
        let caution = if !threatened {
            0.0
        } else if self.config.push_fold {
            // Push hard with a strong hand, stay careful with a mediocre one.
            if strength >= 2.5 {
                self.config.push_caution
            } else {
                self.config.careful_caution
            }
        } else if best_shanten >= 2 {
            0.6
        } else {
            0.2
        } * self.config.defense_weight;
        let allowed: Option<HashSet<Kind>> = None;

        // Pass 2: ukeire for the best shanten candidates.
        let mut best: Option<(Action, Kind, u32, i32, f32)> = None;
        for &(action, kind) in &candidates {
            if let Some(allowed) = &allowed {
                if !allowed.contains(&kind) {
                    continue;
                }
            }
            let mut rest = player.hand;
            rest[kind as usize] -= 1;
            let sh = shanten(&rest, melds);
            if allowed.is_none() && sh > best_shanten {
                continue;
            }
            let uke = useful_kinds(&rest, melds, visible)
                .iter()
                .map(|&(_, n)| n as u32)
                .sum::<u32>();
            let keep = tile_keep_value(&rest, kind);
            // Yaku and dora are worth tiles: a hand that keeps a 役牌 pair or a
            // flush direction is worth more than a slightly wider shape, and a
            // dora tile is worth holding on to.
            let value_bonus = if self.config.yaku_aware {
                let yaku_kept = yaku_score(&rest, &player.melds, round_wind, seat_wind) as f32;
                let dora_kept = dora_kinds
                    .iter()
                    .filter(|&&d| rest[d as usize] > 0)
                    .count() as f32;
                self.config.yaku_weight * yaku_kept + self.config.dora_weight * dora_kept
            } else {
                0.0
            };
            let score = uke as f32 + value_bonus - caution * danger[kind as usize] * 40.0;
            let better = match &best {
                None => true,
                Some((_, _, bu, bk, bv)) => {
                    let prev =
                        *bu as f32 + *bv - caution * danger[*bk as usize] * 40.0;
                    score > prev + 1e-4
                }
            };
            if better {
                best = Some((action, kind, uke, keep, value_bonus));
            }
        }

        let Some((action, _, _, _, _)) = best else {
            return candidates[0].0;
        };

        // Riichi: take it when the wait is alive.
        if riichi_available {
            if let Some((r_action, r_kind)) =
                candidates.iter().find(|(a, _)| matches!(a, Action::Discard { riichi: true, .. }))
                    .map(|&(a, k)| (a, k))
            {
                let mut rest = player.hand;
                rest[r_kind as usize] -= 1;
                let waits = winning_kinds(&rest, melds);
                let live: u32 = waits
                    .iter()
                    .map(|&w| 4u32.saturating_sub(visible[w as usize] as u32))
                    .sum();
                if live >= self.config.riichi_min_live {
                    return r_action;
                }
            }
        }
        action
    }

    fn choose_call(
        &self,
        table: &Table,
        seat: u8,
        decision: &Decision,
        current: i8,
        visible: &Counts,
    ) -> Option<Action> {
        let player = &table.players[seat as usize];
        let melds = player.melds.len() as u8;
        if self.config.call_eagerness == 0 {
            return None;
        }
        // Keep a tenpai hand closed: riichi is worth more than the call.
        if current <= 0 {
            return None;
        }
        // Do not open a hand that has no yaku path.
        let round_wind = table.round_wind;
        let seat_wind = table.seat_wind(seat);

        let mut best: Option<(Action, u32)> = None;
        for &a in decision.actions.iter() {
            let Action::Meld { meld } = a else { continue };
            if meld.kind == MeldKind::Chi && self.config.call_eagerness < 2 {
                // チー is worse than ポン: it exposes the hand without gaining a
                // tile in hand, so the baseline only takes it when eager.
                continue;
            }
            let mut rest = player.hand;
            for &t in meld.as_slice() {
                if t == meld.called {
                    continue; // this tile came from the discard pile
                }
                let k = kind_of(t) as usize;
                if rest[k] == 0 {
                    continue;
                }
                rest[k] -= 1;
            }
            let after = shanten(&rest, melds + 1);
            if after >= current {
                continue;
            }
            if !yaku_potential(&rest, &player.melds, round_wind, seat_wind) {
                continue;
            }
            let uke: u32 = useful_kinds(&rest, melds + 1, visible)
                .iter()
                .map(|&(_, n)| n as u32)
                .sum();
            let better = match best {
                None => true,
                Some((_, bu)) => uke > bu,
            };
            if better {
                best = Some((a, uke));
            }
        }
        best.map(|(a, _)| a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RandomAgent;
    use mmj_core::rules::Rules;
    use mmj_core::state::TableConfig;

    #[test]
    fn efficiency_beats_random_over_a_few_matches() {
        let mut eff_total = 0i64;
        let mut rnd_total = 0i64;
        for seed in 0..6u64 {
            let mut agents: [Box<dyn Agent>; 4] = [
                Box::new(EfficiencyAgent::new("eff")),
                Box::new(RandomAgent::new(seed * 4 + 1)),
                Box::new(EfficiencyAgent::new("eff")),
                Box::new(RandomAgent::new(seed * 4 + 3)),
            ];
            let config = TableConfig {
                rules: Rules::tenhou().single_round(),
                seed,
            };
            let result = crate::play_game(&mut agents, config);
            eff_total += result.scores[0] as i64 + result.scores[2] as i64;
            rnd_total += result.scores[1] as i64 + result.scores[3] as i64;
        }
        assert!(
            eff_total > rnd_total,
            "efficiency {} should outscore random {}",
            eff_total,
            rnd_total
        );
    }

    #[test]
    fn efficiency_wins_hands() {
        let mut wins = 0u32;
        for seed in 0..8u64 {
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
            let result = crate::play_game(&mut agents, config);
            wins += result.wins.iter().sum::<u32>();
        }
        assert!(wins >= 8, "only {} wins in 8 tonpuu matches", wins);
    }
}
