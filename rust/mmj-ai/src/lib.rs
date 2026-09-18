//! `mmj-ai` — agents that play [`mmj_core::state::Table`].
//!
//! # Information contract
//!
//! An agent is handed the full `&Table` but **must only read information that is
//! available to `seat`**: its own hand, the discard piles, exposed melds, the
//! dora indicators, the scores and the wall count. Helpers such as
//! [`mmj_core::state::Table::visible_counts`] and
//! [`mmj_core::state::Table::view`] provide exactly that, and the self-play
//! harness can audit the neural agent against them.

pub mod assisted;
pub mod efficiency;
pub mod filter;
pub mod game;
pub mod nn_agent;
pub mod random;
pub mod replay;

pub use assisted::{AssistRegion, AssistedAgent};
pub use efficiency::{EfficiencyAgent, EfficiencyConfig};
pub use filter::{ActionFilter, CallBiasAgent, FilterAgent};
pub use game::{GameResult, play_game};
pub use nn_agent::NnAgent;
pub use random::RandomAgent;
pub use replay::{Analysis, ReplayFile, analyze, format_report};

use mmj_core::action::Action;
use mmj_core::state::{Decision, Table};

/// Something that can choose an action for one seat.
pub trait Agent: Send {
    /// Display name, used in logs and evaluation output.
    fn name(&self) -> String;

    /// Choose one of `decision.actions`. The table rejects any other action.
    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action;

    /// Notification that a new hand has been dealt.
    fn on_round_start(&mut self, _table: &Table, _seat: u8) {}
}

/// The first action matching a predicate.
pub fn prefer<F: Fn(&Action) -> bool>(decision: &Decision, pred: F) -> Option<Action> {
    decision.actions.iter().copied().find(|a| pred(a))
}

/// Legally safe default: pass when possible.
pub fn fallback(decision: &Decision) -> Action {
    if decision.actions.contains(&Action::Pass) {
        Action::Pass
    } else {
        decision.actions[0]
    }
}
