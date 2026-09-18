//! Play one game with a mixture of agents, optionally recording every
//! decision of the seats that are learning.

use mmj_ai::{Agent, EfficiencyAgent};
use mmj_core::rules::Rules;
use mmj_core::state::{Decision, Event, Table, TableConfig, Trigger};
use mmj_nn::data::{OutcomeParts, RECORD_BYTES, encode_record};
use mmj_nn::{FEATURE_DIM, Net, Obs, POLICY_DIM, encode, sample_from};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// What plays a given seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatSpec {
    /// The network being improved, with exploration.
    Learner,
    /// A frozen network: the same checkpoint, played greedily. This is plain
    /// self-play; point it at an older checkpoint for league play.
    Frozen,
    /// The tile-efficiency baseline.
    Efficiency,
    /// Uniformly random legal actions.
    Random,
}

/// Exploration and rules for one generation run.
#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub rules: Rules,
    /// Sample from the policy instead of taking the argmax (learner only).
    pub sample: bool,
    pub temperature: f32,
    /// Probability of a uniformly random legal action (learner only).
    pub epsilon: f32,
    /// DAgger-style labels: the learner still plays the game, but the recorded
    /// target is the teacher's action for the state *the learner* reached.
    ///
    /// Plain behaviour cloning only sees states the teacher visits; the learner's
    /// own mistakes lead it off that distribution and the error compounds. This
    /// keeps the state distribution on-policy and the labels expert.
    pub dagger_labels: bool,
    /// Restrict DAgger labels to decisions where the learner's hand is at least
    /// this far from tenpai. `-1` disables the shanten rule. Comparisons of the
    /// student against the teacher show the largest disagreement in the middle
    /// of the hand, so this lets a targeted retrain focus there instead of
    /// diluting the data with rows the student already plays exactly like the
    /// teacher.
    pub dagger_min_shanten: i8,
    /// Also include every call window (chi / pon / kan / ron windows) in the
    /// DAgger region. Assisted-play measurements put the student's largest
    /// remaining deficit there.
    pub dagger_calls: bool,
    /// Use the improved teacher (safety model + yaku awareness) for the
    /// rule-based seats when generating imitation data.
    pub teacher_v2: bool,
    /// Use the fourth teacher level: the improved teacher plus the
    /// discard-sequence read in its safety model.
    pub teacher_v4: bool,
    /// Use the *third* teacher level, which also treats well-developed open
    /// hands as threats. Its criterion is already visible to the network — the
    /// encoder's danger block is computed with open threats and strict suji —
    /// so its labels are worth distilling even without a feature change.
    pub teacher_v3: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        RunnerConfig {
            rules: Rules::tenhou(),
            sample: true,
            temperature: 1.0,
            epsilon: 0.02,
            dagger_labels: false,
            dagger_min_shanten: -1,
            dagger_calls: false,
            teacher_v2: false,
            teacher_v4: false,
            teacher_v3: false,
        }
    }
}

/// Final result of one game.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameOutcome {
    pub scores: [i32; 4],
    pub rounds: u32,
}

/// One recorded decision.
#[derive(Clone, Debug)]
pub struct RecordRow {
    pub features: Vec<f32>,
    pub mask: Vec<u8>,
    pub slot: usize,
    pub seat: u8,
    /// The seat's score when the decision was made.
    pub score_at: i32,
    /// Which hand of the match the decision belongs to.
    pub hand: u32,
}

/// Per-thread worker state: one network, per-seat buffers, one RNG.
pub struct Engine {
    net: Net,
    /// Optional frozen opponent checkpoint (league play).
    opponent: Option<Net>,
    obs: Vec<Obs>,
    logits: Vec<Vec<f32>>,
    rng: StdRng,
    fighters: Vec<EfficiencyAgent>,
    pub games: u64,
    pub decisions: u64,
}

impl Engine {
    pub fn new(net: Net, opponent: Option<Net>, seed: u64) -> Self {
        Engine {
            net,
            opponent,
            obs: (0..4).map(|_| Obs::new()).collect(),
            logits: (0..4).map(|_| vec![0.0; POLICY_DIM]).collect(),
            rng: StdRng::seed_from_u64(seed),
            fighters: (0..4)
                .map(|i| EfficiencyAgent::new(format!("eff{}", i)))
                .collect(),
            games: 0,
            decisions: 0,
        }
    }

    /// Encode the current decision into the seat's observation buffer.
    fn encode_obs(&mut self, table: &Table, seat: u8, decision: &Decision) {
        let obs = &mut self.obs[seat as usize];
        obs.features.fill(0.0);
        encode(table, seat, decision, obs);
    }

    /// Slot index of an action the engine offered.
    /// Whether a decision falls inside the DAgger labelling region.
    fn dagger_covers(&self, table: &Table, seat: u8, cfg: &RunnerConfig, trigger: Trigger) -> bool {
        if cfg.dagger_min_shanten < 0 && !cfg.dagger_calls {
            return true;
        }
        let mut covered = false;
        if cfg.dagger_min_shanten >= 0 {
            let view = table.view(seat);
            let shanten = view.players[seat as usize].shanten.unwrap_or(8);
            covered |= shanten >= cfg.dagger_min_shanten;
        }
        if cfg.dagger_calls && !matches!(trigger, Trigger::SelfTurn) {
            covered = true;
        }
        covered
    }

    fn slot_of(&self, seat: u8, action: &mmj_core::action::Action) -> Option<usize> {
        let obs = &self.obs[seat as usize];
        obs.actions
            .iter()
            .position(|a| a == action)
            .map(|i| obs.slots[i])
    }

    /// Run the network and choose a slot, respecting the exploration settings.
    fn nn_slot(
        &mut self,
        table: &Table,
        seat: u8,
        decision: &Decision,
        cfg: &RunnerConfig,
        explore: bool,
        use_opponent: bool,
    ) -> usize {
        self.encode_obs(table, seat, decision);
        let mask = std::mem::take(&mut self.obs[seat as usize].mask);
        let mut logits = std::mem::take(&mut self.logits[seat as usize]);
        let mut value = 0.0f32;
        {
            let features = &self.obs[seat as usize].features;
            if use_opponent {
                self.opponent
                    .as_mut()
                    .expect("frozen seat without an opponent network")
                    .forward(features, &mut logits, &mut value);
            } else {
                self.net.forward(features, &mut logits, &mut value);
            }
        }
        let slot = self.pick_slot(
            &mut logits,
            &mask,
            explore && cfg.sample,
            if explore { cfg.temperature } else { 1.0 },
            if explore { cfg.epsilon } else { 0.0 },
        );
        self.logits[seat as usize] = logits;
        self.obs[seat as usize].mask = mask;
        slot
    }

    /// Run one game and return the encoded record blob.
    pub fn play_game(
        &mut self,
        cfg: &RunnerConfig,
        seats: [SeatSpec; 4],
        record: [bool; 4],
        seed: u64,
    ) -> (GameOutcome, Vec<u8>) {
        // The fighters are created once per thread; switch them to the improved
        // teacher when the caller asked for it.
        for f in self.fighters.iter_mut() {
            let label = f.name();
            *f = if cfg.teacher_v4 {
                EfficiencyAgent::reading("teacher-v4")
            } else if cfg.teacher_v3 {
                EfficiencyAgent::smarter("teacher-v3")
            } else if cfg.teacher_v2 {
                EfficiencyAgent::smart(label)
            } else {
                EfficiencyAgent::new(label)
            };
        }
        let config = TableConfig {
            rules: cfg.rules,
            seed,
        };
        let mut table = Table::new(config);
        let mut rows: Vec<RecordRow> = Vec::with_capacity(700);
        let mut guard = 0usize;

        while !table.finished {
            let decisions = table.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            for decision in decisions {
                let seat = decision.seat;
                let spec = seats[seat as usize];
                let rec = record[seat as usize];
                let score_at = table.players[seat as usize].score;

                let (action, slot) = match spec {
                    SeatSpec::Efficiency => {
                        let action = self.fighters[seat as usize].act(&table, seat, &decision);
                        let slot = if rec {
                            self.encode_obs(&table, seat, &decision);
                            self.slot_of(seat, &action).unwrap_or(0)
                        } else {
                            0
                        };
                        (action, slot)
                    }
                    SeatSpec::Random => {
                        let i = self.rng.gen_range(0..decision.actions.len());
                        let action = decision.actions[i];
                        let slot = if rec {
                            self.encode_obs(&table, seat, &decision);
                            self.slot_of(seat, &action).unwrap_or(0)
                        } else {
                            0
                        };
                        (action, slot)
                    }
                    SeatSpec::Learner | SeatSpec::Frozen => {
                        let explore = spec == SeatSpec::Learner;
                        let use_opponent = spec == SeatSpec::Frozen && self.opponent.is_some();
                        let slot =
                            self.nn_slot(&table, seat, &decision, cfg, explore, use_opponent);
                        let obs = &self.obs[seat as usize];
                        let action = obs
                            .slots
                            .iter()
                            .position(|&s| s == slot)
                            .map(|i| obs.actions[i])
                            .unwrap_or(decision.actions[0]);
                        (action, slot)
                    }
                };

                // A shanten-filtered DAgger run records only the decisions the
                // filter covers, so the resulting file is a pure, undiluted set
                // of "the student was in this region, the teacher played this".
                let outside_region =
                    cfg.dagger_labels && cfg.dagger_min_shanten >= 0 && spec == SeatSpec::Learner;
                let rec = rec && (!outside_region || self.dagger_covers(&table, seat, cfg, decision.trigger));

                if rec {
                    // Under DAgger the label comes from the rule-based teacher
                    // evaluated on the state the learner actually produced.
                    let label_slot = if cfg.dagger_labels
                        && spec == SeatSpec::Learner
                        && self.dagger_covers(&table, seat, cfg, decision.trigger)
                    {
                        let teacher_action = self.fighters[seat as usize].act(&table, seat, &decision);
                        self.slot_of(seat, &teacher_action).unwrap_or(slot)
                    } else {
                        slot
                    };
                    let obs = &self.obs[seat as usize];
                    rows.push(RecordRow {
                        features: obs.features.clone(),
                        mask: obs.mask.clone(),
                        slot: label_slot,
                        seat,
                        score_at,
                        hand: table.rounds_played.saturating_sub(1),
                    });
                }

                if let Err(e) = table.submit(seat, action) {
                    panic!("agent produced an illegal action: {}", e);
                }
                self.decisions += 1;
                guard += 1;
                if guard > 500_000 || table.finished {
                    break;
                }
            }
        }
        self.games += 1;

        let final_scores = table.scores();
        // Return target: what this seat gained *from this decision to the end of
        // the current hand*. Scoring a decision by the whole remaining match
        // buries a discard's effect under everything that follows; the hand
        // scope is the level at which a discard actually matters, and it has
        // far lower variance, which is what makes the value head usable for
        // search as well as as a policy-gradient baseline.
        let mut hand_end: Vec<[i32; 4]> = Vec::new();
        for event in &table.history {
            if let Event::RoundEnd { scores, .. } = event {
                hand_end.push(*scores);
            }
        }
        // Outcome components per hand: won value, dealt-in value, and the rest.
        // `Row` hands are indexed the same way as `hand_end`.
        let mut parts_per_hand: Vec<[OutcomeParts; 4]> = vec![
            [OutcomeParts::default(); 4];
            hand_end.len()
        ];
        let mut hand_index = 0usize;
        for event in &table.history {
            match event {
                Event::RoundEnd { .. } => hand_index += 1,
                Event::Win { seat, from, deltas, .. } => {
                    let h = hand_index;
                    if h < parts_per_hand.len() {
                        let s = *seat as usize;
                        // The winner's gain, and the discarder's loss.
                        parts_per_hand[h][s].won_value += deltas[s].max(0) as f32 / 1000.0;
                        if let Some(f) = from {
                            let f = *f as usize;
                            parts_per_hand[h][f].dealt_value += (-deltas[f]).max(0) as f32 / 1000.0;
                        }
                    }
                }
                _ => {}
            }
        }
        let mut blob = Vec::with_capacity(rows.len() * RECORD_BYTES);
        for row in &rows {
            let ret = match hand_end.get(row.hand as usize) {
                Some(scores) => (scores[row.seat as usize] - row.score_at) as f32 / 1000.0,
                None => (final_scores[row.seat as usize] - row.score_at) as f32 / 1000.0,
            };
            // A win makes the winner's whole hand gain the won value; anything
            // else (sticks, tenpai payments) lands in `other`.
            let mut parts = parts_per_hand
                .get(row.hand as usize)
                .map(|p| p[row.seat as usize])
                .unwrap_or_default();
            parts.other_value = ret - parts.won_value + parts.dealt_value;
            encode_record(&mut blob, &row.features, &row.mask, row.slot, ret, row.seat, parts);
        }
        (
            GameOutcome {
                scores: final_scores,
                rounds: table.rounds_played,
            },
            blob,
        )
    }

    fn pick_slot(
        &mut self,
        logits: &mut [f32],
        mask: &[u8],
        sample: bool,
        temperature: f32,
        epsilon: f32,
    ) -> usize {
        let legal: Vec<usize> = (0..POLICY_DIM).filter(|&i| mask[i] == 1).collect();
        if legal.is_empty() {
            return 0;
        }
        if epsilon > 0.0 && self.rng.gen::<f32>() < epsilon {
            return legal[self.rng.gen_range(0..legal.len())];
        }
        if temperature > 0.0 && (temperature - 1.0).abs() > 1e-6 {
            for l in logits.iter_mut() {
                *l /= temperature;
            }
        }
        mmj_nn::masked_softmax_in_place(logits, mask);
        if sample {
            sample_from(logits, self.rng.gen::<f64>())
        } else {
            logits
                .iter()
                .enumerate()
                .filter(|(i, _)| mask.get(*i).copied().unwrap_or(0) == 1)
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(legal[0])
        }
    }

    /// The network, for callers that need to inspect it.
    pub fn net(&self) -> &Net {
        &self.net
    }

    /// Feature dimension of this build.
    pub fn feature_dim() -> usize {
        FEATURE_DIM
    }

}

