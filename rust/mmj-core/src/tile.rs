//! Tile representation for Japanese (riichi) mahjong.
//!
//! Two levels are used throughout the engine:
//!
//! * [`Kind`] — one of the 34 tile *kinds* (`1m..9m`, `1p..9p`, `1s..9s`,
//!   `E S W N haku hatsu chun`). All shape logic (shanten, yaku, fu) works on
//!   kind counts.
//! * [`Tile`] — one of the 136 *physical* tiles. A physical tile is `kind * 4 +
//!   copy`, so `tile / 4` is its kind. Physical identity matters for red fives
//!   (赤ドラ) and for reproducing shuffles.
//!
//! # Red fives
//!
//! Following the de-facto standard (Tenhou.net), copy `0` of `5m`, `5p` and
//! `5s` is the red five. The wall builder in [`crate::wall`] honours
//! [`crate::rules::Rules::aka`], so a ruleset with no red fives simply has no
//! red copies in play.

use std::fmt;

/// Number of distinct tile kinds.
pub const NUM_KINDS: usize = 34;
/// Number of physical tiles in a 4-player game.
pub const NUM_TILES: u8 = 136;

/// A tile kind in `0..34`.
///
/// * `0..=8`   — `1m`..`9m`
/// * `9..=17`  — `1p`..`9p`
/// * `18..=26` — `1s`..`9s`
/// * `27..=33` — east, south, west, north, haku, hatsu, chun
pub type Kind = u8;

/// A physical tile in `0..136`; its kind is `tile / 4`.
pub type Tile = u8;

pub const EAST: Kind = 27;
pub const SOUTH: Kind = 28;
pub const WEST: Kind = 29;
pub const NORTH: Kind = 30;
pub const HAKU: Kind = 31;
pub const HATSU: Kind = 32;
pub const CHUN: Kind = 33;

/// First kind of each suit; `SUIT_BASE[suit]`.
pub const SUIT_BASE: [Kind; 4] = [0, 9, 18, 27];

/// Human-readable names, indexed by kind. Red fives are handled by the
/// formatter and are printed as `0m` / `0p` / `0s`.
pub const KIND_NAMES: [&str; NUM_KINDS] = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p",
    "8p", "9p", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s", "E", "S", "W", "N", "P", "F",
    "C",
];

/// Kind of a physical tile.
#[inline(always)]
pub const fn kind_of(tile: Tile) -> Kind {
    tile >> 2
}

/// Physical tile built from a kind and a copy index in `0..4`.
#[inline(always)]
pub const fn tile_of(kind: Kind, copy: u8) -> Tile {
    kind * 4 + copy
}

/// Red-five kinds, in suit order (`5m`, `5p`, `5s`).
pub const AKA_KINDS: [Kind; 3] = [4, 13, 22];

/// Is `tile` one of the three standard red fives (`copy == 0` of `5m/5p/5s`)?
#[inline(always)]
pub const fn is_aka_tile(tile: Tile) -> bool {
    tile == 16 || tile == 52 || tile == 88
}

/// Does the *kind* have a red copy in the standard 3-red-five set?
#[inline(always)]
pub const fn kind_has_aka(kind: Kind) -> bool {
    kind == 4 || kind == 13 || kind == 22
}

/// Suit index: `0` man, `1` pin, `2` sou, `3` honors.
#[inline(always)]
pub const fn suit_of(kind: Kind) -> usize {
    (kind / 9) as usize
}

/// `true` for the 13 terminal/honor kinds (幺九牌), i.e. `1m 9m 1p 9p 1s 9s` + 7 honors.
#[inline(always)]
pub const fn is_yaochu(kind: Kind) -> bool {
    if kind >= EAST {
        return true;
    }
    let n = kind % 9;
    n == 0 || n == 8
}

/// One of the 13 orphan kinds that 国士無双 and 九種九牌 care about.
#[inline(always)]
pub const fn is_terminal(kind: Kind) -> bool {
    kind < EAST && (kind % 9 == 0 || kind % 9 == 8)
}

/// `true` for the 21 "simple" kinds (中張牌), i.e. 2-8 of each number suit.
#[inline(always)]
pub const fn is_simple(kind: Kind) -> bool {
    kind < EAST && {
        let n = kind % 9;
        n != 0 && n != 8
    }
}

/// Is this one of the 7 honor kinds?
#[inline(always)]
pub const fn is_honor(kind: Kind) -> bool {
    kind >= EAST
}

/// Is this one of the 3 dragon kinds (三元牌: haku, hatsu, chun)?
#[inline(always)]
pub const fn is_dragon(kind: Kind) -> bool {
    kind >= HAKU
}

/// Is this one of the 4 wind kinds?
#[inline(always)]
pub const fn is_wind(kind: Kind) -> bool {
    kind >= EAST && kind <= NORTH
}

/// `true` for the tiles that can be part of 緑一色 (all-green).
#[inline(always)]
pub const fn is_green(kind: Kind) -> bool {
    matches!(kind, 19 | 20 | 21 | 23 | 24 | HATSU)
}

/// Number of kinds a suit contains (9 for number suits, 7 for honors).
#[inline(always)]
pub const fn suit_len(suit: usize) -> usize {
    if suit == 3 {
        7
    } else {
        9
    }
}

/// The dora indicated by `indicator`.
///
/// Number suits wrap `9 -> 1`; winds wrap `N -> E`; dragons wrap `chun -> haku`.
#[inline(always)]
pub const fn dora_from_indicator(indicator: Kind) -> Kind {
    match indicator {
        0..=26 => {
            let base = (indicator / 9) * 9;
            let n = indicator % 9;
            if n == 8 {
                base
            } else {
                base + n + 1
            }
        }
        EAST => SOUTH,
        SOUTH => WEST,
        WEST => NORTH,
        NORTH => EAST,
        HAKU => HATSU,
        HATSU => CHUN,
        _ => HAKU,
    }
}

/// Wind order helper: `EAST -> SOUTH -> WEST -> NORTH -> EAST`.
#[inline(always)]
pub const fn seat_next(seat: u8) -> u8 {
    (seat + 1) % 4
}

/// Seat offset relative to `from`, in `0..4`: `0` means "me", `1` the next player, etc.
#[inline(always)]
pub const fn seat_diff(from: u8, to: u8) -> u8 {
    (to + 4 - from) % 4
}

/// Format a single kind for display.
pub fn kind_name(kind: Kind) -> &'static str {
    KIND_NAMES[kind as usize % NUM_KINDS]
}

/// Format a physical tile, rendering red fives as `0m` / `0p` / `0s`.
pub fn tile_name(tile: Tile) -> String {
    let kind = kind_of(tile);
    if is_aka_tile(tile) {
        format!("0{}", ["m", "p", "s"][suit_of(kind)])
    } else {
        kind_name(kind).to_string()
    }
}

/// A physical tile wrapper with `Display`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TileDisplay(pub Tile);

impl fmt::Debug for TileDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&tile_name(self.0))
    }
}

impl fmt::Display for TileDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&tile_name(self.0))
    }
}

/// Parse a single tile token such as `1m`, `0p` (red five), `E`, `P`, `F`, `C`.
///
/// Accepted honor spellings: `E/S/W/N` (east/south/west/north), `P/H` (haku),
/// `F/G` (hatsu), `C/R` (chun), and the numeric forms `1z`..`7z`.
pub fn parse_kind(token: &str) -> Option<Kind> {
    let token = token.trim();
    let bytes = token.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let first = bytes[0];
    if bytes.len() == 1 {
        return match first.to_ascii_uppercase() {
            b'E' => Some(EAST),
            b'S' => Some(SOUTH),
            b'W' => Some(WEST),
            b'N' => Some(NORTH),
            b'P' | b'H' => Some(HAKU),
            b'F' | b'G' => Some(HATSU),
            b'C' | b'R' => Some(CHUN),
            _ => None,
        };
    }
    if bytes.len() != 2 {
        return None;
    }
    let suit = match bytes[1].to_ascii_lowercase() {
        b'm' => 0usize,
        b'p' => 1,
        b's' => 2,
        b'z' => 3,
        _ => return None,
    };
    let num = (first as char).to_digit(10)?;
    if suit == 3 {
        // 1z..7z map onto the 7 honor kinds.
        return if (1..=7).contains(&num) {
            Some(EAST + (num as Kind - 1))
        } else {
            None
        };
    }
    // `0m` / `0p` / `0s` is a red five, i.e. kind 5 of that suit.
    let n = if num == 0 { 5 } else { num };
    if !(1..=9).contains(&n) {
        return None;
    }
    Some((suit as Kind) * 9 + (n as Kind - 1))
}

/// Parse a compact hand string such as `123m456p789s11z` or `1m 2m 3m`.
///
/// The `0m`/`0p`/`0s` forms are accepted and map to the corresponding five.
pub fn parse_kinds(s: &str) -> Vec<Kind> {
    let mut out = Vec::new();
    let mut digits: Vec<u8> = Vec::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch as u8 - b'0');
            continue;
        }
        if ch.is_whitespace() || ch == ',' {
            continue;
        }
        let suit = match ch.to_ascii_lowercase() {
            'm' => 0u8,
            'p' => 1,
            's' => 2,
            'z' => 3,
            _ => {
                // Non-suit letter: treat the character as a standalone honor.
                digits.clear();
                if let Some(k) = parse_kind(&ch.to_string()) {
                    out.push(k);
                }
                continue;
            }
        };
        for &d in &digits {
            if suit == 3 {
                if (1..=7).contains(&d) {
                    out.push(EAST + (d - 1));
                }
            } else {
                let n = if d == 0 { 5 } else { d };
                if (1..=9).contains(&n) {
                    out.push(suit * 9 + (n - 1));
                }
            }
        }
        digits.clear();
    }
    out
}

/// Count kinds into a 34-slot count array.
pub fn counts_from_kinds(kinds: &[Kind]) -> [u8; NUM_KINDS] {
    let mut c = [0u8; NUM_KINDS];
    for &k in kinds {
        c[k as usize] += 1;
    }
    c
}

/// Render a count array as a sorted string like `123m456p11z`.
pub fn counts_to_string(counts: &[u8; NUM_KINDS]) -> String {
    let mut out = String::new();
    for suit in 0..4 {
        let mut buf = String::new();
        for n in 0..suit_len(suit) {
            let kind = SUIT_BASE[suit] + n as Kind;
            for _ in 0..counts[kind as usize] {
                buf.push((b'1' + n as u8) as char);
            }
        }
        if !buf.is_empty() {
            out.push_str(&buf);
            out.push(if suit == 3 {
                'z'
            } else {
                ['m', 'p', 's'][suit]
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_geometry() {
        assert_eq!(parse_kind("1m"), Some(0));
        assert_eq!(parse_kind("9m"), Some(8));
        assert_eq!(parse_kind("1p"), Some(9));
        assert_eq!(parse_kind("0s"), Some(22));
        assert_eq!(parse_kind("5s"), Some(22));
        assert_eq!(parse_kind("E"), Some(EAST));
        assert_eq!(parse_kind("7z"), Some(CHUN));
        assert_eq!(parse_kind("x"), None);
        assert!(is_terminal(0) && is_terminal(8) && is_terminal(9));
        assert!(!is_terminal(4));
        assert!(is_yaochu(EAST) && is_yaochu(CHUN) && is_yaochu(17));
        assert!(is_simple(4) && is_simple(13));
        assert!(!is_simple(8) && !is_simple(HAKU));
        assert!(is_dragon(HAKU) && is_dragon(CHUN) && !is_dragon(NORTH));
        assert!(is_wind(NORTH) && !is_wind(HAKU));
        assert_eq!(suit_of(0), 0);
        assert_eq!(suit_of(26), 2);
        assert_eq!(suit_of(33), 3);
    }

    #[test]
    fn dora_wrapping() {
        assert_eq!(dora_from_indicator(0), 1); // 1m -> 2m
        assert_eq!(dora_from_indicator(8), 0); // 9m -> 1m
        assert_eq!(dora_from_indicator(17), 9); // 9p -> 1p
        assert_eq!(dora_from_indicator(NORTH), EAST);
        assert_eq!(dora_from_indicator(CHUN), HAKU);
        assert_eq!(dora_from_indicator(HAKU), HATSU);
    }

    #[test]
    fn parsing_roundtrip() {
        let kinds = parse_kinds("123m456p789s11z");
        assert_eq!(kinds.len(), 3 + 3 + 3 + 2);
        let c = counts_from_kinds(&kinds);
        assert_eq!(c[0], 1);
        assert_eq!(c[4], 0);
        assert_eq!(counts_to_string(&c), "123m456p789s11z");
        // Red five parses to the five of its suit.
        let red = parse_kinds("0m");
        assert_eq!(red, vec![4]);
    }

    #[test]
    fn aka_tiles() {
        assert!(is_aka_tile(tile_of(4, 0)));
        assert!(!is_aka_tile(tile_of(4, 1)));
        assert!(is_aka_tile(tile_of(13, 0)));
        assert!(is_aka_tile(tile_of(22, 0)));
        assert_eq!(tile_name(tile_of(4, 0)), "0m");
        assert_eq!(tile_name(tile_of(4, 2)), "5m");
    }
}
