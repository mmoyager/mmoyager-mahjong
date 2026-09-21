//! Called melds (副露).

use crate::tile::{Kind, Tile, kind_of};
use serde::{Deserialize, Serialize};

/// The five ways a set can be exposed or concealed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeldKind {
    /// チー — a run completed with the tile discarded by the player on the left.
    Chi,
    /// ポン — a triplet completed with any player's discard.
    Pon,
    /// 暗槓 — four concealed tiles of the same kind.
    Ankan,
    /// 大明槓 — an open quad completed with a discard.
    Minkan,
    /// 加槓 — a quad added to an existing ポン.
    Kakan,
}

impl MeldKind {
    /// Is the set exposed to the other players (and therefore 喰い下がり)?
    pub const fn is_open(self) -> bool {
        !matches!(self, MeldKind::Ankan)
    }

    /// Does the meld consist of three tiles?
    pub const fn is_triplet(self) -> bool {
        !matches!(self, MeldKind::Chi)
    }

    /// Is this a quad?
    pub const fn is_kan(self) -> bool {
        matches!(
            self,
            MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan
        )
    }

    /// Number of tiles in the meld.
    pub const fn len(self) -> u8 {
        if self.is_kan() { 4 } else { 3 }
    }

    pub const fn name(self) -> &'static str {
        match self {
            MeldKind::Chi => "chi",
            MeldKind::Pon => "pon",
            MeldKind::Ankan => "ankan",
            MeldKind::Minkan => "minkan",
            MeldKind::Kakan => "kakan",
        }
    }
}

/// A called set. `tiles` is always sorted ascending; `len` says how many entries
/// are meaningful (3 for chi/pon, 4 for the quads).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meld {
    pub kind: MeldKind,
    pub tiles: [Tile; 4],
    pub len: u8,
    /// Seat the called tile came from; the melder's own seat for 暗槓.
    pub from: u8,
    /// The tile that was taken from a discard (the added tile for 加槓).
    pub called: Tile,
    /// 加槓 only: the seat the ポン it was added to came from.
    ///
    /// `from` cannot carry this — a 加槓's fourth tile is self-drawn, so `from`
    /// is the melder — but where the ポン came from stays public, and on a real
    /// table the sideways tile that records it is never moved. Kept in its own
    /// field rather than folded into `from` so that everything already reading
    /// `from` (責任払い, the replay's canonical form) keeps its meaning.
    #[serde(default)]
    pub pon_from: Option<u8>,
}

impl Meld {
    /// Sort only the meaningful slots. `tiles` is a fixed 4-slot array whose
    /// tail is zero-filled for three-tile melds; sorting the padding too would
    /// push a phantom `1m` (tile id 0) into the meld.
    fn normalize(mut tiles: [Tile; 4], len: usize) -> [Tile; 4] {
        tiles[..len].sort_unstable();
        tiles
    }

    /// A run. `tiles` are the three physical tiles, `called` the one that came
    /// from the discard, `from` that discarder's seat.
    pub fn chi(tiles: [Tile; 3], called: Tile, from: u8) -> Self {
        let arr = Self::normalize([tiles[0], tiles[1], tiles[2], 0], 3);
        Meld {
            kind: MeldKind::Chi,
            tiles: arr,
            len: 3,
            from,
            called,
            pon_from: None,
        }
    }

    /// A triplet.
    pub fn pon(tiles: [Tile; 3], called: Tile, from: u8) -> Self {
        let arr = Self::normalize([tiles[0], tiles[1], tiles[2], 0], 3);
        Meld {
            kind: MeldKind::Pon,
            tiles: arr,
            len: 3,
            from,
            called,
            pon_from: None,
        }
    }

    /// Any quad.
    pub fn kan(kind: MeldKind, tiles: [Tile; 4], called: Tile, from: u8) -> Self {
        debug_assert!(kind.is_kan());
        let arr = Self::normalize(tiles, 4);
        Meld {
            kind,
            tiles: arr,
            len: 4,
            from,
            called,
            pon_from: None,
        }
    }

    /// Record where the ポン behind a 加槓 came from; see `pon_from`.
    pub fn with_pon_from(mut self, seat: u8) -> Self {
        self.pon_from = Some(seat);
        self
    }

    /// The physical tiles of this meld.
    #[inline]
    pub fn as_slice(&self) -> &[Tile] {
        &self.tiles[..self.len as usize]
    }

    /// The tile kind of a triplet/quad meld (the shared kind).
    pub fn triplet_kind(&self) -> Option<Kind> {
        if self.kind.is_triplet() {
            Some(kind_of(self.tiles[0]))
        } else {
            None
        }
    }

    /// The lowest tile kind of a run.
    pub fn run_start(&self) -> Option<Kind> {
        if self.kind == MeldKind::Chi {
            Some(kind_of(self.tiles[0]))
        } else {
            None
        }
    }

    /// How many tiles of the meld are red fives, under `rules`.
    pub fn aka_count(&self, rules: &crate::rules::Rules) -> u8 {
        self.as_slice()
            .iter()
            .filter(|&&t| rules.is_aka(t))
            .count() as u8
    }

    /// Was this meld made from another player's discard?
    pub fn is_called_from_other(&self) -> bool {
        self.kind.is_open() && self.kind != MeldKind::Kakan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::tile_of;

    #[test]
    fn chi_is_sorted_and_identifies_its_run() {
        let m = Meld::chi(
            [tile_of(2, 0), tile_of(0, 0), tile_of(1, 0)],
            tile_of(1, 0),
            3,
        );
        assert_eq!(m.as_slice().len(), 3);
        assert_eq!(m.run_start(), Some(0));
        assert!(m.kind.is_open());
        assert!(!m.kind.is_triplet());
    }

    #[test]
    fn three_tile_melds_keep_exactly_three_real_tiles() {
        // Tile id 0 is `1m`; a meld of 5m tiles must never contain it.
        let m = Meld::chi(
            [tile_of(4, 2), tile_of(3, 0), tile_of(2, 1)],
            tile_of(2, 1),
            3,
        );
        assert_eq!(m.as_slice().len(), 3);
        assert!(m.as_slice().iter().all(|&t| t != 0));
        let kinds: Vec<Kind> = m.as_slice().iter().map(|&t| kind_of(t)).collect();
        assert_eq!(kinds, vec![2, 3, 4]);
        assert_eq!(m.run_start(), Some(2));

        let p = Meld::pon(
            [tile_of(4, 0), tile_of(4, 1), tile_of(4, 2)],
            tile_of(4, 2),
            1,
        );
        assert!(p.as_slice().iter().all(|&t| t != 0));
        assert_eq!(p.triplet_kind(), Some(4));
        assert_eq!(p.called, tile_of(4, 2));

        // A meld of 1m legitimately contains tile id 0, so check the count too.
        let p1 = Meld::pon(
            [tile_of(0, 0), tile_of(0, 1), tile_of(0, 2)],
            tile_of(0, 0),
            2,
        );
        assert_eq!(p1.as_slice().iter().filter(|&&t| t == 0).count(), 1);
        assert_eq!(p1.triplet_kind(), Some(0));
    }

    #[test]
    fn kan_lengths_and_openness() {
        assert!(MeldKind::Ankan.is_kan());
        assert!(!MeldKind::Ankan.is_open());
        assert!(MeldKind::Minkan.is_open());
        assert!(MeldKind::Kakan.is_open());
        assert_eq!(MeldKind::Ankan.len(), 4);
        assert_eq!(MeldKind::Pon.len(), 3);
        let m = Meld::kan(
            MeldKind::Ankan,
            [tile_of(4, 0), tile_of(4, 1), tile_of(4, 2), tile_of(4, 3)],
            tile_of(4, 0),
            0,
        );
        assert_eq!(m.triplet_kind(), Some(4));
        assert_eq!(m.aka_count(&crate::rules::Rules::tenhou()), 1);
    }
}
