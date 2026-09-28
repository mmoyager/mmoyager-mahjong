# Riichi client: what must be visible, and how to make it comfortable

Research for the from-scratch browser client in `mmoyager_mahjong/web`. Companion to `meld-layout-report.md`
(which covers meld *geometry*); this document covers everything else the table has to show, plus accessibility.

Method note: every claim below is linked to a page that was actually opened, except where explicitly marked
**[approximate]**. Wikipedia (ja/en) and `kinmaweb.jp` bodies were unreachable from this environment, and PDFs
cannot be fetched by the tool, so a few well-known rules are cited to league/eiquette sources that *were*
readable rather than to the league PDFs themselves.

---

## Part 1 — What the table must show

### 1.1 Riichi: the stick, the sideways tile, and what happens when the declaration tile leaves the pond

**The stick goes in front of the declarer's own pond, lying parallel to the pond** — not in the middle of the
table, not in the discarder's hand area. 土田浩翔「リーチ宣言の作法」: 「リーチ棒を河の前に、河と平行に出します。
自動卓ですとリーチ棒を置く場所があります」 ([mj-news](https://mj-news.net/column/tsuchida-mjall/tsuchida_mahjongdou/2017061840543)).
The same article states the declaration must be called out clearly enough for all three opponents to hear, and
that discarding normally first and *then* turning the tile sideways is wrong — the declaration and the sideways
discard are one act.

Client convention matches the furniture: 電脳麻将 renders the stick **inside that player's pond element**
(`.chouma` inside the `Majiang.UI.He` root, hidden until the player has declared riichi), i.e. the stick is
anchored to the declarer's pond, not to a global "table centre" widget
([koba::blog 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404)).

**The declaration tile is the discard rotated 90°.** 電脳麻将 wraps exactly that one discard in a `.lizhi`
span and rotates it counter-clockwise (`transform: rotate(270deg)`), and gives the wrapper
`aria-label` リーチ ([same source](https://blog.kobalab.net/entry/2026/03/30/070404)).

**When the declaration tile is called away, the sideways tile moves to the next discard.** 電脳麻将's footnote:
「type が 2 以外でリーチ宣言牌を鳴かれた場合は次の捨て牌を横向きにします」 — when the declaration tile is
called, the *next* discard is drawn sideways so the pond still contains exactly one sideways tile per declared
riichi ([same source](https://blog.kobalab.net/entry/2026/03/30/070404)). Client consequences:

- the riichi state must be tracked on the **player**, not inferred from the pond;
- the pond must re-render when a tile is called (the called tile leaves the pond, the next discard inherits the
  sideways mark);
- the stick stays where it was placed, even though the tile it announced is gone.

Rule-level detail that differs by ruleset: Tenhou's 段位戦 says 「リーチ宣言牌で放銃した場合は供託料は発生しない」
— if the declaration tile is ronned, no 1000-point deposit is made ([天鳳 マニュアル](https://tenhou.net/man/)).
That is a *ruleset* switch, not a display switch: the client should render the 供託 count straight from engine
state and never compute it from "did this player declare riichi". Other rulesets treat the declaration as fully
established and take the 1000 points **[approximate]** — the exact set of rulesets that differ could not be
confirmed from a readable primary source in this environment.

Also from the same Tenhou manual page: 供託 at game end goes to the leader (「終了時の供託はトップ取り」),
and on a double ron the 積み棒/供託 go to the player nearest the discarder in turn order (「ダブロンあり。
積み棒/供託は上家取り」).

**積み棒 (honba sticks) sit at the dealer's right edge**: 「連荘の度に１００点棒を１本、積み棒として出し、
親の右端に置きます」, and when a riichi survives a draw the stick is moved **next to the 積み棒** and becomes
供託: 「立直がかかっていて流局した場合、出された立直棒は積み棒の隣に供託棒として供託され、次に和了った人が
もらえます。積み棒はもらえません」 ([はじめての麻雀 35](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407)).
So the client needs **two distinct counters** — honba (100-point sticks, add 300 points each to the win, per the
same article) and 供託 (1000-point sticks in the pot) — and it must show that the pot survives into the next hand.

電脳麻将's board encodes exactly that split: `.score` contains `.jushu`, `.changbang` (本場), `.lizhibang`
(供託) and `.shan` ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).

Our client today: `#round-name` 东1局, `#honba` 0 本场, `#sticks` 供托 N, a strip of `.riichi-stick` elements
(capped at 12 with a "+N" overflow chip) — counters are right; see §1.3 for the missing 裏ドラ and §1.4 for the
missing 聴牌/不聴 display.

### 1.2 Dora and ura-dora indicators

- **Up to five indicator slots each for 表ドラ and 裏ドラ.** 電脳麻将 renders a *fixed run of five* slots for
  each, filling unused ones with the blank placeholder (`for (let i = 0; i < 5; i++) … baopai[i] || '_'`), and
  labels the areas ドラ and 裏ドラ ([shan.js via the blog](https://blog.kobalab.net/entry/2026/03/30/070404)).
  Five is the ceiling because four kans plus the initial flip is the maximum. A client that grows the indicator
  row one tile at a time is fine; one that *reflows* the row when the count changes is not — the row's layout
  must be stable so the eye doesn't have to re-find it.
- **The indicators are only ever "the wall's face".** 電脳麻将 shows the indicator areas and the remaining draw
  count and nothing else of the wall: 「天鳳など牌山をすべて表示する麻雀アプリもありますが、電脳麻将ではドラ
  表示牌と残りツモ枚数だけを表示しています」, with the author's own footnote that 雀魂 does not show the wall
  either ([same source](https://blog.kobalab.net/entry/2026/03/30/070404)). **Tenhou is the outlier that draws the
  whole wall** — if we draw a wall, we are copying Tenhou, not the modern norm.
- **When a kan reveals a new indicator.** The kan itself takes one tile from the dead wall as the replacement
  draw (嶺上牌) and 「カンをすると新しいドラが開く」 ([はじめての麻雀 33](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061740405)).
  *Timing differs by ruleset, exactly as the client implements it*: Tenhou 段位戦 flips the kan-dora indicator
  **immediately for ankan and after the discard (or immediately before the following rinshan draw) for
  minkan/kakan** — 「カンドラは、暗槓は即乗り、明槓/加槓は後めくり（打牌または続く嶺上の直前）」; Tenhou's
  雀荘戦 instead uses 槓ドラ即乗り for everything ([天鳳 マニュアル](https://tenhou.net/man/)). A client that
  animates "kan → new indicator appears" must therefore take the *timing* from the engine's message stream
  rather than assuming, and 電脳麻将 does precisely this: it only calls `shan.update()` (remaining count) on a
  draw, and `shan.redraw()` — the indicator row — on the 開槓 message
  ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).
- **裏ドラ after a win**: the same five-slot row exists as `.fubaopai` and is drawn from
  `shan.fubaopai || []`, i.e. the reveal is data-driven from the win message, and blank slots stay blank
  ([koba::blog 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404)). The practical presentation rule:
  ura indicators are **hidden until the win is declared**, then revealed in place (not in a modal that hides the
  table), and only for a riichi win — 電脳麻将 keeps the ura area in the same component as the table's dora row.
  Our client has `view.dora_indicators` feeding `#dora-tiles` and **no ura container at all** — that is the
  biggest single display gap in Part 1.

### 1.3 Wall remaining, and the 王牌

- The wall is 136 tiles; **14 are set aside as the dead wall (王牌) and are never drawn from**, so a hand has
  **70 live draws** in a 4-player game. はじめての麻雀: 「山牌の王牌（ワンパイ）１４枚を残して最後まで取り切ると、
  流局（リュウキョク）」 ([はじめての麻雀 34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406));
  and 嶺上牌 are the tiles kept to the right of the break in the wall, one of which is stood down to mark the
  dead wall ([はじめての麻雀 33](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061740405)).
- **The counter counts live wall draws, and 0 ends the hand.** 電脳麻将 calls it 残りツモ数 and prints
  `shan.paishu` ([koba::blog 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404)); it decrements on
  every draw including the rinshan draw after a kan (`shan.update()` on ツモ and 槓自摸 — 電脳麻将UI 盤面).
  Tiles taken from the dead wall for kans do *not* reduce it, so the number can reach 0 while kans are still
  being taken — the display must be "draws left", never "tiles left".
- **Show the 王牌 as a fixed, non-interactive fact, not a live area.** 電脳麻将 and 雀魂 show indicators +
  count only ([same source](https://blog.kobalab.net/entry/2026/03/30/070404)); showing the 14 dead-wall tiles
  stacked and greyed (or simply not showing them) is enough — what matters is that the player can tell "this
  hand ends when the counter hits 0", and that the four rinshan tiles are understood to exist.
- Our client shows `余 N` plus a bar filled to `N/70` — correct semantics and a good use of a redundant
  channel. Keep the `70` denominator literal (it is, in `renderWall`), and make sure the bar's colour is not
  the only thing that changes near 0; a text change (e.g. 余 0 / 流局) is what a colour-blind player will see.

### 1.4 Exhaustive draw: 聴牌 display and the noten penalty

- On an exhaustive draw, each player's hand is either revealed as tenpai or left face down. 電脳麻将 gets this
  as a per-player boolean on the 流局 message and calls `shoupai[l].redraw(open)` with
  `open = (player is the viewpoint) || msg.pingju.shoupai[l]` — i.e. **the engine tells the client who was
  tenpai** and the client opens exactly those hands ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).
- **The penalty is a 3000-point split**: 「ノーテン罰符は、３０００点を聴牌していない人で割り算します。
  ３人ノーテンは１０００点ずつ払い、聴牌の１人は３０００点もらえます。２人ノーテンは１５００点ずつ払い、
  聴牌の２人は１５００点ずつもらえます。１人ノーテンは３０００点払い、聴牌の３人は１０００点ずつもらいます。
  ４人聴牌、４人ノーテンは点棒の受け渡しはありません」
  ([はじめての麻雀 34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406)).
- Minimum viable display: per seat, a 聴牌/不聴 tag; the opened tenpai hands; the point deltas; and whether the
  dealer keeps the seat (連荘) or the wind moves on — 「親が聴牌していれば、もう１度親ができる …親がノーテンで
  流局した場合、親は移ります」(part 34 and [part 35](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407)).
  Tenhou's own ruleset is ノーテン親流れ／聴牌連荘 ([天鳳 マニュアル](https://tenhou.net/man/)).
- Our client maps the yaku/result name 荒牌流局 but has **no 聴牌/不聴 surface at all** — no per-seat tag, no
  opened tenpai hands, no penalty breakdown. This is the second-biggest Part 1 gap, and it is the moment a
  beginner most needs the client to explain itself.

### 1.5 What 電脳麻将 does in the pond that we do not

All from [電脳麻将UI 〜 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404); these are *presentation*
details, complementary to `meld-layout-report.md`:

| Detail | 電脳麻将 does | Why it matters |
|---|---|---|
| Row breaks | a `.break` element after every 6th tile (two breaks, then the row wraps) | reading "how many discards since the riichi" is a real skill; a 6-column grid is what the eye expects |
| Rows per player | 3 rows of 6 in `block` mode, or 1 long line in `line` mode — **the same DOM, different CSS** | the side seats can use the one-line form without a separate renderer |
| Tsumogiri | `.pai.zimo { opacity: 0.8 }`, and 0.6 while the tile is still "in flight" | separates "discarded blind" from "thought about it" — a genuine info channel |
| Called-away tiles | `.pai.fulou { opacity: 0.4 }`, and only rendered at all in pond-type 2 | keeps the pond's history honest without cluttering the default view |
| Just-discarded tile | a `.dapai` class on the tile until the next turn starts, plus a small `translate()` nudge | the tile is visibly *in flight*; the class is also the hook for "not yet settled" animation |
| Riichi declaration tile | wrapped in `.lizhi`, rotated 270°, with `aria-label` | one sideways tile per declared riichi, reachable by screen reader |
| Riichi stick | `.chouma` inside the pond, shown once a declaration exists | mirrors the real-table placement |
| Turn change | on a turn change it re-renders the *previous* player's hand and pond, which makes the drawn tile visually join the hand | the 14th tile never stays ambiguously separated |

Our client already marks the call target: `markCallTarget()` adds `.callable` to the pond slot a call window is
about and tints that pond (`web/app.js`), and melds are labelled `碰 5m5m5m（来自下家）`. What we do not have:
the in-flight (`dapai`) vs settled distinction, the tsumogiri dimming, called-away tiles shown at 40 %, and the
row-break rhythm beyond the fixed 6-wide pond grid.

### 1.6 Turn order and seat winds

A player must be able to answer "whose turn is it" without reading a number, and must never confuse 自風 with
場風. Two conventions exist, and the strongest clients use both:

- **Absolute winds on every seat.** We render a wind disc per seat (`自风` in the tooltip), a 亲 dealer tag, and
  東1局 in the round readout. The round wind and the seat winds must be visually distinguishable — ours does this
  by putting the seat wind in a disc and the round wind in the centre panel (the code comments call this out
  explicitly). 電脳麻将 instead keeps only 局数 (東1局) plus位置, and rotates the whole board by viewpoint
  (`class_name = ['main','xiajia','duimian','shangjia']`, positions recomputed as
  `(4 + id - viewpoint) % 4`) — a legitimate alternative that trades "winds" for "relative position"
  ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).
- **A turn marker on the seat, re-evaluated on every state change.** 電脳麻将 removes a `lunban` class from all
  four seats and adds it to the acting player on every `update()` ([same source](https://blog.kobalab.net/entry/2026/04/06/083417)).
  Ours does the equivalent with `.seat.turn` / `.centre-seat.turn`. Note that this is *state*, so it must also be
  true during call windows (waiting on a pon/chi/ron decision) — that is when "whose turn is it" is least obvious
  and matters most.
- **Turn order is 東→南→西→北 counter-clockwise as seen from above** **[approximate]** — this is standard, but I
  could not open a readable primary source for it in this environment; the client's own layout (自 seat at the
  bottom, 下家 to the right, 対面 opposite, 上家 to the left) already encodes it and should not be changed.
- Tenhou's own presentation details worth stealing: score bars are coloured by player type (青/男, 赤/女, 緑/COM)
  and the provisional leader gets a white gradient; hovering the centre panel shows the score **differences**
  ([天鳳 マニュアル](https://tenhou.net/man/)). The score-difference readout is the single most useful
  scoreboard affordance for a beginner — 25000/25000/25000/25000 means much less than ±0/±0/+3000/−3000.
- Call announcements (チー/ポン/カン/リーチ/ロン/ツモ) are shown **as text in that player's own area** until the
  event that resolves them (a discard clears 副露/リーチ captions, 槓自摸 clears カン, a win clears ロン/ツモ, and
  a win or draw clears everyone's) ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)). The
  caption is the non-audio channel for the same information; ours keeps a log (`#log`) instead of a per-seat
  caption, which works but loses the "who said it" spatial anchor.

---

## Part 2 — Accessibility and usability

### 2.1 Contrast on a dark felt background

Normative targets (WCAG 2.2):

- **1.4.3 Contrast (Minimum), AA**: text and images of text **≥ 4.5:1**; "large text" **≥ 3:1**, where large =
  ≥ 18pt, or ≥ 14pt bold — which the W3C itself converts as "approximately 18.5px and 24px", and defines for
  CJK as "font size that would yield equivalent size for Chinese, Japanese and Korean (CJK) fonts"
  ([Understanding 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)). Computed ratios are
  thresholds and are **not rounded** (4.499:1 fails).
- **1.4.11 Non-text Contrast, AA**: **≥ 3:1** for (a) the visual information needed to identify a UI component
  and its **states**, and (b) parts of graphics required to understand the content — this includes the **focus
  indicator** against its adjacent colours
  ([Understanding 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)).
- The exact formula (`(L1+0.05)/(L2+0.05)`, sRGB relative luminance with the 0.04045 breakpoint) is normative and
  appears on the 1.4.3 page, so every ratio below is reproducible rather than eyeballed.

**Measured ratios for the palette actually in `web/style.css`** (computed with the W3C formula; `--felt: #0f5132`,
`--panel: #12452c`, page `#072015`):

| Pair | Ratio | Verdict |
|---|---|---|
| `--ink #eaf5ee` on felt / panel / page | 8.38 / 9.82 / 15.32 | passes AA, ≥7:1 = AAA |
| `--muted #9dc4ae` on panel / felt | 5.72 / 4.88 | passes AA body text |
| `--gold #ffd166` on felt / panel / page | 6.49 / 7.61 / 11.87 | passes AA, AAA on panel |
| riichi tag white on `#c0392b` | 5.44 | passes AA |
| furiten `#ddd` on `#555` | 5.49 | passes AA |
| delta up `#7fd99a` on panel | 6.43 | passes AA |
| delta down `#ff8a80` on panel | 4.81 | passes AA (closest call in the palette) |
| tile face `#f7f4e9` vs suit inks man/pin/sou/honor `#b3261e/#14459c/#17643a/#3b2f2f` | 5.94 / 8.08 / 6.53 / 11.67 | passes AA |

So **the palette is not the problem**; the risks are colour-only state encoding (§2.2) and small sizes (§2.8).
Two notes for new surfaces: measure against the *brightest* felt a text can sit on (the radial gradient in
`#table` makes the centre lighter than `--felt`), and prefer `#fff`/`--ink` on the felt over a mid-tone green,
which is where dark-theme palettes usually land near 3:1.

### 2.2 The red/green problem in a green-table game

- Prevalence: "**About 1 in 12 men have color vision deficiency**", and "the most common type … makes it hard to
  tell the difference between red and green" — National Eye Institute (NIH)
  ([NEI, Color Blindness](https://www.nei.nih.gov/eye-health-information/eye-conditions-and-diseases/color-blindness)).
  1 in 12 ≈ 8.3 %, i.e. on average roughly one player per table.
- The rule to obey: **do not encode information by hue alone**. WCAG's own framing: "Use of Color addresses
  changing **only the color** (hue) of an object or text without otherwise altering the object's form", and a
  change of *luminance contrast* (≥3:1) counts as more than hue
  ([Understanding 1.4.11 → Relationship with Use of Color](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)).
- A trap the W3C calls out explicitly: "the use of predominantly long wavelength colors against darker colors
  (generally appearing black) for those who have **protanopia**. (We provide an advisory technique on avoiding
  red on black for that reason)" ([Understanding 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)).
  A red riichi tag on a near-black panel is exactly that shape — ours is `#c0392b` with **white** text (5.44:1),
  which is fine, but a red *glow*-or-red-border-only state on dark felt would be the failing version.
- Per state in this client:
  - **Turn**: a gold glow plus a 7 % tint on the acting seat. That is a luminance/shape change, so it survives
    deuteranopia — but also change something non-colour on the name plate (raised border, ▶ marker), because
    "which seat is lit" is the most safety-critical reading in the game.
  - **Riichi**: the stick graphic and the 立直 tag text carry it — good. Never let a red dot be the only sign.
  - **Dora**: always show the indicator **tile face** (we do); a red/green dot beside a tile is unreadable to a
    protanope.
  - **Tenpai/noten** (once added): text tags 聴牌/不聴, not green/red chips.
  - **Furiten / 振听**: keep the text tag.
  - **Danger / agari hints**: add a glyph or number to any colour coding.
  - **Delta scores**: the explicit `+`/`−` sign is the correct redundancy — keep the sign even if colour goes.
- Test by emulating a deficiency rather than trusting intuition (Chrome DevTools has a vision-deficiency
  emulation control) **[approximate — the feature exists; no doc page was opened this pass]**. Colour-universal
  palettes (Okabe–Ito, Paul Tol's qualitative schemes) are the standard starting point for any new categorical
  coding **[approximate — not fetched this pass]**.

### 2.3 Numeric legibility

- Use `font-variant-numeric: tabular-nums` (OpenType `tnum`) wherever figures must line up or must not move:
  MDN defines it as "the set of figures where numbers are all of the same size, allowing them to be easily
  aligned like in tables" ([MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/font-variant-numeric)).
  Ours already applies it to `#round-info` and `.centre-seat`.
- Keep **proportional** figures for prose and for numbers set inline in a sentence; `tnum` is for columns.
- Scores: give each seat a **fixed-width, right-aligned** score cell so 25000 → 12000 does not reflow the name
  beside it, and group thousands — 電脳麻将 does this with `defen.replace(/(\d)(\d{3})$/, '$1,$2')` in
  `lib/board.js` (read from the repo's blob API). Position the delta chip so its appearance never moves the
  score.
- Scores change in place during a hand: highlight briefly, but never count a score up. A counting animation is a
  legibility loss for anyone doing arithmetic with it.

### 2.4 Target size and input

- **2.5.8 Target Size (Minimum), AA**: **24 × 24 CSS px**, with five exceptions — **Spacing** (a 24px-diameter
  circle centred on each undersized target must not intersect another target), **Equivalent**, **Inline**,
  **User agent control**, and **Essential** (the doc's own example: "an interactive data visualization where
  targets are necessarily dense")
  ([Understanding 2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)).
- **2.5.5 Target Size (Enhanced), AAA**: **44 × 44 CSS px**
  ([Understanding 2.5.5](https://www.w3.org/WAI/WCAG22/Understanding/target-size-enhanced.html)) — also the page
  from which the W3C links Apple's and Google Material's touch guidance.
- Applied here: hand tiles and action buttons clear both (buttons 46px tall with ≥84px min-width; pond tiles
  32 × 43). The **pond is dense but read-only**, so 2.5.8 does not bite there; the targets that *do* need work
  are the small chrome controls — `#log-head button.mini` (2px/8px padding, 11px text) and the settings
  checkboxes — which should gain padding, or at minimum be shown to satisfy the 24px spacing exception.
- Worth applying from the doc itself: "It can be beneficial to provide an option to increase the active target
  area without increasing the visible target size."

### 2.5 Keyboard and focus

- Where a control is a control, use a real `<button>`: 電脳麻将's accessibility work states the benefits plainly —
  Tab moves between them, Space activates them, and VoiceOver users can cycle buttons with B / Shift+B
  ([電脳麻将UI 〜 VoiceOver対応(2)](https://kobalab.net/liulian/blog/VoiceOver_2)).
- For a tile that is sometimes clickable and sometimes not, the same source's pattern is right: `role="button"` +
  `tabindex="0"` + an Enter handler that fires the click, because "同一の牌であってもあるときはボタンであり、
  あるときはボタンではない" ([same](https://kobalab.net/liulian/blog/VoiceOver_2)). The reusable `selector`
  component behind it uses Arrow keys + Enter by default (`confirm: 'Enter'`, `prev: 'ArrowLeft'`,
  `next: 'ArrowRight'`), maps mouse hover to focus, and on touch takes a first tap to select and a second to
  confirm ([電脳麻将UI 〜 selector](https://blog.kobalab.net/entry/2026/03/15/090923)). The two-tap model is the
  sane answer for a 13-tile hand, and unifying "selection" onto `focus` is what makes it screen-reader-usable.
- Focus must be strong. WCAG 2.2 **2.4.13 Focus Appearance (AAA)** requires at least the area of a **2 CSS px
  perimeter** of the unfocused component and a **≥ 3:1 change of contrast between focused and unfocused pixels**;
  an indicator inset inside the component must be **≥ 3px** to make up the area, and with a two-colour indicator
  only the sufficiently contrasting part counts
  ([Understanding 2.4.13](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance.html)). Removing the
  outline without a replacement is a documented failure of 1.4.11, 2.4.7 and 2.4.13 (F78, same page). Our
  `:focus-visible` rules exist: check the ring is ≥2px, contrasts with both the tile and the felt, and is never
  clipped or covered by the log panel or action bar.
- **2.1.4 Character Key Shortcuts (A)**: if single-key shortcuts are added (1–9 for tiles, `r` for riichi, `p` to
  pass), at least one must hold — the shortcut **can be turned off**, **can be remapped** to include a
  non-printable key, or it is **only active while that component has focus**
  ([Understanding 2.1.4](https://www.w3.org/WAI/WCAG22/Understanding/character-key-shortcuts.html)). This is the
  criterion a game client trips by accident, and it exists for speech-input users, whose dictation turns into a
  barrage of commands.
- The umbrella requirement is 2.1.1 Keyboard: all functionality operable from a keyboard. The concrete pattern
  that satisfies it for a board game is 電脳麻将's three-step plan (§2.7).

### 2.6 Motion

- **`prefers-reduced-motion`** exists to detect a device setting to "minimize the amount of non-essential
  motion"; MDN names the audience — "Such animations can trigger discomfort for those with **vestibular motion
  disorders**" — and singles out "**scaling or panning large objects**" as triggers
  ([MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion)).
  A full-table tile-fly is a panning/scaling large object by that definition.
- **2.2.2 Pause, Stop, Hide (A)** covers "moving, blinking, scrolling, or auto-updating information" that starts
  automatically, lasts more than five seconds and is presented in parallel with other content: there must be a
  mechanism to pause/stop/hide it (or, for auto-updating content, to control its frequency). Helpfully for us, the
  SC's own examples treat **turn-based games as pausable** — "A game is designed so that users take turns rather
  than competing in real-time — One party can pause the game without invalidating the competitive aspect of it"
  — and note that pausing real-time status should jump to the current state on resume
  ([Understanding 2.2.2](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html)). A riichi client
  against bots is turn-based, so an animations-off switch (ours) plus the OS preference (ours) is the right shape.
- **2.3.1 Three Flashes or Below Threshold (A)**: nothing may flash more than 3 times per second, and the
  general/red-flash thresholds define a flash as a pair of opposing relative-luminance changes of ≥10 % where the
  darker state is below 0.80, with a stricter rule for saturated red
  ([Understanding 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html)).
  Consequence: never pulse a large bright element repeatedly. Our `drawn-pulse` on one tile is small enough to be
  safe; a full-table flash on ron/tsumo would not be.
- How much animation is appropriate: keep the **pacing** (one discard at a time is information, not decoration —
  the client's own comment makes this argument) and cut the **travel**: prefer short cross-fades and small offsets
  to long slides or scales, keep each beat short (a discard beat around 100–200 ms reads as responsive rather than
  slow **[approximate — a design judgement, not a sourced figure]**), and never animate something a player must
  read while it moves (scores, wall counter, dora row). Honour the media query by default, let an explicit stored
  in-game toggle override it (the client already persists `mmj-anim`), and make that toggle a properly labelled,
  keyboard-reachable control.

### 2.7 Screen readers for a spatial game

The best-documented attempt on a real riichi client is 電脳麻将's; its plan is the one to copy. From
[電脳麻将UI 〜 VoiceOver対応(0)](https://blog.kobalab.net/entry/2026/04/28/072506):

1. **Name every tile image** so that "盤面をたどれば情報が得られる" — traversing the board yields the
   information. Before this, "牌は alt 属性すらないただの画像" and the buttons were `div`s: nothing was discoverable
   without sight.
2. **Make everything keyboard-operable** (§2.5).
3. **Add an on-screen commentary area** ("実況エリア") for asynchronous changes such as another player's discard,
   writing text into it so the screen reader picks it up. This is the piece that makes a *multiplayer* game work:
   a blind player cannot poll a spatial board fast enough, so events must be pushed as text.

Details that only show up in practice:

- **Alt-text length trap**: Chrome treats alt strings of **two characters or fewer** as an "inappropriate
  description", announces "unlabeled image", and then steers the user to its image-guessing context menu.
  電脳麻将 works around it by extending short names ("本場" → "本場：") or appending a **zero-width space** when
  there is nothing to add ([VoiceOver対応(1)](https://kobalab.net/liulian/blog/VoiceOver_1)). This hits us
  directly: a one-character wind like 東 or a tile named 中 is exactly the failing shape — always name with ≥3
  characters (東風, 白（はく）) or append a zero-width space.
- **Live regions** (MDN: [ARIA live regions](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Guides/Live_regions)):
  `aria-live="polite"` is the default choice ("not so rapid as to be annoying"); `assertive` "should only be used
  for time-sensitive/critical notifications that absolutely require the user's immediate attention" and
  interrupts speech, so it is wrong for routine discards; `off` is **not** silence — updates are still announced
  when focus is inside. A live region must **exist in the initial markup and be empty**, because AT generally
  announces only *changes* to content already exposed. `role="log"`/`role="status"` are implicit live regions but
  want a redundant `aria-live="polite"` for compatibility, and `role="alert"` + `aria-live="assertive"`
  double-speaks in iOS VoiceOver.
- Applied here: `#banner` is `aria-live="polite"` — good, but it is the *only* live region and `#log` (where the
  game's narration actually goes) is not announced. Add one dedicated `role="log" aria-live="polite"` commentary
  element fed from the same event stream as `#log`, and **coalesce** it (queue and drop stale entries, at most one
  announcement per beat) so a fast hand cannot flood a screen reader. Announce the four things a sighted player
  reads at a glance: whose turn it now is, what was just discarded (tile name + seat), any call/riichi
  declaration, and the end-of-hand score changes.
- What cannot realistically be exposed: the pond's two-dimensional arrangement, the shape of a meld layout, the
  spacing between discards, tile counts by inference. Do not try — expose the equivalent facts as text and let the
  player reason from those. Sound is a legitimate complementary channel (電脳麻将 plays a distinct call voice per
  action and keeps a text caption for the same event — [盤面](https://blog.kobalab.net/entry/2026/04/06/083417)),
  but it must never be the only channel, since it is unavailable to Deaf players.
- Decorative pieces (tile backs, felt gradient, the riichi-stick dot) should be `aria-hidden` — the client already
  does this for the centre ring SVG — while everything carrying state (drawn tile, callable tile, riichi/furiten
  tags, wall count, dora indicator) needs a name.

### 2.8 Sizing and density

- The one normative size anchor is the large-text threshold in 1.4.3 (≥ 18pt / ≥ 14pt bold, "approximately 18.5px
  and 24px", with a CJK-specific equivalence clause)
  ([Understanding 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)). Anything smaller
  must clear 4.5:1 — and at 10–11px even 5:1 is hard work. This client's smallest text is `.pond-label` and
  `#centre-wall-text` at 11px, and `.centre-seat .delta` at 11px.
- Practical rule for this board: **≥ 14px for anything read under time pressure** (scores, wind, round, wall
  count, tags), 11–12px only for genuinely secondary chrome (pond labels, captions), ≥ 16px for the
  hand/action surface. **[approximate — a design judgement informed by the CJK large-text clause, not a sourced
  minimum]**
- CJK specifically: Japanese/Chinese glyphs are denser than Latin at the same nominal size, so CJK prose (the log)
  wants more line-height than a Latin UI would **[approximate — not sourced this pass]**; the log's 12.5px at
  line-height 1.55 is a reasonable start but sits at the small end.
- Zoom: text must survive 200 % resize and the layout must not lose controls when text spacing grows (WCAG 1.4.4
  Resize Text / 1.4.12 Text Spacing) **[approximate — SC numbers are correct, but their Understanding pages were
  not opened this pass]**. Because `#app` is a fixed-height flex column with `height: 100%`, test 200 % zoom and
  200 % font size explicitly: that is where the action bar or the log is most likely to be pushed off-screen.
- Density lesson from the data-dense Japanese clients: the numbers that change (score, deltas, wall count) belong
  in a fixed position that never reflows when tiles or melds appear. 電脳麻将 keeps 局数/本場/供託 in one `.score`
  block and the four scores in their own `.defen` rows ([盤面](https://blog.kobalab.net/entry/2026/04/06/083417)),
  and our client's comment already cites 雀魂 for the same decision ("雀魂 keeps a player's wind and score with
  the table rather than in the corner of their own box, which also means the numbers never move as a hand grows
  or a meld lands"). Keep that invariant — it is the single most valuable layout rule in the UI.

---

## Deliverable 1 — element → what must be shown → minimum contrast / size

| Element | What must be shown | Minimum contrast | Minimum size | Source basis |
|---|---|---|---|---|
| Your hand tiles | face of each tile; the drawn tile distinguishable; clickable state on your turn | tile vs table 3:1; glyph vs tile face ≥ 4.5:1 (measured 5.94–11.67) | 24×24 (AA); aim 44×44 for the tappable area | [1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html), [2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html), [2.5.5](https://www.w3.org/WAI/WCAG22/Understanding/target-size-enhanced.html) |
| Opponents' hands | backs + concealed count; who has declared riichi | tiles vs table 3:1 | — (not interactive) | [1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) |
| Pond (河) | rows of 6 with breaks; the sideways riichi declaration tile; called-away tiles dimmed or marked; the just-played tile distinguishable | tile vs table 3:1; the riichi marker must not be hue-only | — | [koba::blog 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404) |
| Riichi stick (リーチ棒) | one per declared riichi, in front of the declarer's pond, parallel to it | graphic vs table 3:1 | — (not interactive) | [土田](https://mj-news.net/column/tsuchida-mjall/tsuchida_mahjongdou/2017061840543), [はじめての麻雀35](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407) |
| Honba sticks (積み棒) | count, at the dealer's side; +300 per stick on a win | text ≥ 4.5:1 (measured 8.38 for `--ink` on felt) | ≥ 14px text | [はじめての麻雀35](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407) |
| 供託 counter | numeric count of 1000-point sticks in the pot, carried across hands | 4.5:1 | ≥ 14px, tabular figures | [天鳳](https://tenhou.net/man/), [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/font-variant-numeric) |
| Dora indicators (表ドラ) | up to 5 slots, stable layout, blanks kept as blanks | indicator tile vs table 3:1 | tile tall enough to identify (≥ ~24px) | [shan.js via koba::blog](https://blog.kobalab.net/entry/2026/03/30/070404) |
| Ura-dora indicators (裏ドラ) | hidden until the win, then up to 5 slots revealed in place | same | same | [same](https://blog.kobalab.net/entry/2026/03/30/070404) |
| Wall counter | **remaining live draws** (70 → 0); 0 = exhaustive draw | 4.5:1; the "0" state must change text, not only colour | ≥ 14px, tabular figures | [はじめての麻雀34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406) |
| Round + wind | round wind and number (東1局), each seat's own wind, dealer marker | 4.5:1 (measured 6.49 gold on felt) | ≥ 14px; wind disc ≥ 24px | [koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417) |
| Turn indicator | which seat is acting, including during call windows | must not be hue-only | — | [1.4.11 → Use of Color](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) |
| Scores | 4 scores, thousands-separated, never reflowing; deltas with an explicit sign | 4.5:1 (measured 8.38–9.82) | ≥ 14px, `tabular-nums`, fixed-width cell | [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/font-variant-numeric) |
| Melds (副露) | chi/pon/minkan/ankan layout with the sideways tile in the source slot; the source player identifiable | tile vs table 3:1 | — | `meld-layout-report.md`, [koba::blog](https://blog.kobalab.net/entry/2026/03/30/070404) |
| Exhaustive-draw result | per-seat 聴牌/不聴, opened tenpai hands, the 3000-point split, dealer continuation | 4.5:1; not hue-only | ≥ 14px tags | [はじめての麻雀34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406) |
| Action buttons | the legal actions, which is primary, remaining time | text 4.5:1; boundary/state 3:1 | 24×24 min, **44×44** recommended (ours: 46px tall) | [2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html), [2.5.5](https://www.w3.org/WAI/WCAG22/Understanding/target-size-enhanced.html) |
| Keyboard focus ring | visible on every operable element | **≥ 3:1 change** vs unfocused, and ≥ 3:1 vs adjacent colours | area ≥ 2 CSS px perimeter (≥3px if inset) | [2.4.13](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance.html), [1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) |
| Log / commentary | who did what, newest visible; also the screen-reader channel | 4.5:1 (measured 5.72 for `--muted`) | ≥ 12px, CJK-friendly line-height | [MDN live regions](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Guides/Live_regions) |
| Small chrome (mini buttons, toggles) | — | 4.5:1 text / 3:1 boundary | 24×24 min **or** the 24px-spacing exception — currently the weakest spot (11px, 2px padding) | [2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html) |
| Motion | every animation honours `prefers-reduced-motion` **and** an in-game switch; nothing flashes > 3×/s | — | — | [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion), [2.2.2](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html), [2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html) |

---

## Deliverable 2 — Top 10 accessibility fixes, ranked by value

1. **Add the 裏ドラ reveal and a per-seat 聴牌/不聴 result surface.** Missing functionality, not polish: a player
   cannot verify a riichi win's score, and cannot see why the noten penalty was charged
   ([shan.js](https://blog.kobalab.net/entry/2026/03/30/070404), [はじめての麻雀34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406)).
2. **Add one `role="log" aria-live="polite"` commentary region, with coalescing**, fed by the same events as
   `#log`. Without it a screen-reader player cannot follow a hand at all; with it the game becomes playable. Keep
   it in the initial markup and empty, and never use `assertive` for routine events
   ([MDN](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Guides/Live_regions),
   [電脳麻将 VoiceOver(0)](https://blog.kobalab.net/entry/2026/04/28/072506)).
3. **Make the turn state non-hue-only** — a second channel (outline, ▶ marker, or a text tag in the name plate)
   so the most safety-critical reading survives deuteranopia and low vision
   ([1.4.11 → Use of Color](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)).
4. **Guarantee a strong focus ring on tiles and buttons**: ≥ 2px, ≥ 3:1 change of contrast, never clipped or
   covered by the log/action bar ([2.4.13](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance.html)).
5. **Use `role="button" + tabindex="0" + Enter` for clickable tiles and real `<button>`s elsewhere**, with an
   Arrow/Enter selector for the hand so a full turn is playable without a pointer
   ([VoiceOver(2)](https://kobalab.net/liulian/blog/VoiceOver_2), [selector](https://blog.kobalab.net/entry/2026/03/15/090923)).
6. **Fix the short-label alt trap**: every exposed name (winds, dragons, round) must be ≥ 3 characters or carry a
   zero-width space, or Chrome announces it as an unlabeled image
   ([VoiceOver(1)](https://kobalab.net/liulian/blog/VoiceOver_1)).
7. **Grow the small chrome to 24×24 with spacing, and 44×44 for primary actions** — `#log-head button.mini` and
   the settings checkboxes are currently under any comfortable target
   ([2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)).
8. **Guarantee keyboard reachability of every control, and honour 2.1.4 if hotkeys are added** (turn off /
   remap / focus-scoped) ([2.1.4](https://www.w3.org/WAI/WCAG22/Understanding/character-key-shortcuts.html)).
9. **Complete the reduced-motion path**: the media query exists, but confirm *every* animation (tile fly, meld
   landing, drawn pulse, wall-bar transition) is covered, and that the in-game switch is a labelled control, not a
   bare checkbox ([MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/@media/prefers-reduced-motion),
   [2.2.2](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html)).
10. **Hold the invariant that changing numbers never move**, and raise anything read under time pressure to
    ≥ 14px; scores stay `tabular-nums` and fixed-width
    ([MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/font-variant-numeric),
    [1.4.3 large-text clause](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)).

---

## Deliverable 3 — Non-negotiable riichi display conventions

Each of these, if wrong, is a bug in the eyes of a player rather than a style choice.

1. **One sideways tile per riichi, in that player's pond** — and when the declaration tile is called away, the
   *next* discard inherits the sideways mark ([koba::blog](https://blog.kobalab.net/entry/2026/03/30/070404)).
2. **The riichi stick sits in front of the declarer's own pond, parallel to it** — not at the table centre
   ([土田](https://mj-news.net/column/tsuchida-mjall/tsuchida_mahjongdou/2017061840543), [koba::blog](https://blog.kobalab.net/entry/2026/03/30/070404)).
3. **Riichi sticks survive an exhaustive draw into 供託 next to the 積み棒; the pot is won by the next winner, the
   積み棒 are not** ([はじめての麻雀35](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407)).
4. **Honba and 供託 are two separate counters**, and honba add **300 points each** to a win
   ([same](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061940407)).
5. **The wall counter counts remaining live draws (70 at the start of a 4-player hand), decreasing on every draw
   including the rinshan draw; 0 ends the hand** — the 14-tile 王牌 is never drawn from
   ([はじめての麻雀34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406), [はじめての麻雀33](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061740405)).
6. **Up to 5 dora slots and up to 5 ura-dora slots**, revealed one at a time, kan timing per the engine's ruleset
   (ankan immediate; minkan/kakan deferred in Tenhou 段位戦)
   ([shan.js](https://blog.kobalab.net/entry/2026/03/30/070404), [天鳳](https://tenhou.net/man/)).
7. **裏ドラ is not shown until the win is declared**, and then it is revealed in place
   ([koba::blog](https://blog.kobalab.net/entry/2026/03/30/070404)).
8. **The noten penalty is a 3000-point split — 1000×3 / 1500×2 / 3000×1 — and 4-tenpai or 4-noten pays nothing**
   ([はじめての麻雀34](https://mj-news.net/column/tsuchida-mjall/tsuchida_guide/2016061840406)).
9. **Meld orientation is fixed**: chi sideways-tile leftmost; pon/minkan sideways tile in the slot of the source
   (left = 上家, middle = 対面, right = 下家); ankan `[back][face][face][back]`; melds to the right of the hand in
   chronological order (`meld-layout-report.md`, with its own citations).
10. **Each seat shows its own wind plus a dealer marker, and the round wind/number is shown separately** —
    conflating 自風 with 場風 must be impossible ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).
11. **A call or riichi announcement is attributed to the player who made it**, in that player's area, and persists
    until the resolving event ([koba::blog 盤面](https://blog.kobalab.net/entry/2026/04/06/083417)).
12. **A called tile leaves the pond** (dimmed to ~40 % at most, or removed): the pond is a history of what is still
    available, not of what was touched ([koba::blog 牌山と河](https://blog.kobalab.net/entry/2026/03/30/070404)).

---

## Not verified in this pass (do not treat as sourced)

- WCAG 3.0 draft status and APCA as a contrast method: **not researched**; nothing here depends on them.
- The Understanding pages for **1.4.1, 1.4.4, 1.4.12, 2.1.1, 2.4.7, 2.4.11** were not opened, so those SC numbers
  appear only where a page I *did* read referenced them.
- Vendor touch guidance (Apple HIG 44pt, Material 48dp): reached only as links from the 2.5.5 Understanding page.
- Okabe–Ito / Paul Tol palette values and colour-blindness simulators (Coblis, Sim Daltonism, DevTools
  emulation): named from general knowledge, not fetched.
- Japanese-language typography guidance (JIS X 8341-3, government web accessibility guidance): not researched.
- Whether a ron on the riichi declaration tile takes the 1000-point deposit differs by ruleset: Tenhou says it
  does **not** ([天鳳](https://tenhou.net/man/)); a readable primary source for the contrary convention was not
  found.
- `kinmaweb.jp` article bodies and all Japanese Wikipedia pages were unreachable from this environment, and PDFs
  (league 競技規定, WRC/EMA rulebooks) cannot be fetched by the tool at all.
- The client's CSS/JS changed while this report was being written (`web/style.css` mtime moved mid-pass), so
  line-level claims are a snapshot rather than a stable contract.

---

## Appendix — deltas from the second (delegated) accessibility pass

A parallel accessibility pass covered the sources I did not open. Its findings that **change or extend** the
sections above, with its citations:

- **WCAG 3.0 / APCA**: the Working Draft keeps contrast as an outcome ("Text contrast sufficient"), defines a
  "contrast ratio test" as *Exploratory*, names **no algorithm**, and carries an editor's note that "The contrast
  algorithm used in WCAG 3 is yet to be determined" — **APCA is not named in the draft at all**, it self-describes
  as a *candidate* for WCAG 3 and reports `Lc` values (Lc 90 preferred for body text, Lc 75 minimum body text,
  Lc 60 non-body text, Lc 45 large/heavy text and fine pictograms, Lc 30 absolute floor for text/"mostly solid"
  icons, Lc 15 non-semantic non-text) ([WCAG 3.0 WD](https://www.w3.org/TR/wcag-3.0/),
  [APCA in a Nutshell](https://git.apcacontrast.com/documentation/APCA_in_a_Nutshell)). Conclusion: **do not
  design to WCAG 3 or APCA yet**, but note APCA's argument that WCAG 2.x math overstates contrast for dark
  colours — which is exactly the dark-felt case, so prefer thresholds comfortably above 4.5:1 for text on felt.
- **1.4.1 Use of Color (A) — normative**: "Color is **not used as the only visual means** of conveying
  information, indicating an action, prompting a response, or distinguishing a visual element"; the doc adds that
  colour coding is fine "if it is complemented by other visual indication", and where a user must "perceive or
  differentiate a particular color an additional visual indicator will be required **regardless of the contrast
  ratio**". Techniques G14/G182/G205/G111 (colour **+ pattern**, for colour inside images — our exact case);
  failure F81 ([Understanding 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html)).
- **Prevalence, cross-checked**: NEI says only "About 1 in 12 men"; the 8 % / 0.5 % pair is
  [Colour Blind Awareness](https://www.colourblindawareness.org/) ("1 in 12 men (8%) and 1 in 200 women"), and
  [Okabe & Ito](https://jfly.uni-koeln.de/color/) add ancestry splits (8 % Caucasian, 5 % Asian, 4 % African
  males). Okabe–Ito's own rules are worth following literally: vermilion instead of red "since it is
  recognizable also to protanopes", "Colors between yellow and green are all avoided", and "Use not only different
  colors but also a combination of different shapes, positions, line types and coloring patterns".
- **Palette caveat that must not be misread**: the second pass computed ratios for *hypothetical* felt colours
  (`#0B3D2E`) and found that the reds/greens a designer instinctively reaches for fail there — `#DC2626` = 2.53:1
  and `#166534` = 1.71:1, i.e. **below even 1.4.11's 3:1** — and that on dark felt light warm hues carry
  (`#FFD54F` = 8.65:1, `#7DD3FC` = 7.32:1, `#FFFFFF` = 12.20:1). That is a warning about palette *choices*, not a
  finding about this client: the client's **actual** hexes all pass (§2.1, measured). Keep both facts in mind —
  the palette is fine today, and the next colour added is where it typically breaks.
- **Vendor targets (advisory, now sourced)**: Apple's HIG gives **44×44 pt as the default control size and
  28×28 pt as the stated minimum** (with ~12 pt padding around bezelled controls, ~24 pt otherwise) — so the
  common claim that "Apple requires 44×44" is imprecise; Android/Google recommends **≥48dp×48dp**
  ([Apple HIG](https://developer.apple.com/design/human-interface-guidelines/accessibility),
  [Android accessibility](https://developer.android.com/guide/topics/ui/accessibility/apps)).
- **Dense-board maths worth doing before shrinking tiles**: a 360 CSS px viewport ÷ 13 tiles ≈ **27.7 px per
  tile**, so a full hand just clears 24×24 — but a 14th drawn tile, melds, or any padding pushes tiles under it.
  2.5.8's spacing exception then demands that a 24 px-diameter circle on each tile's bounding box not intersect
  another *target* (so ~20 px tiles need ≥4 px gaps, and a wrapped second row must clear them vertically too),
  and the requirement is **independent of zoom**. The safe moves are to keep the *hit box* ≥24×24 even when the
  drawn tile is smaller, never overlap hit areas, and provide an "Equivalent" path (a keyboard-accessible hand
  list or a larger-tile layout) — which is what turns a dense-board case into a defensible conformance claim
  ([2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)).
- **Screen-reader architecture, more precisely**: `role="log"` has an implicit `polite` and `aria-atomic="false"`
  and is explicitly for "chat logs, messaging history, **game log**, or an error log" — it appends to the end and
  **requires an accessible name**; `role="status"` is for advisory status text; `role="alert"` equals
  `assertive` + `aria-atomic="true"` and should be reserved for connection loss, timer expiry, furiten
  ([MDN log](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Roles/log_role),
  [status](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Roles/status_role),
  [alert](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Roles/alert_role)). Never
  `aria-atomic="true"` on a growing log (it re-reads the whole scrollback every discard); use `aria-busy` while
  applying a batch; keep log nodes plain text since live regions are announced as plain text; and coalesce bursts
  (~150–300 ms **[approximate]**) into one sentence per reaction.
- **Hand-as-widget semantics**: a single-row hand is best exposed as a labelled `role="toolbar"` with **roving
  tabindex** (only one tile in the tab sequence, arrows move within), or as a layout grid if it wraps to two rows;
  avoid `listbox` (tiles are actions, not a selected value) and avoid data-grid semantics; prefer roving tabindex
  over `aria-activedescendant`
  ([APG keyboard interface](https://www.w3.org/WAI/ARIA/apg/practices/keyboard-interface/),
  [Toolbar](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/), [Grid](https://www.w3.org/WAI/ARIA/apg/patterns/grid/)).
- **Text alternatives and structure**: keep labels in DOM text rather than only inside the tile SVG — 1.4.12
  states that "canvas implementations of text are considered to be images of text"; DOM order must be the reading
  order (1.3.2, technique C27) and instructions must not rely on shape/colour/size/location/sound alone (1.3.3,
  failure F14); a clickable tile must never be `aria-hidden`, while the felt and tile backs should be
  ([1.3.2](https://www.w3.org/WAI/WCAG22/Understanding/meaningful-sequence.html),
  [1.3.3](https://www.w3.org/WAI/WCAG22/Understanding/sensory-characteristics.html),
  [aria-hidden](https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Attributes/aria-hidden)).
  Sound cannot substitute for a text cue (1.1.1's own example pairs a sound effect with a text description).
- **Resize and spacing, normative numbers**: **1.4.4** — text resizable to **200 %** without loss of content or
  functionality (failure F69 = clipping at 200 %); **1.4.12** — no loss when line height ≥**1.5×**, paragraph
  spacing ≥**2×**, letter spacing ≥**0.12×**, word spacing ≥**0.16×**, and it notes Japanese does not use
  paragraph spacing; **1.4.10 Reflow** — no two-dimensional scrolling at **320 CSS px** width, with Note 2 naming
  "**games**" among content that legitimately needs 2-D layout (individual cells still need Reflow)
  ([1.4.4](https://www.w3.org/WAI/WCAG22/Understanding/resize-text.html),
  [1.4.12](https://www.w3.org/WAI/WCAG22/Understanding/text-spacing.html),
  [1.4.10](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html)).
- **Japanese typography, sourced (this replaces the [approximate] notes in §2.8)**: Japan's Digital Agency design
  system recommends a body line-box "少なくとも1.5倍" with 150 % as the floor, and sanctions **120–130 %
  ("Dense") for 管理画面や業務システム** where density matters; its baseline body/UI size is **16 CSS px 以上**, with
  14 px 「基本的には使用しません」 and 「14 CSS px未満の大きさの使用は原則として許容されません」 — i.e. **buy density
  with tighter line-height, not with 12 px text**; and it verifies the `'Noto Sans JP', sans-serif` stack
  ([デジタル庁 DADS タイポグラフィ](https://design.digital.go.jp/dads/foundations/typography/)). Japanese
  conformance in practice is **JIS X 8341-3:2016** per [WAIC](https://waic.jp/guideline/), with a WCAG 2.2-derived
  revision still a draft proposal as of Dec 2025.
- **Motion, extra sources**: 2.3.3 Animation from Interactions (AAA) requires interaction-triggered motion
  animation to be disableable unless essential, and explicitly does **not** count "changes of color, blurring, or
  opacity which do not change the perceived size, shape, or position" as motion animation — which is why a
  cross-fade is the safe reduced-motion substitute
  ([2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html)). 2.2.1's Real-time
  Exception plausibly covers a server-enforced turn clock in live human-vs-human play, but not a vs-AI or async
  game ([2.2.1](https://www.w3.org/WAI/WCAG22/Understanding/timing-adjustable.html)). Val Head's three risk
  factors (relative size of movement, mismatched direction/speed, distance covered) and Josh Comeau's argument
  against the blanket `animation-duration: 0.01ms` reset are the best practical guides
  ([A List Apart](https://alistapart.com/article/designing-safer-web-animation-for-motion-sensitivity/),
  [Comeau](https://www.joshwcomeau.com/react/prefers-reduced-motion/)); Material's 150–200 ms desktop transition
  guidance is a usable ceiling ([Material 1 motion](https://m1.material.io/motion/duration-easing.html)).
- **Extra element rows to fold into Deliverable 1**: a **timer** should show remaining time as text (mm:ss), not
  as a shrinking ring alone (4.5:1 text / 3:1 ring, 2.2.1); **danger/safe hints** need a pattern-density grade plus
  an in-place legend rather than a red→green heatmap (1.4.1 G111 — and Okabe–Ito warn that "matching same colors in
  distant places is extremely difficult", so never push users to a far-away key).
