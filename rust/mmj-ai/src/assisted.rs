//! Teacher-assisted policy: a measuring instrument for a learned agent.
//!
//! The wrapper plays the network everywhere except in one explicitly named
//! region of the game, where it defers to the rule-based teacher. Playing such a
//! hybrid against the plain network in a paired evaluation answers a question no
//! agreement statistic can: *which decisions actually cost the network points?*
//!
//! * Hybrid scores clearly better -> the network is still losing ground in that
//!   region, and teaching it there is worth effort.
//! * Hybrid scores the same or worse -> the network already outgrew the teacher
//!   in that region, and no amount of imitation from that teacher will help.

use mmj_core::action::Action;
use mmj_core::state::{Decision, Table, Trigger};

use crate::{Agent, EfficiencyAgent, NnAgent};

/// Which region the teacher is allowed to take over.
#[derive(Clone, Copy, Debug, Default)]
pub struct AssistRegion {
    /// Defer to the teacher on own-turn discards while the hand is at least this
    /// far from tenpai. `None` disables the rule.
    pub min_shanten: Option<i8>,
    /// Defer to the teacher in call windows (chi / pon / kan / ron windows).
    pub calls: bool,
}

pub struct AssistedAgent {
    nn: NnAgent,
    teacher: EfficiencyAgent,
    region: AssistRegion,
    /// Terminal tiles the region covers, for context in reports.
    pub assisted: u64,
    pub total: u64,
}

impl AssistedAgent {
    pub fn new(nn: NnAgent, teacher: EfficiencyAgent, region: AssistRegion) -> Self {
        AssistedAgent {
            nn,
            teacher,
            region,
            assisted: 0,
            total: 0,
        }
    }

    /// Fraction of decisions the teacher took over.
    pub fn assist_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.assisted as f64 / self.total as f64
        }
    }

    fn defers(&self, table: &Table, seat: u8, decision: &Decision) -> bool {
        // A win is never delegated: the teacher values the hand differently, but
        // accepting a win is not a judgement call.
        if decision
            .actions
            .iter()
            .any(|a| matches!(a, Action::Tsumo | Action::Ron))
        {
            return false;
        }
        match decision.trigger {
            Trigger::SelfTurn => {
                let Some(min_shanten) = self.region.min_shanten else {
                    return false;
                };
                if !decision
                    .actions
                    .iter()
                    .any(|a| matches!(a, Action::Discard { .. }))
                {
                    return false;
                }
                let view = table.view(seat);
                let shanten = view.players[seat as usize].shanten.unwrap_or(8);
                shanten >= min_shanten
            }
            Trigger::Discard { .. } | Trigger::Chankan { .. } => self.region.calls,
        }
    }
}

impl Agent for AssistedAgent {
    fn name(&self) -> String {
        let mut r = String::from("assisted(");
        match self.region.min_shanten {
            Some(s) => r.push_str(&format!("sh>={}", s)),
            None => r.push_str("sh=off"),
        }
        if self.region.calls {
            r.push_str("+calls");
        }
        r.push(')');
        r
    }

    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        if decision.actions.len() == 1 {
            return decision.actions[0];
        }
        self.total += 1;
        if self.defers(table, seat, decision) {
            self.assisted += 1;
            return self.teacher.act(table, seat, decision);
        }
        self.nn.act(table, seat, decision)
    }

    fn on_round_start(&mut self, table: &Table, seat: u8) {
        self.nn.on_round_start(table, seat);
        self.teacher.on_round_start(table, seat);
    }
}
