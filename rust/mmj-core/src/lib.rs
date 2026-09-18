//! `mmj-core` — a Japanese (riichi) mahjong rules engine.
//!
//! The crate is organised so that the *rules* never depend on how a decision is
//! made. [`state::Table`] is a deterministic state machine: drivers (the web
//! server, a self-play harness, a replay analyser) ask for the decisions that
//! are currently pending, submit one action per pending seat, and read back the
//! resulting events.
//!
//! ```text
//! tile    tile kinds and physical tiles, red fives, dora indicators
//! hand    winning shape, shanten, waits, tile acceptance
//! meld    called melds
//! wall    wall and dead wall, dora indicators, rinshan draws
//! rules   ruleset flags (Tenhou-standard defaults)
//! score   yaku, fu and point calculation
//! action  the action space shared by the engine, the UI and the network
//! state   the table state machine
//! ```

pub mod action;
pub mod danger;
pub mod hand;
pub mod pref;
pub mod meld;
pub mod rules;
pub mod score;
pub mod state;
pub mod tile;
pub mod wall;

pub use action::{Action, ActionKind};
pub use danger::danger_table;
pub use hand::{Counts, is_agari, is_tenpai, shanten, winning_kinds};
pub use meld::{Meld, MeldKind};
pub use rules::{GameLength, Rules, Ruleset};
pub use score::{ScoreResult, WinContext, Yaku, YakuList};
pub use state::{Event, Phase, PlayerView, Table, TableConfig};
pub use tile::{Kind, NUM_KINDS, NUM_TILES, Tile};
pub use wall::Wall;
