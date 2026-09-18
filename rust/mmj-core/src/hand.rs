//! Hand shape analysis: winning shape, shanten (向听数), waits (待ち) and tile
//! acceptance (有効牌 / 受け入れ).
//!
//! # Shanten
//!
//! The engine uses the classic block formula
//!
//! ```text
//! shanten = 8 - 2 * sets - partials - head
//! ```
//!
//! subject to `sets + partials <= 4` and `head <= 1`, where
//!
//! * `sets` counts complete melds (including called melds),
//! * `partials` counts two-tile blocks that need one more tile (ryanmen,
//!   kanchan, penchan, and *pairs used as a stepping stone*, 対子),
//! * `head` is `1` when a pair is reserved as the final pair.
//!
//! The formula is evaluated over *every* decomposition of the hand, so a
//! sequence such as `123p` + `234p` in `1122334p` is found even though a greedy
//! scan would stop at the first match.
//!
//! Seven pairs and thirteen orphans are evaluated separately and only apply to
//! fully concealed hands.
//!
//! A complete 14-tile hand yields `-1`; a tenpai hand yields `0`.

use crate::tile::{Kind, NUM_KINDS, SUIT_BASE};

/// A count array indexed by [`Kind`].
pub type Counts = [u8; NUM_KINDS];

/// Number of melds required for a standard winning hand.
const SETS_NEEDED: u8 = 4;

/// Encode a block profile into a compact 50-bit mask.
///
/// `bit = (sets * 5 + partials) * 2 + head`
#[inline(always)]
const fn pbit(sets: u8, partials: u8, head: u8) -> u64 {
    1u64 << ((sets as u32 * 5 + partials as u32) * 2 + head as u32)
}

/// Total number of tiles in a count array.
#[inline]
pub fn total(counts: &Counts) -> u32 {
    counts.iter().map(|&c| c as u32).sum()
}

/// Total number of red fives in a concealed count array (informational only; the
/// authoritative count lives on the physical tiles in [`crate::state`]).
#[inline]
pub fn is_chiitoitsu(counts: &Counts) -> bool {
    let mut pairs = 0;
    for &c in counts.iter() {
        match c {
            0 | 1 => {}
            2 => pairs += 1,
            _ => return false,
        }
    }
    pairs == 7
}

/// Is this the thirteen orphans shape (国士無双)?
pub fn is_kokushi(counts: &Counts) -> bool {
    let mut pairs = 0;
    let mut kinds = 0;
    for (k, &c) in counts.iter().enumerate() {
        if c == 0 {
            continue;
        }
        if !crate::tile::is_yaochu(k as Kind) {
            return false;
        }
        kinds += 1;
        match c {
            1 => {}
            2 => pairs += 1,
            _ => return false,
        }
    }
    kinds == 13 && pairs == 1
}

/// Can the remaining counts be split into exactly `need` complete sets?
///
/// This uses the standard "lowest remaining tile must start a triplet or a
/// sequence" argument, so the search is complete and branch-free apart from the
/// two legal choices.
fn all_sets(c: &mut Counts, need: u8) -> bool {
    if need == 0 {
        return c.iter().all(|&x| x == 0);
    }
    let i = match c.iter().position(|&x| x > 0) {
        Some(i) => i,
        None => return false,
    };
    if c[i] >= 3 {
        c[i] -= 3;
        let ok = all_sets(c, need - 1);
        c[i] += 3;
        if ok {
            return true;
        }
    }
    // Sequences only exist inside the three number suits.
    if i < 27 && i % 9 <= 6 && c[i + 1] > 0 && c[i + 2] > 0 {
        c[i] -= 1;
        c[i + 1] -= 1;
        c[i + 2] -= 1;
        let ok = all_sets(c, need - 1);
        c[i] += 1;
        c[i + 1] += 1;
        c[i + 2] += 1;
        if ok {
            return true;
        }
    }
    false
}

/// Is `counts` a complete winning shape, given `melds` called melds?
///
/// `counts` must hold `14 - 3 * melds` tiles.
pub fn is_agari(counts: &Counts, melds: u8) -> bool {
    if melds > SETS_NEEDED {
        return false;
    }
    if melds == 0 && (is_chiitoitsu(counts) || is_kokushi(counts)) {
        return true;
    }
    let need = SETS_NEEDED - melds;
    if total(counts) != 3 * need as u32 + 2 {
        return false;
    }
    let mut c = *counts;
    for p in 0..NUM_KINDS {
        if c[p] >= 2 {
            c[p] -= 2;
            let ok = all_sets(&mut c, need);
            c[p] += 2;
            if ok {
                return true;
            }
        }
    }
    false
}

/// All tile kinds that complete `counts` into a winning hand.
///
/// `counts` holds `13 - 3 * melds` tiles; the returned kinds are the waits.
pub fn winning_kinds(counts: &Counts, melds: u8) -> Vec<Kind> {
    let mut out = Vec::with_capacity(8);
    let mut c = *counts;
    for k in 0..NUM_KINDS {
        if c[k] >= 4 {
            continue; // no fifth copy exists
        }
        c[k] += 1;
        if is_agari(&c, melds) {
            out.push(k as Kind);
        }
        c[k] -= 1;
    }
    out
}

/// Waits that ignore tile availability entirely (形式聴牌 / 5 枚目を待つ聴牌).
///
/// The standard exhaustive-draw check and the 立直 tenpai check ask "does a
/// completing tile exist for this shape", not "is one still in the wall".
pub fn tenpai_kinds(counts: &Counts, melds: u8) -> Vec<Kind> {
    let mut out = Vec::with_capacity(8);
    let mut c = *counts;
    for k in 0..NUM_KINDS {
        c[k] += 1;
        if is_agari(&c, melds) {
            out.push(k as Kind);
        }
        c[k] -= 1;
    }
    out
}

/// Is the hand tenpai? Shape only — availability of the wait is ignored, which
/// is the standard rule for the exhaustive-draw tenpai check.
pub fn is_tenpai(counts: &Counts, melds: u8) -> bool {
    !tenpai_kinds(counts, melds).is_empty()
}

// ---------------------------------------------------------------------------
// Block profiles
// ---------------------------------------------------------------------------

/// Base-6 encoding of a 9-slot suit count array, used as a cache key.
///
/// The base has to be 6, not 5, because the *shape* queries deliberately look
/// one tile past what a hand can hold: `tenpai_kinds` adds a fifth copy to ask
/// whether a shape is a wait at all (形式聴牌), and a base-5 key silently folds a
/// digit of 5 into the next place — `[5,0,…]` and `[0,1,…]` collide on the same
/// key, so one of them would be answered with the other's cached profile. A
/// count never exceeds 5 (a six-copy hand is impossible), so base 6 is exact.
#[inline]
fn suit_key(c: &[u8; 9]) -> u32 {
    let mut key = 0u32;
    for i in (0..9).rev() {
        key = key * 6 + c[i] as u32;
    }
    key
}

fn suit_rec(c: &mut [u8; 9], i: usize, sets: u8, partials: u8, head: u8, out: &mut u64) {
    if sets + partials > SETS_NEEDED {
        return;
    }
    let mut i = i;
    while i < 9 && c[i] == 0 {
        i += 1;
    }
    if i == 9 {
        *out |= pbit(sets, partials, head);
        return;
    }
    if c[i] >= 3 {
        c[i] -= 3;
        suit_rec(c, i, sets + 1, partials, head, out);
        c[i] += 3;
    }
    if i + 2 < 9 && c[i + 1] > 0 && c[i + 2] > 0 {
        c[i] -= 1;
        c[i + 1] -= 1;
        c[i + 2] -= 1;
        suit_rec(c, i, sets + 1, partials, head, out);
        c[i] += 1;
        c[i + 1] += 1;
        c[i + 2] += 1;
    }
    if c[i] >= 2 {
        c[i] -= 2;
        suit_rec(c, i, sets, partials + 1, head, out);
        if head == 0 {
            suit_rec(c, i, sets, partials, 1, out);
        }
        c[i] += 2;
    }
    // Ryanmen / kanchan using the next tile kind.
    if i + 1 < 9 && c[i + 1] > 0 {
        c[i] -= 1;
        c[i + 1] -= 1;
        suit_rec(c, i, sets, partials + 1, head, out);
        c[i] += 1;
        c[i + 1] += 1;
    }
    // Kanchan spanning one tile.
    if i + 2 < 9 && c[i + 2] > 0 {
        c[i] -= 1;
        c[i + 2] -= 1;
        suit_rec(c, i, sets, partials + 1, head, out);
        c[i] += 1;
        c[i + 2] += 1;
    }
    // Everything left at this kind is a floater: floaters never help, so all of
    // them can be dropped at once. This keeps the search small.
    let saved = c[i];
    c[i] = 0;
    suit_rec(c, i, sets, partials, head, out);
    c[i] = saved;
}

fn honors_rec(c: &mut [u8; 7], i: usize, sets: u8, partials: u8, head: u8, out: &mut u64) {
    if sets + partials > SETS_NEEDED {
        return;
    }
    let mut i = i;
    while i < 7 && c[i] == 0 {
        i += 1;
    }
    if i == 7 {
        *out |= pbit(sets, partials, head);
        return;
    }
    if c[i] >= 3 {
        c[i] -= 3;
        honors_rec(c, i, sets + 1, partials, head, out);
        c[i] += 3;
    }
    if c[i] >= 2 {
        c[i] -= 2;
        honors_rec(c, i, sets, partials + 1, head, out);
        if head == 0 {
            honors_rec(c, i, sets, partials, 1, out);
        }
        c[i] += 2;
    }
    let saved = c[i];
    c[i] = 0;
    honors_rec(c, i, sets, partials, head, out);
    c[i] = saved;
}

/// A group's profiles reduced to what the combination actually needs.
///
/// The shanten formula is `8 - 2·sets - partials - head` under the constraints
/// `sets + partials <= 4` and `head <= 1`, and combining groups only ever adds
/// cost and value. So for one suit it is enough to keep, for every `(cost =
/// sets + partials, head)` pair, the largest value `2·sets + partials + head`:
/// any other profile with the same cost and head is strictly worse in every
/// combination. That is ten numbers instead of a list of Pareto triples, and it
/// turns the combination of the four groups into a ten-state dynamic program.
#[derive(Clone, Copy)]
struct Group {
    /// `value[cost * 2 + head]`, or `i8::MIN` when unreachable.
    value: [i8; 10],
}

impl Group {
    const EMPTY: Group = Group {
        value: [i8::MIN; 10],
    };

    fn reduce(mask: u64) -> Group {
        let mut g = Group::EMPTY;
        for bit in 0..50u32 {
            if mask & (1u64 << bit) == 0 {
                continue;
            }
            let head = (bit % 2) as i8;
            let rest = bit / 2;
            let partials = (rest % 5) as i8;
            let sets = (rest / 5) as i8;
            let cost = sets + partials;
            if cost > 4 {
                continue;
            }
            let value = 2 * sets + partials + head;
            let slot = (cost * 2 + head) as usize;
            if value > g.value[slot] {
                g.value[slot] = value;
            }
        }
        g
    }
}

thread_local! {
    /// Suit and honour profiles memoised per thread: shanten is on the hot path
    /// of the observation encoder and of the rule-based agents.
    static SUIT_CACHE: std::cell::RefCell<std::collections::HashMap<u32, Group>> =
        std::cell::RefCell::new(std::collections::HashMap::with_capacity(8192));
    static HONOR_CACHE: std::cell::RefCell<std::collections::HashMap<u32, Group>> =
        std::cell::RefCell::new(std::collections::HashMap::with_capacity(4096));
}

/// Profiles for one 9-tile suit pattern, computed from scratch.
fn suit_profiles_uncached(c9: &[u8; 9]) -> u64 {
    let mut out = 0u64;
    let mut work = *c9;
    suit_rec(&mut work, 0, 0, 0, 0, &mut out);
    out
}

/// Reduced profiles for one suit, memoised per thread.
fn suit_profiles(c9: &[u8; 9]) -> Group {
    let key = suit_key(c9);
    SUIT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&g) = cache.get(&key) {
            return g;
        }
        let g = Group::reduce(suit_profiles_uncached(c9));
        cache.insert(key, g);
        g
    })
}

fn honors_profiles(c7: &[u8; 7]) -> Group {
    let mut key = 0u32;
    for i in (0..7).rev() {
        key = key * 6 + c7[i] as u32; // base 6, see `suit_key`
    }
    HONOR_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&g) = cache.get(&key) {
            return g;
        }
        let mut mask = 0u64;
        let mut work = *c7;
        honors_rec(&mut work, 0, 0, 0, 0, &mut mask);
        let g = Group::reduce(mask);
        cache.insert(key, g);
        g
    })
}

/// Best shanten for the standard (four sets + a pair) hand shape.
///
/// `melds` is the number of called melds; `counts` holds the concealed tiles.
///
/// The four groups (three suits and the honours) are combined with a dynamic
/// program over `(blocks used, head used)` — ten states — instead of enumerating
/// the product of their profiles, which is what made this the hottest function
/// in the engine.
pub fn regular_shanten(counts: &Counts, melds: u8) -> i8 {
    if melds > SETS_NEEDED {
        return 8;
    }
    let mut suit9 = [[0u8; 9]; 3];
    for suit in 0..3 {
        for n in 0..9 {
            suit9[suit][n] = counts[(SUIT_BASE[suit] + n as crate::tile::Kind) as usize];
        }
    }
    let mut honor7 = [0u8; 7];
    honor7.copy_from_slice(&counts[27..34]);

    let groups = [
        suit_profiles(&suit9[0]),
        suit_profiles(&suit9[1]),
        suit_profiles(&suit9[2]),
        honors_profiles(&honor7),
    ];

    // Called melds are complete sets: they consume blocks and add value.
    let mut dp = [i8::MIN; 10];
    dp[melds as usize * 2] = 2 * melds as i8;
    for group in &groups {
        let mut next = [i8::MIN; 10];
        for cost in 0..=4usize {
            for head in 0..2usize {
                let current = dp[cost * 2 + head];
                if current == i8::MIN {
                    continue;
                }
                for gcost in 0..=(4 - cost) {
                    for ghead in 0..(2 - head) {
                        let add = group.value[gcost * 2 + ghead];
                        if add == i8::MIN {
                            continue;
                        }
                        let slot = (cost + gcost) * 2 + head + ghead;
                        let value = current + add;
                        if value > next[slot] {
                            next[slot] = value;
                        }
                    }
                }
            }
        }
        dp = next;
    }
    match dp.iter().copied().max() {
        Some(best) if best != i8::MIN => 8 - best,
        _ => 8,
    }
}

/// Shanten for seven pairs (七対子). Only meaningful for a closed hand.
pub fn chiitoitsu_shanten(counts: &Counts) -> i8 {
    let mut pairs = 0u8;
    let mut kinds = 0u8;
    for &c in counts.iter() {
        if c >= 1 {
            kinds += 1;
        }
        if c >= 2 {
            pairs += 1;
        }
    }
    6 - pairs as i8 + (7i8 - kinds as i8).max(0)
}

/// Shanten for thirteen orphans (国士無双). Only meaningful for a closed hand.
pub fn kokushi_shanten(counts: &Counts) -> i8 {
    let mut kinds = 0u8;
    let mut has_pair = 0u8;
    for k in 0..NUM_KINDS {
        let c = counts[k];
        if c == 0 || !crate::tile::is_yaochu(k as Kind) {
            continue;
        }
        kinds += 1;
        if c >= 2 {
            has_pair = 1;
        }
    }
    13 - kinds as i8 - has_pair as i8
}

/// Overall shanten: the minimum over the standard shape, seven pairs and
/// thirteen orphans. `-1` means the hand is complete.
pub fn shanten(counts: &Counts, melds: u8) -> i8 {
    let mut best = regular_shanten(counts, melds);
    if melds == 0 {
        best = best.min(chiitoitsu_shanten(counts));
        best = best.min(kokushi_shanten(counts));
    }
    best
}

/// Which special shape (if any) the hand is heading for, for AI tie-breaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Regular,
    Chiitoitsu,
    Kokushi,
}

/// Pick the shape with the lowest shanten (regular wins ties).
pub fn best_shape(counts: &Counts, melds: u8) -> Shape {
    let reg = regular_shanten(counts, melds);
    if melds > 0 {
        return Shape::Regular;
    }
    let chi = chiitoitsu_shanten(counts);
    let kok = kokushi_shanten(counts);
    if kok < reg && kok <= chi {
        Shape::Kokushi
    } else if chi < reg {
        Shape::Chiitoitsu
    } else {
        Shape::Regular
    }
}

/// A crude "how much is this tile worth keeping" score for a hand.
///
/// It counts same-kind copies and neighbours within two ranks, and honours are
/// valued by whether they are already paired. Both the rule-based agent (as its
/// final tie-break) and the observation encoder (as an explicit feature) use
/// this one implementation, so the two can never drift apart.
pub fn tile_keep_value(counts: &Counts, kind: Kind) -> i32 {
    let mut v = 0;
    if crate::tile::is_honor(kind) {
        v += if counts[kind as usize] >= 2 { 6 } else { 2 };
    } else {
        let suit = crate::tile::suit_of(kind);
        let n = (kind % 9) as i32;
        for d in -2..=2i32 {
            if d == 0 {
                continue;
            }
            let m = n + d;
            if (0..9).contains(&m) {
                let k = (suit * 9) as Kind + m as Kind;
                if counts[k as usize] > 0 {
                    v += if d.abs() == 1 { 3 } else { 1 };
                }
            }
        }
        v += 2 * counts[kind as usize] as i32;
    }
    v
}

/// How many distinct yaku this hand is plausibly heading for, capped at three.
///
/// Deliberately coarse: it only has to rank discards. Used by the rule-based
/// agent and exposed to the network as a feature, so both share one definition.
pub fn yaku_score(
    counts: &Counts,
    melds: &[crate::meld::Meld],
    round_wind: Kind,
    seat_wind: Kind,
) -> u8 {
    let mut score = 0u8;
    let mut all_simple = true;
    let mut suits = [false; 3];
    let mut yakuhai = false;
    let mut triplets = 0;
    for k in 0..NUM_KINDS {
        let c = counts[k];
        if c == 0 {
            continue;
        }
        let kind = k as Kind;
        if !crate::tile::is_simple(kind) {
            all_simple = false;
        }
        if crate::tile::is_honor(kind) {
            // counted through `suits`/`yakuhai` only
        } else {
            suits[crate::tile::suit_of(kind)] = true;
        }
        if c >= 3 {
            triplets += 1;
        }
        if c >= 2
            && (crate::tile::is_dragon(kind) || kind == round_wind || kind == seat_wind)
        {
            yakuhai = true;
        }
    }
    for m in melds {
        if let Some(k) = m.triplet_kind() {
            triplets += 1;
            if crate::tile::is_dragon(k) || k == round_wind || k == seat_wind {
                yakuhai = true;
            }
        }
    }
    if all_simple {
        score += 1; // 断幺九
    }
    if yakuhai {
        score += 1; // 役牌
    }
    if suits.iter().filter(|&&x| x).count() <= 1 {
        score += 1; // 混一色 / 清一色
    }
    if triplets >= 2 && score == 0 {
        score += 1; // 対々和 as a fallback plan
    }
    score.min(3)
}

/// Rough value of a hand, used to decide whether to push through a threat.
///
/// `shanten` is the concealed hand's shanten, `dora` the number of dora tiles it
/// holds. Shared by the rule-based agent and the observation encoder.
pub fn hand_strength(
    counts: &Counts,
    melds: &[crate::meld::Meld],
    shanten: i8,
    dora: u8,
    round_wind: Kind,
    seat_wind: Kind,
) -> f32 {
    let base = match shanten {
        i8::MIN..=0 => 3.0, // tenpai or already complete
        1 => 2.2,
        2 => 1.2,
        3 => 0.5,
        _ => 0.0,
    };
    base + 0.5 * yaku_score(counts, melds, round_wind, seat_wind) as f32 + 0.25 * dora.min(3) as f32
}

/// Tile kinds that lower the shanten by at least one, with the number of copies
/// believed to still be live.
///
/// `visible` is a count array of tiles the player can see (their own hand plus
/// every discard and every exposed meld). `own` is added automatically from
/// `counts`.
pub fn useful_kinds(counts: &Counts, melds: u8, visible: &Counts) -> Vec<(Kind, u8)> {
    let base = shanten(counts, melds);
    let mut out = Vec::with_capacity(16);
    let mut c = *counts;
    for k in 0..NUM_KINDS {
        if c[k] >= 4 {
            continue;
        }
        c[k] += 1;
        let after = shanten(&c, melds);
        c[k] -= 1;
        if after < base {
            let seen = c[k].saturating_add(visible[k]);
            let live = 4u8.saturating_sub(seen);
            if live > 0 {
                out.push((k as Kind, live));
            }
        }
    }
    out
}

/// Sum of live copies for [`useful_kinds`].
pub fn ukeire(counts: &Counts, melds: u8, visible: &Counts) -> u32 {
    useful_kinds(counts, melds, visible)
        .iter()
        .map(|&(_, n)| n as u32)
        .sum()
}

/// Which kinds could possibly lower the shanten of `counts` when drawn.
///
/// A drawn tile helps only if it lands in a block the hand is already building:
/// it pairs/triplets an existing tile, or it sits within two ranks of one so the
/// two can become a run. A lone tile surrounded by nothing is not a block, so it
/// cannot improve any shape — with one exception: 国士無双 wants tiles the hand
/// does *not* hold, so every 么九 kind is a candidate while kokushi is still tied
/// for the best shape.
///
/// This is exact, not an approximation, and it removes roughly a third of the
/// shanten searches [`ukeire_count`] would otherwise run. Acceptance is measured
/// for up to eight candidate discards per observed decision, so this function is
/// the single hottest spot in self-play and in evaluation.
fn improvement_candidates(counts: &Counts, base: i8, melds: u8) -> [bool; NUM_KINDS] {
    use crate::tile::{is_honor, is_yaochu, suit_of};
    let mut ok = [false; NUM_KINDS];
    for k in 0..NUM_KINDS {
        if counts[k] >= 4 {
            continue;
        }
        if counts[k] >= 1 {
            ok[k] = true;
            continue;
        }
        if is_honor(k as Kind) {
            continue; // an isolated honour cannot start a block
        }
        let suit = suit_of(k as Kind);
        let n = (k % 9) as i32;
        for d in [-2i32, -1, 1, 2] {
            let m = n + d;
            if (0..9).contains(&m) {
                let j = (suit * 9) as usize + m as usize;
                if counts[j] > 0 {
                    ok[k] = true;
                    break;
                }
            }
        }
    }
    // Thirteen orphans is the only shape that is helped by a tile the hand does
    // not have; it can only matter while it is still the best shape in hand.
    if melds == 0 && kokushi_shanten(counts) <= base {
        for k in 0..NUM_KINDS {
            if is_yaochu(k as Kind) && counts[k] < 4 {
                ok[k] = true;
            }
        }
    }
    ok
}

/// Like [`ukeire`] but without allocating: this runs inside the observation
/// encoder, once per candidate discard, so the temporary vector matters.
pub fn ukeire_count(counts: &Counts, melds: u8, visible: &Counts) -> u32 {
    let base = shanten(counts, melds);
    if base <= -1 {
        return 0; // already complete: nothing to accept
    }
    let candidates = improvement_candidates(counts, base, melds);
    let mut total = 0u32;
    let mut c = *counts;
    for k in 0..NUM_KINDS {
        if !candidates[k] {
            continue;
        }
        c[k] += 1;
        let after = shanten(&c, melds);
        c[k] -= 1;
        if after < base {
            total += 4u32.saturating_sub(counts[k].saturating_add(visible[k]) as u32);
        }
    }
    total
}

#[cfg(test)]
mod tests {


    /// The shape caches must not confuse a fifth copy with a different pattern.
    ///
    /// `tenpai_kinds` and `ukeire_count` deliberately add a tile to a count
    /// array that may already hold all four copies, so a cache key has to stay
    /// exact up to a count of 5. A base-5 key does not: `[5,0,…]` folds into the
    /// same key as `[0,1,…]`, so whichever pattern is looked up second gets
    /// answered with the first one's cached profile. This pins the key down by
    /// comparing every cached lookup against the uncached computation.
    #[test]
    fn cache_keys_distinguish_five_copies() {
        let patterns: [[u8; 9]; 5] = [
            [5, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0],
            [0, 5, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 5],
            [4, 1, 0, 0, 0, 0, 0, 0, 0],
        ];
        for pattern in patterns {
            for other in patterns {
                let _ = suit_profiles(&other); // warm with every neighbour first
            }
            let cached = suit_profiles(&pattern);
            let direct = Group::reduce(suit_profiles_uncached(&pattern));
            assert_eq!(
                cached.value, direct.value,
                "cached profile for {:?} differs from the uncached one",
                pattern
            );
        }
    }

    /// The candidate pruning inside [`ukeire_count`] must be invisible in its
    /// result: compare against a brute-force count over every kind.
    #[test]
    fn ukeire_count_matches_brute_force() {
        let mut state = 0x243F6A8885A308D3u64;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut checked = 0;
        for _ in 0..4000 {
            let melds = (rand() % 5) as u8;
            let concealed = 13 - 3 * melds as usize;
            let mut counts = [0u8; NUM_KINDS];
            for _ in 0..concealed {
                // Bias towards neighbouring tiles so real shapes appear often.
                let k = (rand() % NUM_KINDS as u64) as usize;
                if counts[k] < 4 {
                    counts[k] += 1;
                }
            }
            let mut visible = [0u8; NUM_KINDS];
            for k in 0..NUM_KINDS {
                visible[k] = counts[k].saturating_add((rand() % 3) as u8).min(4);
            }
            let base = super::shanten(&counts, melds);
            let mut brute = 0u32;
            let mut c = counts;
            for k in 0..NUM_KINDS {
                if c[k] >= 4 {
                    continue;
                }
                c[k] += 1;
                let after = super::shanten(&c, melds);
                c[k] -= 1;
                if after < base {
                    brute += 4u32.saturating_sub(counts[k].saturating_add(visible[k]) as u32);
                }
            }
            assert_eq!(
                super::ukeire_count(&counts, melds, &visible),
                brute,
                "mismatch for {:?} melds={}",
                counts,
                melds
            );
            checked += 1;
        }
        assert!(checked > 0);
    }

    use super::*;
    use crate::tile::{kind_name, parse_kind, parse_kinds, NUM_KINDS};

    fn c(s: &str) -> Counts {
        let mut counts = [0u8; NUM_KINDS];
        for k in parse_kinds(s) {
            counts[k as usize] += 1;
        }
        counts
    }

    fn sh(s: &str, melds: u8) -> i8 {
        shanten(&c(s), melds)
    }

    fn names(v: &[(Kind, u8)]) -> Vec<&'static str> {
        let mut n: Vec<&'static str> = v.iter().map(|&(k, _)| kind_name(k)).collect();
        n.sort();
        n
    }

    #[test]
    fn standard_agari() {
        assert!(is_agari(&c("123m456m789m123p11s"), 0));
        assert!(is_agari(&c("111m222m333m444m55m"), 0));
        assert!(is_agari(&c("123m123m123m456p77p"), 0));
        assert!(!is_agari(&c("123m456m789m123p12s"), 0));
        // 4 copies of a kind cannot be used as a pair + a set.
        assert!(!is_agari(&c("1111m234m567m789p1s"), 0));
        // Kan-style shapes: a concealed 4-of-a-kind is a triplet plus a floater.
        assert!(!is_agari(&c("1111m222m333m444m5s"), 0));
    }

    #[test]
    fn agari_with_melds() {
        // 2 called melds -> 8 concealed tiles including the winning tile.
        assert!(is_agari(&c("123m456m11p"), 2));
        assert!(!is_agari(&c("123m456m12p"), 2));
        assert!(is_agari(&c("123m111p11s"), 2));
    }

    #[test]
    fn seven_pairs() {
        let seven = c("1133557799m1122p");
        assert!(is_chiitoitsu(&seven));
        assert!(is_agari(&seven, 0));
        assert_eq!(chiitoitsu_shanten(&seven), -1);
        // Quad of a kind is a single pair for seven pairs.
        assert!(!is_chiitoitsu(&c("111133557799m11p")));
        assert_eq!(chiitoitsu_shanten(&c("1133557799m123p")), 1);
        assert_eq!(chiitoitsu_shanten(&c("11335577m12345p")), 2);
        // Seven pairs never applies with melds.
        assert!(!is_agari(&seven, 1));
    }

    #[test]
    fn kokushi() {
        let complete = c("19m19p19s12345677z");
        assert!(is_kokushi(&complete));
        assert!(is_agari(&complete, 0));
        let thirteen_wait = c("19m19p19s1234567z");
        assert_eq!(kokushi_shanten(&thirteen_wait), 0);
        assert_eq!(kokushi_shanten(&complete), -1);
        assert!(!is_agari(&thirteen_wait, 0));
    }

    #[test]
    fn shanten_tenpai_and_agari() {
        assert_eq!(sh("123m456m789m123p11s", 0), -1);
        assert_eq!(sh("112233m445566p77s", 0), -1);
        assert_eq!(sh("123m456m789m123p12s", 0), 0);
        assert_eq!(sh("123m456m789m123p1s", 0), 0);
        // 4 sets + a floater is a tanki tenpai.
        assert_eq!(sh("123m456m789m123p3s", 0), 0);
        // Pure nine gates, 13 tiles.
        assert_eq!(sh("1112345678999m", 0), 0);
        // Thirteen orphans 13-wait.
        assert_eq!(sh("19m19p19s1234567z", 0), 0);
        // Seven pairs tenpai beats a worse regular shape.
        assert_eq!(sh("1133557799m1122p", 0), -1);
    }

    #[test]
    fn shanten_block_arithmetic() {
        // 3 sets + head + 1 partial => tenpai.
        assert_eq!(regular_shanten(&c("123m456m789m11p23p"), 0), 0);
        // 3 sets + 2 partials, no pair: only one partial may be used.
        assert_eq!(regular_shanten(&c("123m456m789m12p34s"), 0), 1);
        // 2 sets + 2 partials + head, with a floater.
        assert_eq!(regular_shanten(&c("123m456m78m11p22p9s"), 0), 1);
        // 4 sets, no pair: tanki.
        assert_eq!(regular_shanten(&c("123m456m789m123p9s"), 0), 0);
        // Two complete sets, everything else isolated junk: 2 sets + 7 floaters.
        assert_eq!(regular_shanten(&c("123m456m1p4p7p9s2z4z6z"), 0), 4);
        // Overlapping sequences must be found (1122334p holds two runs).
        assert_eq!(regular_shanten(&c("123m456m1122334p"), 0), 0);
    }

    #[test]
    fn shanten_with_melds() {
        // 2 melds -> 7 concealed tiles.
        assert_eq!(sh("123m11p23s", 2), 0);
        assert_eq!(sh("123m11p2s5s", 2), 1);
        // 1 meld -> 10 concealed tiles.
        assert_eq!(sh("123m456m11s34s", 1), 0);
        assert_eq!(sh("123m456m11s3s9p", 1), 1);
        // closed, 13 concealed tiles: 3 sets + head, one floater
        assert_eq!(sh("123m456m789m11s2p", 0), 1);
    }

    #[test]
    fn agari_and_shanten_agree_on_random_hands() {
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..4000 {
            let mut counts = [0u8; NUM_KINDS];
            let mut left = 14usize;
            while left > 0 {
                let k = (rnd() % NUM_KINDS as u64) as usize;
                if counts[k] < 4 {
                    counts[k] += 1;
                    left -= 1;
                }
            }
            let agari = is_agari(&counts, 0);
            let value = shanten(&counts, 0);
            assert_eq!(agari, value == -1, "counts {:?} shanten {}", counts, value);
            assert!(value >= -1, "shanten below -1: {} for {:?}", value, counts);
        }
    }

    #[test]
    fn winning_kinds_are_the_waits() {
        let w = winning_kinds(&c("123m456m789m123p1s"), 0);
        assert_eq!(w, vec![parse_kind("1s").unwrap()]);
        let w2 = winning_kinds(&c("112233m445566p7s"), 0);
        assert_eq!(w2, vec![parse_kind("7s").unwrap()]);
        let w3 = names(&winning_kinds(&c("123m456m789m11p23p"), 0)
            .into_iter()
            .map(|k| (k, 1u8))
            .collect::<Vec<_>>());
        assert_eq!(w3, vec!["1p", "4p"]);
        // Four copies of the wait in hand means the shape is complete elsewhere.
        assert!(winning_kinds(&c("123m456m789m123p11s"), 0).is_empty());
    }

    #[test]
    fn ukeire_of_a_tenpai_hand_is_just_the_waits() {
        let hand = c("123m456m789m11p23p");
        let visible = [0u8; NUM_KINDS];
        assert_eq!(names(&useful_kinds(&hand, 0, &visible)), vec!["1p", "4p"]);
        // 1p: the pair already holds two copies, so two remain live;
        // 4p: all four remain live.
        assert_eq!(ukeire(&hand, 0, &visible), 6);
        // If the four copies are visible the tile is dead.
        let mut visible2 = [0u8; NUM_KINDS];
        visible2[parse_kind("1p").unwrap() as usize] = 4;
        assert_eq!(names(&useful_kinds(&hand, 0, &visible2)), vec!["4p"]);
    }

    #[test]
    fn one_shanten_ukeire_is_wide() {
        let hand = c("123m456m78m11p22p9s");
        let visible = [0u8; NUM_KINDS];
        let u = useful_kinds(&hand, 0, &visible);
        // 9m completes a run, 2p completes a triplet, and any pair completion
        // keeps the shape on track.
        let n = names(&u);
        assert!(n.contains(&"9m"), "{:?}", n);
        assert!(n.contains(&"2p"), "{:?}", n);
        assert!(ukeire(&hand, 0, &visible) >= 8);
    }

    #[test]
    fn best_shape_prefers_specials_when_ahead() {
        assert_eq!(best_shape(&c("19m19p19s1234567z"), 0), Shape::Kokushi);
        assert_eq!(best_shape(&c("1133557799m1122p"), 0), Shape::Chiitoitsu);
        assert_eq!(best_shape(&c("123m456m789m11p23p"), 0), Shape::Regular);
        assert_eq!(best_shape(&c("123m11p23s"), 2), Shape::Regular);
    }
}
