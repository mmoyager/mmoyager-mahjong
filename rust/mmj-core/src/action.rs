//! The action space shared by the engine, the bots, the UI and the network.

use crate::meld::{Meld, MeldKind};
use crate::tile::{Kind, Tile, kind_of};
use serde::{Deserialize, Serialize};

/// Size of the flat action space used by the policy network.
///
/// | range      | meaning                                                        |
/// |------------|----------------------------------------------------------------|
/// | `0..34`    | discard tile kind (no riichi)                                   |
/// | `34..68`   | discard tile kind **and** declare riichi                        |
/// | `68..102`  | declare a kan of that tile kind (暗槓 or 加槓, whichever applies) |
/// | `102`      | 自摸 (tsumo)                                                    |
/// | `103`      | 栄和 (ron) — also used for 九種九牌, they never co-occur         |
/// | `104`      | pass / decline                                                  |
/// | `105..108` | チー variants, indexed by the position of the called tile in the run |
/// | `108`      | ポン                                                             |
pub const ACTION_SPACE: usize = 109;

/// Coarse classification of an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    Discard,
    Riichi,
    Chi,
    Pon,
    Kan,
    Tsumo,
    Ron,
    Pass,
    Kyuushu,
}

/// A concrete legal action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Discard `tile`; when `riichi` is set the same action also declares riichi.
    Discard { tile: Tile, riichi: bool },
    /// Call a meld. For チー/ポン/大明槓 the source seat and called tile are in
    /// the [`Meld`]; for 暗槓/加槓 the meld refers to the player's own hand.
    Meld { meld: Meld },
    /// 自摸 — win on the drawn tile.
    Tsumo,
    /// 栄和 — win on another player's discard.
    Ron,
    /// Declare 九種九牌.
    Kyuushu,
    /// Decline the current opportunity.
    Pass,
}

impl Action {
    pub fn kind(&self) -> ActionKind {
        match self {
            Action::Discard { riichi: false, .. } => ActionKind::Discard,
            Action::Discard { riichi: true, .. } => ActionKind::Riichi,
            Action::Meld { meld } => match meld.kind {
                MeldKind::Chi => ActionKind::Chi,
                MeldKind::Pon => ActionKind::Pon,
                MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan => ActionKind::Kan,
            },
            Action::Tsumo => ActionKind::Tsumo,
            Action::Ron => ActionKind::Ron,
            Action::Kyuushu => ActionKind::Kyuushu,
            Action::Pass => ActionKind::Pass,
        }
    }

    /// Slot in the flat policy action space, or `None` for actions that are not
    /// part of it (there are none today, but the signature keeps callers honest).
    pub fn index(&self) -> Option<usize> {
        Some(match self {
            Action::Discard { tile, riichi: false } => kind_of(*tile) as usize,
            Action::Discard { tile, riichi: true } => 34 + kind_of(*tile) as usize,
            Action::Meld { meld } => match meld.kind {
                MeldKind::Chi => {
                    let start = meld.run_start()?;
                    let called = kind_of(meld.called);
                    105 + (called - start) as usize
                }
                MeldKind::Pon => 108,
                MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan => {
                    68 + meld.triplet_kind()? as usize
                }
            },
            Action::Tsumo => 102,
            Action::Ron => 103,
            Action::Kyuushu => 103,
            Action::Pass => 104,
        })
    }

    /// Is this action a claim on another player's tile?
    pub fn is_call(&self) -> bool {
        matches!(
            self.kind(),
            ActionKind::Chi | ActionKind::Pon | ActionKind::Kan | ActionKind::Ron
        )
    }

    pub fn is_discard(&self) -> bool {
        matches!(self, Action::Discard { .. })
    }

    /// Short label used in logs and the UI.
    pub fn label(&self) -> String {
        match self {
            Action::Discard { tile, riichi } => {
                let t = crate::tile::tile_name(*tile);
                if *riichi {
                    format!("riichi+{}", t)
                } else {
                    t
                }
            }
            Action::Meld { meld } => {
                let tiles: Vec<String> = meld.as_slice().iter().map(|&t| crate::tile::tile_name(t)).collect();
                format!("{}{}", meld.kind.name(), tiles.join(""))
            }
            Action::Tsumo => "tsumo".to_string(),
            Action::Ron => "ron".to_string(),
            Action::Kyuushu => "kyuushu".to_string(),
            Action::Pass => "pass".to_string(),
        }
    }
}

/// Map a flat action slot to a *kind-level* description. Used by the network-side
/// code to build action features without needing an actual physical tile.
pub fn slot_kind(slot: usize) -> SlotKind {
    match slot {
        0..=33 => SlotKind::Discard(slot as Kind),
        34..=67 => SlotKind::RiichiDiscard((slot - 34) as Kind),
        68..=101 => SlotKind::Kan((slot - 68) as Kind),
        102 => SlotKind::Tsumo,
        103 => SlotKind::Ron,
        104 => SlotKind::Pass,
        105..=107 => SlotKind::Chi((slot - 105) as u8),
        _ => SlotKind::Pon,
    }
}

/// Kind-level meaning of a flat action slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Discard(Kind),
    RiichiDiscard(Kind),
    Kan(Kind),
    Tsumo,
    Ron,
    Pass,
    /// Offset of the called tile inside the run (`0` = low end).
    Chi(u8),
    Pon,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::tile_of;

    #[test]
    fn slots_are_unique_per_decision_type() {
        let d = Action::Discard {
            tile: tile_of(4, 0),
            riichi: false,
        };
        assert_eq!(d.index(), Some(4));
        let r = Action::Discard {
            tile: tile_of(4, 0),
            riichi: true,
        };
        assert_eq!(r.index(), Some(38));
        assert_eq!(Action::Tsumo.index(), Some(102));
        assert_eq!(Action::Ron.index(), Some(103));
        assert_eq!(Action::Pass.index(), Some(104));
        let chi = Action::Meld {
            meld: Meld::chi(
                [tile_of(2, 0), tile_of(3, 0), tile_of(1, 0)],
                tile_of(1, 0),
                3,
            ),
        };
        // tiles 3m/4m/2m form the run 2m3m4m with the called tile 2m: the
        // lowest tile of the run *is* the called tile, so this is variant 0.
        assert_eq!(chi.index(), Some(105));
        let pon = Action::Meld {
            meld: Meld::pon([tile_of(4, 0), tile_of(4, 1), tile_of(4, 2)], tile_of(4, 2), 1),
        };
        assert_eq!(pon.index(), Some(108));
        let ankan = Action::Meld {
            meld: Meld::kan(
                MeldKind::Ankan,
                [tile_of(31, 0), tile_of(31, 1), tile_of(31, 2), tile_of(31, 3)],
                tile_of(31, 0),
                0,
            ),
        };
        assert_eq!(ankan.index(), Some(68 + 31));
        assert!(ankan.index().unwrap() < ACTION_SPACE);
    }

    #[test]
    fn slot_kind_roundtrip() {
        assert_eq!(slot_kind(4), SlotKind::Discard(4));
        assert_eq!(slot_kind(38), SlotKind::RiichiDiscard(4));
        assert_eq!(slot_kind(68 + 31), SlotKind::Kan(31));
        assert_eq!(slot_kind(102), SlotKind::Tsumo);
        assert_eq!(slot_kind(107), SlotKind::Chi(2));
        assert_eq!(slot_kind(108), SlotKind::Pon);
    }
}
