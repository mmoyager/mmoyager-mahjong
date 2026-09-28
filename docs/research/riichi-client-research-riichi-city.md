# 麻雀一番街 / Riichi City — client research

Completes the set in `riichi-client-research.md` + `riichi-client-research-addendum.md`. Read this as the
§3 that the main document left as a placeholder.

**Developer/publisher:** Formirai Co., Ltd. (Steam appid 1954420, released 2022-09-08, free-to-play with
in-app purchases, cross-platform multiplayer)
([Steam appdetails API](https://store.steampowered.com/api/appdetails?appids=1954420&l=japanese)).
Unity, mobile-first, on iOS/Android/Steam/Windows/Mac/Linux
([iTunes lookup](https://itunes.apple.com/lookup?id=1578816591&country=jp)).

**Method and evidence quality.** Most third-party community pages (NGA, 百度贴吧, ptwweb, applion, altema,
Google Play) returned 403 or Cloudflare challenges, so **no claim here rests on them**. The evidence is
(a) the JA Wikipedia mirror, the Steam appdetails/news/reviews APIs and the iTunes lookup API, (b) four
Japanese/English blog and review pages, and (c) **the official Steam and App Store match screenshots
downloaded at full resolution and inspected directly, including 3× crops of the HUD, centre widget, ponds
and hand**. Layout and rendering claims are therefore primary visual evidence from developer-published
screenshots. Anything not established is marked **approximate**.

---

## 1. Layout

**Orientation is fixed with self at the bottom.** Both reference match screenshots agree, and no
rotatable or 4-way view option appears in any reachable source (**approximate** — absence of evidence, not
a documented denial). The game is landscape-only; GameWith tags it 横画面
([GameWith](https://gamewith.jp/gamedb/article/game/show/7432/19657)).

Measured from the 1920 × 1080 Steam screenshot:

| Element | Placement |
|---|---|
| **Own hand** | one single flat row of 14 tiles along the bottom, **left-aligned** — the 13-tile block starts near the left edge, then a gap, then the drawn tile at the right end |
| **Centre** | an octagonal "table plate" widget dead centre, ringed by the four ponds; seat winds 東/南/西/北 on **coloured corner tabs** |
| **Ponds** | grids hugging the centre plate on each player's side, **rotated to face their owner** (the top player's pond is drawn 180° to the viewer); roughly **6 tiles per row** in the visible rows (**approximate** — read off a screenshot) |
| **Player panels** | square avatar with a coloured ring frame, a name plate, and a 称号 title bar beneath. Left opponent upper-left, across-player top-right, right opponent mid-right, **self bottom-left**. **No score is shown in the panels.** |
| **Action buttons** | a **vertical stack of four single-kanji buttons on the left edge** — 理 (riichi, highlighted green when available), 和 (win), 鳴 (call), 切 (discard/pass) — with a small ▶ expander below. Wikipedia JA confirms settings/options sit "at the left edge" during a match ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)) |
| **Right edge** | `?` help and a gear (settings) at top-right; a circular emote/chat button mid-right |
| **Bottom-right** | the timer as two numbers, e.g. `4+18` (§5); after riichi a `!` button appears here that pops up your wait tiles ([キンマweb](https://kinmaweb.jp/game/mahjong-ichibangai)) |
| **Pending decisions** | `リーチ` / `パス` appear as wide horizontal buttons centred above the hand |

The design optimises for **spectacle plus usable density**: a large ornamental character bust and a full 3D
wall occupy most of the screen while the informational furniture is compressed into a small top-left stack,
a centre plate, four edge chips, and a four-button column. The mode/team label sits in a chip at top-left
(`四人・友人半荘戦`).

Two layout decisions are genuinely unusual and worth studying:

- **The hand is left-aligned, not centred.** Of the clients surveyed, 電脳麻将 and Riichi City left-align;
  しらぎく centres roughly; 天鳳's solver centres the row by construction. Left alignment puts the drawn
  tile in a **fixed** screen position, which is faster to acquire at speed — but it makes the discard
  target move as melds accumulate.
- **The action buttons are single kanji in a vertical column on the left edge**, not a horizontal row of
  labelled verbs. This is the most compact action surface in the survey — four ~1-character targets — at
  the cost of legibility for a non-Japanese reader and of putting the primary interaction on the opposite
  side of the screen from the hand.

## 2. Information design

- **Top-left HUD panel** (always visible in-match): the match-mode label, then a row of **tile-shaped slots
  showing the dora indicator(s) face-up**, followed by blank pale-outlined placeholder slots for kan-dora
  indicators; below that, **two real point-stick (点棒) icons each with an `×N` counter** — one stick with
  a red dot and one with a dot pattern, i.e. riichi sticks placed and honba respectively (both `×0` in the
  reference screenshot).
- **Centre plate:** an inner dark rectangle showing **round wind + hand number** (`東三局`) and, in **large
  cyan type, the remaining wall count** (`36`), with a **green horizontal progress bar** beneath it
  showing wall depletion. The **four scores** (`25000` / `28300` / `21700` / `25000`) are printed around
  the plate, **each rotated to face its owner** — the top player's score is upside-down to the viewer.
- **Round/honba/riichi sticks are shown as objects, not text.** Dora is a row of face-up tiles; riichi
  sticks and honba are stick icons with multipliers. Riichi City is the only client in this survey that
  renders honba as a **stick count with an ×N badge** — the physically correct representation, and better
  than 電脳麻将's bare number or 天鳳's numeric honba.
- **Text vs icon: overwhelmingly icon and numeric.** The only running text on the match screen is the mode
  chip and the player names/titles.
- **No in-match action log found.** The game record lives outside the match: the home screen's bottom nav
  has a **牌譜 tab**, and there is a separate official **牌譜屋** site, which per Wikipedia JA covers
  **銀河卓 only, and only games from 2025-06-18 05:00 onward**
  ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)).
  Because no match with many turns logged was available for inspection, the absence of an in-match log is
  **approximate**.

The **rotated-per-owner scores** are the notable idea here: the centre plate uses the physical-table
convention that each player reads their own information upright, which is exactly what 電脳麻将 and
しらぎく do *not* do (both draw all scores upright). It costs nothing and removes a persistent source of
misreading.

## 3. Tile and table rendering

- **Flat 2D tiles with a thin extruded side and a drop shadow** — essentially 2.5D top-down, not polygonal
  3D. Each pond tile shows a visible bottom edge in the crops. Tile aspect looks roughly **1 : 1.3–1.4**
  — **approximate**, measured by eye.
- **The wall is the genuinely 3D element, and it is the client's best rendering idea.** The live wall is
  rendered as **real stacked tile rows on all four sides of the table** — a short two-row stack top,
  vertical stacks left and right, a near-side stack bottom-left with a **highlighted tile marking the next
  draw**. A Steam reviewer who plays both clients names this as a concrete advantage: *"The wall is visible
  during a game. Both RC and MS show the number of tiles left in the center of the compass, but actually
  seeing the wall makes it much easier to visualize how much longer a hand will go before exhaustive
  draw"* ([Steam reviews API, English](https://store.steampowered.com/appreviews/1954420?json=1&num_per_page=30&language=english&filter=all&purchase_type=all)).
  This is worth taking seriously: it converts an abstract counter into **spatial** information about
  remaining hand length, which is how players actually reason about a hand.
- **Felt:** teal/dark green cloth with faint seam lines, over an anime-style scene; screenshots show both a
  "winter" skin and a plain green skin.
- **Tile faces:** standard cream/white with red 萬子, green 索子, blue-black 筒子, red-accented dora.
- **Tile backs differ between screenshots (light blue vs green)** because tile backs are a **purchasable
  cosmetic**: the App Store description lists カスタマイズ可能な卓背景、麻雀牌、立直棒
  ([iTunes lookup](https://itunes.apple.com/lookup?id=1578816591&country=jp)), and the update notes list
  "Tiles", "Tabletop", "Table Frame" and "Riichi Stick" as separate slots, some "Animated"
  ([Steam news API](https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=1954420&count=40&maxlength=8000&format=json)).
- **Tsumogiri is rendered dark grey in the pond.** In a crop of the left player's pond, one tile is drawn
  dark grey against cream neighbours. This is a documented setting, **「ツモ切り暗転表示」**, and per the
  reviewer it applies in **ranked play, not just CPU games**
  ([ぽんぽんロン](https://ponponron.com/category1/ichibangai.html)).
- **Sideways riichi tile:** in the same crop a tile is rotated out of the pond's own grid orientation
  (perpendicular to its neighbours), read as the riichi declaration tile. The exact rotation/dimming
  combination is **approximate**.
- **Kan-dora indicators are not laid on the table**; they appear as revealed slots in the top-left HUD.

## 4. Input model

- **Click/tap to discard.** Both screenshots show a flat, non-dragged hand; no source describes
  drag-to-discard. Treat click as the model and drag as **unverified but undocumented**.
- **Right-click is the signature PC affordance, and it is better than either famous client's.** Per
  ぽんぽんロン: 「特に快適なのが、捨て牌選択時に右クリック一つでツモ切りや鳴きのパスができること。これは天鳳や雀魂には
  ないシステム」 — **one right-click does either tsumogiri or passes a call**, which the reviewer states
  天鳳 and 雀魂 do not have
  ([ぽんぽんロン](https://ponponron.com/category1/ichibangai.html)). Note the nuance: 天鳳 *does* have
  right-click pass (paid Windows/Flash only) and double-click tsumogiri, and Maru-Jan has right-click
  tsumogiri; Riichi City is the only one where a **single** right-click resolves **both** the discard and
  the call decision.
- **Touch:** the same single tap on a hand tile.
- **Keyboard shortcuts: none could be established.** No reachable source documents a keyboard map —
  **approximate / unverified**.
- **Auto/skip options** live in the left-edge in-match settings menu, per Wikipedia JA: **自動理牌**
  (auto-sort), **自動和了** (auto-win), **鳴きなし** (no calls), **ツモ切り** (auto-discard the drawn tile),
  plus **自動カン** (auto-kan, applied during riichi and when tsumogiri is on) and **自動北抜き** (auto
  north-pull, 3-player) ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)).
  ぽんぽんロン adds 「ツモ切り暗転表示」 and 「パス・ツモ切りの方式」 (the *method* used for pass/tsumogiri,
  i.e. the right-click behaviour is configurable).
- **Assists:**
  - After riichi, the bottom-right **`!` button reveals your waiting tiles**
    ([キンマweb](https://kinmaweb.jp/game/mahjong-ichibangai)).
  - **A full Mortal AI integration.** You can run your own 4-player games through the AI **inside the
    client**, and the analysis mode **rates you 1–5 stars on agreement with the AI**, with a per-turn
    comparison of your choice against the model's — costing in-game vouchers, or 10 free per day with the
    monthly mascot ([Gamesoft Robo Fun Club](https://gamesoftrobo.ghost.io/untitled-6/)). **No client in
    this survey comes close to this**, and it is the strongest learning feature found anywhere.
  - A Duolingo-style tutorial/practice mode with AI review, repeatedly praised in Steam reviews.
- **No evidence of a shanten/tenpai HUD, discard-hint arrows, or a furiten/safety indicator** — treat as
  **probably absent**.
- **Documented settings:** `Settings > General > Graphics Quality` (High/Medium/Low)
  ([Steam news, 09/29](https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=1954420&count=40&maxlength=8000&format=json));
  **SFW Mode**, which since 2026-08-11 has an **"Apply to My Character"** sub-option (off = only your own
  outfit is shown, everyone else reverts to default); a **watermark toggle** added 2026-07-30, drawn
  **top-left of the screen**.

## 5. Motion and sound

- **Physical table animation is simulated, not decorative.** キンマweb notes 「局の最初にドラがアップになる、
  14トンを区切ってリンシャン牌を下ろす」 — at the start of a hand the dora indicator is **flipped up**, and
  the wall's 14-tile segment is **cut and the rinshan tile lowered**
  ([キンマweb](https://kinmaweb.jp/game/mahjong-ichibangai)). Drawing and discarding motion is described as
  very smooth; **chat stamps are animated, not still images**; and wins of **満貫 or above get ド派手な
  エフェクト**.
- **Character cut-ins and voice:** full voice acting (calls and yaku read-outs) with animated cut-ins on
  big hands ([GameWith](https://gamewith.jp/gamedb/article/game/show/7432/19657)); the App Store copy
  advertises 動ける立ち絵 (animated standing art).
- **Animation is a paid cosmetic dimension with named per-action entries**: "Winning Animations" (e.g.
  *Phoenix Cry*, *Quintuplets' Magic*, *Zafkiel*, *Fallen Down*, *Wave Crash*), a separate
  **"Riichi Animation"** class played on declaration (*Feather Storm*, *Evaluation*), and "Animated"
  tabletops, table frames and riichi sticks
  ([Steam news, 09/29 and 07/10](https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=1954420&count=40&maxlength=8000&format=json)).
- **The animation/effect control is only `Settings > General > Graphics Quality`**, whose three levels
  "vary visual effects" and explicitly optimise winning animations, riichi animations, tabletops, table
  frames and riichi sticks. **No explicit "skip animation" or reduced-motion toggle was found** — a
  genuine, notable gap, and the sharpest contrast with しらぎく麻雀 and MJ. Complaints about excess
  animation are common: the **top-voted Japanese negative Steam review is titled 「演出が過剰すぎて遅延と
  変わらない仕様」** ("the effects are so excessive it's no different from lag")
  ([Steam reviews API, Japanese](https://store.steampowered.com/appreviews/1954420?json=1&num_per_page=30&language=japanese&filter=all&purchase_type=all)).
- **Turn pacing:** ranked 4-player is **1 discard per 5 seconds plus a 20-second bank**, and running out
  auto-discards ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)).
  The UI shows this as two stacked numbers, e.g. **`4+18`** (remaining 5-second allowance + remaining
  bank), and the timer reappears during calls. This is a **different presentation of the same clock Tenhou
  has** — Tenhou shows a single numeric countdown with ticks and a +1 refund; Riichi City shows both pools
  simultaneously, which is more informative but less glanceable.
- **Sound:** in-match BGM **changes on riichi**, and Lobby/In-Game/Riichi BGM are separately purchasable.

## 6. Settlement and result screens

- **Ron/tsumo cut-in:** the App Store screenshot of the 「役満の女神」 cut-in shows a full-bleed
  illustration with the yaku name as **huge characters** (天和) and **the winning hand laid out face-up as
  a flat tile row across the illustration** ([iTunes lookup screenshots](https://itunes.apple.com/lookup?id=1578816591&country=jp)).
  Putting the actual winning hand into the cut-in, rather than only into a dialog, is a nice touch.
- **Round settlement:** points are calculated automatically on win and the timer is displayed during the
  agari ([キンマweb](https://kinmaweb.jp/game/mahjong-ichibangai)). Beyond that the **exact round-end panel
  contents (yaku list / han-fu / dora breakdown layout) are approximate**. The clearest official evidence a
  yaku list exists is the practice-mode change: *"We will extend the display time for the Practice Match
  results screen. Players will be able to tap on Yakus to view detailed description popups"* — i.e. the
  result screen is **a list of tappable yaku entries**.
- **Uma/oka are documented as rules, not as UI:** 配給原点 25,000, 返し点 30,000, 順位点 1st +30 / 2nd +10
  / 3rd −10 / 4th −30, with oka
  ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)).
- **Post-game:** (a) the **Mortal AI analysis view** with a 1–5 star accuracy rating and turn-by-turn
  comparison; (b) a **牌譜 tab** in the home-screen bottom nav; (c) the separate official **牌譜屋** site,
  limited to 銀河卓 and to games from 2025-06-18 onward. A separate "Dossier → Replay Memories" section
  replays **event stories, not matches**. Whether the in-client replay viewer offers seek/step/AI overlays
  is **approximate**.
- **Rank layer:** 21 ranks from 初登場 to 天下一番, with rank points changing **by placement only — no 素点
  term**, unlike 雀魂
  ([Weblio/Wikipedia JA](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E4%B8%80%E7%95%AA%E8%A1%97)).

## 7. Versus 雀魂

**Better.** Riichi City's clearest wins are informational and procedural, and three of them are worth
copying outright. First, **the table is physically legible**: the live wall is rendered as real stacked
tiles on all four sides, so a player can *see* how deep into the hand they are instead of reading a number
in the compass — a reviewer who plays both clients names exactly this as why the wall matters
([Steam reviews API, English](https://store.steampowered.com/appreviews/1954420)). Second, **tedashi vs
tsumogiri is disambiguated by rendering drawn-tile discards dark grey** (`ツモ切り暗転表示`), applied in
ranked play ([ぽんぽんロン](https://ponponron.com/category1/ichibangai.html)) — the same insight しらぎく
and MFC arrived at independently. Third, **the PC input contract is better**: a single right-click resolves
either tsumogiri or a call pass, which the same reviewer says 雀魂 (double-click) and 天鳳 lack. Fourth,
**learning and review tooling is far deeper** — built-in Mortal analysis with per-turn comparison and an
accuracy rating, plus a Duolingo-style tutorial that multiple English reviewers call the best new-player
experience of any client. Fifth, **the four centre scores are each rotated to face their owner**, matching
the physical table and beating the upright-everywhere convention. Sixth, customisation is a whole product
surface: tiles, tabletop, table frame, riichi stick, winning animation and riichi animation are independent
and sometimes animated.

**Worse.** Riichi City is markedly more gacha- and fanservice-forward, and this is the single most-cited
negative: reviewers call the outfits and cut-ins over-sexualised and say **SFW Mode is weak** — originally
it only forced everyone to default outfits, and the "Apply to My Character" refinement only landed in
2026-08 ([Steam reviews API, English](https://store.steampowered.com/appreviews/1954420)). Several reviews
claim event and shop pages still leak NSFW art through the filter. Second, **presentation overhead is the
domestic complaint**: the top-voted Japanese negative review is that the effects amount to lag, and the
only remedy is a three-step Graphics Quality setting with **no obvious skip-animation switch** — a real
accessibility regression against しらぎく麻雀's named toggle. Third, the shell outside the match is weak: a
Japanese review says the match screen is fine but the **home and shop UI are hard to read**, and calls the
game 雀魂に似すぎている ("too similar to Mahjong Soul"). Fourth, population and record depth are thin —
牌譜屋 covers only 銀河卓 from 2025-06-18, and rank points ignore 素点 and rating, unlike 雀魂. Fifth,
RNG-trust complaints are loud in both corpora (「ツモが偏りすぎ」, "RNG IS INSANE") with the developer
publicly rebutting each. Finally the Steam review split is itself a signal: **988 English reviews at 750
positive ("Mostly Positive") versus 336 Japanese reviews at only 139 positive ("Mixed")**
([English](https://store.steampowered.com/appreviews/1954420),
[Japanese](https://store.steampowered.com/appreviews/1954420)) — the domestic audience is the harsher one,
which for a Japanese-market-oriented client is the verdict that matters.

---

## 8. What this changes in the cross-client picture

**New entry into the ranked top-10 — the visible wall.** Riichi City renders the live wall as real stacked
tiles on all four sides with the next draw highlighted. Every other client — including 天鳳, 電脳麻将,
雀魂 and MJ — reduces the wall to **a number in the centre**. Rendering it converts an abstract counter into
spatial information about how many more turns the hand can run, which is how players actually reason, and a
reviewer independently identified it as the feature that makes hand length intuitive
([Steam reviews API, English](https://store.steampowered.com/app/reviews/1954420)). This is the single
highest-value rendering idea found in any client surveyed, and it composes badly with nothing — it is
purely additive to a hand/pond/centre layout.

**Updated divergence table rows.**

| Row | Update |
|---|---|
| **Where scores live** | Riichi City is a fourth and distinct answer: **all four scores in the centre plate, each rotated to face its owner**, with **no score at all in the player panels**. This is the physical-table convention and is better than 電脳麻将's and しらぎく's upright-everywhere centre scores. |
| **Hand alignment** | Riichi City is **left-aligned**; so is 電脳麻将. しらぎく centres approximately. 天鳳 centres by construction. This is a genuine split, and left-alignment fixes the drawn tile's screen position. |
| **Honba / riichi-stick display** | Riichi City renders both as **point-stick icons with ×N multipliers** — the physically correct representation — against 電脳麻将's bare numbers and 天鳳's numeric honba. しらぎく's is unverified. |
| **Tsumogiri indication** | **Four clients now encode it**: 天鳳 (paid ツモ切り暗転表示), しらぎく (grey, optional, user-requested, default off), MFC (darker), Riichi City (dark grey, **applied in ranked play**). It is as close to a universal convention as anything in this survey. |
| **Wall representation** | **Only Riichi City draws the wall as tiles.** All others use a numeric counter. |
| **Animation toggle** | Riichi City **has none** — only `Graphics Quality` (High/Medium/Low) — despite being the client with by far the heaviest animation load, and its top Japanese negative review is precisely about effect overload. This strengthens the case that a proper toggle is a differentiator, not table stakes. |
| **Keyboard** | Unchanged: only MFC (PC) and Maru-Jan (Windows) have keyboard play. Riichi City has none. |
| **Right-click semantics** | Three clients use right-click, with **three different meanings**: 天鳳 = pass only (paid builds); Maru-Jan = tsumogiri anywhere on the table, suppressed during a ron/tsumo prompt; **Riichi City = one gesture that resolves either tsumogiri or a call pass**, and which is configurable via 「パス・ツモ切りの方式」. Riichi City's is the most complete; 雀魂 has double-click only. |
| **Analysis tooling** | Riichi City's **in-client Mortal integration with a 1–5 star agreement rating and per-turn comparison** has no peer in this survey. The nearest are Tenhou's click-a-tile-to-open-牌理 in the replay viewer and MFC's 牌譜→何切る quiz pipeline. |
