//! An agent that plays with a trained policy/value network.

use crate::{Agent, fallback};
use mmj_core::action::{Action, ActionKind};
use mmj_core::state::{Decision, Table, Trigger};
use mmj_nn::{Net, Obs, POLICY_DIM, encode, encode_state, sample_from};
use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::io;
use std::path::{Path, PathBuf};

/// Wraps a [`Net`] as an [`Agent`].
pub struct NnAgent {
    /// One or more networks. With more than one, the action distribution is the
    /// average of theirs — an *ensemble* rather than averaged weights, which
    /// requires no shared loss basin and is therefore valid for independently
    /// trained checkpoints.
    nets: Vec<Net>,
    obs: Obs,
    /// Accumulated ensemble distribution.
    probs: Vec<f32>,
    logits: Vec<f32>,
    rng: StdRng,
    /// Sample from the policy instead of taking the argmax.
    pub sample: bool,
    /// Softmax temperature; `1.0` is the trained distribution.
    pub temperature: f32,
    /// Probability of playing a uniformly random legal action instead.
    pub epsilon: f32,
    /// Number of top policy candidates re-ranked by a one-ply value lookahead.
    /// `0` disables the search and plays the raw policy.
    pub search_k: usize,
    /// Scratch observations for the lookahead probes.
    probe: Vec<Obs>,
    label: String,
}

impl NnAgent {
    /// Load a checkpoint written by the Python trainer.
    pub fn from_checkpoint(path: &Path, seed: u64, sample: bool) -> io::Result<Self> {
        Self::from_checkpoint_labeled(path, seed, sample, None)
    }

    /// Load a checkpoint with an explicit display name.
    pub fn from_checkpoint_labeled(
        path: &Path,
        seed: u64,
        sample: bool,
        label: Option<String>,
    ) -> io::Result<Self> {
        let net = Net::load(path)?;
        let label = label.unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "nn".to_string())
        });
        Ok(Self::from_net(net, seed, sample, label))
    }

    /// Load several checkpoints and play their averaged policy.
    pub fn from_checkpoints(
        paths: &[PathBuf],
        seed: u64,
        sample: bool,
        label: Option<String>,
    ) -> io::Result<Self> {
        let mut nets = Vec::with_capacity(paths.len());
        for p in paths {
            nets.push(Net::load(p)?);
        }
        if nets.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no checkpoints given",
            ));
        }
        let label = label.unwrap_or_else(|| match paths.len() {
            1 => paths[0]
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "nn".to_string()),
            n => format!("ensemble x{}", n),
        });
        Ok(Self::from_nets(nets, seed, sample, label))
    }

    /// Wrap in-memory networks (see [`NnAgent::from_checkpoints`]).
    pub fn from_nets(nets: Vec<Net>, seed: u64, sample: bool, label: String) -> Self {
        NnAgent {
            nets,
            obs: Obs::new(),
            probs: vec![0.0; POLICY_DIM],
            logits: vec![0.0; POLICY_DIM],
            rng: StdRng::seed_from_u64(seed),
            sample,
            temperature: 1.0,
            epsilon: 0.0,
            search_k: 0,
            probe: Vec::new(),
            label,
        }
    }

    /// Wrap a single in-memory network.
    pub fn from_net(net: Net, seed: u64, sample: bool, label: String) -> Self {
        Self::from_nets(vec![net], seed, sample, label)
    }

    /// Number of parameters (for logging).
    pub fn params(&self) -> usize {
        self.nets.iter().map(|n| n.params()).sum()
    }

    /// How many networks this agent averages.
    pub fn ensemble_size(&self) -> usize {
        self.nets.len()
    }

    /// Enable one-ply lookahead over the `k` most likely actions.
    pub fn with_search(mut self, k: usize) -> Self {
        self.search_k = k;
        self
    }

    /// Value estimate for an already-encoded state (used by the analyser's
    /// one-ply lookahead, which has no decision to score, and by the search).
    pub fn value_of(&mut self, obs: &Obs) -> f32 {
        let mut value = 0.0f32;
        for net in self.nets.iter_mut() {
            let mut v = 0.0f32;
            net.forward(&obs.features, &mut self.logits, &mut v);
            value += v;
        }
        value / self.nets.len().max(1) as f32
    }

    /// Distribution over the legal actions plus the value estimate.
    ///
    /// Returns pairs of `(action, probability)` in the order the engine offered
    /// them, which is what the replay analysis displays.
    pub fn evaluate(
        &mut self,
        table: &Table,
        seat: u8,
        decision: &Decision,
    ) -> (Vec<(Action, f32)>, f32) {
        encode(table, seat, decision, &mut self.obs);
        let mut value = 0.0f32;
        self.probs.iter_mut().for_each(|p| *p = 0.0);
        for net in self.nets.iter_mut() {
            let mut v = 0.0f32;
            net.forward(&self.obs.features, &mut self.logits, &mut v);
            if (self.temperature - 1.0).abs() > 1e-6 && self.temperature > 0.0 {
                for l in self.logits.iter_mut() {
                    *l /= self.temperature;
                }
            }
            mmj_nn::masked_softmax_in_place(&mut self.logits, &self.obs.mask);
            for (acc, p) in self.probs.iter_mut().zip(self.logits.iter()) {
                *acc += *p;
            }
            value += v;
        }
        let n = self.nets.len().max(1) as f32;
        self.probs.iter_mut().for_each(|p| *p /= n);
        value /= n;
        // Copy the averaged distribution into the buffer the sampler reads.
        self.logits.copy_from_slice(&self.probs);
        let mut out = Vec::with_capacity(self.obs.actions.len());
        for (i, &action) in self.obs.actions.iter().enumerate() {
            let slot = self.obs.slots[i];
            out.push((action, self.probs[slot]));
        }
        (out, value)
    }

    /// Value of the state reached by playing `action`, from `seat`'s point of
    /// view. The opponent replies are not modelled: the table is simply
    /// advanced with everyone passing until `seat` has to decide again, which
    /// makes the candidates comparable without a full search.
    fn probe_value(
        &mut self,
        table: &Table,
        seat: u8,
        action: Action,
        scratch: &mut Obs,
    ) -> Option<f32> {
        let hand = table.rounds_played;
        let mut probe = table.fork();
        probe.submit(seat, action).ok()?;
        // A candidate that ends the hand (a win, or a discard that somebody
        // rons) would be scored on the *next* hand, where the value head sees
        // roughly zero for everyone. That makes winning look worthless and the
        // search learns to pass on wins, so such candidates are not searchable.
        if probe.rounds_played != hand {
            return None;
        }
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 64 || probe.finished {
                return None;
            }
            let decisions = probe.decisions().to_vec();
            if decisions.is_empty() {
                return None;
            }
            if decisions.iter().any(|d| d.seat == seat) {
                break;
            }
            let mut acted = false;
            for d in decisions {
                if probe.submit(d.seat, Action::Pass).is_ok() {
                    acted = true;
                }
            }
            if !acted {
                return None;
            }
        }
        encode_state(&probe, seat, Trigger::SelfTurn, scratch, None);
        Some(self.value_of(scratch))
    }

    /// Public wrapper around the lookahead, used by the replay analyser.
    pub fn lookahead_value(&mut self, table: &Table, seat: u8, action: Action) -> Option<f32> {
        let mut scratch = Obs::new();
        self.probe_value(table, seat, action, &mut scratch)
    }

    /// Choose by one-ply lookahead among the most likely actions.
    ///
    /// Only discards are searched: they are the overwhelming majority of
    /// decisions and the only ones where the successor state is comparable.
    fn act_with_search(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        let (dist, _) = self.evaluate(table, seat, decision);
        // Wins are never searched: taking a win always beats not taking it, and
        // the probe cannot see the payment because the hand ends. Calls are also
        // left to the policy — the successor states are not comparable with a
        // hand-scoped value.
        if !matches!(decision.trigger, Trigger::SelfTurn)
            || decision.actions.iter().any(|a| matches!(a, Action::Tsumo | Action::Ron))
        {
            return self.policy_choice(decision);
        }
        let mut candidates: Vec<(f32, Action)> = dist
            .iter()
            .filter(|(a, _)| matches!(a, Action::Discard { .. }))
            .map(|(a, p)| (*p, *a))
            .collect();
        if candidates.len() < 2 {
            return self.policy_choice(decision);
        }
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(self.search_k.max(1));

        let mut scratch = std::mem::take(&mut self.probe);
        if scratch.is_empty() {
            scratch.push(Obs::new());
        }
        let mut best: Option<(f32, Action)> = None;
        for (prob, action) in candidates {
            let v = self.probe_value(table, seat, action, &mut scratch[0]);
            // The policy probability is a weak tie-break: it keeps the search
            // from preferring a move the network considers nonsense when the
            // value head cannot separate them.
            let score = match v {
                Some(v) => v + 0.05 * prob,
                None => 0.05 * prob - 100.0,
            };
            if best.as_ref().map(|(b, _)| score > *b).unwrap_or(true) {
                best = Some((score, action));
            }
        }
        self.probe = scratch;
        best.map(|(_, a)| a).unwrap_or(decision.actions[0])
    }

    /// The policy's own choice for the current decision (top legal slot).
    fn policy_choice(&mut self, decision: &Decision) -> Action {
        let mask = std::mem::take(&mut self.obs.mask);
        let slot = self.choose_slot(&mask);
        self.obs.mask = mask;
        self.obs
            .slots
            .iter()
            .position(|&s| s == slot)
            .map(|i| self.obs.actions[i])
            .unwrap_or_else(|| fallback(decision))
    }

    /// Pick a slot, honouring the exploration settings.
    fn choose_slot(&mut self, mask: &[u8]) -> usize {
        let legal: Vec<usize> = (0..POLICY_DIM).filter(|&i| mask[i] == 1).collect();
        if legal.is_empty() {
            return 0;
        }
        if self.epsilon > 0.0 && self.rng.gen::<f32>() < self.epsilon {
            return legal[self.rng.gen_range(0..legal.len())];
        }
        if self.sample {
            sample_from(&self.logits, self.rng.gen::<f64>())
        } else {
            let mut best = legal[0];
            for &i in &legal {
                if self.logits[i] > self.logits[best] {
                    best = i;
                }
            }
            best
        }
    }
}

impl Agent for NnAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn act(&mut self, table: &Table, seat: u8, decision: &Decision) -> Action {
        if decision.actions.len() == 1 {
            return decision.actions[0];
        }
        if self.search_k > 1 && decision.actions.len() > 1 {
            return self.act_with_search(table, seat, decision);
        }
        let (_, _) = self.evaluate(table, seat, decision);
        let mask = std::mem::take(&mut self.obs.mask);
        let slot = self.choose_slot(&mask);
        self.obs.mask = mask;
        // Several legal actions can share a slot (a red five and a normal five
        // of the same kind); the first one wins.
        self.obs
            .slots
            .iter()
            .position(|&s| s == slot)
            .map(|i| self.obs.actions[i])
            .unwrap_or_else(|| fallback(decision))
    }
}

/// Convenience: does this decision contain a riichi declaration?
pub fn has_riichi(decision: &Decision) -> bool {
    decision
        .actions
        .iter()
        .any(|a| matches!(a, Action::Discard { riichi: true, .. }))
}

/// Convenience: the kind of the winning action, if any.
pub fn win_kind(decision: &Decision) -> Option<ActionKind> {
    decision
        .actions
        .iter()
        .map(|a| a.kind())
        .find(|k| matches!(k, ActionKind::Ron | ActionKind::Tsumo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmj_core::rules::Rules;
    use mmj_core::state::TableConfig;
    use mmj_nn::FEATURE_DIM;

    #[test]
    fn nn_agent_returns_legal_actions() {
        let net = Net::new(&[64, 32], FEATURE_DIM, 3);
        let mut agent = NnAgent::from_net(net, 1, true, "test".to_string());
        let mut table = Table::new(TableConfig {
            rules: Rules::tenhou().single_round(),
            seed: 5,
        });
        for _ in 0..40 {
            let decisions = table.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            for d in decisions {
                let a = agent.act(&table, d.seat, &d);
                assert!(d.allows(&a), "{:?} not in {:?}", a, d.actions);
                table.submit(d.seat, a).unwrap();
                if table.finished {
                    break;
                }
            }
            if table.finished {
                break;
            }
        }
    }

    #[test]
    fn evaluate_is_a_distribution() {
        let net = Net::new(&[32], FEATURE_DIM, 9);
        let mut agent = NnAgent::from_net(net, 2, false, "test".to_string());
        let table = Table::new(TableConfig::new(11));
        let d = table.decisions()[0].clone();
        let (dist, value) = agent.evaluate(&table, 0, &d);
        let sum: f32 = dist.iter().map(|&(_, p)| p).sum();
        assert!((sum - 1.0).abs() < 1e-4, "sum {}", sum);
        assert!(dist.iter().all(|&(_, p)| p >= 0.0));
        assert!(value.is_finite());
        assert!(dist.len() > 1);
    }
}
