//! Ruleset configuration.
//!
//! The defaults follow the de-facto online standard (Tenhou.net four-player
//! rules), because the project goal is "the most standard reading of a disputed
//! rule". Every contested point is a flag so an alternative reading can be
//! selected without touching engine logic.

use serde::{Deserialize, Serialize};

/// Match length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameLength {
    /// 東風戦 — East round only.
    Tonpuu,
    /// 半荘戦 — East and South rounds.
    Hanchan,
}

impl GameLength {
    /// Number of wind rounds.
    pub const fn rounds(self) -> u8 {
        match self {
            GameLength::Tonpuu => 1,
            GameLength::Hanchan => 2,
        }
    }
}

/// How strictly 食い替え (kuikae) is forbidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KuikaeScope {
    /// 食い替え無し — neither the called tile nor the tile that would rebuild
    /// the same run on the other side (筋食い替え) may be discarded.
    Forbidden,
    /// Only the called tile itself may not be discarded.
    SameTileOnly,
    /// Anything may be discarded.
    Allowed,
}

/// How 人和 (renhou) is scored. Tenhou does not award it at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenhouValue {
    Off,
    Mangan,
    Yakuman,
}

/// A named preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ruleset {
    /// Tenhou.net standard — the default for this project.
    Tenhou,
    /// A stricter competitive reading (WRC-style) where it differs.
    Competitive,
}

/// The full ruleset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Red fives per number suit. `1` means one red five in each of man, pin
    /// and sou (3 in total), which is the standard.
    pub aka_per_suit: u8,
    /// 食い断 — open hands may score 断幺九.
    pub kuitan: bool,
    /// 後付け — a yaku may be completed by the winning tile itself.
    pub atozuke: bool,
    /// 切り上げ満貫 — 4 han 30 fu and 3 han 60 fu are rounded up to mangan.
    /// Tenhou: off.
    pub kiriage_mangan: bool,
    /// ダブル役満 — thirteen-sided kokushi, nine gates, big four winds and the
    /// four concealed triplets pair wait count twice. Tenhou: off.
    pub double_yakuman: bool,
    /// May a hand contain several yakuman at once (each worth a full yakuman)?
    /// Tenhou: yes — 役満 composes, each worth 32000 / 48000.
    pub stack_yakuman: bool,
    /// 流し満貫 at an exhaustive draw. Tenhou: on.
    pub nagashi_mangan: bool,
    /// アガリやめ — the dealer may end the match by winning the last hand.
    /// Tenhou: on (dealer in first place only).
    pub agari_yame: bool,
    /// テンパイやめ — the last dealer may also end the match on a tenpai
    /// exhaustive draw while leading. Tenhou: on.
    pub tenpai_yame: bool,
    /// 撃飛 — the match ends when a player drops below zero. Tenhou: on.
    pub tobi: bool,
    /// 食い替え — how much of the "swap calling" prohibition applies.
    /// Tenhou forbids it entirely, including 筋食い替え.
    pub kuikae: KuikaeScope,
    /// ダブロン — two players may win from one discard. Tenhou: on, and
    /// 積み棒 / 供託 go to the winner closest counter-clockwise from the
    /// discarder ("上家取り").
    pub multi_ron: bool,
    /// 三家和 — three rons on one discard is an abortive draw. Tenhou: on.
    pub abort_triple_ron: bool,
    /// May 国士無双 rob a *concealed* kan? Tenhou: no, never.
    pub kokushi_robs_ankan: bool,
    /// 形式聴牌 — a hand counts as tenpai even when every copy of its wait is
    /// already visible. Tenhou: yes.
    pub fifth_tile_wait_tenpai: bool,
    /// 九種九牌 abortive draw.
    pub abort_nine_terminals: bool,
    /// 四家立直 abortive draw.
    pub abort_four_riichi: bool,
    /// 四槓散了 abortive draw.
    pub abort_four_kans: bool,
    /// 四風連打 abortive draw.
    pub abort_four_winds: bool,
    /// 人和 value.
    pub renhou: RenhouValue,
    /// A double wind pair (round wind == seat wind) is worth 4 fu instead of 2.
    /// Tenhou: `true` — 4 fu, per the Tenhou manual (「連風牌は4符」), which is
    /// what `docs/RULES.md` §4.4 and the rules table record. Other rulesets use
    /// 2 fu; that is the `competitive()` preset.
    pub double_wind_pair_fu: bool,
    /// 責任払い applies to 大三元 and 大四喜 (the standard scope).
    pub pao: bool,
    /// Minimum number of live wall tiles required to declare riichi.
    pub min_riichi_wall: u8,
    /// Starting score for each player.
    pub starting_score: i32,
    /// Target score used for the final ranking conversion (not used in play).
    pub return_score: i32,
    /// ウマ for the final ranking: `[1st, 2nd, 3rd, 4th]` in points.
    pub uma: [i32; 4],
    /// Match length.
    pub length: GameLength,
    /// 西入り — after the last scheduled hand, continue into the West round when
    /// no player has reached [`Rules::return_score`].
    pub west_extension: bool,
    /// A win requires at least one yaku. Always true in standard mahjong; kept
    /// explicit because the engine must enforce it.
    pub require_yaku: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self::tenhou()
    }
}

impl Rules {
    /// Tenhou.net four-player standard rules.
    pub const fn tenhou() -> Self {
        Rules {
            aka_per_suit: 1,
            kuitan: true,
            atozuke: true,
            kiriage_mangan: false,
            double_yakuman: false,
            stack_yakuman: true,
            nagashi_mangan: true,
            agari_yame: true,
            tenpai_yame: true,
            tobi: true,
            kuikae: KuikaeScope::Forbidden,
            multi_ron: true,
            abort_triple_ron: true,
            kokushi_robs_ankan: false,
            fifth_tile_wait_tenpai: true,
            abort_nine_terminals: true,
            abort_four_riichi: true,
            abort_four_kans: true,
            abort_four_winds: true,
            renhou: RenhouValue::Off,
            double_wind_pair_fu: true,
            pao: true,
            min_riichi_wall: 4,
            starting_score: 25000,
            return_score: 30000,
            uma: [20, 10, -10, -20],
            length: GameLength::Hanchan,
            west_extension: true,
            require_yaku: true,
        }
    }

    /// A stricter competitive reading: no red fives, no 流し満貫, no
    /// アガリやめ, no 撃飛, all abortive draws on, 切り上げ満貫 off.
    pub const fn competitive() -> Self {
        Rules {
            aka_per_suit: 0,
            kuitan: true,
            atozuke: true,
            kiriage_mangan: false,
            double_yakuman: false,
            stack_yakuman: false,
            nagashi_mangan: false,
            agari_yame: false,
            tenpai_yame: false,
            tobi: false,
            kuikae: KuikaeScope::SameTileOnly,
            multi_ron: true,
            abort_triple_ron: false,
            kokushi_robs_ankan: true,
            fifth_tile_wait_tenpai: true,
            abort_nine_terminals: true,
            abort_four_riichi: true,
            abort_four_kans: true,
            abort_four_winds: true,
            renhou: RenhouValue::Mangan,
            double_wind_pair_fu: false,
            pao: true,
            min_riichi_wall: 4,
            starting_score: 25000,
            return_score: 30000,
            uma: [20, 10, -10, -20],
            length: GameLength::Hanchan,
            west_extension: false,
            require_yaku: true,
        }
    }

    /// Build a ruleset from a preset.
    pub const fn preset(preset: Ruleset) -> Self {
        match preset {
            Ruleset::Tenhou => Self::tenhou(),
            Ruleset::Competitive => Self::competitive(),
        }
    }

    /// Single round for quick self-play and tests.
    pub fn single_round(mut self) -> Self {
        self.length = GameLength::Tonpuu;
        self.agari_yame = false;
        self.tobi = false;
        self
    }

    /// Is `tile` a red five under this ruleset?
    #[inline]
    pub fn is_aka(&self, tile: crate::tile::Tile) -> bool {
        self.aka_per_suit > 0 && crate::tile::is_aka_tile(tile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::{Tile, tile_of};

    #[test]
    fn presets_are_consistent() {
        let t = Rules::tenhou();
        assert_eq!(t.aka_per_suit, 1);
        assert!(!t.kiriage_mangan);
        assert_eq!(t.kuikae, KuikaeScope::Forbidden);
        assert!(t.multi_ron);
        assert_eq!(t.uma.iter().sum::<i32>(), 0);
        let c = Rules::competitive();
        assert_eq!(c.aka_per_suit, 0);
        assert!(!c.is_aka(tile_of(4, 0) as Tile));
        assert!(t.is_aka(tile_of(4, 0)));
        assert!(!t.is_aka(tile_of(4, 1)));
    }
}
