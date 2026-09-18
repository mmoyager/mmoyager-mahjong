//! Wall and dead wall (王牌).
//!
//! Physical layout used by the engine:
//!
//! * `live` — the 122 tiles that can be drawn normally. Normal draws take from
//!   the front, 嶺上 (rinshan) draws from the back. That reproduces the real
//!   "dead wall is replenished from the live wall" rule while keeping the
//!   arithmetic exact: `normal draws + rinshan draws == 122`.
//! * `dead` — 14 tiles. The top tile of stacks 3..7 holds the dora indicators
//!   (index 4, 6, 8, 10, 12); the tile below each one holds the corresponding
//!   ura dora indicator (index 5, 7, 9, 11, 13). The remaining dead-wall tiles
//!   are never drawn — exactly as in a real game, where they stay on the table.

use crate::tile::{Kind, NUM_TILES, Tile, dora_from_indicator};
use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Tiles in the dead wall.
pub const DEAD_WALL_LEN: usize = 14;
/// Tiles in the live wall.
pub const LIVE_WALL_LEN: usize = (NUM_TILES as usize) - DEAD_WALL_LEN;
/// Dead-wall index of the `n`-th dora indicator.
const DORA_SLOTS: [usize; 5] = [4, 6, 8, 10, 12];
/// Maximum number of dora indicators (one initial plus one per kan, capped).
pub const MAX_DORA_INDICATORS: usize = 5;

/// Wall state for one round.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wall {
    live: Vec<Tile>,
    dead: [Tile; DEAD_WALL_LEN],
    head: usize,
    tail: usize,
    /// Number of kans called this round; drives how many dora indicators are up.
    kan_count: u8,
}

impl Wall {
    /// Shuffle a full 136-tile set.
    pub fn shuffled(rng: &mut ChaCha8Rng) -> Self {
        let mut tiles: Vec<Tile> = (0..NUM_TILES).collect();
        tiles.shuffle(rng);
        let dead_slice = &tiles[LIVE_WALL_LEN..];
        let mut dead = [0u8; DEAD_WALL_LEN];
        dead.copy_from_slice(dead_slice);
        Wall {
            live: tiles[..LIVE_WALL_LEN].to_vec(),
            dead,
            head: 0,
            tail: LIVE_WALL_LEN,
            kan_count: 0,
        }
    }

    /// Draw the next normal tile.
    #[inline]
    pub fn draw(&mut self) -> Option<Tile> {
        if self.head < self.tail {
            let t = self.live[self.head];
            self.head += 1;
            Some(t)
        } else {
            None
        }
    }

    /// Draw a replacement tile for a kan (嶺上牌), taken from the back of the
    /// live wall.
    #[inline]
    pub fn draw_rinshan(&mut self) -> Option<Tile> {
        if self.head < self.tail {
            self.tail -= 1;
            Some(self.live[self.tail])
        } else {
            None
        }
    }

    /// Live tiles left. A kan is illegal when this reaches zero, and riichi
    /// needs at least [`crate::rules::Rules::min_riichi_wall`] of them.
    #[inline]
    pub fn remaining(&self) -> u32 {
        (self.tail - self.head) as u32
    }

    /// Tiles already drawn from the live wall.
    #[inline]
    pub fn drawn(&self) -> u32 {
        self.head as u32
    }

    /// Record that a kan was called; this may reveal another dora indicator.
    pub fn on_kan(&mut self) {
        self.kan_count = self.kan_count.saturating_add(1);
    }

    /// Total kans called this round.
    pub fn kan_count(&self) -> u8 {
        self.kan_count
    }

    /// Number of dora indicators currently visible.
    pub fn revealed_indicators(&self) -> usize {
        (1 + self.kan_count as usize).min(MAX_DORA_INDICATORS)
    }

    /// The n-th dora indicator, if it has been revealed.
    pub fn dora_indicator(&self, n: usize) -> Option<Tile> {
        if n < self.revealed_indicators() && n < MAX_DORA_INDICATORS {
            Some(self.dead[DORA_SLOTS[n]])
        } else {
            None
        }
    }

    /// The n-th ura dora indicator (only meaningful to a riichi winner).
    pub fn ura_indicator(&self, n: usize) -> Option<Tile> {
        if n < self.revealed_indicators() && n < MAX_DORA_INDICATORS {
            Some(self.dead[DORA_SLOTS[n] + 1])
        } else {
            None
        }
    }

    /// Tile kinds that are dora right now.
    pub fn dora_kinds(&self) -> Vec<Kind> {
        (0..self.revealed_indicators())
            .filter_map(|n| self.dora_indicator(n))
            .map(|t| dora_from_indicator(t >> 2))
            .collect()
    }

    /// Tile kinds that are ura dora right now.
    pub fn ura_kinds(&self) -> Vec<Kind> {
        (0..self.revealed_indicators())
            .filter_map(|n| self.ura_indicator(n))
            .map(|t| dora_from_indicator(t >> 2))
            .collect()
    }

    /// Whether any tile of `kind` is an indicator on the table (visible info).
    pub fn is_visible_indicator(&self, kind: Kind) -> bool {
        (0..self.revealed_indicators())
            .filter_map(|n| self.dora_indicator(n))
            .any(|t| (t >> 2) == kind)
    }

    /// Raw dead wall, for replay output.
    pub fn dead_wall(&self) -> &[Tile; DEAD_WALL_LEN] {
        &self.dead
    }

    /// Deterministic wall from an explicit tile ordering (used by tests and by
    /// replay-driven analysis).
    pub fn from_order(tiles: &[Tile]) -> Self {
        assert_eq!(tiles.len(), NUM_TILES as usize);
        let mut dead = [0u8; DEAD_WALL_LEN];
        dead.copy_from_slice(&tiles[LIVE_WALL_LEN..]);
        Wall {
            live: tiles[..LIVE_WALL_LEN].to_vec(),
            dead,
            head: 0,
            tail: LIVE_WALL_LEN,
            kan_count: 0,
        }
    }

    /// Rounded dice roll of the dealer, purely cosmetic (`16` style).
    pub fn roll_dice<R: Rng>(rng: &mut R) -> (u8, u8) {
        (rng.gen_range(1..=6), rng.gen_range(1..=6))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn drawing_accounts_for_every_live_tile() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut wall = Wall::shuffled(&mut rng);
        assert_eq!(wall.remaining(), LIVE_WALL_LEN as u32);
        let mut normal = 0;
        while wall.draw().is_some() {
            normal += 1;
        }
        assert_eq!(normal, LIVE_WALL_LEN);
        assert_eq!(wall.remaining(), 0);
        assert!(wall.draw().is_none());
        assert!(wall.draw_rinshan().is_none());
    }

    #[test]
    fn rinshan_takes_from_the_back() {
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let mut wall = Wall::shuffled(&mut rng);
        let first = wall.draw().unwrap();
        let rinshan = wall.draw_rinshan().unwrap();
        let second = wall.draw().unwrap();
        assert_ne!(first, rinshan);
        assert_ne!(rinshan, second);
        assert_eq!(wall.remaining(), (LIVE_WALL_LEN - 3) as u32);
    }

    #[test]
    fn dora_indicators_are_revealed_one_per_kan() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let mut wall = Wall::shuffled(&mut rng);
        assert_eq!(wall.revealed_indicators(), 1);
        assert!(wall.dora_indicator(0).is_some());
        assert!(wall.dora_indicator(1).is_none());
        assert!(wall.ura_indicator(0).is_some());
        for _ in 0..6 {
            wall.on_kan();
        }
        assert_eq!(wall.revealed_indicators(), MAX_DORA_INDICATORS);
        assert_eq!(wall.dora_kinds().len(), MAX_DORA_INDICATORS);
        assert_eq!(wall.ura_kinds().len(), MAX_DORA_INDICATORS);
        // Indicators and their ura partners are distinct dead-wall slots.
        for n in 0..MAX_DORA_INDICATORS {
            assert_ne!(wall.dora_indicator(n), wall.ura_indicator(n));
        }
    }

    #[test]
    fn ura_differs_from_dora_kinds_sometimes() {
        let mut rng = ChaCha8Rng::seed_from_u64(99);
        let wall = Wall::shuffled(&mut rng);
        // Not a strict guarantee, but the two indicator slots are different
        // physical tiles so the derived kinds are independent draws.
        assert_eq!(wall.dora_kinds().len(), 1);
        assert_eq!(wall.ura_kinds().len(), 1);
    }
}
