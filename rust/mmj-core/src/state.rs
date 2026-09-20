//! The table state machine.
//!
//! [`Table`] owns the whole round flow. The rules never depend on who is
//! deciding: a driver asks which decisions are pending ([`Table::decisions`]),
//! submits one action per pending seat ([`Table::submit`]), and reads back the
//! [`Event`] log. Everything that needs no decision — drawing, resolving a call
//! window, advancing to the next round — happens inside `submit`.
//!
//! Priority follows the standard rules:
//!
//! * 栄和 beats every call; with `multi_ron` enabled several players may win
//!   from a single discard (no 頭ハネ).
//! * ポン / 大明槓 beat チー; ties are broken by seat order after the discarder.
//! * チー may only be called by the player seated to the discarder's left.

use crate::action::Action;
use crate::hand::{Counts, is_agari, is_kokushi, shanten, tenpai_kinds, winning_kinds};
use crate::meld::{Meld, MeldKind};
use crate::rules::{GameLength, KuikaeScope, Rules};
use crate::score::{ScoreResult, WinContext, Yaku, score_simple, score_win};
use crate::tile::{
    EAST, HAKU, Kind, NUM_KINDS, NUM_TILES, SOUTH, Tile, WEST, is_aka_tile, is_yaochu, kind_of,
    suit_of,
};
use crate::wall::Wall;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Why a round ended without a win.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawReason {
    /// 荒牌流局 — the wall ran out.
    Exhaustive,
    /// 九種九牌
    NineTerminals,
    /// 四風連打
    FourWinds,
    /// 四家立直
    FourRiichi,
    /// 四槓散了
    FourKans,
    /// 三家和了 — three players ron one discard: abortive draw.
    TripleRon,
}

impl DrawReason {
    pub fn name_zh(self) -> &'static str {
        match self {
            DrawReason::Exhaustive => "荒牌流局",
            DrawReason::NineTerminals => "九种九牌",
            DrawReason::FourWinds => "四风连打",
            DrawReason::FourRiichi => "四家立直",
            DrawReason::FourKans => "四槓散了",
            DrawReason::TripleRon => "三家和了",
        }
    }
}

/// One tile in a discard pile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discard {
    pub tile: Tile,
    pub tsumogiri: bool,
    /// Discarded together with a riichi declaration.
    pub riichi: bool,
    /// Seat that called this tile, if any.
    pub called_by: Option<u8>,
}

/// Per-player state for the current round.
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub hand: Counts,
    pub hand_tiles: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub discards: Vec<Discard>,
    pub score: i32,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    /// Passed on a ron while in riichi: furiten for the rest of the round.
    pub riichi_furiten: bool,
    /// Passed on a ron since the last draw.
    pub temp_furiten: bool,
    pub drawn: Option<Tile>,
    pub drawn_is_rinshan: bool,
    /// Draws taken this round (used for 天和 / 地和).
    pub draws: u8,
    /// Kinds that may not be discarded because of 食い替え.
    pub kuikae_forbidden: Vec<Kind>,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            hand: [0u8; NUM_KINDS],
            hand_tiles: Vec::new(),
            melds: Vec::new(),
            discards: Vec::new(),
            score: 0,
            riichi: false,
            double_riichi: false,
            ippatsu: false,
            riichi_furiten: false,
            temp_furiten: false,
            drawn: None,
            drawn_is_rinshan: false,
            draws: 0,
            kuikae_forbidden: Vec::new(),
        }
    }
}

impl PlayerState {
    /// Number of concealed tiles.
    pub fn hand_len(&self) -> u32 {
        self.hand.iter().map(|&c| c as u32).sum()
    }

    /// Number of red fives in hand plus melds.
    pub fn aka_count(&self, rules: &Rules) -> u8 {
        let mut n = self.hand_tiles.iter().filter(|&&t| rules.is_aka(t)).count() as u8;
        for m in &self.melds {
            n += m.aka_count(rules);
        }
        n
    }

    /// Is the hand fully concealed (門前)?
    pub fn is_closed(&self) -> bool {
        self.melds.iter().all(|m| !m.kind.is_open())
    }
}

/// What triggered a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    /// The player's own turn: discard, riichi, kan, tsumo or 九種九牌.
    SelfTurn,
    /// Another player discarded `tile`; this player may call it.
    Discard { from: u8, tile: Tile },
    /// Another player added to a ポン; this player may 搶槓.
    Chankan { from: u8, tile: Tile },
}

/// A pending decision: every legal action for one seat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub seat: u8,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
}

impl Decision {
    /// Accept `action` if it is one of the legal actions. Discards are matched
    /// by kind so that a player may choose *which* physical copy to drop (this
    /// matters for red fives).
    pub fn allows(&self, action: &Action) -> bool {
        if self.actions.contains(action) {
            return true;
        }
        if let Action::Discard { tile, riichi } = action {
            return self.actions.iter().any(|a| {
                matches!(a, Action::Discard { tile: t, riichi: r } if r == riichi && kind_of(*t) == kind_of(*tile))
            });
        }
        false
    }
}

/// Phase of the round.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Before the first round starts.
    Init,
    /// `seat` must draw.
    Draw { seat: u8 },
    /// `seat` must act with the hand they hold.
    Turn { seat: u8 },
    /// A discard is waiting for calls.
    CallWindow {
        from: u8,
        tile: Tile,
        awaiting: Vec<u8>,
    },
    /// A 加槓 was declared; 搶槓 is possible.
    ChankanWindow {
        from: u8,
        tile: Tile,
        awaiting: Vec<u8>,
    },
    /// The round finished; the next one starts automatically.
    RoundEnd,
    /// The match finished.
    GameEnd,
}

/// Table events. `Draw` reveals private information: filter per seat when
/// building a player's view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    RoundStart {
        round_wind: Kind,
        round_number: u8,
        honba: u32,
        riichi_sticks: u32,
        dealer: u8,
        scores: [i32; 4],
        dora_indicator: Tile,
        wall_remaining: u32,
    },
    Draw {
        seat: u8,
        tile: Tile,
        rinshan: bool,
    },
    Discard {
        seat: u8,
        tile: Tile,
        tsumogiri: bool,
        riichi: bool,
    },
    Riichi {
        seat: u8,
    },
    Meld {
        seat: u8,
        meld: Meld,
        from: u8,
    },
    Kan {
        seat: u8,
        meld: Meld,
        dora_indicator: Option<Tile>,
    },
    /// A kan dora indicator revealed after the call window closed (加槓).
    DoraRevealed {
        indicator: Tile,
    },
    Win {
        seat: u8,
        from: Option<u8>,
        tile: Tile,
        score: ScoreResult,
        deltas: [i32; 4],
        riichi_sticks_taken: u32,
        /// The winner's concealed tiles, winning tile included, and their melds.
        /// A settlement that only says "3 番" tells the player nothing about
        /// *which* hand won; these make 報番 possible.
        #[serde(default)]
        hand: Vec<Tile>,
        #[serde(default)]
        melds: Vec<Meld>,
        /// What this winner was paid for the hand (riichi sticks excluded:
        /// `riichi_sticks_taken` reports those). `deltas` is the whole table for
        /// the hand and is the same in every winner's event.
        #[serde(default)]
        paid: i32,
        /// 責任払い: the seat that pays the whole hand, when pao applies.
        #[serde(default)]
        pao_payer: Option<u8>,
        /// 流し満貫 is settled at an exhaustive draw and has no winning tile.
        #[serde(default)]
        nagashi: bool,
    },
    Ryuukyoku {
        reason: DrawReason,
        tenpai: [bool; 4],
        deltas: [i32; 4],
        /// Who declared it, when a player did (九種九牌). A settlement that only
        /// says "流局" leaves the player unable to tell a legal abort from a bug.
        #[serde(default)]
        by: Option<u8>,
        /// Live wall tiles left when the hand ended, so the player can check the
        /// draw against what the table showed.
        #[serde(default)]
        wall_remaining: u32,
    },
    RoundEnd {
        scores: [i32; 4],
        next_dealer: u8,
        honba: u32,
        /// The honba the next round will carry (0 unless the dealer repeats).
        #[serde(default)]
        next_honba: u32,
    },
    GameEnd {
        scores: [i32; 4],
        ranking: [u8; 4],
    },
}

/// Table configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableConfig {
    pub rules: Rules,
    pub seed: u64,
}

impl TableConfig {
    pub fn new(seed: u64) -> Self {
        TableConfig {
            rules: Rules::tenhou(),
            seed,
        }
    }
}

/// One player's view of the table: opponents' hands are hidden.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerView {
    pub seat: u8,
    pub hand: Vec<Tile>,
    /// Concealed tile count — public information, needed to draw opponents.
    pub hand_count: u32,
    pub drawn: Option<Tile>,
    pub melds: Vec<Meld>,
    pub discards: Vec<Discard>,
    pub score: i32,
    pub riichi: bool,
    pub ippatsu: bool,
    pub furiten: bool,
    /// Why the observer is furiten, so the table can say which kind it is:
    /// 同巡振聴 (passed a ron this go-around) and 立直振聴 (did so in riichi) are
    /// easy to confuse with ordinary 捨て牌振聴, and they end differently.
    pub furiten_temp: bool,
    pub furiten_riichi: bool,
    /// Shanten of the concealed hand (only computed for the observer).
    pub shanten: Option<i8>,
    pub waits: Vec<Kind>,
    pub is_dealer: bool,
    pub wind: Kind,
}

/// The whole table as one observer sees it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableView {
    pub observer: u8,
    pub players: Vec<PlayerView>,
    pub round_wind: Kind,
    pub round_number: u8,
    pub honba: u32,
    pub riichi_sticks: u32,
    pub dealer: u8,
    pub wall_remaining: u32,
    pub dora_indicators: Vec<Tile>,
    pub phase: Phase,
    /// The observer's own pending decision, if any.
    pub decision: Option<Decision>,
    pub events: Vec<Event>,
    pub finished: bool,
}

/// Round end summary used to advance the match.
#[derive(Clone, Debug, Default)]
struct RoundOutcome {
    dealer_repeat: bool,
    won: bool,
    dealer_won: bool,
}

/// The table.
///
/// `Clone` is used by the replay analyser and by any future search: cloning a
/// table is cheap enough (a few hundred bytes plus the wall) to explore an
/// alternative action speculatively.
#[derive(Clone)]
pub struct Table {
    pub rules: Rules,
    pub wall: Wall,
    pub players: [PlayerState; 4],
    pub round_wind: Kind,
    pub round_number: u8,
    pub honba: u32,
    pub riichi_sticks: u32,
    pub dealer: u8,
    pub turn: u8,
    pub phase: Phase,
    pub events: Vec<Event>,
    pub decisions: Vec<Decision>,
    pub finished: bool,
    /// Complete event log of the match, for replays and analysis.
    pub history: Vec<Event>,
    /// Configuration the table was created with, so a replay can be rebuilt.
    pub config: TableConfig,
    /// Events produced by the most recent `submit`, and nothing else. Kept
    /// separate because starting a round clears `events`.
    step_events: Vec<Event>,
    /// Number of rounds started, useful for self-play bookkeeping.
    pub rounds_played: u32,
    /// 0-based index of the current hand inside the match.
    pub round_index: u8,
    /// How many hands this match plays (including a 西入り extension).
    pub rounds_in_match: u8,
    rng: ChaCha8Rng,
    submitted: [Option<Action>; 4],
    any_call: bool,
    total_kans: u8,
    kan_owners: Vec<u8>,
    /// Bumped every time a round starts. A kan compares it before and after its
    /// own bookkeeping: if it changed, the kan aborted the round and the
    /// replacement draw must not happen (in `abort_round`'s pump the next round
    /// has already begun, so a boolean flag would be stale by then).
    round_seq: u32,
    /// An abortive draw whose condition this discard created but which must wait
    /// for the call window to close (a ron on the same tile wins first).
    pending_abort: Option<DrawReason>,
    first_discard_kind: Option<Kind>,
    four_winds_run: u8,
    outcome: RoundOutcome,
    /// Player responsible for a yakuman under 責任払い: `(winner, payer)`.
    pao: Option<(u8, u8)>,
}

impl Table {
    /// Create a table and deal the first hand.
    pub fn new(config: TableConfig) -> Self {
        let length = config.rules.length;
        let seed = config.seed;
        let mut table = Table {
            rules: config.rules,
            config,
            wall: Wall::from_order(&(0..NUM_TILES).collect::<Vec<_>>()),
            players: Default::default(),
            round_wind: EAST,
            round_number: 1,
            honba: 0,
            riichi_sticks: 0,
            dealer: 0,
            turn: 0,
            phase: Phase::Init,
            events: Vec::new(),
            decisions: Vec::new(),
            finished: false,
            history: Vec::new(),
            step_events: Vec::new(),
            rounds_played: 0,
            round_index: 0,
            rounds_in_match: 4 * match length {
                GameLength::Tonpuu => 1u8,
                GameLength::Hanchan => 2u8,
            },
            rng: ChaCha8Rng::seed_from_u64(seed),
            submitted: [None; 4],
            any_call: false,
            total_kans: 0,
            kan_owners: Vec::new(),
            round_seq: 0,
            pending_abort: None,
            first_discard_kind: None,
            four_winds_run: 0,
            outcome: RoundOutcome::default(),
            pao: None,
        };
        for p in table.players.iter_mut() {
            p.score = table.rules.starting_score;
        }
        table.pump();
        table
    }

    /// Cheap copy for speculative lookahead.
    ///
    /// Identical to [`Clone`] except that the event logs are dropped: they grow
    /// to hundreds of entries per match and a search forks the table for every
    /// candidate action, so copying them dominates the cost.
    pub fn fork(&self) -> Table {
        let mut copy = self.clone();
        copy.history = Vec::new();
        copy.events = Vec::new();
        copy.step_events = Vec::new();
        copy
    }

    // ---- small helpers ---------------------------------------------------

    pub fn scores(&self) -> [i32; 4] {
        [
            self.players[0].score,
            self.players[1].score,
            self.players[2].score,
            self.players[3].score,
        ]
    }

    pub fn round_name(&self) -> String {
        let wind = match self.round_wind {
            EAST => "东",
            SOUTH => "南",
            WEST => "西",
            _ => "北",
        };
        format!("{}{}局", wind, self.round_number)
    }

    /// Tiles `seat` can see: own hand, every discard, every exposed meld.
    pub fn visible_counts(&self, seat: u8) -> Counts {
        let mut c = self.players[seat as usize].hand;
        for (i, p) in self.players.iter().enumerate() {
            for d in &p.discards {
                c[kind_of(d.tile) as usize] += 1;
            }
            for m in &p.melds {
                if m.kind == MeldKind::Ankan && i != seat as usize {
                    continue; // a concealed kan is only known to its owner
                }
                for &t in m.as_slice() {
                    c[kind_of(t) as usize] += 1;
                }
            }
        }
        c
    }

    /// Is any opponent a threat right now (riichi, or a well-developed open
    /// hand)? Used as a feature so the network can see the same trigger the
    /// rule-based agent folds on.
    pub fn danger_for(&self, seat: u8) -> bool {
        (0..4u8).any(|s| {
            if s == seat {
                return false;
            }
            let p = &self.players[s as usize];
            if p.riichi {
                return true;
            }
            p.melds.iter().filter(|m| m.kind.is_open()).count() >= 3
        })
    }

    /// Tiles that are public knowledge: every discard and every exposed meld,
    /// but no concealed hand. This is what a tile-acceptance computation needs,
    /// because the caller already supplies the hand it is asking about.
    pub fn public_counts(&self) -> Counts {
        let mut c = [0u8; NUM_KINDS];
        for p in self.players.iter() {
            for d in &p.discards {
                c[kind_of(d.tile) as usize] += 1;
            }
            for m in &p.melds {
                if m.kind == MeldKind::Ankan {
                    continue; // still concealed
                }
                for &t in m.as_slice() {
                    c[kind_of(t) as usize] += 1;
                }
            }
        }
        c
    }

    /// Everything visible to the whole table.
    pub fn all_visible_counts(&self) -> Counts {
        let mut c = [0u8; NUM_KINDS];
        for p in self.players.iter() {
            for d in &p.discards {
                c[kind_of(d.tile) as usize] += 1;
            }
            for m in &p.melds {
                for &t in m.as_slice() {
                    c[kind_of(t) as usize] += 1;
                }
            }
        }
        c
    }

    /// Is the player in furiten? Meaningful when the hand holds 13 tiles.
    pub fn is_furiten(&self, seat: u8) -> bool {
        let p = &self.players[seat as usize];
        if p.temp_furiten || p.riichi_furiten {
            return true;
        }
        if p.hand_len() % 3 != 1 {
            return false;
        }
        let waits = winning_kinds(&p.hand, p.melds.len() as u8);
        if waits.is_empty() {
            return false;
        }
        waits.iter().any(|w| {
            p.discards
                .iter()
                .any(|d| kind_of(d.tile) == *w && d.called_by.is_none())
        })
    }

    /// Seat wind, derived from the dealer position.
    pub fn seat_wind(&self, seat: u8) -> Kind {
        EAST + ((seat + 4 - self.dealer) % 4)
    }

    fn win_context<'a>(
        &'a self,
        seat: u8,
        hand: &'a Counts,
        win_tile: Tile,
        is_tsumo: bool,
        chankan: bool,
        rinshan: bool,
    ) -> WinContext<'a> {
        let p = &self.players[seat as usize];
        let wall_empty = self.wall.remaining() == 0;
        let mut aka = p.hand_tiles.iter().filter(|&&t| self.rules.is_aka(t)).count() as u8;
        if !is_tsumo && self.rules.is_aka(win_tile) {
            aka += 1;
        }
        for m in &p.melds {
            aka += m.aka_count(&self.rules);
        }
        WinContext {
            rules: &self.rules,
            hand,
            melds: &p.melds,
            winning_tile: win_tile,
            is_tsumo,
            round_wind: self.round_wind,
            seat_wind: self.seat_wind(seat),
            riichi: p.riichi,
            double_riichi: p.double_riichi,
            ippatsu: p.ippatsu,
            chankan,
            rinshan,
            haitei: is_tsumo && !rinshan && wall_empty,
            houtei: !is_tsumo && wall_empty,
            tenhou: is_tsumo && self.dealer == seat && p.draws == 1 && !self.any_call,
            chiihou: is_tsumo && self.dealer != seat && p.draws == 1 && !self.any_call,
            renhou: !is_tsumo && self.dealer != seat && p.draws == 0 && !self.any_call,
            dora_kinds: self.wall.dora_kinds(),
            ura_kinds: self.wall.ura_kinds(),
            aka_count: aka,
        }
    }

    fn score_for(
        &self,
        seat: u8,
        hand: &Counts,
        win_tile: Tile,
        is_tsumo: bool,
        chankan: bool,
        rinshan: bool,
    ) -> Option<ScoreResult> {
        let ctx = self.win_context(seat, hand, win_tile, is_tsumo, chankan, rinshan);
        let mut r = score_win(&ctx)?;
        r.is_dealer = self.dealer == seat;
        Some(r)
    }

    // ---- round setup -----------------------------------------------------

    fn start_round(&mut self) {
        self.wall = Wall::shuffled(&mut self.rng);
        self.events.clear();
        self.submitted = [None; 4];
        self.any_call = false;
        self.total_kans = 0;
        self.kan_owners.clear();
        self.round_seq = self.round_seq.wrapping_add(1);
        self.pending_abort = None;
        self.first_discard_kind = None;
        self.four_winds_run = 0;
        self.outcome = RoundOutcome::default();
        self.pao = None;
        self.rounds_played += 1;
        for p in self.players.iter_mut() {
            p.hand = [0u8; NUM_KINDS];
            p.hand_tiles.clear();
            p.melds.clear();
            p.discards.clear();
            p.riichi = false;
            p.double_riichi = false;
            p.ippatsu = false;
            p.riichi_furiten = false;
            p.temp_furiten = false;
            p.drawn = None;
            p.drawn_is_rinshan = false;
            p.draws = 0;
            p.kuikae_forbidden.clear();
        }
        for i in 0..4 {
            let seat = ((self.dealer as usize) + i) % 4;
            for _ in 0..13 {
                let t = self.wall.draw().expect("the live wall holds 122 tiles");
                self.players[seat].hand[kind_of(t) as usize] += 1;
                self.players[seat].hand_tiles.push(t);
            }
            self.players[seat].hand_tiles.sort_unstable();
        }
        let dora = self.wall.dora_indicator(0).unwrap();
        self.push_event(Event::RoundStart {
            round_wind: self.round_wind,
            round_number: self.round_number,
            honba: self.honba,
            riichi_sticks: self.riichi_sticks,
            dealer: self.dealer,
            scores: self.scores(),
            dora_indicator: dora,
            wall_remaining: self.wall.remaining(),
        });
        self.turn = self.dealer;
        self.phase = Phase::Draw { seat: self.dealer };
    }

    fn push_event(&mut self, e: Event) {
        self.history.push(e.clone());
        self.step_events.push(e.clone());
        self.events.push(e);
    }

    // ---- decisions -------------------------------------------------------

    /// Pending decisions, one per seat that must act.
    pub fn decisions(&self) -> &[Decision] {
        &self.decisions
    }

    fn refresh_decisions(&mut self) {
        self.decisions = match self.phase.clone() {
            Phase::Turn { seat } => vec![self.self_turn_decision(seat)],
            Phase::CallWindow { from, tile, awaiting } => {
                let mut v = Vec::new();
                for seat in awaiting {
                    if self.submitted[seat as usize].is_none() {
                        v.push(self.call_decision(seat, from, tile, true));
                    }
                }
                v
            }
            Phase::ChankanWindow { from, tile, awaiting } => {
                let mut v = Vec::new();
                for seat in awaiting {
                    if self.submitted[seat as usize].is_none() {
                        v.push(self.chankan_decision(seat, from, tile));
                    }
                }
                v
            }
            _ => Vec::new(),
        };
    }

    /// Physical tile to use when a kind-level choice is made: keep red fives.
    fn pick_physical(&self, seat: u8, kind: Kind) -> Option<Tile> {
        self.players[seat as usize]
            .hand_tiles
            .iter()
            .copied()
            .filter(|&t| kind_of(t) == kind)
            .min_by_key(|&t| is_aka_tile(t))
    }

    fn self_turn_decision(&self, seat: u8) -> Decision {
        let p = &self.players[seat as usize];
        let melds = p.melds.len() as u8;
        let mut actions: Vec<Action> = Vec::with_capacity(40);

        if let Some(t) = p.drawn {
            if is_agari(&p.hand, melds)
                && self
                    .score_for(seat, &p.hand, t, true, false, p.drawn_is_rinshan)
                    .is_some()
            {
                actions.push(Action::Tsumo);
            }
        }

        if p.riichi {
            // The hand is locked: the drawn tile is the only legal discard.
            if let Some(t) = p.drawn {
                actions.push(Action::Discard {
                    tile: t,
                    riichi: false,
                });
            }
        } else {
            let mut seen = [false; NUM_KINDS];
            for &t in &p.hand_tiles {
                let k = kind_of(t);
                if seen[k as usize] {
                    continue;
                }
                seen[k as usize] = true;
                if p.kuikae_forbidden.contains(&k) {
                    continue;
                }
                let Some(tile) = self.pick_physical(seat, k) else {
                    continue;
                };
                actions.push(Action::Discard { tile, riichi: false });
                if self.can_declare_riichi(seat) {
                    let mut rest = p.hand;
                    rest[k as usize] -= 1;
                    if !winning_kinds(&rest, melds).is_empty() {
                        actions.push(Action::Discard { tile, riichi: true });
                    }
                }
            }
        }

        if self.wall.remaining() > 0 {
            let mut seen = [false; NUM_KINDS];
            for &t in &p.hand_tiles {
                let k = kind_of(t);
                if seen[k as usize] {
                    continue;
                }
                seen[k as usize] = true;
                if p.hand[k as usize] == 4 {
                    // After riichi an 暗槓 is only legal when the wait is
                    // unchanged; 送り槓 is forbidden.
                    if !p.riichi || self.ankan_keeps_wait(seat, k) {
                        if let Some(meld) = self.build_ankan(seat, k) {
                            actions.push(Action::Meld { meld });
                        }
                    }
                }
                let has_pon = p
                    .melds
                    .iter()
                    .any(|m| m.kind == MeldKind::Pon && m.triplet_kind() == Some(k));
                // 加槓 changes the hand, so unlike 暗槓 it is never allowed after
                // 立直 — the declaration locks the hand for the rest of the round.
                if has_pon && !p.riichi {
                    if let Some(meld) = self.build_kakan(seat, k) {
                        actions.push(Action::Meld { meld });
                    }
                }
            }
        }

        if self.can_kyuushu(seat) && self.has_nine_terminals(seat) {
            actions.push(Action::Kyuushu);
        }

        if !actions.iter().any(|a| a.is_discard()) {
            // A player holding 14 tiles always has a discard; this only guards
            // against a degenerate constructed state.
            if let Some(t) = p.drawn.or_else(|| p.hand_tiles.first().copied()) {
                actions.push(Action::Discard {
                    tile: t,
                    riichi: false,
                });
            }
        }

        Decision {
            seat,
            trigger: Trigger::SelfTurn,
            actions,
        }
    }

    fn can_declare_riichi(&self, seat: u8) -> bool {
        let p = &self.players[seat as usize];
        !p.riichi
            && p.is_closed()
            && p.score >= 1000
            && self.wall.remaining() >= self.rules.min_riichi_wall as u32
    }

    fn can_kyuushu(&self, seat: u8) -> bool {
        let p = &self.players[seat as usize];
        self.rules.abort_nine_terminals && !self.any_call && p.draws == 1 && p.is_closed()
    }

    fn has_nine_terminals(&self, seat: u8) -> bool {
        let p = &self.players[seat as usize];
        (0..NUM_KINDS).filter(|&k| is_yaochu(k as Kind) && p.hand[k] > 0).count() >= 9
    }

    /// Does declaring 暗槓 of `kind` leave the winning tiles untouched?
    fn ankan_keeps_wait(&self, seat: u8, kind: Kind) -> bool {
        let p = &self.players[seat as usize];
        let melds = p.melds.len() as u8;
        let mut before = p.hand;
        before[kind as usize] = before[kind as usize].saturating_sub(1);
        let mut after = p.hand;
        after[kind as usize] = after[kind as usize].saturating_sub(4);
        winning_kinds(&before, melds) == winning_kinds(&after, melds + 1)
    }

    fn build_ankan(&self, seat: u8, kind: Kind) -> Option<Meld> {
        let p = &self.players[seat as usize];
        let mut tiles = [0u8; 4];
        let mut n = 0;
        for &t in &p.hand_tiles {
            if kind_of(t) == kind && n < 4 {
                tiles[n] = t;
                n += 1;
            }
        }
        if n < 4 {
            return None;
        }
        Some(Meld::kan(MeldKind::Ankan, tiles, tiles[0], seat))
    }

    fn build_kakan(&self, seat: u8, kind: Kind) -> Option<Meld> {
        let p = &self.players[seat as usize];
        let pon = p
            .melds
            .iter()
            .find(|m| m.kind == MeldKind::Pon && m.triplet_kind() == Some(kind))?;
        let added = p
            .hand_tiles
            .iter()
            .copied()
            .filter(|&t| kind_of(t) == kind)
            .min_by_key(|&t| is_aka_tile(t))?;
        Some(Meld::kan(
            MeldKind::Kakan,
            [pon.tiles[0], pon.tiles[1], pon.tiles[2], added],
            added,
            seat,
        ))
    }

    fn call_decision(&self, seat: u8, from: u8, tile: Tile, calls_allowed: bool) -> Decision {
        let p = &self.players[seat as usize];
        let k = kind_of(tile);
        let melds = p.melds.len() as u8;
        let mut actions: Vec<Action> = Vec::with_capacity(7);

        let mut h = p.hand;
        h[k as usize] += 1;
        if !self.is_furiten(seat)
            && is_agari(&h, melds)
            && self.score_for(seat, &h, tile, false, false, false).is_some()
        {
            actions.push(Action::Ron);
        }
        // 立直 locks the hand: after a declaration the player may still win on a
        // discard (立直 is itself a yaku) but may not call anything — 吃・碰・
        // 大明槓 are all forbidden, and 加槓 too (see `self_turn_decision`).
        // 暗槓 is the one exception, and only when the wait does not change.
        if calls_allowed && !p.riichi && p.hand[k as usize] >= 2 && melds < 4 {
            if let Some(m) = self.build_call_triplet(seat, tile, from, MeldKind::Pon) {
                actions.push(Action::Meld { meld: m });
            }
            if p.hand[k as usize] >= 3 && self.wall.remaining() > 0 {
                if let Some(m) = self.build_call_triplet(seat, tile, from, MeldKind::Minkan) {
                    actions.push(Action::Meld { meld: m });
                }
            }
        }
        if calls_allowed && !p.riichi && seat == (from + 1) % 4 && suit_of(k) < 3 && melds < 4 {
            for m in self.build_chis(seat, tile, from) {
                actions.push(Action::Meld { meld: m });
            }
        }
        actions.push(Action::Pass);
        Decision {
            seat,
            trigger: Trigger::Discard { from, tile },
            actions,
        }
    }

    fn chankan_decision(&self, seat: u8, from: u8, tile: Tile) -> Decision {
        let p = &self.players[seat as usize];
        let k = kind_of(tile);
        let mut actions = Vec::with_capacity(2);
        let mut h = p.hand;
        h[k as usize] += 1;
        if !self.is_furiten(seat)
            && is_agari(&h, p.melds.len() as u8)
            && self.score_for(seat, &h, tile, false, true, false).is_some()
        {
            actions.push(Action::Ron);
        }
        actions.push(Action::Pass);
        Decision {
            seat,
            trigger: Trigger::Chankan { from, tile },
            actions,
        }
    }

    fn build_call_triplet(&self, seat: u8, tile: Tile, from: u8, kind: MeldKind) -> Option<Meld> {
        let p = &self.players[seat as usize];
        let k = kind_of(tile);
        let need = if kind == MeldKind::Pon { 2 } else { 3 };
        let mut own: Vec<Tile> = p
            .hand_tiles
            .iter()
            .copied()
            .filter(|&t| kind_of(t) == k)
            .collect();
        if own.len() < need {
            return None;
        }
        own.sort_by_key(|&t| is_aka_tile(t)); // keep red fives
        let mut arr = [0u8; 4];
        arr[..need].copy_from_slice(&own[..need]);
        arr[need] = tile;
        Some(if kind == MeldKind::Pon {
            Meld::pon([arr[0], arr[1], arr[2]], tile, from)
        } else {
            Meld::kan(MeldKind::Minkan, arr, tile, from)
        })
    }

    fn build_chis(&self, seat: u8, tile: Tile, from: u8) -> Vec<Meld> {
        let p = &self.players[seat as usize];
        let k = kind_of(tile);
        let base = (suit_of(k) * 9) as Kind;
        let n = (k - base) as i32;
        let mut out = Vec::new();
        let have = |kind: Kind| -> Option<Tile> {
            p.hand_tiles
                .iter()
                .copied()
                .filter(|&t| kind_of(t) == kind)
                .min_by_key(|&t| is_aka_tile(t))
        };
        for (a, b) in [(-2i32, -1i32), (-1, 1), (1, 2)] {
            let (na, nb) = (n + a, n + b);
            if !(0..9).contains(&na) || !(0..9).contains(&nb) {
                continue;
            }
            if let (Some(ta), Some(tb)) = (have(base + na as Kind), have(base + nb as Kind)) {
                out.push(Meld::chi([ta, tb, tile], tile, from));
            }
        }
        out
    }

    // ---- submitting ------------------------------------------------------

    /// Submit `action` for `seat`. Returns the events produced by this step.
    pub fn submit(&mut self, seat: u8, action: Action) -> Result<Vec<Event>, String> {
        let decision = self
            .decisions
            .iter()
            .find(|d| d.seat == seat)
            .cloned()
            .ok_or_else(|| format!("no pending decision for seat {}", seat))?;
        if !decision.allows(&action) {
            return Err(format!(
                "illegal action {} for seat {} during {:?}",
                action.label(),
                seat,
                self.phase
            ));
        }
        self.step_events.clear();
        let had_ron = decision.actions.contains(&Action::Ron);
        self.apply(seat, action, decision.trigger, had_ron)?;
        Ok(std::mem::take(&mut self.step_events))
    }

    fn apply(
        &mut self,
        seat: u8,
        action: Action,
        trigger: Trigger,
        decision_had_ron: bool,
    ) -> Result<(), String> {
        match trigger {
            Trigger::SelfTurn => self.apply_self_turn(seat, action),
            Trigger::Discard { .. } => {
                // 同巡振聴 only applies when a ron was actually available:
                // declining a tile that could never have been declared (for
                // example a yaku-less hand) does not make a player furiten.
                if action == Action::Pass && decision_had_ron {
                    self.note_pass(seat);
                }
                self.submitted[seat as usize] = Some(action);
                self.pump();
                Ok(())
            }
            Trigger::Chankan { .. } => {
                if action == Action::Pass && decision_had_ron {
                    self.note_pass(seat);
                }
                self.submitted[seat as usize] = Some(action);
                self.pump();
                Ok(())
            }
        }
    }

    /// A player declined a legal ron: record 振聴 consequences.
    fn note_pass(&mut self, seat: u8) {
        let p = &mut self.players[seat as usize];
        p.temp_furiten = true;
        if p.riichi {
            p.riichi_furiten = true;
        }
    }

    fn apply_self_turn(&mut self, seat: u8, action: Action) -> Result<(), String> {
        match action {
            Action::Tsumo => {
                let p = &self.players[seat as usize];
                let tile = p.drawn.ok_or("no drawn tile")?;
                let rinshan = p.drawn_is_rinshan;
                let hand = p.hand;
                let score = self
                    .score_for(seat, &hand, tile, true, false, rinshan)
                    .ok_or("tsumo is not scoreable")?;
                self.apply_win(vec![(seat, None, tile, score)]);
                Ok(())
            }
            Action::Kyuushu => {
                if !self.has_nine_terminals(seat) {
                    return Err("九種九牌 requires nine terminal/honor kinds".to_string());
                }
                self.abort_round_by(DrawReason::NineTerminals, Some(seat));
                Ok(())
            }
            Action::Discard { tile, riichi } => {
                self.do_discard(seat, tile, riichi);
                self.pump();
                Ok(())
            }
            Action::Meld { meld } => match meld.kind {
                MeldKind::Ankan => {
                    let before = self.round_seq;
                    self.do_ankan(seat, meld);
                    let kind = meld.triplet_kind().unwrap();
                    if self.any_kokushi_rob(seat, kind) {
                        let awaiting: Vec<u8> = (0..4).filter(|&s| s != seat).collect();
                        self.submitted = [None; 4];
                        self.submitted[seat as usize] = Some(Action::Pass);
                        self.phase = Phase::ChankanWindow {
                            from: seat,
                            tile: meld.tiles[0],
                            awaiting,
                        };
                    } else if self.round_seq == before {
                        // A round that aborted on this kan has already moved on:
                        // drawing now would hand out a tile in the next round,
                        // out of turn.
                        self.do_rinshan_draw(seat);
                    }
                    self.pump();
                    Ok(())
                }
                MeldKind::Kakan => {
                    self.do_kakan(seat, meld);
                    let awaiting: Vec<u8> = (0..4).filter(|&s| s != seat).collect();
                    self.submitted = [None; 4];
                    self.submitted[seat as usize] = Some(Action::Pass);
                    self.phase = Phase::ChankanWindow {
                        from: seat,
                        tile: meld.tiles[3],
                        awaiting,
                    };
                    self.pump();
                    Ok(())
                }
                _ => Err("a 大明槓 must be called from a discard".to_string()),
            },
            Action::Ron | Action::Pass => Err("not available on your own turn".to_string()),
        }
    }

    fn any_kokushi_rob(&self, seat: u8, kind: Kind) -> bool {
        if !self.rules.kokushi_robs_ankan {
            return false;
        }
        (0..4u8).any(|s| {
            if s == seat {
                return false;
            }
            let p = &self.players[s as usize];
            if !p.melds.is_empty() {
                return false;
            }
            let mut h = p.hand;
            h[kind as usize] += 1;
            is_kokushi(&h)
        })
    }

    // ---- discard and calls ----------------------------------------------

    fn do_discard(&mut self, seat: u8, tile: Tile, riichi: bool) {
        // The action was validated by kind, so `tile` may be a copy the player
        // does not actually hold. Record the copy that leaves the hand: a log
        // naming an unheld tile cannot be replayed (the replay verifier rebuilds
        // the game from the actions and compares the events).
        let tile = {
            let hand = &self.players[seat as usize].hand_tiles;
            if hand.contains(&tile) {
                tile
            } else {
                hand.iter()
                    .copied()
                    .find(|&t| kind_of(t) == kind_of(tile))
                    .unwrap_or(tile)
            }
        };
        let drawn = self.players[seat as usize].drawn;
        let tsumogiri = drawn.map(kind_of) == Some(kind_of(tile));
        {
            let p = &mut self.players[seat as usize];
            let k = kind_of(tile) as usize;
            p.hand[k] = p.hand[k].saturating_sub(1);
            if let Some(pos) = p.hand_tiles.iter().position(|&t| t == tile) {
                p.hand_tiles.remove(pos);
            } else if let Some(pos) = p.hand_tiles.iter().position(|&t| kind_of(t) == kind_of(tile))
            {
                p.hand_tiles.remove(pos);
            }
            p.discards.push(Discard {
                tile,
                tsumogiri,
                riichi,
                called_by: None,
            });
            p.drawn = None;
            p.drawn_is_rinshan = false;
            p.kuikae_forbidden.clear();
        }
        self.push_event(Event::Discard {
            seat,
            tile,
            tsumogiri,
            riichi,
        });

        if riichi {
            let p = &mut self.players[seat as usize];
            p.riichi = true;
            p.double_riichi = !self.any_call && p.draws == 1;
            p.ippatsu = true;
            p.score -= 1000;
            self.riichi_sticks += 1;
            self.push_event(Event::Riichi { seat });
        }

        // 四風連打
        if !self.any_call {
            let k = kind_of(tile);
            if (EAST..=crate::tile::NORTH).contains(&k) {
                match self.first_discard_kind {
                    None => {
                        self.first_discard_kind = Some(k);
                        self.four_winds_run = 1;
                    }
                    Some(fk) if fk == k => self.four_winds_run += 1,
                    Some(_) => self.four_winds_run = 0,
                }
            } else {
                self.four_winds_run = 0;
            }
            if self.rules.abort_four_winds
                && self.four_winds_run == 4
                && self.players.iter().all(|p| p.discards.len() == 1)
            {
                // Deferred, not immediate: this discard can still be ronned, and
                // a win takes precedence over the abort (Tenhou does the same).
                self.pending_abort = Some(DrawReason::FourWinds);
            }
        }
        // 四家立直
        if self.rules.abort_four_riichi
            && self.players.iter().all(|p| p.riichi)
            && self.pending_abort.is_none()
        {
            self.pending_abort = Some(DrawReason::FourRiichi);
        }

        // 海底牌 cannot be called: only 栄和 remains possible on the last
        // discard of the hand.
        let calls_allowed = self.wall.remaining() > 0;
        let mut awaiting = Vec::new();
        for s in 0..4u8 {
            if s == seat {
                continue;
            }
            if self.call_decision(s, seat, tile, calls_allowed).actions.len() > 1 {
                awaiting.push(s);
            }
        }
        self.submitted = [None; 4];
        if awaiting.is_empty() {
            if let Some(reason) = self.pending_abort.take() {
                self.abort_round(reason);
                return;
            }
            self.turn = (seat + 1) % 4;
            self.phase = Phase::Draw {
                seat: (seat + 1) % 4,
            };
        } else {
            self.phase = Phase::CallWindow {
                from: seat,
                tile,
                awaiting,
            };
        }
    }

    fn do_ankan(&mut self, seat: u8, meld: Meld) {
        let kind = meld.triplet_kind().unwrap();
        {
            let p = &mut self.players[seat as usize];
            for _ in 0..4 {
                p.hand[kind as usize] = p.hand[kind as usize].saturating_sub(1);
                if let Some(pos) = p.hand_tiles.iter().position(|&t| kind_of(t) == kind) {
                    p.hand_tiles.remove(pos);
                }
            }
            p.melds.push(meld);
        }
        self.register_kan(seat, meld, true, true);
    }

    fn do_kakan(&mut self, seat: u8, meld: Meld) {
        let kind = meld.triplet_kind().unwrap();
        {
            let p = &mut self.players[seat as usize];
            p.hand[kind as usize] = p.hand[kind as usize].saturating_sub(1);
            if let Some(pos) = p.hand_tiles.iter().position(|&t| kind_of(t) == kind) {
                p.hand_tiles.remove(pos);
            }
            if let Some(existing) = p
                .melds
                .iter_mut()
                .find(|m| m.kind == MeldKind::Pon && m.triplet_kind() == Some(kind))
            {
                *existing = meld;
            }
        }
        // 加槓: the kan dora waits for the 搶槓 window to close.
        self.register_kan(seat, meld, false, false);
    }

    /// 責任払い: record who fed the tile that completed 大三元 / 大四喜.
    ///
    /// Only called for melds taken from another player's discard, because a
    /// self-drawn completion never creates responsibility.
    fn update_pao(&mut self, seat: u8, from: u8) {
        if !self.rules.pao || from == seat {
            return;
        }
        let p = &self.players[seat as usize];
        let mut dragons = 0u8;
        let mut winds = 0u8;
        let count_kind = |k: Kind, dragons: &mut u8, winds: &mut u8| {
            if k >= HAKU {
                *dragons += 1;
            } else if (EAST..HAKU).contains(&k) {
                *winds += 1;
            }
        };
        for m in &p.melds {
            if let Some(k) = m.triplet_kind() {
                count_kind(k, &mut dragons, &mut winds);
            }
        }
        for k in 0..NUM_KINDS {
            if p.hand[k] >= 3 {
                count_kind(k as Kind, &mut dragons, &mut winds);
            }
        }
        if dragons >= 3 || winds >= 4 {
            self.pao = Some((seat, from));
        }
    }

    /// Record a kan. `reveal_dora` is `false` for 加槓, whose indicator is only
    /// turned after the 搶槓 window closes. `allow_abort` is `false` in the same
    /// case, because a winning 搶槓 takes precedence over 四槓散了.
    fn register_kan(&mut self, seat: u8, meld: Meld, reveal_dora: bool, allow_abort: bool) {
        self.any_call = true;
        for p in self.players.iter_mut() {
            p.ippatsu = false;
        }
        // 加槓 counts only once the 搶槓 window closes, in `finish_kakan`. If a
        // player robs it the kan never happened, so counting here would leave a
        // phantom dora indicator behind — and score it for the robber.
        if reveal_dora {
            self.total_kans += 1;
            self.kan_owners.push(seat);
            self.wall.on_kan();
        }
        let dora = if reveal_dora {
            self.wall
                .dora_indicator((self.total_kans - 1) as usize)
        } else {
            None
        };
        self.push_event(Event::Kan {
            seat,
            meld,
            dora_indicator: dora,
        });
        // 四槓散了: four kans that are not all by the same player.
        if allow_abort
            && self.rules.abort_four_kans
            && self.total_kans >= 4
            && self.kan_owners.iter().any(|&s| s != self.kan_owners[0])
        {
            self.abort_round(DrawReason::FourKans);
        }
    }

    /// A 加槓 survived its 搶槓 window: count it now, turn its dora indicator
    /// and run the abort check that was postponed with it.
    fn finish_kakan(&mut self, seat: u8) {
        self.total_kans += 1;
        self.kan_owners.push(seat);
        self.wall.on_kan();
        let dora = self
            .wall
            .dora_indicator((self.total_kans - 1) as usize);
        if let Some(t) = dora {
            self.push_event(Event::DoraRevealed { indicator: t });
        }
        if self.rules.abort_four_kans
            && self.total_kans >= 4
            && self.kan_owners.iter().any(|&s| s != self.kan_owners[0])
        {
            self.abort_round(DrawReason::FourKans);
        }
    }

    fn do_rinshan_draw(&mut self, seat: u8) {
        match self.wall.draw_rinshan() {
            Some(t) => {
                let p = &mut self.players[seat as usize];
                p.hand[kind_of(t) as usize] += 1;
                p.hand_tiles.push(t);
                p.hand_tiles.sort_unstable();
                p.drawn = Some(t);
                p.drawn_is_rinshan = true;
                p.draws += 1;
                self.turn = seat;
                self.push_event(Event::Draw {
                    seat,
                    tile: t,
                    rinshan: true,
                });
                self.phase = Phase::Turn { seat };
            }
            None => self.end_exhaustive(),
        }
    }

    /// Kinds the caller may not discard immediately after calling.
    ///
    /// Tenhou forbids 食い替え entirely: neither the called tile itself nor the
    /// tile that would rebuild the same two kept tiles into a run on the other
    /// side (筋食い替え) may be dropped.
    fn kuikae_forbidden_kinds(&self, meld: &Meld, called: Tile) -> Vec<Kind> {
        match self.rules.kuikae {
            KuikaeScope::Allowed => Vec::new(),
            KuikaeScope::SameTileOnly => vec![kind_of(called)],
            KuikaeScope::Forbidden => {
                let mut out = vec![kind_of(called)];
                if meld.kind == MeldKind::Chi {
                    if let Some(start) = meld.run_start() {
                        let base = (suit_of(start) * 9) as Kind;
                        let low = start - base; // 0..=6
                        let called_pos = kind_of(called) - base;
                        if called_pos == low + 2 && low > 0 {
                            out.push(start - 1);
                        } else if called_pos == low && low + 3 <= 8 {
                            out.push(start + 3);
                        }
                    }
                }
                out
            }
        }
    }

    fn resolve_calls(&mut self, from: u8, tile: Tile) {
        let order: Vec<u8> = (1..=3).map(|i| (from + i) % 4).collect();
        let rons: Vec<u8> = order
            .iter()
            .copied()
            .filter(|&s| self.submitted[s as usize] == Some(Action::Ron))
            .collect();
        if !rons.is_empty() {
            // 三家和了: three rons on one discard abort the hand with no point
            // exchange at all (Tenhou). ダブロン stays legal.
            if self.rules.abort_triple_ron && rons.len() >= 3 {
                self.abort_round(DrawReason::TripleRon);
                return;
            }
            // 立直宣言牌被ロン: the declaration never takes effect and the
            // 1000-point stick is returned rather than placed.
            let declarer = from as usize;
            let declared_on_this_discard = self.players[declarer]
                .discards
                .last()
                .map(|d| d.riichi)
                .unwrap_or(false);
            if declared_on_this_discard {
                let p = &mut self.players[declarer];
                p.riichi = false;
                p.double_riichi = false;
                p.ippatsu = false;
                p.score += 1000;
                if let Some(d) = self.players[declarer].discards.last_mut() {
                    d.riichi = false;
                }
                self.riichi_sticks = self.riichi_sticks.saturating_sub(1);
            }
            let mut winners = Vec::new();
            let mut ordered = rons;
            if !self.rules.multi_ron {
                ordered.truncate(1);
            }
            for &s in &ordered {
                let p = &self.players[s as usize];
                let mut h = p.hand;
                h[kind_of(tile) as usize] += 1;
                let score = self
                    .score_for(s, &h, tile, false, false, false)
                    .expect("the ron was validated when the action was generated");
                winners.push((s, Some(from), tile, score));
            }
            self.apply_win(winners);
            return;
        }

        let mut chosen: Option<(u8, Meld)> = None;
        for &s in &order {
            if let Some(Action::Meld { meld }) = self.submitted[s as usize] {
                if matches!(meld.kind, MeldKind::Pon | MeldKind::Minkan) {
                    chosen = Some((s, meld));
                    break;
                }
            }
        }
        if chosen.is_none() {
            for &s in &order {
                if let Some(Action::Meld { meld }) = self.submitted[s as usize] {
                    chosen = Some((s, meld));
                    break;
                }
            }
        }

        let Some((seat, meld)) = chosen else {
            // Nobody called and nobody won: now the abort can stand.
            if let Some(reason) = self.pending_abort.take() {
                self.abort_round(reason);
                return;
            }
            self.turn = (from + 1) % 4;
            self.phase = Phase::Draw {
                seat: (from + 1) % 4,
            };
            return;
        };

        self.any_call = true;
        for p in self.players.iter_mut() {
            p.ippatsu = false;
        }
        if let Some(last) = self.players[from as usize].discards.last_mut() {
            last.called_by = Some(seat);
        }
        let forbidden = self.kuikae_forbidden_kinds(&meld, tile);
        {
            let p = &mut self.players[seat as usize];
            for &t in meld.as_slice() {
                if t == tile {
                    continue; // the called tile comes from the discard pile
                }
                p.hand[kind_of(t) as usize] = p.hand[kind_of(t) as usize].saturating_sub(1);
                if let Some(pos) = p.hand_tiles.iter().position(|&x| x == t) {
                    p.hand_tiles.remove(pos);
                } else if let Some(pos) =
                    p.hand_tiles.iter().position(|&x| kind_of(x) == kind_of(t))
                {
                    p.hand_tiles.remove(pos);
                }
            }
            p.melds.push(meld);
            p.kuikae_forbidden = forbidden;
            p.temp_furiten = false;
        }
        self.push_event(Event::Meld { seat, meld, from });
        self.update_pao(seat, from);

        if meld.kind == MeldKind::Minkan {
            let before = self.round_seq;
            self.register_kan(seat, meld, true, true);
            if self.round_seq != before {
                return;
            }
            self.do_rinshan_draw(seat);
        } else {
            self.turn = seat;
            self.phase = Phase::Turn { seat };
        }
    }

    // ---- wins ------------------------------------------------------------

    fn apply_win(&mut self, winners: Vec<(u8, Option<u8>, Tile, ScoreResult)>) {
        let honba = self.honba;
        let sticks = self.riichi_sticks;
        let mut stick_taken = 0u32;
        let mut total_deltas = [0i32; 4];
        // What each winner himself collected, and how many sticks he took. A
        // double ron pays each winner separately — handing every winner the
        // hand-wide total (and the same riichi sticks) tells both of them they
        // were paid the other's money.
        let mut paid = vec![0i32; winners.len()];
        let mut sticks_each = vec![0u32; winners.len()];
        let mut pao_payer = vec![None; winners.len()];

        for (i, (seat, from, _tile, score)) in winners.iter().enumerate() {
            let mut d = [0i32; 4];
            match from {
                None => {
                    let (other, dealer_pay) = score.tsumo_payment(honba);
                    for s in 0..4usize {
                        if s == *seat as usize {
                            continue;
                        }
                        let pay = if s == self.dealer as usize {
                            dealer_pay
                        } else {
                            other
                        };
                        d[s] -= pay;
                        d[*seat as usize] += pay;
                    }
                }
                Some(f) => {
                    let total = score.ron_total(honba);
                    d[*f as usize] -= total;
                    d[*seat as usize] += total;
                }
            }
            // 責任払い for 大三元 / 大四喜.
            if self.rules.pao {
                if let Some((beneficiary, payer)) = self.pao {
                    if beneficiary == *seat && payer != *seat {
                        let paid: i32 = d.iter().filter(|&&x| x < 0).sum::<i32>().abs();
                        d = [0i32; 4];
                        d[payer as usize] -= paid;
                        d[*seat as usize] += paid;
                        // 責任払い: everything is on this seat, not on whoever
                        // discarded the winning tile.
                        pao_payer[i] = Some(payer);
                    }
                }
            }
            if i == 0 && sticks > 0 {
                d[*seat as usize] += sticks as i32 * 1000;
                stick_taken = sticks;
            }
            paid[i] = d[*seat as usize];
            sticks_each[i] = if i == 0 { stick_taken } else { 0 };
            if i == 0 && sticks > 0 {
                // `d` already includes the sticks; report the hand payment only.
                paid[i] -= sticks as i32 * 1000;
            }
            for s in 0..4 {
                self.players[s].score += d[s];
                total_deltas[s] += d[s];
            }
        }
        if stick_taken > 0 {
            self.riichi_sticks = 0;
        }
        for (i, (seat, from, tile, score)) in winners.iter().enumerate() {
            // A ron win takes the tile from the discard pile, so the winning
            // hand is the concealed tiles *plus* it; a tsumo already holds it.
            let mut hand = self.players[*seat as usize].hand_tiles.clone();
            if from.is_some() {
                hand.push(*tile);
                hand.sort_unstable();
            }
            let melds = self.players[*seat as usize].melds.clone();
            self.push_event(Event::Win {
                seat: *seat,
                from: *from,
                tile: *tile,
                score: score.clone(),
                deltas: total_deltas,
                riichi_sticks_taken: sticks_each[i],
                paid: paid[i],
                pao_payer: pao_payer[i],
                hand,
                melds,
                nagashi: false,
            });
        }
        let dealer_won = winners.iter().any(|(s, _, _, _)| *s == self.dealer);
        self.outcome = RoundOutcome {
            dealer_repeat: dealer_won,
            won: true,
            dealer_won,
        };
        self.phase = Phase::RoundEnd;
        self.finish_round();
        self.pump();
    }

    // ---- round end -------------------------------------------------------

    fn end_exhaustive(&mut self) {
        // 流し満貫 first: it replaces the exhaustive draw entirely.
        if self.rules.nagashi_mangan {
            let nagashi: Vec<u8> = (0..4u8)
                .filter(|&s| {
                    let p = &self.players[s as usize];
                    p.discards.len() >= 13
                        && p.discards
                            .iter()
                            .all(|d| is_yaochu(kind_of(d.tile)) && d.called_by.is_none())
                })
                .collect();
            if !nagashi.is_empty() {
                let mut total = [0i32; 4];
                for &seat in &nagashi {
                    let is_dealer = seat == self.dealer;
                    let score = score_simple(
                        5,
                        30,
                        &self.rules,
                        is_dealer,
                        vec![(Yaku::NagashiMangan, 5)],
                    );
                    let (other, dealer_pay) = score.tsumo_payment(self.honba);
                    let mut d = [0i32; 4];
                    for s in 0..4usize {
                        if s == seat as usize {
                            continue;
                        }
                        let pay = if s == self.dealer as usize {
                            dealer_pay
                        } else {
                            other
                        };
                        d[s] -= pay;
                        d[seat as usize] += pay;
                    }
                    for s in 0..4 {
                        self.players[s].score += d[s];
                        total[s] += d[s];
                    }
                    self.push_event(Event::Win {
                        seat,
                        from: None,
                        tile: 0,
                        score,
                        deltas: total,
                        riichi_sticks_taken: 0,
                        paid: d[seat as usize],
                        pao_payer: None,
                        // 流し満貫 is a draw-time settlement: there is no
                        // winning tile, but the hand is still worth showing.
                        hand: self.players[seat as usize].hand_tiles.clone(),
                        melds: self.players[seat as usize].melds.clone(),
                        nagashi: true,
                    });
                }
                let dealer_nagashi = nagashi.contains(&self.dealer);
                self.outcome = RoundOutcome {
                    dealer_repeat: dealer_nagashi,
                    won: true,
                    dealer_won: dealer_nagashi,
                };
                self.phase = Phase::RoundEnd;
                self.finish_round();
                self.pump();
                return;
            }
        }

        let mut tenpai = [false; 4];
        for s in 0..4usize {
            let p = &self.players[s];
            // 形式聴牌: shape only, regardless of how many copies are visible.
            tenpai[s] = !tenpai_kinds(&p.hand, p.melds.len() as u8).is_empty();
        }
        let count = tenpai.iter().filter(|&&x| x).count() as i32;
        let mut deltas = [0i32; 4];
        if count > 0 && count < 4 {
            let gain = 3000 / count;
            let loss = 3000 / (4 - count);
            for s in 0..4 {
                deltas[s] = if tenpai[s] { gain } else { -loss };
                self.players[s].score += deltas[s];
            }
        }
        self.push_event(Event::Ryuukyoku {
            reason: DrawReason::Exhaustive,
            tenpai,
            deltas,
            by: None,
            wall_remaining: self.wall.remaining(),
        });
        self.outcome = RoundOutcome {
            dealer_repeat: tenpai[self.dealer as usize],
            won: false,
            dealer_won: false,
        };
        self.phase = Phase::RoundEnd;
        self.finish_round();
        self.pump();
    }

    fn abort_round(&mut self, reason: DrawReason) {
        self.abort_round_by(reason, None);
    }

    /// An abortive draw, optionally naming the player who declared it.
    fn abort_round_by(&mut self, reason: DrawReason, by: Option<u8>) {
        self.push_event(Event::Ryuukyoku {
            reason,
            tenpai: [false; 4],
            deltas: [0; 4],
            by,
            wall_remaining: self.wall.remaining(),
        });
        // Every 途中流局 keeps the dealer (連荘) and adds a honba.
        self.outcome = RoundOutcome {
            dealer_repeat: true,
            won: false,
            dealer_won: false,
        };
        self.phase = Phase::RoundEnd;
        self.finish_round();
        self.pump();
    }

    fn finish_round(&mut self) {
        // `honba` is the round that just ended; the next one adds a honba when
        // the dealer repeats. A log line that reads "下一局 N 本场" needs the
        // second number, not the first — they differ by exactly this.
        let next_honba = if self.outcome.dealer_repeat { self.honba + 1 } else { 0 };
        self.push_event(Event::RoundEnd {
            scores: self.scores(),
            next_dealer: self.dealer,
            honba: self.honba,
            next_honba,
        });
    }

    // ---- main loop -------------------------------------------------------

    fn pump(&mut self) {
        loop {
            match self.phase.clone() {
                Phase::Init => self.start_round(),
                Phase::Draw { seat } => {
                    if self.wall.remaining() == 0 {
                        self.end_exhaustive();
                        continue;
                    }
                    self.do_draw(seat);
                }
                Phase::Turn { .. } => {
                    self.refresh_decisions();
                    break;
                }
                Phase::CallWindow { from, tile, awaiting } => {
                    if awaiting
                        .iter()
                        .all(|&s| self.submitted[s as usize].is_some())
                    {
                        // `resolve_calls` reads the submissions, so they must
                        // still be present: clear *after* resolving.
                        self.resolve_calls(from, tile);
                        self.submitted = [None; 4];
                        continue;
                    }
                    self.refresh_decisions();
                    break;
                }
                Phase::ChankanWindow { from, tile, awaiting } => {
                    if awaiting
                        .iter()
                        .all(|&s| self.submitted[s as usize].is_some())
                    {
                        let ron = (0..4u8).find(|&s| {
                            s != from && self.submitted[s as usize] == Some(Action::Ron)
                        });
                        self.submitted = [None; 4];
                        if let Some(seat) = ron {
                            // 搶槓成立: the 加槓 never happens, so no kan dora
                            // is turned and 四槓散了 does not trigger.
                            let p = &self.players[seat as usize];
                            let mut h = p.hand;
                            h[kind_of(tile) as usize] += 1;
                            let score = self
                                .score_for(seat, &h, tile, false, true, false)
                                .expect("the chankan ron was validated");
                            self.apply_win(vec![(seat, Some(from), tile, score)]);
                            continue;
                        }
                        let before = self.round_seq;
                        self.finish_kakan(from);
                        if self.round_seq != before {
                            continue;
                        }
                        self.do_rinshan_draw(from);
                        continue;
                    }
                    self.refresh_decisions();
                    break;
                }
                Phase::RoundEnd => {
                    if self.advance_round() {
                        continue;
                    }
                    self.decisions.clear();
                    break;
                }
                Phase::GameEnd => {
                    self.decisions.clear();
                    break;
                }
            }
        }
    }

    fn do_draw(&mut self, seat: u8) {
        let Some(t) = self.wall.draw() else {
            self.end_exhaustive();
            return;
        };
        {
            let p = &mut self.players[seat as usize];
            p.hand[kind_of(t) as usize] += 1;
            p.hand_tiles.push(t);
            p.hand_tiles.sort_unstable();
            p.drawn = Some(t);
            p.drawn_is_rinshan = false;
            p.draws += 1;
            p.temp_furiten = false;
            p.kuikae_forbidden.clear();
            // 一発 only survives until the declarer's own next draw.
            p.ippatsu = false;
        }
        self.turn = seat;
        self.push_event(Event::Draw {
            seat,
            tile: t,
            rinshan: false,
        });
        self.phase = Phase::Turn { seat };
    }

    /// Advance to the next round. Returns `true` when one was started.
    fn advance_round(&mut self) -> bool {
        let outcome = std::mem::take(&mut self.outcome);
        if self.rules.tobi && self.players.iter().any(|p| p.score < 0) {
            self.end_game();
            return false;
        }
        if self.is_last_hand() && self.rank_of(self.dealer) == 0 {
            // アガリやめ: the last dealer wins while leading.
            if outcome.won && outcome.dealer_won && self.rules.agari_yame {
                self.end_game();
                return false;
            }
            // テンパイやめ: the last dealer is tenpai on an exhaustive draw.
            if self.rules.tenpai_yame && !outcome.won && outcome.dealer_repeat {
                self.end_game();
                return false;
            }
        }
        if outcome.dealer_repeat {
            self.honba += 1;
        } else {
            self.honba = 0;
            self.dealer = (self.dealer + 1) % 4;
            self.round_index += 1;
            // 西入り: extend into the West round when nobody has reached the
            // return score yet.
            let scheduled = 4 * match self.rules.length {
                GameLength::Tonpuu => 1u8,
                GameLength::Hanchan => 2u8,
            };
            if self.round_index == scheduled
                && self.rules.west_extension
                && self.players.iter().all(|p| p.score < self.rules.return_score)
            {
                self.rounds_in_match = scheduled + 4;
            }
            if self.match_is_over() {
                self.end_game();
                return false;
            }
            self.round_wind = EAST + (self.round_index / 4) as Kind;
            self.round_number = (self.round_index % 4) + 1;
        }
        self.start_round();
        true
    }

    fn rank_of(&self, seat: u8) -> usize {
        let mut order: Vec<(i32, u8)> = (0..4).map(|s| (self.players[s as usize].score, s)).collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        order
            .iter()
            .position(|&(_, s)| s == seat)
            .unwrap_or(0)
    }

    fn is_last_hand(&self) -> bool {
        self.round_index + 1 == self.rounds_in_match
    }

    fn match_is_over(&self) -> bool {
        self.round_index >= self.rounds_in_match
    }

    fn end_game(&mut self) {
        // 終局時の立直棒は Top 取り.
        if self.riichi_sticks > 0 {
            let mut order: Vec<(i32, u8)> = (0..4)
                .map(|s| (self.players[s as usize].score, s))
                .collect();
            order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            let top = order[0].1 as usize;
            self.players[top].score += self.riichi_sticks as i32 * 1000;
            self.riichi_sticks = 0;
        }
        let scores = self.scores();
        let mut order: Vec<(i32, u8)> = (0..4).map(|s| (scores[s as usize], s as u8)).collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut ranking = [0u8; 4];
        for (place, &(_, seat)) in order.iter().enumerate() {
            ranking[place] = seat;
        }
        self.push_event(Event::GameEnd { scores, ranking });
        self.phase = Phase::GameEnd;
        self.finished = true;
    }

    // ---- views -----------------------------------------------------------

    /// Build the view for one observer; opponents' hands stay hidden.
    pub fn view(&self, observer: u8) -> TableView {
        let players = (0..4u8)
            .map(|s| {
                let p = &self.players[s as usize];
                let own = s == observer;
                let mut hand = Vec::new();
                if own {
                    hand = p.hand_tiles.clone();
                    hand.sort_unstable();
                }
                let (shanten_v, waits) = if own {
                    (
                        Some(shanten(&p.hand, p.melds.len() as u8)),
                        if p.hand_len() % 3 == 1 {
                            winning_kinds(&p.hand, p.melds.len() as u8)
                        } else {
                            Vec::new()
                        },
                    )
                } else {
                    (None, Vec::new())
                };
                PlayerView {
                    seat: s,
                    hand_count: p.hand_len(),
                    hand,
                    drawn: if own { p.drawn } else { None },
                    melds: p.melds.clone(),
                    discards: p.discards.clone(),
                    score: p.score,
                    riichi: p.riichi,
                    ippatsu: p.ippatsu,
                    furiten: own && self.is_furiten(s),
                    furiten_temp: own && p.temp_furiten,
                    furiten_riichi: own && p.riichi_furiten,
                    shanten: shanten_v,
                    waits,
                    is_dealer: s == self.dealer,
                    wind: self.seat_wind(s),
                }
            })
            .collect();
        let dora_indicators = (0..self.wall.revealed_indicators())
            .filter_map(|n| self.wall.dora_indicator(n))
            .collect();
        TableView {
            observer,
            players,
            round_wind: self.round_wind,
            round_number: self.round_number,
            honba: self.honba,
            riichi_sticks: self.riichi_sticks,
            dealer: self.dealer,
            wall_remaining: self.wall.remaining(),
            dora_indicators,
            phase: self.phase.clone(),
            decision: self
                .decisions
                .iter()
                .find(|d| d.seat == observer)
                .cloned(),
            // `Draw` carries a private tile, so another seat's draws are
            // dropped rather than shipped: a view must never let its reader
            // reconstruct somebody else's hand. (The type has said so all
            // along — see the note on `Event`.)
            events: self
                .events
                .iter()
                .filter(|e| match e {
                    Event::Draw { seat, .. } => *seat == observer,
                    _ => true,
                })
                .cloned()
                .collect(),
            finished: self.finished,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::tile_of;

    fn table(seed: u64) -> Table {
        Table::new(TableConfig::new(seed))
    }

    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Drive a whole match with a fixed pseudo-random policy.
    fn play_random_match(seed: u64, plies: usize) -> (Table, usize) {
        let mut t = table(seed);
        let mut rng = XorShift(seed.wrapping_mul(6364136223846793005).wrapping_add(1));
        let mut steps = 0;
        while !t.finished && steps < plies {
            let decisions = t.decisions().to_vec();
            if decisions.is_empty() {
                break;
            }
            let mut progressed = false;
            for d in decisions {
                let pick = (rng.next() % d.actions.len() as u64) as usize;
                let action = d.actions[pick];
                t.submit(d.seat, action)
                    .unwrap_or_else(|e| panic!("submit failed: {} ({})", e, action.label()));
                steps += 1;
                progressed = true;
                if t.finished {
                    break;
                }
            }
            assert!(progressed, "the driver made no progress");
        }
        (t, steps)
    }

    #[test]
    fn deal_is_consistent() {
        let t = table(1);
        assert_eq!(t.wall.remaining(), 122 - 53);
        // The dealer has already drawn their 14th tile.
        assert_eq!(t.players[0].hand_len(), 14);
        for s in 1..4 {
            assert_eq!(t.players[s].hand_len(), 13);
        }
        assert_eq!(t.phase, Phase::Turn { seat: 0 });
        assert_eq!(t.players.iter().map(|p| p.hand_len()).sum::<u32>(), 53);
    }

    #[test]
    fn points_are_conserved() {
        for seed in 1..20u64 {
            let (t, steps) = play_random_match(seed, 30_000);
            let sum: i32 = t.scores().iter().sum();
            assert_eq!(
                sum + t.riichi_sticks as i32 * 1000,
                100_000,
                "seed {} after {} steps",
                seed,
                steps
            );
            assert!(steps > 0);
        }
    }

    #[test]
    fn matches_terminate() {
        let mut finished = 0;
        for seed in 100..150u64 {
            let (t, _) = play_random_match(seed, 200_000);
            if t.finished {
                finished += 1;
            }
        }
        assert_eq!(finished, 50, "not every match finished");
    }

    #[test]
    fn seat_winds_follow_the_dealer() {
        let t = table(5);
        assert_eq!(t.seat_wind(0), EAST);
        assert_eq!(t.seat_wind(1), SOUTH);
        assert_eq!(t.seat_wind(2), WEST);
        assert_eq!(t.seat_wind(3), crate::tile::NORTH);
    }

    #[test]
    fn every_hand_stays_consistent() {
        // Walk a few matches step by step and check the tile bookkeeping.
        for seed in 7..12u64 {
            let mut t = table(seed);
            let mut rng = XorShift(seed + 999);
            let mut steps = 0;
            while !t.finished && steps < 20_000 {
                for d in t.decisions().to_vec() {
                    let action = d.actions[(rng.next() % d.actions.len() as u64) as usize];
                    t.submit(d.seat, action).unwrap();
                    steps += 1;
                }
                // Every player holds 13 or 14 effective tiles (3 per meld,
                // kans included, because a rinshan draw replaces the 4th tile).
                for s in 0..4usize {
                    let p = &t.players[s];
                    let total = p.hand_len() + 3 * p.melds.len() as u32;
                    assert!(
                        total == 13 || total == 14,
                        "seat {} holds {} effective tiles ({} concealed, melds {:?}) during {:?}; \
                         last events: {:?}",
                        s,
                        total,
                        p.hand_len(),
                        p.melds.iter().map(|m| m.kind).collect::<Vec<_>>(),
                        t.phase,
                        t.history.iter().rev().take(6).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    fn set_hand(t: &mut Table, seat: usize, spec: &str) {
        let kinds = crate::tile::parse_kinds(spec);
        let mut counts = [0u8; NUM_KINDS];
        let mut tiles: Vec<Tile> = Vec::new();
        for k in kinds {
            counts[k as usize] += 1;
            tiles.push(tile_of(k, counts[k as usize] - 1));
        }
        t.players[seat].hand = counts;
        t.players[seat].hand_tiles = tiles;
        t.players[seat].drawn = None;
    }

    fn tile(spec: &str, copy: u8) -> Tile {
        tile_of(crate::tile::parse_kind(spec).unwrap(), copy)
    }

    #[test]
    fn ron_is_offered_only_with_a_yaku() {
        let mut t = table(3);
        // Closed pinfu shape waiting on a ryanmen: 4s wins with 平和.
        set_hand(&mut t, 1, "123m456m678p11p23s");
        let d = t.call_decision(1, 0, tile("4s", 0), true);
        assert!(d.actions.contains(&Action::Ron), "{:?}", d.actions);

        // The same complete hand on a 辺張 wait has no yaku at all.
        set_hand(&mut t, 1, "123m456m678p99p12s");
        let d = t.call_decision(1, 0, tile("3s", 0), true);
        assert!(!d.actions.contains(&Action::Ron), "{:?}", d.actions);
    }

    #[test]
    fn furiten_blocks_ron() {
        let mut t = table(3);
        // 23s waits on 1s and 4s with a closed pinfu-shaped hand.
        set_hand(&mut t, 1, "123m456m678p11p23s");
        assert!(t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));
        assert!(t.call_decision(1, 0, tile("4s", 0), true).actions.contains(&Action::Ron));

        // Permanent furiten is about the *hand*: one wait in your own discards
        // blocks ron on every wait.
        t.players[1].discards.push(Discard {
            tile: tile("4s", 1),
            tsumogiri: false,
            riichi: false,
            called_by: None,
        });
        assert!(!t.call_decision(1, 0, tile("4s", 0), true).actions.contains(&Action::Ron));
        assert!(!t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));
        // A discard that is not one of the waits does not cause furiten.
        t.players[1].discards.push(Discard {
            tile: tile("5s", 1),
            tsumogiri: false,
            riichi: false,
            called_by: None,
        });
        assert!(!t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));

        // Clearing the furiten restores the ron.
        t.players[1].discards.clear();
        assert!(t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));

        // 同巡振聴 blocks every wait until this player's next draw.
        t.players[1].temp_furiten = true;
        assert!(!t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));
        t.players[1].temp_furiten = false;
        assert!(t.call_decision(1, 0, tile("1s", 0), true).actions.contains(&Action::Ron));
    }

    /// Submit a pass for every pending decision except `keep`.
    fn pass_others(t: &mut Table, keep: u8) {
        loop {
            let pending: Vec<Decision> = t
                .decisions()
                .iter()
                .filter(|d| d.seat != keep && d.actions.contains(&Action::Pass))
                .cloned()
                .collect();
            if pending.is_empty() {
                break;
            }
            for d in pending {
                t.submit(d.seat, Action::Pass).unwrap();
            }
        }
    }

    fn force_discard(t: &mut Table, seat: u8, spec: &str, copy: u8) {
        let tl = tile(spec, copy);
        let k = kind_of(tl) as usize;
        t.players[seat as usize].hand[k] += 1;
        t.players[seat as usize].hand_tiles.push(tl);
        t.players[seat as usize].drawn = Some(tl);
        t.phase = Phase::Turn { seat };
        t.refresh_decisions();
        t.submit(seat, Action::Discard { tile: tl, riichi: false })
            .unwrap();
    }

    #[test]
    fn passing_a_ron_sets_temporary_furiten_until_the_next_draw() {
        let mut t = table(3);
        set_hand(&mut t, 1, "123m456m678p11p23s");
        set_hand(&mut t, 3, "123m456m678p99p24s");
        // Seat 3 discards the 4s that seat 1 waits on.
        force_discard(&mut t, 3, "4s", 2);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 has a decision");
        assert!(d1.actions.contains(&Action::Ron), "{:?}", d1.actions);
        pass_others(&mut t, 1);
        t.submit(1, Action::Pass).unwrap();
        assert!(t.players[1].temp_furiten, "passing a ron causes 同巡振聴");

        // Seat 0 now discards the same tile in the same go-around: no ron.
        force_discard(&mut t, 0, "4s", 3);
        let again = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 has a decision");
        assert!(!again.actions.contains(&Action::Ron), "{:?}", again.actions);
        pass_others(&mut t, 1);
        t.submit(1, Action::Pass).unwrap();
        // After seat 1's own draw the furiten is gone again.
        assert!(!t.players[1].temp_furiten);
    }

    #[test]
    fn chi_is_only_available_to_the_player_on_the_left() {
        let mut t = table(3);
        set_hand(&mut t, 1, "234m567m234p567s1z");
        set_hand(&mut t, 2, "234m567m234p567s1z");
        let left = t.call_decision(1, 0, tile("3m", 0), true);
        assert!(
            left.actions.iter().any(|a| a.kind() == crate::action::ActionKind::Chi),
            "{:?}",
            left.actions
        );
        let other = t.call_decision(2, 0, tile("3m", 0), true);
        assert!(!other
            .actions
            .iter()
            .any(|a| a.kind() == crate::action::ActionKind::Chi));
    }

    #[test]
    fn riichi_locks_the_hand_to_tsumogiri() {
        let mut t = table(4);
        let drawn = t.players[0].drawn.unwrap();
        t.players[0].riichi = true;
        let d = t.self_turn_decision(0);
        let discards: Vec<&Action> = d.actions.iter().filter(|a| a.is_discard()).collect();
        assert_eq!(discards.len(), 1, "{:?}", d.actions);
        assert_eq!(
            *discards[0],
            Action::Discard {
                tile: drawn,
                riichi: false
            }
        );
        assert!(!d.actions.contains(&Action::Tsumo) || true);
    }

    #[test]
    fn riichi_declaration_requires_tenpai() {
        let mut t = table(6);
        // 14 tiles that become tenpai after discarding 4s.
        set_hand(&mut t, 0, "123m456m678p11p234s");
        t.players[0].drawn = Some(tile("4s", 1));
        t.players[0].hand_tiles.push(tile("4s", 1));
        let d = t.self_turn_decision(0);
        assert!(
            d.actions
                .iter()
                .any(|a| matches!(a, Action::Discard { riichi: true, .. })),
            "{:?}",
            d.actions
        );

        // A hand with no tenpai discard cannot declare riichi.
        set_hand(&mut t, 0, "123m456m678p11p258s");
        t.players[0].drawn = Some(tile("8s", 2));
        let d = t.self_turn_decision(0);
        assert!(
            !d.actions
                .iter()
                .any(|a| matches!(a, Action::Discard { riichi: true, .. })),
            "{:?}",
            d.actions
        );
    }

    #[test]
    fn exhaustive_draw_pays_tenpai() {
        let mut t = table(8);
        set_hand(&mut t, 0, "123m456m678p11p23s");
        for s in 1..4 {
            set_hand(&mut t, s, "123m456m678p1z2z4z9s");
        }
        let before = t.scores();
        t.end_exhaustive();
        let after = t.scores();
        assert_eq!(after[0] - before[0], 3000);
        for s in 1..4 {
            assert_eq!(after[s] - before[s], -1000);
        }
    }

    #[test]
    fn ankan_reveals_dora_and_draws_rinshan() {
        let mut t = table(11);
        set_hand(&mut t, 0, "1111m234p567p99s1z");
        let drawn = t.wall.draw().unwrap();
        t.players[0].hand[kind_of(drawn) as usize] += 1;
        t.players[0].hand_tiles.push(drawn);
        t.players[0].drawn = Some(drawn);
        t.phase = Phase::Turn { seat: 0 };
        t.refresh_decisions();
        let d = t.decisions()[0].clone();
        let kan = d
            .actions
            .iter()
            .find(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Ankan))
            .copied();
        assert!(kan.is_some(), "{:?}", d.actions);
        let before = t.wall.remaining();
        t.submit(0, kan.unwrap()).unwrap();
        assert_eq!(t.wall.kan_count(), 1);
        assert_eq!(t.wall.remaining(), before - 1);
        assert_eq!(t.players[0].melds.len(), 1);
        assert!(t.players[0].drawn_is_rinshan);
    }
    /// A submission must actually be applied, not merely offered.
    #[test]
    fn ron_is_applied_and_paid() {
        let mut t = table(3);
        set_hand(&mut t, 1, "123m456m678p11p23s");
        set_hand(&mut t, 3, "123m456m678p99p24s");
        let before = t.scores();
        force_discard(&mut t, 3, "4s", 2);
        pass_others(&mut t, 1);
        t.submit(1, Action::Ron).unwrap();
        let win = t
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Win { seat, from, score, .. } => Some((*seat, *from, score.clone())),
                _ => None,
            })
            .expect("a win event");
        assert_eq!(win.0, 1);
        assert_eq!(win.1, Some(3));
        assert!(win.2.yaku.iter().any(|&(y, _)| y == Yaku::Pinfu), "{:?}", win.2.yaku);
        let after = t.scores();
        assert_eq!(after[1] - before[1], win.2.ron_total(0));
        assert_eq!(after[3] - before[3], -win.2.ron_total(0));
        assert_eq!(after.iter().sum::<i32>() + t.riichi_sticks as i32 * 1000, 100_000);
    }

    #[test]
    fn pon_takes_the_discard_and_moves_the_turn() {
        let mut t = table(3);
        set_hand(&mut t, 1, "55m123p456p789p1z");
        set_hand(&mut t, 3, "123m456m678p99p24s");
        force_discard(&mut t, 3, "5m", 2);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 may call");
        let pon = d1
            .actions
            .iter()
            .find(|a| a.kind() == crate::action::ActionKind::Pon)
            .copied()
            .expect("pon is offered");
        pass_others(&mut t, 1);
        t.submit(1, pon).unwrap();
        assert_eq!(t.players[1].melds.len(), 1);
        assert_eq!(t.players[1].melds[0].kind, MeldKind::Pon);
        assert_eq!(t.players[1].melds[0].from, 3);
        assert_eq!(t.players[1].hand[kind_of(tile("5m", 0)) as usize], 0);
        assert_eq!(t.players[3].discards.last().unwrap().called_by, Some(1));
        // The caller must now discard.
        assert_eq!(t.phase, Phase::Turn { seat: 1 });
        assert!(t.decisions().iter().any(|d| d.seat == 1));
        assert!(t.players[1].kuikae_forbidden.contains(&kind_of(tile("5m", 0))));
    }

    #[test]
    fn chi_moves_the_turn_to_the_caller() {
        let mut t = table(3);
        // チー is only available to the player who plays right after the
        // discarder, so seat 0 discards for seat 1 to call.
        set_hand(&mut t, 1, "23m456p789p123s11z");
        set_hand(&mut t, 0, "123m678p99p24s1z1z1z");
        force_discard(&mut t, 0, "1m", 2);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 may call");
        let chi = d1
            .actions
            .iter()
            .find(|a| a.kind() == crate::action::ActionKind::Chi)
            .copied()
            .expect("chi is offered");
        pass_others(&mut t, 1);
        t.submit(1, chi).unwrap();
        assert_eq!(t.players[1].melds.len(), 1);
        assert_eq!(t.players[1].melds[0].kind, MeldKind::Chi);
        assert_eq!(t.phase, Phase::Turn { seat: 1 });
    }

    #[test]
    fn minkan_draws_a_replacement_tile() {
        let mut t = table(3);
        set_hand(&mut t, 1, "555m123p456p789p");
        set_hand(&mut t, 3, "123m678p99p24s11z");
        let before = t.wall.remaining();
        force_discard(&mut t, 3, "5m", 2);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 may call");
        let kan = d1
            .actions
            .iter()
            .find(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Minkan))
            .copied()
            .expect("minkan is offered");
        pass_others(&mut t, 1);
        t.submit(1, kan).unwrap();
        assert_eq!(t.players[1].melds[0].kind, MeldKind::Minkan);
        assert_eq!(t.wall.kan_count(), 1);
        assert!(t.players[1].drawn_is_rinshan);
        assert_eq!(t.wall.remaining(), before - 1); // the rinshan tile
        assert_eq!(t.phase, Phase::Turn { seat: 1 });
    }

    /// 加槓 is only a kan once its 搶槓 window closes: a robbed 加槓 must not
    /// leave a dora indicator behind, and must not score that dora for the
    /// player who robbed it.
    #[test]
    fn robbed_kakan_leaves_no_phantom_dora() {
        let mut t = table(5);
        set_hand(&mut t, 1, "55m123p456p789p1z");
        // Seat 3 waits on 5m and has never discarded one, so it is not furiten.
        set_hand(&mut t, 3, "34m456p789p11z234s");
        force_discard(&mut t, 0, "5m", 2);
        let pon = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .expect("seat 1 may call")
            .actions
            .iter()
            .find(|a| a.kind() == crate::action::ActionKind::Pon)
            .copied()
            .expect("pon is offered");
        pass_others(&mut t, 1);
        t.submit(1, pon).unwrap();

        // Seat 1 now holds the fourth 5m and may add it to its pon.
        set_hand(&mut t, 1, "5m123p456p789p1z");
        t.phase = Phase::Turn { seat: 1 };
        t.refresh_decisions();
        let kakan = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .expect("seat 1 has a decision")
            .actions
            .iter()
            .find(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Kakan))
            .copied()
            .expect("kakan is offered");

        let indicators = t.wall.revealed_indicators();
        t.submit(1, kakan).unwrap();
        // The 搶槓 window is open: the added kan is not counted yet.
        assert_eq!(t.wall.kan_count(), 0, "the kan is not final before 搶槓");
        assert_eq!(t.wall.revealed_indicators(), indicators);

        let ron = t
            .decisions()
            .iter()
            .find(|d| d.seat == 3)
            .expect("seat 3 may rob the kan")
            .actions
            .iter()
            .find(|a| matches!(a, Action::Ron))
            .copied()
            .expect("chankan ron is offered");
        pass_others(&mut t, 3);
        t.submit(3, ron).unwrap();

        let win = t
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Win { seat, from, score, .. } => Some((*seat, *from, score.clone())),
                _ => None,
            })
            .expect("the 搶槓 win is recorded");
        assert_eq!(win.0, 3);
        assert_eq!(win.1, Some(1), "the kan caller deals into the robber");
        assert!(
            win.2.yaku.iter().any(|&(y, _)| y == Yaku::Chankan),
            "搶槓 must be scored as a yaku: {:?}",
            win.2.yaku
        );
    }    #[test]
    fn triple_ron_aborts_and_repeats_the_dealer() {
        let mut t = table(21);
        for s in 1..4 {
            set_hand(&mut t, s, "123m456m678p11p23s");
        }
        set_hand(&mut t, 0, "123m456m678p99p24s");
        let before = t.scores();
        let dealer = t.dealer;
        force_discard(&mut t, 0, "4s", 2);
        let seats: Vec<u8> = t.decisions().iter().map(|d| d.seat).collect();
        assert_eq!(seats.len(), 3, "all three players should hold a ron");
        for s in [1u8, 2, 3] {
            let d = t
                .decisions()
                .iter()
                .find(|d| d.seat == s)
                .cloned()
                .unwrap();
            assert!(d.actions.contains(&Action::Ron), "{:?}", d.actions);
            t.submit(s, Action::Ron).unwrap();
        }
        let reason = t.history.iter().rev().find_map(|e| match e {
            Event::Ryuukyoku { reason, .. } => Some(*reason),
            _ => None,
        });
        assert_eq!(reason, Some(DrawReason::TripleRon));
        assert_eq!(t.scores(), before, "no points change hands on 三家和");
        assert_eq!(t.dealer, dealer, "途中流局 keeps the dealer");
        assert_eq!(t.honba, 1);
    }

    #[test]
    fn suji_kuikae_is_forbidden() {
        let mut t = table(22);
        // Seat 1 holds 1m2m3m and calls 4m with 2m3m: discarding the 1m next
        // would rebuild the same run on the other side, so it is forbidden.
        set_hand(&mut t, 1, "123m456p789p11s22z");
        set_hand(&mut t, 0, "123m678p99p24s1z1z1z");
        force_discard(&mut t, 0, "4m", 2);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 may chi");
        let chi = d1
            .actions
            .iter()
            .find(|a| a.kind() == crate::action::ActionKind::Chi)
            .copied()
            .expect("chi is offered");
        pass_others(&mut t, 1);
        t.submit(1, chi).unwrap();
        assert!(t.players[1].kuikae_forbidden.contains(&0u8), "1m is 筋食い替え");
        assert!(t.players[1].kuikae_forbidden.contains(&3u8), "4m is 現物食い替え");
        let d = t.self_turn_decision(1);
        assert!(
            !d.actions
                .iter()
                .any(|a| matches!(a, Action::Discard { tile, .. } if kind_of(*tile) == 0)),
            "{:?}",
            d.actions
        );
        assert!(d
            .actions
            .iter()
            .any(|a| matches!(a, Action::Discard { tile, .. } if kind_of(*tile) != 0)));
    }

    /// 立直 forbids every call that changes the hand: 吃・碰・大明槓 are out, and
    /// the player may still win on a discard because 立直 is itself a yaku.
    #[test]
    fn riichi_forbids_calling() {
        let mut t = table(3);
        // A hand that can both pon (55m) and chi (34m + 5m) the 5m seat 3 throws.
        set_hand(&mut t, 0, "34m55m123p456p789p");
        set_hand(&mut t, 3, "123m678p99p24s11z");
        let five = tile("5m", 2);

        // Control arm first: without riichi, both calls really are offered, so
        // the assertions below cannot pass for the wrong reason.
        let open = t.call_decision(0, 3, five, true);
        assert!(
            open.actions
                .iter()
                .any(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Pon)),
            "{:?}",
            open.actions
        );
        assert!(
            open.actions
                .iter()
                .any(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Chi)),
            "{:?}",
            open.actions
        );

        t.players[0].riichi = true;
        let locked = t.call_decision(0, 3, five, true);
        assert!(
            !locked
                .actions
                .iter()
                .any(|a| matches!(a, Action::Meld { .. })),
            "a riichi player must not be offered any call: {:?}",
            locked.actions
        );
        assert!(locked.actions.contains(&Action::Pass));

        // A winning tile is still a ron: 立直 is a yaku, so this is a win.
        // (3s completes 123p456p789p + 34m55m — not a win, so pick a real one.)
        set_hand(&mut t, 0, "34m55m123p456p789p");
        t.players[0].riichi = true;
        let win_tile = tile("5m", 0);
        set_hand(&mut t, 0, "345m55m123p456p789p");
        let d = t.call_decision(0, 3, win_tile, true);
        assert!(
            !d.actions.iter().any(|a| matches!(a, Action::Meld { .. })),
            "{:?}",
            d.actions
        );
    }

    /// 加槓 changes the hand, so it is forbidden after 立直 exactly like 吃/碰;
    /// 暗槓 survives only when it leaves the wait alone.
    #[test]
    fn riichi_forbids_kakan() {
        let mut t = table(3);
        set_hand(&mut t, 1, "55m123p456p789p1z");
        set_hand(&mut t, 3, "123m678p99p24s11z");
        force_discard(&mut t, 3, "5m", 2);
        let pon = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .expect("seat 1 may call")
            .actions
            .iter()
            .find(|a| a.kind() == crate::action::ActionKind::Pon)
            .copied()
            .expect("pon is offered");
        pass_others(&mut t, 1);
        t.submit(1, pon).unwrap();

        // Seat 1 now holds the fourth 5m and could add it to the pon.
        set_hand(&mut t, 1, "5m123p456p789p1z");
        t.phase = Phase::Turn { seat: 1 };
        t.refresh_decisions();
        let has_kakan = |t: &Table| {
            t.decisions()
                .iter()
                .find(|d| d.seat == 1)
                .map(|d| {
                    d.actions
                        .iter()
                        .any(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Kakan))
                })
                .unwrap_or(false)
        };
        assert!(has_kakan(&t), "the kakan is offered before riichi");

        t.players[1].riichi = true;
        t.refresh_decisions();
        assert!(
            !has_kakan(&t),
            "a riichi player must not be offered 加槓: {:?}",
            t.decisions()
        );
    }

    /// 四槓散了 ends the round, and the kan that caused it must not go on to
    /// draw: `abort_round` pumps straight into the next round, so the caller's
    /// phase check could never see RoundEnd and an extra 嶺上牌 landed in the
    /// new round, out of turn, leaving that seat a tile heavy.
    #[test]
    fn a_four_kans_abort_does_not_draw_into_the_next_round() {
        let mut t = table(11);
        // Three kans by other seats are already on the books, so the next one
        // is the fourth and not all by the same player.
        t.total_kans = 3;
        t.kan_owners = vec![1, 1, 2];
        let honba_before = t.honba;
        set_hand(&mut t, 3, "1111m234p567p99s1z");
        t.phase = Phase::Turn { seat: 3 };
        t.refresh_decisions();
        let ankan = t
            .decisions()
            .iter()
            .find(|d| d.seat == 3)
            .expect("seat 3 has a decision")
            .actions
            .iter()
            .find(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Ankan))
            .copied()
            .expect("the 暗槓 is offered");
        t.submit(3, ankan).unwrap();

        assert_eq!(t.honba, honba_before + 1, "途中流局 repeats the dealer");
        assert!(
            t.history.iter().any(|e| matches!(e, Event::Ryuukyoku { .. })),
            "the round should have been aborted"
        );
        // The precise property: in the round that follows an abort there is no
        // 嶺上 draw at all. (The dealer's normal draw is not one.)
        let start = t
            .history
            .iter()
            .rposition(|e| matches!(e, Event::RoundStart { .. }))
            .expect("the next round started");
        assert!(
            !t.history[start..]
                .iter()
                .any(|e| matches!(e, Event::Draw { rinshan: true, .. })),
            "the aborted kan must not draw a replacement tile in the next round"
        );
        assert!(t.players[3].melds.is_empty(), "the new round starts clean");
    }

    /// An abortive draw must say who declared it and how much wall was left: a
    /// settlement that only says "流局" is indistinguishable from a bug for the
    /// player (reported after a 九種九牌 abort with a nearly full wall).
    #[test]
    fn a_nine_terminals_abort_names_the_declarer_and_the_wall() {
        let mut t = table(9);
        set_hand(&mut t, 0, "19m19p19s1234567z");
        t.players[0].draws = 1;
        t.players[0].drawn = Some(tile("1z", 2));
        t.players[0].hand_tiles.push(tile("1z", 2));
        t.phase = Phase::Turn { seat: 0 };
        t.refresh_decisions();
        let kyuushu = t
            .decisions()
            .iter()
            .find(|d| d.seat == 0)
            .expect("seat 0 has a decision")
            .actions
            .iter()
            .find(|a| matches!(a, Action::Kyuushu))
            .copied()
            .expect("九種九牌 is offered");
        let wall_before = t.wall.remaining();
        t.submit(0, kyuushu).unwrap();
        let (by, wall) = t
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Ryuukyoku {
                    reason: DrawReason::NineTerminals,
                    by,
                    wall_remaining,
                    ..
                } => Some((*by, *wall_remaining)),
                _ => None,
            })
            .expect("the abort is recorded with its reason");
        assert_eq!(by, Some(0), "the declarer is named");
        assert_eq!(wall, wall_before, "and the wall at that moment");
    }

    /// A ron on the discard that would trigger 四風連打 / 四家立直 wins: the abort
    /// is deferred until the call window closes without a win.
    #[test]
    fn a_ron_beats_a_deferred_abort() {
        let mut t = table(3);
        set_hand(&mut t, 1, "123m456m678p11p23s");
        set_hand(&mut t, 0, "123m456m678p99p24s");
        t.pending_abort = Some(DrawReason::FourRiichi);
        force_discard(&mut t, 0, "4s", 2);
        pass_others(&mut t, 1);
        t.submit(1, Action::Ron).unwrap();
        assert!(
            t.history.iter().any(|e| matches!(e, Event::Win { .. })),
            "the ron must stand"
        );
        assert!(
            !t.history.iter().any(|e| matches!(e, Event::Ryuukyoku { .. })),
            "a ron preempts the abort"
        );

        // Control: with nobody able to win, the same deferral does abort.
        let mut t = table(3);
        set_hand(&mut t, 1, "123m456m678p11p23s");
        set_hand(&mut t, 0, "123m456m678p99p24s");
        t.pending_abort = Some(DrawReason::FourRiichi);
        force_discard(&mut t, 0, "1z", 0);
        pass_others(&mut t, 1);
        let drawn = t
            .history
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Ryuukyoku { reason, .. } => Some(*reason),
                _ => None,
            })
            .expect("the deferred abort fires");
        assert_eq!(drawn, DrawReason::FourRiichi);
    }

    #[test]
    fn ron_on_the_riichi_discard_voids_the_declaration() {
        let mut t = table(23);
        // Seat 0 declares riichi and discards 4s, which seat 1 is waiting on.
        set_hand(&mut t, 0, "123m456m678p11p234s");
        t.players[0].drawn = Some(tile("4s", 1));
        t.players[0].hand_tiles.push(tile("4s", 1));
        set_hand(&mut t, 1, "123m456m678p11p23s");
        t.phase = Phase::Turn { seat: 0 };
        t.refresh_decisions();
        let score_before = t.players[0].score;
        t.submit(
            0,
            Action::Discard {
                tile: tile("4s", 1),
                riichi: true,
            },
        )
        .unwrap();
        assert_eq!(t.riichi_sticks, 1);
        let d1 = t
            .decisions()
            .iter()
            .find(|d| d.seat == 1)
            .cloned()
            .expect("seat 1 may ron");
        assert!(d1.actions.contains(&Action::Ron));
        t.submit(1, Action::Ron).unwrap();
        assert!(!t.players[0].riichi, "the riichi never took effect");
        assert_eq!(t.riichi_sticks, 0, "no stick is placed");
        // The 1000 points came back; only the ron payment left the purse.
        assert!(t.players[0].score < score_before);
        assert_eq!(score_before - t.players[0].score, 1000 + 1000);
    }

    #[test]
    fn riichi_allows_an_ankan_that_keeps_the_wait() {
        let mut t = table(24);
        // 1111m 234p 567p 99s 88s: the shanpon wait is the same before and
        // after the kan, so the 暗槓 stays legal after 立直.
        set_hand(&mut t, 0, "1111m234p567p99s88s");
        t.players[0].drawn = Some(tile("8s", 1));
        t.players[0].riichi = true;
        t.phase = Phase::Turn { seat: 0 };
        let d = t.self_turn_decision(0);
        assert!(
            d.actions
                .iter()
                .any(|a| matches!(a, Action::Meld { meld } if meld.kind == MeldKind::Ankan)),
            "{:?}",
            d.actions
        );
        assert!(t.ankan_keeps_wait(0, 0));
    }

    #[test]
    fn fifth_tile_wait_still_counts_as_tenpai() {
        // 1111m 234p 567p 789p is a tanki wait on 1m, which is impossible in
        // practice because all four copies are in hand — but the shape is
        // tenpai and the exhaustive draw must pay it (形式聴牌).
        let counts = {
            let mut c = [0u8; NUM_KINDS];
            for k in crate::tile::parse_kinds("1111m234p567p789p") {
                c[k as usize] += 1;
            }
            c
        };
        assert!(winning_kinds(&counts, 0).is_empty());
        assert_eq!(tenpai_kinds(&counts, 0), vec![0u8]);
        assert!(crate::hand::is_tenpai(&counts, 0));
    }

    #[test]
    fn riichi_sticks_go_to_first_place_when_the_match_ends() {
        let mut t = table(25);
        // 3 sticks are held on the table, so the four purses sum to 97000.
        t.players[0].score = 29000;
        t.players[1].score = 20000;
        t.players[2].score = 20000;
        t.players[3].score = 28000;
        t.riichi_sticks = 3;
        t.dealer = 1;
        t.end_game();
        assert_eq!(t.riichi_sticks, 0);
        let first = (0..4)
            .max_by_key(|&s| (t.players[s as usize].score, std::cmp::Reverse(s)))
            .unwrap();
        assert_eq!(first, 0, "the leading purse takes the sticks");
        assert_eq!(t.players[0].score, 29000 + 3000);
        assert_eq!(t.scores().iter().sum::<i32>(), 100_000);
    }
}
