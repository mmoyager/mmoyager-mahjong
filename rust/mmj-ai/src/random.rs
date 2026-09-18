//! A uniformly random legal agent: the weakest possible baseline.

use crate::Agent;
use mmj_core::action::Action;
use mmj_core::state::{Decision, Table};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Chooses uniformly at random among the legal actions.
pub struct RandomAgent {
    rng: StdRng,
    label: String,
}

impl RandomAgent {
    pub fn new(seed: u64) -> Self {
        RandomAgent {
            rng: StdRng::seed_from_u64(seed),
            label: format!("random-{}", seed),
        }
    }
}

impl Agent for RandomAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn act(&mut self, _table: &Table, _seat: u8, decision: &Decision) -> Action {
        let i = self.rng.gen_range(0..decision.actions.len());
        decision.actions[i]
    }
}
