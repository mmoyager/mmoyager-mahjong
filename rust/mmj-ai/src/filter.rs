//! Action-space filters: strategic ablations of a learned policy.
//!
//! The learned policy is a single network over every decision, so "does this
//! agent over- or under-declare riichi?" cannot be read off its weights. This
//! wrapper answers such questions the only way that counts — by playing matches
//! with one strategic option removed or forced, and comparing the paired score
//! against the untouched policy.
//!
//! Unlike a counterfactual rollout, an ablation measures the consequence over a
//! whole match, and it needs no model of the opponents.

use mmj_core::action::Action;
use mmj_core::state::{Decision, Table, Trigger};

use crate::{Agent, NnAgent};

/// Which option is meddled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionFilter {
    /// Never declare riichi (still discard, still win).
    NoRiichi,
    /// Declare riichi whenever it is legal.
    ForceRiichi,
    /// Never chi / pon / kan on another player's discard.
    NoCall,
    /// Always take a call when one is offered.
    ForceCall,
}

impl ActionFilter {
    fn keeps(&self, action: &Action, trigger: Trigger) -> bool {
        let is_riichi = matches!(action, Action::Discard { riichi: true, .. });
        let is_call = matches!(action, Action::Meld { .. });
        match self {
            ActionFilter::NoRiichi => !is_riichi,
            ActionFilter::ForceRiichi => is_riichi,
            // A concealed or added kan is a self-turn decision, not a call on
            // somebody else's discard, so it is left alone.
            ActionFilter::NoCall => !(is_call && !matches!(trigger, Trigger::SelfTurn)),
            ActionFilter::ForceCall => is_call && !matches!(trigger, Trigger::SelfTurn),
        }
    }

    /// A short label used in evaluation specs and reports.
    pub fn tag(&self) -> &'static str {
        match self {
            ActionFilter::NoRiichi => "no-riichi",
            ActionFilter::ForceRiichi => "force-riichi",
            ActionFilter::NoCall => "no-call",
            ActionFilter::ForceCall => "force-call",
        }
    }
}

pub struct FilterAgent {
    nn: NnAgent,
    filter: ActionFilter,
    /// Decisions where the filter actually changed the legal set.
    pub touched: u64,
    pub total: u64,
}

impl FilterAgent {
    pub fn new(nn: NnAgent, filter: ActionFilter) -> Self {
        FilterAgent {
            nn,
            filter,
            touched: 0,
            total: 0,
        }
    }
}

impl Agent for FilterAgent {
    fn name(&self) -> String {
        format!("filter({})", self.filter.tag())
    }

    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        if decision.actions.len() <= 1 {
            return decision.actions[0];
        }
        self.total += 1;
        let kept: Vec<Action> = decision
            .actions
            .iter()
            .copied()
            .filter(|a| self.filter.keeps(a, decision.trigger))
            .collect();
        // A filter that empties the legal set has nothing to say about this
        // decision; fall back to the unmodified policy.
        if kept.is_empty() || kept.len() == decision.actions.len() {
            return self.nn.act(table, seat, decision);
        }
        self.touched += 1;
        let narrowed = Decision {
            seat: decision.seat,
            trigger: decision.trigger,
            actions: kept,
        };
        self.nn.act(table, seat, &narrowed)
    }

    fn on_round_start(&mut self, table: &Table, seat: u8) {
        self.nn.on_round_start(table, seat);
    }
}

/// Bias the learned policy towards (or away from) calling.
///
/// The alignment diagnostic (round 17) found the call window to be the model's
/// weakest imitation class: agreement 0.9618 there against 0.98+ elsewhere, and
/// 76% of those disagreements are the model passing where its teacher would
/// call. In self-play the model calls on 10-12% of hands while its teacher calls
/// on 18%. This wrapper shifts that threshold at play time without retraining,
/// which is the cheapest way to ask whether the gap costs points.
pub struct CallBiasAgent {
    nn: NnAgent,
    pub call_bias: f32,
}

impl CallBiasAgent {
    pub fn new(nn: NnAgent, call_bias: f32) -> Self {
        CallBiasAgent { nn, call_bias }
    }
}

impl Agent for CallBiasAgent {
    fn name(&self) -> String {
        format!("call-bias({:.2})", self.call_bias)
    }

    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        use mmj_core::state::Trigger;
        if decision.actions.len() <= 1 {
            return decision.actions[0];
        }
        // A win is never passed up.
        if let Some(a) = decision
            .actions
            .iter()
            .find(|a| matches!(a, Action::Tsumo | Action::Ron))
        {
            return *a;
        }
        // Only call windows are nudged; a concealed or added kan is a self-turn
        // decision, not a call on somebody else's discard.
        if !matches!(decision.trigger, Trigger::Discard { .. } | Trigger::Chankan { .. }) {
            return self.nn.act(table, seat, decision);
        }
        let (dist, _) = self.nn.evaluate(table, seat, decision);
        let mut best: Option<(f32, Action)> = None;
        for (action, p) in dist {
            let weight = if matches!(action, Action::Meld { .. }) {
                p * self.call_bias
            } else {
                p
            };
            if best.map(|(bp, _)| weight > bp).unwrap_or(true) {
                best = Some((weight, action));
            }
        }
        best.map(|(_, a)| a).unwrap_or_else(|| crate::fallback(decision))
    }

    fn on_round_start(&mut self, table: &Table, seat: u8) {
        self.nn.on_round_start(table, seat);
    }
}
