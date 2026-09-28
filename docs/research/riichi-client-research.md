# How established riichi clients design their tables

Research for the from-scratch browser client in `mmoyager_mahjong/web`. Covers 天鳳, 電脳麻将,
麻雀一番街 (Riichi City), MJ / SEGA NET MJ, 麻雀格闘倶楽部, しらぎく麻雀, plus open-source clients.
Companion to `meld-layout-report.md` (meld geometry) and `riichi-client-display-a11y-report.md`.

## Method and evidence status

Three evidence classes are used, and they are not equal:

- **Source-verified** — read from the client's own shipped code or its official manual. Strongest.
  For 天鳳 I downloaded the live bundle `https://tenhou.net/3/latest.js` (225 KB, minified) and read it;
  for 電脳麻将 I read `kobalab/Majiang@master` `src/css/*.styl`, `src/html/inc/*.pug`,
  `src/js/*.js` and `kobalab/majiang-ui@master` `lib/*.js`.
- **Documented** — stated on the operator's own how-to/settings page. Good.
- **Approximate** — inferred from a screenshot, a forum post, a search snippet, or a visual reading of
  minified code. Marked **approximate** inline.

Anything I could not establish at all is called out rather than guessed. Fetched pages were treated as
data; nothing in them was executed or followed as an instruction.

---

## 0. 雀魂 (Mahjong Soul) — the baseline being compared against

The brief says "other than 雀魂", but every "better/worse" claim needs an anchor, so two verified facts
about 雀魂's input layer matter:

- **雀魂 has no native keyboard layer.** The existence and popularity of the unofficial extension
  「雀魂(じゃんたま)キー打ち」 — which maps number/symbol keys to discards, `Space` to tsumogiri/skip and
  `Z`/`X`/`C`/`V` to auto-sort and similar toggles — is direct evidence that players want keyboard play
  and cannot get it natively. The extension's own description says it deliberately excludes
  balance-affecting features such as 危険牌表示 (danger-tile display)
  ([Crx搜搜 listing for 雀魂(じゃんたま)キー打ち](https://www.crxsoso.com/webstore/detail/bfdfnkojgajbgnclggaejpjifihfjnmf)).
- A second extension, 「ツモ切りにゃ！」, exists purely to add auto-tsumogiri
  ([Chrome Web Store](https://chromewebstore.google.com/detail/%E3%83%84%E3%83%A2%E5%88%87%E3%82%8A%E3%81%AB%E3%82%83%EF%BC%81/edapfebegphhagpgaknifmjdnckcnkcf)).

Beyond that I did not audit 雀魂's rendering or settings, so comparative statements below are framed as
"what the other client demonstrably does", not as audited claims about 雀魂 internals.

---

## 1. 天鳳 (Tenhou.net)

Tenhou is the oldest and most information-dense of the group, and the least helpfully documented. It
ships in several builds with materially different feature sets — Windows (paid), Flash Premium/Economy
(legacy), Web, app, Desktop4K — and one comparison table on the official manual lists what each build
gets, including several features explicitly gated behind payment
([天鳳 マニュアル／有料版・無料版について](https://tenhou.net/man/)).

### 1.1 Layout

The table is drawn on a single `<canvas>` (`kc.P.canvas`) with the hand, pond, dora and meld tiles as
canvas objects addressed as `U[1024|who<<8|slot]` (hand), `U[3072|…]` (pond), `U[2048|…]` (meld) and
`U[5120|n]` (dora). Only the chrome — the command buttons, the timer, the settings dialog — is DOM.
This is the opposite of 電脳麻将, which puts every tile in the DOM.

Rather than fixing a layout per breakpoint, Tenhou **solves for tile size**. `Q.ca(width, height)`
iterates a tile width downward from 200 px and, for each candidate, computes the whole table and tests
whether it fits (`return f <= l && m <= g`). The constants that fall out of that solver, read from
`latest.js`:

| Quantity | Value | Evidence |
|---|---|---|
| Tile face proportion | **width 31 : length 47** (≈1 : 1.516) | `k[0]=~~(47*b[0]/31)-d[0]` |
| Visible tile thickness/depth | **12 units at width 31** (≈0.26 × length) | `d[0]=Math.min(~~(12*b[0]/31), …)` |
| Own-hand row budget | **14.1 tile pitches** across the available width | `b[4]=~~((l-b[0])/14.1)` |
| Own-hand tile vs opponent/pond tile | capped at **1.7×** | `b[4]=Math.min(b[4],~~(1.7*b[0]))` |
| Minimum board width | `14.1 × hand-tile + 1 small tile` | `f=Math.max(f,~~(14.1*b[4])+b[0])` |
| Centre info panel footprint | **2 tile-lengths + 1 tile-thickness** | `2*k[1]+d[0]` |
| Pond tile size derived from centre | `~~(31*(2*k[1]+d[0])/47)` | same expression |

The centre-panel rule is the single most reusable idea in Tenhou's geometry: the middle of the table is
sized to exactly match a two-tile stack plus one tile edge, which is precisely what the real object it
represents (the wall/dora stack) occupies. The solver then derives the *pond* tile size back from that,
so the middle of the table is the anchor and the tiles follow — not the other way round.

Drawn tile offset: the freshly drawn tile is nudged by 10 % of a tile in each axis
(`x.i=x.X+~~(.1*Q.vb[b])`). Called tiles being placed into a meld animate from their old position with a
distance-derived duration (`C=Math.sqrt(C*C+J*J)/4E3`), i.e. **animation time is a function of travel
distance**, not a constant.

**What the design optimises for:** turn latency and information per pixel. There is no ornament, no
character art, no cut-ins. A sighted, experienced player can read the entire game state in one fixation.

### 1.2 Information design

Always on screen: the wall count, round/honba, all four scores, the dora indicators, all four ponds, all
four hands (yours face-up, others as backs), and the call announcements. Two details are worth stealing:

- **Score bars are colour-coded by an irrelevant attribute.** The official manual states that the bar next
  to each score is 青 for male, 赤 for female, 緑 for COM, and that the provisional leader gets a white
  gradient ([天鳳 マニュアル／その他](https://tenhou.net/man/)). Tenhou uses colour for *identity*, not for
  state — a decision a new client should probably not copy, because it burns the strongest visual channel
  on the least useful variable.
- **Score difference on demand.** Hovering the centre panel shows the point differences between players
  — documented as Flash-Premium/Windows-only ([同](https://tenhou.net/man/)). The code confirms a
  centre-panel hover handler and a dedicated `rc` module that positions the countdown relative to whatever
  element currently has focus.

Round/honba/wall/dora are **numeric and textual**, not iconographic: the manual's own language is about
枚 numbers, and the dora is a row of face-up tiles rather than a badge.

**The action log.** I grepped the shipped bundle and found **no persistent in-game textual log panel**;
the table view has no scrolling event list. Tenhou's textual reputation rests on two other artefacts:

1. **The mjlog format**, which is Tenhou's native text serialisation and the de-facto interchange format
   for the whole riichi tooling ecosystem. Its pond array encodes *why* a tile is drawn specially:
   in Tenhou's own log parser, `255` in a kawa array means "riichi was declared on this discard" and
   `254` means "this tile was called away" (`255==r?(U.Fd(n,u),…):254==r?y=!0:…`). That is one byte per
   special case, and it is why 電脳麻将 and dozens of viewers can replay a game exactly.
2. **The replay viewer**, where the manual documents 「牌譜ビューア操作 : クリック/キー押下でステップ実行」
   — click or any key-press steps one event forward, right-click steps back
   ([天鳳 マニュアル／牌譜](https://tenhou.net/man/)). The paid Web/app builds additionally show the
   wall contents and the waits inside the log ([同](https://tenhou.net/man/)).

So the density people associate with Tenhou is: *every token of state is a number or a tile, and the
event stream is available as text on demand*, rather than a log printed on the table.

**Subtitles.** Tenhou renders 和了/流局 as on-screen "subtitles", and in tsumogiri mode the maximum
subtitle wait is **halved** ([同](https://tenhou.net/man/)) — an explicit design decision that an
attentive player should not be made to wait for an animation they have already opted out of.

**Furiten** is displayed by a dedicated element whose visibility the **server** drives
(`FURITEN:function(h){oc.style.display=~~h.show?"":"none"}`) — the client never computes it.

**Voice/call announcements** are bold white text with no background box, positioned at a per-seat anchor
(`fontWeight:"bold", color:"#FFF", whiteSpace:"nowrap"`) and suppressed entirely when the voice flag is
off (`~b&Hc.za && (n.style.display="none")`). Text and audio are two renderings of one event.

### 1.3 Tile and table rendering

- **Pseudo-3D, not flat.** The thumbnail depth of 12 units at width 31 is a real drawn side face, and a
  `Lb()` shading pass compares each tile's position with its neighbours
  (`this.i+Q.I[this.L]==this.ta.i+…`) to decide how to shade the joint. So Tenhou has a lighting model,
  not a drop shadow.
- **Tile back colour is user-configurable via RGB sliders** (settings page 6, buttons `hR`/`hG`/`hB`) and
  the background via an image URL plus hue/saturation/value sliders (`iH`/`iS`/`iV`)
  ([天鳳 マニュアル／有料版・無料版について](https://tenhou.net/man/), feature 「背景や牌背の色変更」;
  sliders read from `latest.js`).
- **Pond placement is deliberately not a grid.** The official manual states 「ツモ切り以外の打牌位置は
  ランダム(ツモ牌の位置を含まない)」 — every non-tsumogiri discard is jittered
  ([天鳳 マニュアル／仕様](https://tenhou.net/man/)). Tenhou is reproducing the irregularity of a real
  table. A browser client can copy this cheaply and gain a lot of texture; it does make automated
  screenshot diffing harder.
- **Called-tile dimming and assist highlighting share one mechanism.** Per-tile state flags are mapped to
  DOM background colours: `#060` (green) for a highlighted and *valid* tile, `#600` (red) for a
  highlighted but *invalid* one, and `#030`/`#300` for the unhighlighted variants. When you draw in
  tsumogiri mode, every tile except the drawn one is dimmed; when a call is available, the algorithm
  marks pon candidates with `c[d%9]|=3`, sequence partners with `|=2`, and dims anything whose suit
  differs or that fails the test. **Tenhou does have real in-client assist highlighting** — this is the
  most under-appreciated part of its design.
- **The sideways riichi tile** is not a rotated asset: the pond object `U.Fd(who, index)` marks the
  special slot from the `255` marker in the log/model, and the renderer draws that slot rotated.
  **approximate** — I read the marker handling and the `Fd` call but not the drawing routine itself.

### 1.4 Input model

**This is the brief's biggest correction: 天鳳 has no keyboard gameplay.** In the live bundle, `keydown`
occurs exactly once and only resets the idle timer; `keyup`, `keypress` and `keyCode` never appear.
Input is `touchstart`/`touchmove`/`touchend`/`mousedown`/`mousemove`/`mouseup` plus a `wheel` listener,
with `oncontextmenu` suppressed. The complete documented and code-verified model:

| Gesture | Effect | Source |
|---|---|---|
| Left click a hand tile | Discard it | code + [manual](https://tenhou.net/man/) |
| Right click | Pass | [manual](https://tenhou.net/man/) |
| Double click | The timeout action (pass / tsumogiri / OK) | [manual](https://tenhou.net/man/) |
| Mouse wheel | Step replay backward/forward — **replay mode only** | code (`z.a==3` guard) |
| Click / any key | Step replay forward | [manual](https://tenhou.net/man/) |
| On-screen `‹` `›` | Move the hand cursor one tile left/right | code (`CP_L`/`CP_R` → `qc.Ke(±1)`) |
| Drag | Manual tile sorting — **paid Windows build only** | [manual](https://tenhou.net/man/) |

Two thresholds worth copying verbatim: a click is treated as a tap when movement is **under 10 px** and
duration **under 1000 ms** (`Math.abs(r.pageX-n)<10*cb && … && 1E3>Date.now()-h`). That one line is the
whole tap-versus-drag arbitration.

The "keyboard-like" feel people remember is the **tile cursor**: a highlighted hand slot that the `‹`/`›`
buttons move, and which drives where the countdown timer is drawn (`rc.ca` snaps to tile centres). Tenhou
built keyboard ergonomics out of clickable arrows because it could not use keys.

**The timeout model is more interesting than the input model:**

- 「鳴かない」 mode passes every pon/open-kan/chi but not ron, and auto-clears at the start of each hand.
- 「ツモ切り」 mode passes everything except tsumo/ron/ankan/kakan/riichi, halves the win/draw subtitle
  wait, and auto-clears at hand start *if the mouse responds*.
- **60 seconds with no mouse movement or key input force-enables tsumogiri.** This is how Tenhou handles
  AFK players without kicking them.
- After riichi, tsumogiri is forced except for ron/tsumo/ankan.
- 自動和了 (auto-win) and 自動聴牌止め (auto tenpai-stop) exist; auto-win does not function if the
  connection drops.
- The timeout itself is a **visible numeric countdown** with ticks at ≤3 s and ≤1 s, and a floating "+1"
  that fades out over 250 ms with a 500 ms delay when reserve time is refunded
  (`transition:"all 250ms ease-in-out 500ms"`). Reserve time is refunded by responding within 1 s of a
  draw ([manual](https://tenhou.net/man/)).

All of the above: [天鳳 マニュアル／仕様](https://tenhou.net/man/).

### 1.5 Motion and sound

Tenhou is *fast by construction* rather than by toggle:

- **Yaku reveal is staggered.** On the win screen each yaku fades in over a share of a 600 ms budget
  (3000 ms for yakuman), with a **250 ms pause between yaku** and a longer 1200 ms pause before the ura
  dora row — but only if aka/ura dora are in play (`h=-(~z.H&t.Qb&&m&&n==f-1?1200:250)`).
- The win total is set at `font-size:250%` and the yaku table at `150%`; the 終局 title at `400%` in a
  brush typeface (`font-family: cwTeX-Q-Kai-T,icons2,serif`).
- The 終局 screen fades the whole board in with `transition:all 1500ms ease-out 0ms`.
- **Result screens auto-advance.** A negative timeout is passed to the countdown module
  (`rc.o(0==z.ia[0]&&0==z.ia[1]?-z.$c:…)`), so an attentive player never has to click OK — the hand
  moves on by itself.
- There is a large sound vocabulary (roughly 40 named effect slots in the enum `O`), and BGM/SE are
  replaceable with custom files in the paid builds.

**There is no setting called anything like "animation off", and no `prefers-reduced-motion` handling.**
Tenhou does not need one because it has almost no motion. What it offers instead is *aesthetic* control
(tile-back RGB, background image + HSV, custom BGM/SE). **approximate** for the labels of two settings
pages (`lth`, `yam`) whose strings live in a separate i18n bundle I did not fetch.

### 1.6 Settlement and result screens

- **Round-end (和了)**: big total, then yaku name / han value in a **two-column table at 50 % width each**
  (`p=4>n.length?0:Math.ceil(n.length/2)`), then the dora indicator row (up to 5 tiles) and the ura dora
  row, then OK. Yakuman is handled by a separate branch that prints 役滿 plus per-yakuman chip counts.
- **Draw (流局)**: the draw type name at 400 %, a payment/extent line, OK.
- **Game end (終局)**: a `width=75%` centred table of all four players sorted by score. The sort is a
  literal in-place bubble sort over a 4-element index array — a reminder that this is optimised for
  authoring speed, not elegance.
- **Long-term**: 段位/Rate/戦績 pages, a 40-entry local replay URL history, and mjlog download/analysis
  behind the paid tier. The replay URL supports `&tw=?` to anonymise names
  ([manual](https://tenhou.net/man/)).

### 1.7 Versus 雀魂

**Better.** Tenhou's assist highlighting is the thing 雀魂 should be judged against and rarely is: on a
call opportunity it computes exactly which hand tiles can participate and dims everything else, using a
green/red state per tile rather than a generic glow, and it does this for riichi candidate tiles and for
tsumogiri mode too. Combined with a **server-authoritative furiten indicator** and a **visible numeric
countdown with a time-refund animation**, the table tells an expert player everything they need without a
single tooltip. Its timeout affordances are also genuinely better designed: 「鳴かない」 and 「ツモ切り」
are orthogonal modes that clear themselves, and the 60-second inactivity fallback means a disconnected
player costs the table three seconds, not a hand. Finally, the irregular pond placement and the
distance-proportional meld animation make the table feel like an object rather than a spreadsheet.

**Worse.** Tenhou is hostile to a new player in a way 雀魂 is not. There is no onboarding, no shanten or
wait display during play, no yaku reference in-client (牌理 is a separate tool at
[tenhou.net/2/](https://tenhou.net/2/)), no character or reward layer, and the interface is a 2006-era
canvas table with a hard-coded brush font. Several quality-of-life features that 雀魂 gives everyone free
— wait display, wait/hint assists, replay download — are **paywalled** in Tenhou
([manual](https://tenhou.net/man/)), and the client is spread across five builds with different feature
sets, which is a maintenance and support burden a new client should not imitate. Accessibility is
effectively absent: canvas tiles are invisible to assistive technology, and there is no reduced-motion
path.

---

## 2. 電脳麻将 (kobalab/Majiang) — the open-source reference

MIT licensed, 740★, still actively maintained (last push 2026-09-17), and unusual in that its primary
source language on GitHub is **Stylus** — the CSS is a first-class artefact
([kobalab/Majiang](https://api.github.com/repos/kobalab/Majiang)). It is really an ecosystem:
`majiang-core` (rules), `majiang-server` (WebSocket), `majiang-ai` (bots), `majiang-analog` (log
analysis), plus `tenhou-log` and `tenhou-url-log` for converting to and from Tenhou's format
([kobalab repos](https://api.github.com/users/kobalab/repos?per_page=100)).

### 2.1 Layout — a fixed design canvas, scaled

`src/css/board.styl` and `src/css/desktop.styl` define a **fixed 800 × 680 board**. Everything inside is
`position:absolute` with `transform-origin: 0% 0%` and positioned by `transform: translate(x, y)`; the
whole thing is then uniformly shrunk to fit by `lib/scale.js`:

```js
let scale = dh / bh;
board.css('transform', `translate(0px, ${margin}px) scale(${scale})`);
```

Note `scale` is only ever applied when `bh > dh`, so **it never scales up** — on a large monitor you get
the same 800 px design, centred. `body.board > * { width: 800px; margin: 0 auto; }` keeps everything in
one column.

The concrete geometry (desktop, 4-player):

| Element | Position / size | Notes |
|---|---|---|
| Centre `.score` panel | `280 × 160` at `(260, 260)` | exactly centred; 35 % × 23.5 % of the board |
| 東一局 label | `font-size: 32px`, brush font | `.jushu`, kanji numerals |
| 供託 sticks | `img.chouma` at `60 × 8 px` | **7.5 : 1** — real stick proportion |
| Dora indicators | 42 px tile height, inside `.score` | `.shan` |
| Score slots | `120 × 24`, `font-size: 16px` | diamond: `.duimian (80,0)`, `.shangjia (10,24)`, `.main (80,48)`, `.xiajia (150,24)` |
| Own hand | `680 × 60`, concealed tiles **56 px**, melds **42 px** | `translate(120, 620)` — flush to the right edge |
| Opponent hands | `680 × 60` (across) and `560 × 60` (sides), **42 px** | rotated 180°/270°/90° |
| Ponds | `300 × 42`, 3 rows | `.main (310,420)`, `.xiajia (540,430) rotate(270)`, `.duimian (490,260) rotate(180)`, `.shangjia (260,250) rotate(90)` |
| Call announcement | `200 × 100`, **`font-size: 64px`** | one per seat, mid-way between hand and pond |
| Action buttons | `4em × 24px` each at `(120, 580)` | below the hand, above nothing |
| Meld-option previews | `mianzi-size: 35px` at `(120, 572)` | the chi/pon chooser |
| Player name plates | `160 × 30`, `border-radius: 6px` | `.main (120,580)`, `.xiajia (540,440)`, `.duimian (500,60)`, `.shangjia (80,206)` |
| Win dialog | full board, inner `min-width: 460px` | dora at 35 px, winning hand at **49 px** |
| Payment breakdown | `380 × 80`, `88 px` per seat | **reuses the centre panel's exact diamond** |
| Summary | `640`px-wide table, `18px`, totals `24px` | full-board overlay |

**The layout's three best ideas:**

1. **Rotate one component into four seats.** The same `.he` element is `rotate(270deg)` for 下家,
   `180deg` for 対面 and `90deg` for 上家. No duplicated markup, no per-seat CSS beyond a transform.
2. **Your hand is bigger than theirs.** Own concealed tiles are 56 px; every opponent is 42 px (a 4:3
   ratio), and your own *melds* are also 42 px. That single asymmetry creates the visual hierarchy that
   雀魂-style clients achieve with camera perspective, at zero cost.
3. **The payment breakdown reuses the centre panel's geometry.** Because both use the same diamond of
   four seat slots, the player reads the settlement in the same spatial arrangement they have been
   reading all game. This is the cheapest possible consistency win.

Responsive behaviour is deliberately minimal: at `max-width: 800px` the board grows to 720 px tall and
the playback controller moves *below* it (`transform: translate(0px, 680px); width: 800px`). The fixed
canvas is never reflowed, only scaled. `@media (max-height: 680px)` shows a `#space` spacer and hides the
nav.

### 2.2 Information design

Everything is text and numbers, in a **five-role font system** (`src/css/font.styl`):

```
title-font = "HGP行書体", STKaiti, serif      ← headings, yaku names, 局数
defen-font = Georgia, serif                   ← all numbers and scores
menu-font  = Verdana, sans-serif              ← buttons
text-font  = Century, serif                   ← player names, chat
fixed-font = Menlo, Consolas, Courier, monospace  ← inputs
```

Scores are struck from `defen-font` — a different family from the text font — which is why 電脳麻将's
numbers read cleanly at small sizes. A new client should copy the *idea* of role-based font tokens even
if it chooses different faces.

What the centre panel shows, per `lib/board.js`:

- `jushu` = `feng_hanzi[zhuangfeng] + shu_hanzi[jushu] + '局'` → **東一局**, in kanji numerals.
- `changbang` (本場) and `lizhibang` (供託) as **separate plain numbers** — not as stick icons, though
  the win dialog and centre do draw 60×8 stick images.
- Wall count `paishu` as plain text, updated on every draw (`update()` only touches `paishu`).
- All four scores prefixed with the seat wind: `` `${feng_hanzi[l]}: ${defen}` `` with a thousands comma
  inserted by regex (`defen.replace(/(\d)(\d{3})$/,'$1,$2')`) → **"東: 25,000"**.
- The current player gets class `lunban`, coloured **cyan `#0ff`**.

Gains are `#0ff` cyan and losses are `red`/`#f00` throughout the settlement screens. There is **no
in-game action log** — the board is the log, and the 牌譜 viewer (a separate page) is the event list.
The 検討 (study) mode is an overlay, not a panel: `body.analyzer #board > .analyzer` is
`700 × 400 at (50, 160)` with `background: rgba(0,0,0,0.4)` and a `2px 2px 4px black` text shadow.

### 2.3 Tile and table rendering

- **Tile aspect is 5 : 7**, from `pai-width($height) { $height / 7 * 5 }` → width = **0.714 × height**.
  That is narrower than both Tenhou (0.66) and FluffyStuff's art (0.75) and the project's current
  `32px × 43px` (0.744).
- **Flat tiles, no depth.** There is no thickness term anywhere in the tile CSS; the only 3D cue is
  `.pai.dapai { transform: translate(h/28, h/56) }` — every pond tile is nudged down-right by
  width/20 and width/40, which is enough to read as a dropped tile.
- Felt is `background: #154` (rgb 17, 85, 68) — a dark desaturated green, within a few points of the
  project's `--felt: #0f5132`.
- **Called-tile dimming is handled in the model, not the CSS.** `lib/mianzi.js` encodes the origin of the
  called tile as `+` (from 下家), `=` (from 対面) or `-` (from 上家), and lays out the three tiles so the
  sideways one lands in the physically correct place:
  - `+` → sideways tile **first** (leftmost);
  - `=` → sideways tile **second** (middle);
  - `-` → sideways tile **third**.
  Chi is always sideways-first. Each meld gets a real `aria-label` built from that, e.g.
  `"シモチャからポン"`, `"トイメンからカン"`, `"チー"`, `"アンカン"`.
- **Ankan is drawn as back–face–face–back** (`pai('_')`, face, face, `pai('_')`) — the outer two tiles
  shown as backs, the inner two as faces.
- **The sideways tile is pure CSS.** In the pond, `.lizhi { width: $dapai-height; transform:
  rotate(270deg) }` wraps a normal tile image; in a meld, `.rotate { width: $height; transform-origin:
  0% 0%; transform: rotate(270deg) translate(-$height, 0px) }`. **No pre-rotated asset is needed**, which
  matters directly for a project already using the FluffyStuff SVG set.
- **Pond layout rule, exactly**: `if (i < 6 * 3 && i % 6 == 0) append('<span class="break">')` — a row
  break every 6 tiles, **but only for the first three rows**. Row 4 and beyond run on unwrapped. The
  break spacer is `pai-width($dapai-height) * 0.1` — one tenth of a tile width.
- **Face-down tiles are rendered as a real tile-back image** (`this._pai('_')`), not as a CSS fill.
- The pond's riichi stick lives in a dedicated strip above it: `div.lizhi { width: pai-width(h)*6;
  height: h*2/7 }` with `.chouma { width: 60% }`, and it is revealed by `removeClass('hide')` the moment
  a `*`-marked tile appears in the pond.
- **Blink for "look here"**: `.pai.blink { animation: blink 0.5s infinite }` with
  `@keyframes blink { 100% { opacity: 0.7 } }`.

### 2.4 Input model

**Click/tap only — there is no drag-to-discard and no keyboard play.** But 電脳麻将 is the only client in
this survey with **real keyboard accessibility**, and it does it in two unusual ways:

1. **Focus is the hand cursor, and focus is drawn as a lifted tile.**
   `.pai:focus { transform: translate(0, -$height / 7); outline: none }` — no focus ring, the tile itself
   rises by a seventh of its height. Combined with `.pai[role="button"] { cursor: pointer }`, the hand is
   a focusable button row.
2. **A documented global shortcut set**, exposed as `title` tooltips in
   `src/html/inc/controller.pug`:

| Key | Action |
|---|---|
| `q` | 終了 — exit |
| `?` | 集計表示 — show the summary |
| `a` | 音声OFF/ON — toggle sound |
| `i` | 検討ON/OFF — toggle the analysis overlay |
| `t` | 天鳳牌譜 — export as a Tenhou log |
| `←` | 配牌/前局 — first hand / previous hand |
| `↑` | 戻る — step back |
| `Space` | 再開/停止 — play/pause |
| `↓` | 進む — step forward |
| `→` | 結果/次局 — result / next hand |
| `-` / `+` | speed down / up |

`lib/gamectl.js` implements `a`, `-` and `+` on `keyup`; speed clamps to **1–5** and is shown as five
`16 × 8 px` pills whose `visibility` is `i < speed ? 'visible' : 'hidden'`. **The entire persisted
settings object is `{ sound_on: true, speed: 3 }`.**

That is the honest verdict on 電脳麻将's input model: superb for replay and study, bare for play. It is
nevertheless the best existing precedent for keyboard-operable tiles in a browser mahjong client.

Two more behaviours worth stealing:

- **Discarding reveals nothing you should not know.** When an opponent discards, the client cannot find
  the tile in their (face-down) hand, so it picks one at random to mark deleted:
  `dapai.eq(Math.random()*(dapai.length-1)|0)` — the concealed count stays right and no information leaks.
- **Hand overflow is absorbed by shifting, not scaling.** `adjust()` computes
  `overflow = bingpai + fulou - shoupai` and sets `bingpai.css('margin-left', -overflow)`, i.e. the whole
  concealed row slides left under the melds. It is re-run on every `window resize`. No tile ever changes
  size.

### 2.5 Motion and sound

Motion is deliberately minimal and entirely CSS-transition based (`lib/fadein.js`):

- A discarded hand tile **collapses its width to zero** over 300 ms after a 100 ms delay and fades out:
  `.pai.deleted { transition: width 0.3s ease-out 0.1s; opacity: 0; width: 0px }`.
- Overlays fade: `.say { transition: opacity 0.4s ease-out }`, `.hule-dialog { transition: opacity 0.2s
  ease-in }`, `.summary { transition: opacity 0.2s ease-out }`.
- `fadeIn`/`fadeOut` are class toggles plus `transitionend`, with a 100 ms + 20 ms double `setTimeout` for
  the reflow dance. No `requestAnimationFrame`, no Web Animations API.
- Timing constants found: the win dialog waits **400 ms** before appearing; 三家和 waits 400 ms and every
  other draw waits **0 ms** before the result dialog.
- **Audio is per-seat.** `set_audio` builds arrays for `dapai, chi, peng, gang, rong, zimo, lizhi` × 4
  players plus `gong` (yakuman) = **29 samples**, each cloned per playback so overlapping sounds work,
  with an optional `volume` attribute read from the element.
- **`sound_on` is the only motion/audio toggle, and there is no `prefers-reduced-motion` rule anywhere
  in the CSS.**

So: 電脳麻将's answer to "how much animation" is "almost none, and what exists is a CSS opacity
transition". For a client that wants a calmer option, this is the cheapest credible target.

### 2.6 Settlement and result screens

- **和了 dialog** (400 ms delay): dora indicators floated **left** with `margin-left: 40px` and ura dora
  floated **right** with `margin-right: 40px` at 35 px; the winning hand re-rendered at 49 px with the
  hand block 70 px tall; then the **yaku table at `font-size: 24px`** with the **name left-aligned and the
  han count right-aligned** (`.fanshu { text-align: right }`, `.defen { text-align: right }`,
  `line-height: 1`), a 供託 stick row at 16 px, and a **380 × 80 payment diagram** using the same four-seat
  diamond positions as the centre panel, with `.plus { color: #0ff }` and `.minus { color: #f00 }`.
- **流局**: `.pingju { font-size: 48px; margin-top: 50px; margin-bottom: 100px }`.
- **Summary** (final results): a 640 px-wide HTML table at 18 px, `.r_player` with a 114 px name column
  and a 2 px bottom border, a `.lizhi` column of fixed `1.2em` width for riichi sticks, dealer rows shaded
  `rgba(128,128,128,0.4)`, points right-aligned with `padding-right: 1.2em`, totals at 24 px and centred,
  and `tbody tr:hover { background: rgba(256,256,256,0.4); cursor: pointer }` because rows are clickable
  replays. There is a download link rendering `牌譜.json` from a `Blob`.

### 2.7 Accessibility — the one real precedent

電脳麻将 is the only client in this survey that does assistive-technology work at all:

- `aria-label` on 手牌, 捨て牌, リーチ, ドラ, 裏ドラ, and per-meld labels like `"シモチャからポン"`.
- **`aria-live="assertive"` on every discarded tile** (`this._pai(p).addClass('dapai').attr('aria-live',
  'assertive')`) — a screen reader announces each discard as it happens.
- `[role="button"] * { pointer-events: none }` so composite tile buttons click correctly.
- `.pai:focus` lifting rather than outlining.

It still has **no `prefers-reduced-motion`**, no colour-blind affordance beyond the art itself, and no
documented keyboard play. But it proves the ARIA layer is cheap in a DOM-rendered table.

### 2.8 Versus 雀魂

**Better.** 電脳麻将 renders the table as **DOM, not canvas**, which makes the ARIA layer, the `:focus`
tile cursor, `aria-live` discard announcements, CSS `transform` rotations for four seats and text
selection all free — none of which a canvas client can match without a parallel accessible tree. It is
also ruthlessly fast and uncluttered: five semantic fonts, a 64 px call announcement with a four-way text
shadow instead of a cut-in, and a total settings surface of *two options*. Its pond encodes the origin
of every called tile in the model (`+`/`=`/`-`) and renders the sideways tile at the physically correct
end with pure CSS, so it works with any tile art including the CC0 SVG set. Its 検討 overlay and its
11-shortcut replay/analysis keyboard layer are a genuinely better study tool than 雀魂's replay, and its
MIT licence plus the `majiang-core`/`majiang-server`/`majiang-ai` split means a browser client can lift
verified rule logic rather than reimplementing it.

**Worse.** It is a 2015-vintage desktop web app with no concession to phones — the layout is a fixed
800 × 680 canvas that only ever scales *down*, so on a large display it wastes the screen and on a small
one the text becomes unreadable without zoom. It has no character, reward or social layer, no matchmaking,
no rule-set discovery, no sound design to speak of (29 clips), and effectively no motion — which reads as
"dead" to the audience 雀魂 serves. Its play-input model has no assists at all beyond the 検討 overlay, no
shanten or wait hint during play, and no auto-discard. And its accessibility work, while the best here,
stops short: no reduced-motion path, and no keyboard play despite having the tile-focus machinery already
built.

---

## 3. 麻雀一番街 (Riichi City)

*This section was assigned to a parallel researcher and had not returned when this document was written.
See the delivery note at the end of the accompanying report — the Riichi City findings should be appended
here.*

---

## 4. SEGA NET MJ / MJ ARCADE

### 4.1 Layout

A **single full-screen 3D table with floating UI** — no app-like chrome, the table stays primary. Tap-tabs
(案内 / 個人情報 / 試合状況) overlay information. The **情報エリア** sits right with GP and credits, drops
run down the left, and a **chat tab is on the far-left edge**
([便利機能](http://www.sega-mj.com/arcade/howto/play/function/index.html)). The **table centre holds the wall
counter and all four scores**; press-and-hold swaps scores for **point differences** plus a provisional
SCORE. Action buttons appear **bottom-right** as a contextual window
([MJ4 基本操作](https://www.sega-mj.com/mj4/control.html)), with the 全鳴有/ドラ鳴 toggle bottom-right and
detailed-call buttons **bottom-left** on wide cabinets
([操作方法](http://www.sega-mj.com/arcade/howto/play/op/index.html)). MJ4's own geometry documents
**情報エリア（上）** for shop event/mode/rules and **情報エリア（下）** for dan and results
([MJ4 基本操作](https://www.sega-mj.com/mj4/control.html)); the net drift is stacked panels → left/right
frame with tap-tabs. Dora indicators are at the **top-left** ("画面左上のドラ表示牌",
[MJ4 基本操作](https://www.sega-mj.com/mj4/control.html)). Whether the own hand is centred or left-aligned,
and whether it ever wraps to two rows, is **approximate** — no text source.

Sega's framing of the design intent is explicit and worth quoting as a counter-example: "ド派手な演出"
plus an "アツイ実況" commentary layer ([MJ ARCADEとは？](https://www.sega-mj.com/arcade/about/)) —
spectacle first, information bolted on.

### 4.2 Information design

MJ's information layer is **interrogative, not printed**. The 試合状況 tab holds 対局の推移 (a point-trend
graph with win and deal-in counts), a **1位獲得条件** panel that computes the han/fu needed to take first,
and a 乱数情報 panel exposing the player's random seed and "不確定要素"
([便利機能](http://www.sega-mj.com/arcade/howto/play/function/index.html)). Wall generation is publicly
documented with an online **牌山Viewer** ([MJ Viewer / 牌山](http://mj.sega.jp/mj5evo/viewer/index.html)).
Round wind / hand number / honba rendering is **approximate** — screenshot-only.

**Replay is a first-class social object.** A QR code appears after a match; scanning opens the **MJ
Viewer** replay with a URL and embed code for sharing
([MJ Viewer](http://mj.sega.jp/mj5evo/viewer/index.html)). In-store, the **ライブモニター** plays curated
replays with original commentary ([ライブモニター](http://www.sega-mj.com/arcade/howto/live/index.html)).

### 4.3 Tile and table rendering

Fully 3D — tiles, table and hands
([Wikipedia ja「セガNET麻雀 MJ」via Weblio](https://www.weblio.jp/content/%E3%82%BB%E3%82%ACNET%E9%BA%BB%E9%9B%80+MJ))
— and MJ4 Evolution shipped a transparent **ガラス牌** rule, i.e. real material work rather than sprites
([MJ4 Evolution](https://www.sega-mj.com/mj4/sp_evo.html)). The surface is **cosmetic and equippable**:
item groups are フェイス・ボディ / ハンド / 枠 / 背景 / 表面
([表示設定](http://www.sega-mj.com/arcade/howto/custom/display.html)). A default green felt is folklore —
**approximate**.

One directly implementable detail: **捨て牌整列表示**, a setting that aligns discards neatly and stops them
being "逆向きに表示" — meaning **the default deliberately rotates each seat's pond and this toggle flattens
it** ([表示設定](http://www.sega-mj.com/arcade/howto/custom/display.html)). Melds and discards can be
marked with a **brief tap → blue mark**, same tap to remove
([便利機能](http://www.sega-mj.com/arcade/howto/play/function/index.html)). The sideways riichi tile is
**approximate** — undescribed in any source found.

### 4.4 Input model

**Arcade**: touch-first, but the cabinet also has **physical buttons** for discard, call and win —
"タッチパネルを使用しなくても、手元のボタンを使うことで、捨て、鳴き、アガリなどの、基本操作を行うことが
できます" ([MJ4 基本操作](https://www.sega-mj.com/mj4/control.html)). It is not a flat
ツモ/ロン/リーチ/ポン/チー/カン row: the buttons switch between 「通常」 and 「特殊」. In 特殊 you hold a
button and tap a hand tile to attach 鳴有/二鳴/鳴無 permission, and a dedicated **アガリボタン** opens the
wait window ([MJ4 上級者向け操作](https://www.sega-mj.com/mj4/control_expert.html);
[MJ4 サポート機能](https://www.sega-mj.com/mj4/control_support.html)).

**Touch/PC**: discard is **two-tap confirm** ("牌は2回タッチで捨てます"); a hard tap is **強打** with
5-step sensitivity and an off switch ([操作方法](http://www.sega-mj.com/arcade/howto/play/op/index.html);
[操作設定](http://www.sega-mj.com/arcade/howto/custom/op.html)). Drag only sorts tiles. **No keyboard map
is documented.**

**Assists are spatial and on-demand** — the best instance of this pattern found anywhere in the survey:
hold a tile → every visible copy turns **red**; hold two ターツ tiles → accepting tiles turn **yellow**;
hold a dora indicator → visible dora **yellow**; hold a riichi stick → every tile discarded after that
riichi turns **red** ([便利機能](http://www.sega-mj.com/arcade/howto/play/function/index.html)). Auto-play
splits into オートアガリ and オートツモ切り, both default OFF and auto-disabling with an "×" when tenpai
breaks ([MJ4 サポート機能](https://www.sega-mj.com/mj4/control_support.html)). Call policy has three
states: 全鳴有 / 全鳴無 / **ドラ鳴** ([操作方法](http://www.sega-mj.com/arcade/howto/play/op/index.html)).

### 4.5 Motion and sound

The setting is **演出設定**: one **「演出モード」** switch (シンプル / 標準) plus per-effect overrides.
シンプル disables 面子演出, 危険牌演出, 配牌示唆演出, ドラ牌示唆演出, all 鳴き/リーチ/打牌/アガリ宣言
カットイン, ホワイトアウト演出, シャンテン数良化演出 and the アガリボタン SE
([演出設定](http://www.sega-mj.com/arcade/howto/custom/effect.html)). **This is a genuine reduced-motion
mode with per-effect granularity**, with one deliberate carve-out: even with cut-ins off, ドラポン/カン
and 役満 cut-ins still play. Audio is central — a 実況 commentary system and a ゴッドハンド演出, with
English commentary from Ver.5.0
([Wikipedia ja via Weblio](https://www.weblio.jp/content/%E3%82%BB%E3%82%ACNET%E9%BA%BB%E9%9B%80+MJ)).

### 4.6 Settlement and results

The centre plate doubles as the settlement surface — 点棒 movement, then point differences and a
provisional SCORE on hold ([MJ4 基本操作](https://www.sega-mj.com/mj4/control.html)). MJ4 added an unusual
evaluation cue: at hand end, opponents' winning tiles that score no evaluation points get a **grey frame**,
and the same-tile highlight expands into the wait window and opponents' hands
([MJ4 サポート機能](https://www.sega-mj.com/mj4/control_support.html)). Long-term, the cabinet shows
**打ち筋** (win rate, average value, speed, deal-in) as **5-step grades** plus 戦績 for 本日 / 今月・先月,
with deeper analysis in MJ.NET
([戦績・打ち筋](http://www.sega-mj.com/arcade/howto/system/record/index.html)).

### 4.7 Versus 雀魂

**Better.** MJ's *queryable table* beats 雀魂's highlighting: holding a tile and lighting up every visible
copy, taatsu acceptance, visible dora, or everything after a riichi stick — each in a distinct colour — is
a more expressive assist vocabulary than a uniform glow, and the 1位獲得条件 panel has no 雀魂 equivalent.
Its **provable-fairness layer** (published wall generation, a public 牌山Viewer, an in-match seed/
不確定要素 tab) is a different class of transparency. Call policy is far more granular (全鳴有 / 全鳴無 /
ドラ鳴 plus per-tile permission). Replay is a better social object because MJ Viewer hands out a URL and an
embed code. And 演出モード シンプル with per-effect overrides is a better accessibility story than a single
animations toggle.

**Worse.** The information architecture is hostile to reading: 対局の推移 and 1位獲得条件 hide behind taps,
points hide behind press-and-hold, and **no source describes any persistent textual action log** — MJ
narrates via commentary audio instead ([MJ ARCADEとは？](https://www.sega-mj.com/arcade/about/)). Input is
slower per turn: two taps to discard, no keyboard map, and a 強打 gesture that needs sensitivity tuning.
Auto-play is blunt — it refuses *all* calls, so you cannot auto-discard while still ponning. On-cabinet
stats are four letter grades with real analysis gated behind membership — worse than 雀魂's built-in
牌譜/データ screens.

---

## 5. 麻雀格闘倶楽部 (Konami MFC / Extreme / UNION / SP)

### 5.1 Layout

A **panel-heavy table on a 32-inch touch monitor** (upgraded from 21.5"), with tiles sized to match real
automatic-table tiles ("牌の大きさも全自動雀卓の牌の大きさ")
([Konami MFC Extreme](https://www.konami.com/arcadegames/products/am_mfc_extreme/);
[4Gamer](https://www.4gamer.net/games/334/G033490/20170913064/)). It is **touch-only** — "モード選択から
ゲームプレーまで全ての操作は画面を直接タッチするだけ" — with no button panel at all, the exact inverse of MJ.
In the SP mobile build **utility buttons run down the right edge**, settings hide behind a top-right gear,
and holding the right-side button swaps the readout
([対局の便利機能](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/taikyoku_kinou/index.html)); each player's
name carries a **3-step connection indicator**. Arcade hand/pond/centre geometry is **approximate** —
Konami documents behaviour, not coordinates. A 2015 reviewer found the mobile hand area "ぎゅうぎゅう詰め"
with hard-to-identify call targets ([麻雀豆腐](https://majandofu.com/online-mahjong-fight-club)) — evidence
of a dense rather than airy layout. **UNION** (live 2025-10-15) is styled "黒を基調としたラグジュアリーな
デザイン" with a rebuilt mode-select screen
([gamebiz](https://gamebiz.jp/news/414291);
[Konami](https://p.eagate.573.jp/game/mfc/ac/news/202510/10/news.html)).

### 5.2 Information design

MFC's signature is **toggleable, state-changing readouts** rather than a fixed HUD. Holding the right-side
button cycles the table between **あなたとの点差 / 一位との点差 / シャンテン数 / 起家**; tapping your own
score cycles the 点差 view one step
([対局の便利機能](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/taikyoku_kinou/index.html)). Draws you
discarded are rendered **slightly darker** (ツモ切り牌), encoding tedashi/tsumogiri at a glance — the same
idea しらぎく added in 2021. UNION added three named numeric assists: **待ち牌残り数表示**, **同牌強調**
(how many of a tile you hold and how many are in the pond) and **向聴数カウント**
([gamebiz](https://gamebiz.jp/news/414291)). SP prints the wait's remaining count at the tile's
bottom-right ([対局の便利機能](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/taikyoku_kinou/index.html)).

Replay is strong: last-50 牌譜 browsing, per-hand replay, long-press fast-forward, 牌譜ID sharing, **name
anonymisation before sharing**, mid-hand snapshots that resume replay, and turning any tenpai moment into a
public **何切る** quiz others vote on
([牌譜再生](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/haifu/index.html)).

### 5.3 Tile and table rendering

Konami stresses spectacle: the bigger panel makes the **thunder effect** grander, and cabinet **LED
lighting changes colour with match state**
([Konami](https://www.konami.com/arcadegames/products/am_mfc_extreme/);
[4Gamer](https://www.4gamer.net/games/334/G033490/20170913064/)). Tiles are real-table-sized, implying a
fairly flat near-top-down presentation; whether these are true 3D meshes or pre-rendered 3D-look sprites is
**approximate**. The surface is cosmetic — ビジュアルアレンジ covers プレーヤーパネル, 卓背景 and 牌
([ビジュアルアレンジ](https://p.eagate.573.jp/game/mfc/playguide/contents/paseli04.html)) — so "MFC felt
colour" has no single answer (**approximate**). Rather than dimming melds, MFC *highlights*: 同牌強調
reports copies held and ponded, and SP's 同牌検索 lights every matching visible tile **green** while you
hold a tile, including melds and discards
([対局の便利機能](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/taikyoku_kinou/index.html)). The sideways
riichi tile is **approximate** — undescribed.

### 5.4 Input model

Arcade is touch-only; discard is **select-then-confirm** ("1回目のタッチで牌を選択し、2回目のタッチで実行"),
and declining a call means tapping **パス**
([SP 操作方法](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/operate/index.html)).

**The PC (コナステ) build has the most directly copyable keyboard map in this survey**: click to select,
click again to commit; **A** riichi, **S** chi, **D** pon, **F** ankan/minkan, **Space** tsumo/ron,
**Z** pass, **X** toggle never-call. Right-click also works (hold to show, move while held to select,
release to confirm), and **press-and-hold for 1 second** on the kan or tsumo/ron button enables auto-kan or
auto-win ([PC操作方法](https://p.eagate.573.jp/game/mfc/eacloud/p/info/pc_operation.html)). Note the mnemonic
logic: R/C/P/K are taken by nothing, so chi/pon/kan land on S/D/F — adjacent keys under the left hand, with
Space as the win and Z/X as the two "decline" gestures.

Assists are **tiered** across skill levels, which is the best onboarding model found: the base tier gives
アガリAUTO, 鳴無 and (pre-UNION) 長考; the SP **対局アシスト** package adds 狙える役表示 with tap-for-detail,
役完成表示, **安全牌表示** (an icon on tiles safe against a riichi player), テンパイ数表示,
あなたの番です表示, ツモ切り表示, ドラ表示, メンツ完成表示, 捨てるとテンパイ表示, テンパイ牌表示 and
立直・ツモ等ができなくなる表示 — and it is **disabled in プラチナ/ダイヤモンド and some event tables**
([対局の便利機能](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/taikyoku_kinou/index.html)). That is a
cleaner skill-segregation answer than a uniform client.

Timing is split into **打牌時間 (white)** and **長考時間 (orange)**; when the discard clock expires the
long-think pool drains automatically and refills at hand start, with configurable seconds per table
([SP 操作方法](https://p.eagate.573.jp/game/mfc/mfc_sp/howto/operate/index.html)). Two removals matter:
the **長考ボタン was removed from the match screen in UNION** in favour of automatic 長考
([PC操作方法](https://p.eagate.573.jp/game/mfc/eacloud/p/info/pc_operation.html)), and the beginner
countdown changed from あがり to **テンパイ** in Ver.2.3.0
([Konami 2025/11/18](https://p.eagate.573.jp/game/mfc/ac/news/202511/18/news.html)).

### 5.5 Motion and sound

"A" bold graphics plus sound effects ([MFC playguide](https://p.eagate.573.jp/game/mfc/playguide/index.html));
the flagship example is a **lightning strike on mangan-or-better**, which a 2015 reviewer calls satisfying
and notes comes with **vibration** ([麻雀豆腐](https://majandofu.com/online-mahjong-fight-club)), and the
high-grade cabinet amplifies exactly that effect plus state-driven LED
([4Gamer](https://www.4gamer.net/games/334/G033490/20170913064/)). UNION references a **登場演出設定画面**
reachable from the mode-select screen
([Konami](https://p.eagate.573.jp/game/mfc/ac/news/202510/10/news.html)). **No setting named 演出スピード
could be verified** — what is documented is 登場演出設定 plus support-feature ON/OFF under プレミアムエリア
→ 表示設定. Treat "MFC 演出スピード" as **approximate/unverified**; its reduced-motion story is weaker than
MJ's, with no master equivalent to 演出モード シンプル.

### 5.6 Settlement and results

A 点棒 exchange with lightning spectacle and vibration at mangan+
([麻雀豆腐](https://majandofu.com/online-mahjong-fight-club)). UNION **abolished 打ち切り制**, and leftover
供託 now goes to the leader instead of vanishing — previously "「Extreme」までは消滅していた"
([Wikipedia ja via Weblio](https://www.weblio.jp/content/%E9%BA%BB%E9%9B%80%E6%A0%BC%E9%97%98%E5%80%B6%E6%A5%BD%E9%83%A8);
[Konami 2025/10/30](https://p.eagate.573.jp/game/mfc/ac/news/202510/30/news3.html)); UNION also adopted
**切り上げ満貫** ([gamebiz](https://gamebiz.jp/news/414291)). The hand-end yaku/han/fu panel layout is
**approximate**. Long-term, dan rank, 戦績, 打ち筋 and あがり率 are checkable after each match, with more on
e-amusement ([MFC playguide](https://p.eagate.573.jp/game/mfc/playguide/index.html)); the mobile client
reports 平均順位, あがり率, 平均アガり翻数 and a **last-30-match placement trend chart**
([麻雀豆腐](https://majandofu.com/online-mahjong-fight-club)).

### 5.7 Versus 雀魂

**Better.** The assist tiering is the best onboarding model in a competitive client: a named 対局アシスト
package with 安全牌表示, 捨てるとテンパイ表示 and 立直・ツモ等ができなくなる表示, **switched off in the top
tables** — a cleaner answer to "how do beginners and experts share a client" than 雀魂's uniform surface.
The **牌譜→何切る pipeline**, where a saved hand becomes a public quiz other players vote on and replays
can be anonymised before sharing, has no 雀魂 equivalent. And the PC keyboard map
(A/S/D/F/Space/Z/X with right-click select-confirm and hold-to-auto) is a concrete one-hand scheme 雀魂
lacks entirely.

**Worse.** The visual language is ageing and cluttered — a 2015 review calls it "やや古臭い" with a
hard-to-read table, poor call-target legibility, no timer gauge and unpleasant discard sounds
([麻雀豆腐](https://majandofu.com/online-mahjong-fight-club)); **approximate** for UNION, but Konami's own
discoverability rationale concedes the problem. Real analysis lives on the web/e-amusement side rather than
in the client, and support features sit inside a プレミアムエリア — a paid-tier-shaped surface 雀魂 exposes
free. Its rule surface also moves underfoot: UNION changed 切り上げ満貫, 赤ドラ counts, 食い替え, 供託,
打ち切り and the 長考 button at once, so a client must not hard-code assumptions.

---

## 6. しらぎく麻雀 (Shiragiku Mahjong)

This turned out to be far more relevant than its obscurity suggests: it is an **HTML5 + JavaScript browser
mahjong client** — the same medium as the target — written by しらぎくさいと, and its author documents the
UI decisions more openly than any commercial operator. It implements riichi plus 関西式ブー麻雀,
アルシァル麻雀, 台湾麻将 (16-tile), 中共麻将 (MCR), 三人麻雀, 三人リーチ麻雀, ポンリー (Kyoto-style
three-player) and 北抜き, with a simplified しらぎくモバイル麻雀 alongside
([しらぎく麻雀 top page](https://marguerite.gingerbeardman.com/Nihongo/Games/index.html);
[しらぎく麻雀](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/index.html)).
The Flash builds were retired on 2019-12-31. Cookie use is declared. Play is at
`https://marguerite.gingerbeardman.com/Games/Marguerite_Mahjong/`.

**Verification limit:** the game shell returns an empty body to any fetcher, so **tile geometry, pond row
count, felt colour, tile-back design and the sideways riichi tile could not be inspected** — all
**approximate/unverified** for this client. What follows is from the author's own documentation.

### 6.1 Layout

Not documented in coordinates. The author's method documentation implies a **top menu bar** ("画面上端の
メニューバー") carrying, from left to right: the sound-mode indicator, the animation-effect indicator, and a
菜単に戻る button at the right edge
([基本的な操作方法](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E5%9F%BA%E6%9C%AC%E7%9A%84%E3%81%AA%E6%93%8D%E4%BD%9C%E6%96%B9%E6%B3%95.html)).
Commands are rendered **above the hand** ("手牌の上に入力可能なコマンドが表示されます"). Layout proportions
are **approximate**.

**Disabled controls are rendered as faint text** ("薄い文字で表示され") rather than hidden — the sound
indicator greys out when no audio data has loaded or sound is pinned off, and the 菜単に戻る button greys
out on menu screens. That is a small, good pattern: a control that exists but is unavailable is more
legible than one that vanishes.

### 6.2 Information design

Not documented in detail, but two verifiable decisions stand out:

- **Called/declared state is on the model, not inferred.** The author added, on 2021-01-04, a feature to
  **distinguish 摸切牌 from 手出し牌** (tsumogiri from tedashi)
  ([しらぎく麻雀 index](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/index.html))
  — the same information MFC encodes by drawing tsumogiri slightly darker.
- **A callable discard is marked by a blinking arrow**, not by a highlight or colour: 「他者の打牌に対して
  コマンドが入力出来る場合は、当該打牌を点滅する矢印で示します」
  ([基本的な操作方法](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E5%9F%BA%E6%9C%AC%E7%9A%84%E3%81%AA%E6%93%8D%E4%BD%9C%E6%96%B9%E6%B3%95.html)).
  Motion is being used as the state marker, which is exactly the thing a reduced-motion mode would break —
  worth noting as a cautionary example.

There is no in-client action log documented; the rule documentation lives on the website by design, which
is a legitimate architectural choice for a single-author client.

### 6.3 Tile and table rendering

**Unverified.** No rendering detail is documented and the shell is not fetchable. The only related datum is
that the dice, when the animation effect is off, are **laid statically in the roller's pond — two dice, or
three for 台湾麻将 — with only their faces changing rapidly before locking**: 「この賽子は動かず目だけが
変わります」 ([アニメーション視覚効果…](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%82%A2%E3%83%8B%E3%83%A1%E3%83%BC%E3%82%B7%E3%83%A7%E3%83%B3%E8%A6%96%E8%A6%9A%E5%8A%B9%E6%9E%9C%E3%81%A8%E3%81%9D%E3%81%AE%E6%9C%89%E7%84%A1%E3%81%AE%E5%88%87%E6%9B%BF%E3%81%AB%E3%81%A4%E3%81%84%E3%81%A6.html)).
That is a genuinely good reduced-motion fallback: keep the *information* (the dice roll and its result) and
drop only the *travel*.

### 6.4 Input model

**Click/tap only, no keyboard, no drag.** The documented model
([基本的な操作方法](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E5%9F%BA%E6%9C%AC%E7%9A%84%E3%81%AA%E6%93%8D%E4%BD%9C%E6%96%B9%E6%B3%95.html)):

- **牌の指定** — click/tap the tile itself.
- **Command buttons appear above the hand** whenever a discard choice or a call decision is pending. The
  commands are カン / ポン / チー / 立直 / 和了 / 流局, with **進行** to pass, replaced by **摸切** during
  discard selection.
- **Progressive disambiguation**: カン auto-commits if only one tile can be kan'd, otherwise asks which;
  チー auto-commits if only one combination exists, otherwise asks for the first tile and then, only if
  still ambiguous, the second. **Red fives are treated as a distinct combination**, so the chooser appears
  for them.
- **立直 is declared before discarding**, and is **reversible**: clicking 立直 again before discarding
  cancels it. Declaring while not tenpai and then discarding also cancels it **with no penalty**
  (罰符は課されません). Open riichi is a double-click on 立直. The button is hidden when not menzen
  (except in ポンリー) or when no draws remain. 台湾麻将 shows a 聴牌 button instead on the first discard.
- **和了 is hidden when it would be illegal.** 「役がないなどの理由で和了が出来ない場合は和了コマンドは
  表示されません。このため、不当な和了コマンドに依る錯和(チョンボ)は発生しません」 — **the UI makes
  chonbo impossible by construction.** This is the strongest single UX idea found in the whole survey for
  a new client: rather than validating an action after the fact, do not offer it.
- 九種幺九倒牌 is a 流局 command on the first draw, disabled if anyone called before your first draw.
  流し満貫 wins automatically at the draw.

There are **no documented keyboard shortcuts and no auto-discard.**

### 6.5 Motion and sound — the best documented reduced-motion story in the survey

しらぎく麻雀 animates three things: the drawn tile **flying from the wall to the hand**, the discarded tile
**flying from the hand to the pond**, and the **dice flying** when deciding the dealer and at the deal.

The author then states the reason for the toggle plainly: 「HTML5 に於いてこの動画効果は非力な環境の元では
相当重くなる事が判明しました」 — in HTML5 the effect turned out to be very heavy on weak environments —
**so version 3.400 (2019-11-01) added an on/off switch**
([アニメーション視覚効果とその有無の切替について](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%82%A2%E3%83%8B%E3%83%A1%E3%83%BC%E3%82%B7%E3%83%A7%E3%83%B3%E8%A6%96%E8%A6%9A%E5%8A%B9%E6%9E%9C%E3%81%A8%E3%81%9D%E3%81%AE%E6%9C%89%E7%84%A1%E3%81%AE%E5%88%87%E6%9B%BF%E3%81%AB%E3%81%A4%E3%81%84%E3%81%A6.html)).

The **toggle is called 「麻雀牌等の動画効果」** (animation effects for mahjong tiles etc.), with values
**あり / 無し**, and it is reachable two ways:

1. **Before the game**: 菜単 → **対局環境設定** → 「麻雀牌等の動画効果」 → あり or 無し. The active value is
   shown in white.
2. **During the game, at any time**: click **「麻雀牌等の動画効果：入」** or
   **「麻雀牌等の動画効果：切」** in the top menu bar. If an effect is mid-flight when you switch off, it
   completes and the setting becomes 切 at that point.

The chosen value is **persisted in the browser and inherited on later visits**. Sound has a parallel
control: **無音モード / 有音モード**, also a clickable indicator at the left of the menu bar, greyed out when
audio is unavailable (it is deliberately silenced in the Pale Moon browser, and audio on Android/iOS only
exists in しらぎくモバイル麻雀, where it may desynchronise).

This is the only client found with a **named, documented, user-facing animation toggle motivated by
performance** rather than by taste. There is no `prefers-reduced-motion` handling, so the option is manual.

### 6.6 Settlement and result screens

**Unverified** beyond the existence of a summary/進行 flow — advance to the next hand is a **進行** command,
and the same command serves as pass/proceed during play.

### 6.7 Versus 雀魂

**Better.** しらぎく麻雀 is the only client here that treats **animation as an option rather than a
feature**, with a named toggle (`麻雀牌等の動画効果`), a settings-menu path, an in-game shortcut, persistence
across sessions, and a stated performance reason for existing — plus a graceful fallback where the dice
stay put and only their faces change, so the *information* survives. Its command model also beats 雀魂 on
safety: the 和了 command is hidden whenever the win is illegal, which makes chonbo structurally impossible,
and the 立直 command is reversible before the discard with no penalty for a mis-click. And its scope is
extraordinary for one author — nine rule sets including 台湾麻将, 中共麻将 and four three-player variants,
each with its own FAQ.

**Worse.** Everything 雀魂 does to make a client feel alive, しらぎく麻雀 does not do: no matchmaking, no
accounts or ranks, no character or progression layer, no social features, and an interface whose author
explicitly warns it is not suitable for phones. Its information design is thin — a callable discard is
signalled by a blinking arrow, which is both a weaker cue than a colour highlight and inaccessible to
anyone who cannot perceive motion. There is no keyboard input, no auto-discard, no assist layer, and no
documented layout or rendering system to learn from, which limits how much of it can actually be reused.

---

## 7. Other well-regarded and open-source clients

### 7.1 Riichi Advanced — `EpicOrange/riichi_advanced`

Elixir/Phoenix LiveView with Rust NIFs, AGPL-3.0, 72★, live at
[riichiadvanced.com](https://riichiadvanced.com/), with 28+ rule sets and a custom `MahjongScript` DSL
([repo](https://api.github.com/repos/EpicOrange/riichi_advanced)). **Its layout is the most directly
reusable artefact found in this entire survey**, because the whole table is sized by one variable in
[`assets/css/app.css`](https://cdn.jsdelivr.net/gh/EpicOrange/riichi_advanced@main/assets/css/app.css):

- Landscape: `@media (min-aspect-ratio: 6/5)` → `--tile-size: calc(100vh / 20)` with the comment
  `16 (table height) + 1 (table margin) + 1.5 (top margin) + 1.5 (bottom margin) = 20`.
- Portrait: `@media (max-aspect-ratio: 6/5)` → `--tile-size: min(calc(100vw / 19), calc(80vh / 19))`.
- The playfield is `height: calc(16 * var(--tile-size)); aspect-ratio: 1/1;` — an exact **16 × 16 tile
  square**, with a felt backdrop `main::before` inset half a tile and `--bg-color: #0f6f2f`.
- Seats are the same element rotated: `.hand.shimocha { rotate(270deg) }`, `toimen { rotate(180deg) }`,
  `kamicha { rotate(90deg) translateX(calc(2 * var(--tile-size))) }`.
- **Pond is 6-wide × 3 rows**: `div.pond { width: calc(100% - 5 * var(--tile-size)); height: calc(3 *
  var(--tile-size)) }`, with row breaking in pure CSS via `order`:
  `.pond > .tile:nth-child(n+7) { order: 1 }`, `n+13 → order: 2`.
- Centre: `div.compass { width: calc(4.75 * var(--tile-size)) }` with four `div.direction` quadrants and a
  per-seat `div.riichi-tray { width: calc(2.25 * …); height: calc(0.625 * …) }`; the 1000-point stick is
  `.riichi-tray.riichi::before`.
- The message log is a side panel in landscape (`left: calc(100% + var(--tile-size))`) and is **hoisted
  under the table in portrait** (`top: calc(100% + 1.5 * var(--tile-size)); height: max(10rem, calc(100vh -
  100%))`).

Tiles are **flat 3:4 rectangles from one sprite sheet**: `--tile-width: calc(var(--tile-scale-factor) *
0.75 * var(--tile-size)); --tile-height: calc(var(--tile-scale-factor) * 1 * var(--tile-size))`, with
`--tile-front: #f4f0eb; --tile-front-side: #eee4d8; --tile-back: #f0974c;`. The 3D look is faked with two
stacked `box-shadow` offsets and `clip-path`, with per-seat variants so the extrusion points the right way
and a `.flat` class that disables it. **Sideways and called tiles are pre-baked sprite columns**
(`div.tile.sideways { width: var(--tile-height); background-position-x: calc(-0.75 *
var(--tile-size) * var(--tile-scale-factor)) }`). Called-tile dimming is `div.tile.inactive { opacity: 0.7;
--tile-brightness: 60% }`. **It ships a tile-number overlay** — `div.tile.one::after { content: "1" }` …
`.E::after { content: "E" }`, toggled by `input.tile-numbers-checkbox`, with `--number-color` and a
four-way `text-shadow` halo: the only built-in suit-identification assist found in any client.

Input is click-only (`div.hand.self > div.tiles > div.tile { cursor: pointer }`) plus a bottom-right
button row and a `div.call-buttons-container` at `right: calc(4.5 * var(--tile-size))`. Phones get a
**piano-key hand strip** (`div.hand-piano-container`, keys `flex: 0 0 calc(1.125 * var(--tile-size))`,
`transform: scale(2)`). Motion is short CSS keyframes whose durations track the tile grid:
`tilePlayed 0.75s ease` (hand tile collapses width to 0), `slideUp 0.2s`, `slideLeft 0.2s`,
`slideRight 0.4s` (call), `textFade 1.5s`, `showWaitsLoading 0.5s` for delayed wait hints. There is
**no keyboard layer, no `aria-`, no `tabindex`, and no `prefers-reduced-motion`** — verified by grepping
the complete fetched file.

### 7.2 FluffyStuff's tile set and OpenRiichi

The CC0 tile set the project already uses is per-tile SVG at a declared
`width="300" height="400" viewBox="0 0 300 400"` — i.e. **3:4** — with `Regular` and `Black` variants,
`Front.svg`, `Back.svg`, `Blank.svg`, and red-five dora as separate files (`Man5-Dora.svg`,
`Pin5-Dora.svg`, `Sou5-Dora.svg`); all assets are in the public domain
([README](https://raw.githubusercontent.com/FluffyStuff/riichi-mahjong-tiles/master/README.md),
[Regular/Man1.svg](https://cdn.jsdelivr.net/gh/FluffyStuff/riichi-mahjong-tiles@master/Regular/Man1.svg)).
**There is no pre-rotated or perspective variant** — every 3D client maps the flat art onto meshes.

`FluffyStuff/OpenRiichi` (Vala/GTK/OpenGL, GPL-3.0, 153★) contributes the best *timing* idea found:
[`AnimationTimings.vala`](https://api.github.com/repos/FluffyStuff/OpenRiichi/contents/source/Game/Logic/AnimationTimings.vala)
is a serialisable object holding `round_over_delay`, `decision_time` and `AnimationTime` (fade + duration)
pairs for `tile_draw`, `tile_discard`, `call`, `dora_flip`, `win`, `riichi`, `hand_order` and more, and it
computes `get_animation_round_end_delay(round)` by summing per-yaku durations so a replay runs at exactly
the original pace. The timings travel **with the game state**, and
[`GameAnimationTimings.vala`](https://api.github.com/repos/FluffyStuff/OpenRiichi/contents/source/Game/Rendering/GameAnimationTimings.vala)
builds a `GameRenderContext(AnimationTimings server_times, float tile_scale, Vec3 tile_size, int
observer_index, int dealer, int wall_split)`. **For a server-authoritative browser client this is the
pattern worth stealing**: put animation timings in the state document, keyed off the observer, so replay
and spectating reproduce the original pacing and a reduced-motion client can scale one document instead of
rewriting keyframes.

### 7.3 Counter-examples and smaller projects

- **`pwmarcz/autotable`** (TypeScript + three.js, 96★) is a deliberate counter-example: its README states
  that "automatic tile drawing, sorting, scoring etc. … is contrary to the project's philosophy". Input is
  drag-and-drop plus a rubber-band `#selection` box; there is no auto-discard, no hints, no keyboard layer.
  Its reusable idea is representational: `src/slot.ts` models the board as named **slots**, tiles as
  **things** with a quaternion rotation and a `place` (position + rotation + dimensions), plus a `shift`
  operation for pushing neighbours when sorting — a clean mental model for server-authoritative state.
  **Caution on licensing**: the code is MIT but `img/tiles.svg` is **CC BY-NC-SA (non-commercial)**
  ([COPYING](https://api.github.com/repos/pwmarcz/autotable/contents/COPYING),
  [README](https://api.github.com/repos/pwmarcz/autotable/contents/README.md)).
- **`EmeraldCoder/riichi-ui`** (Vue 3 + SCSS, MIT) is small but has the cleanest *tokenised* tile CSS:
  [`_variables.scss`](https://api.github.com/repos/EmeraldCoder/riichi-ui/contents/packages/riichi-ui-css/src/_variables.scss)
  sets `$defaultTileIconWidth: 58px; $defaultTileIconHeight: 78px;` (again ≈ 3:4),
  `$defaultTilePadding: 5px`, and exposes exactly two custom properties —
  `--riichi-tile-back-color: #e9b501; --riichi-tile-border-color: #000000;` — with `small` (÷2) and
  `x-small` (÷3) tiers. Its
  [`tile.scss`](https://api.github.com/repos/EmeraldCoder/riichi-ui/contents/packages/riichi-ui-css/src/components/tile.scss)
  shows a clean sideways tile with **no second sprite**:
  `transform: rotate(-90deg) translateX(calc(($defaultTileWidth - $defaultTileHeight) / 2))` plus
  compensating negative margins, and `.riichi-tile--reversed { background: var(--riichi-tile-back-color);
  .riichi-tile-icon { visibility: hidden } }`. Its component set is deliberately partial — `tile`,
  `tile-group`, `ankan`, `chii`, `pon`, `daiminkan`, `shouminkan`, `tenbou` — i.e. called sets as
  components, with no board, input or animation.
- **`SakaiTaka23/riichi-mahjong-tiles`** on npm (MIT, a fork of FluffyStuff's) publishes 433 files of React
  19 SVG components including **pre-rotated variants**
  ([registry](https://registry.npmjs.org/riichi-mahjong-tiles/latest)) — the cheapest way to get rotated
  assets without writing the transform.
- **Not verifiable / do not cite as existing**: `Euphyllia/…` mahjong, `Apaszke/…` mahjong, `saki-rs`,
  `riichi-mahjong-js`, `ninegate/…` and `jong` were all searched for and **not found**. No open-source
  雀魂-grade client exists, so its table rendering is not available to copy.

---

## 8. Cross-client conventions

Things nearly every client does the same way. These are the load-bearing conventions; breaking them costs
credibility with experienced players.

1. **Four fixed seats around a square table with your seat at the bottom.** Every client surveyed; MJ and
   MFC differ only in how much the camera tilts.
2. **Your hand is a single horizontal row at the bottom, concealed tiles together, melds separated.**
3. **The drawn tile is visually separated** from the other 13 — by a gap (電脳麻将: a `.zimo` span with a
   0.1-tile left margin), by an offset (天鳳: 10 % of a tile), or by both.
4. **Called tiles lie sideways in the meld, and the position of the sideways tile encodes the origin:**
   leftmost from 下家, middle from 対面, third from 上家; chi is always leftmost. 電脳麻将 encodes this
   exactly (`+`/`=`/`-` in `lib/mianzi.js`). This is physical-table correctness, not style.
5. **The riichi declaration tile lies sideways in the pond, and the 1000-point stick is attached to the
   declarer's pond** — 電脳麻将 has a dedicated `.chouma` inside the pond element and a stick strip above it.
6. **Melds are read left-to-right in ascending tile order within each set.**
7. **The centre of the table carries round wind, hand number, honba, and the remaining wall count.** MJ
   adds scores there too.
8. **Dora indicators are face-up tiles in a dedicated row, not a badge** — 電脳麻将 renders up to 5 slots,
   filling unused ones with the tile-back image; MJ puts them top-left; Tenhou derives the pond tile size
   from the centre panel that holds them.
9. **A round-end screen that lists yaku name + han, then the points, then a breakdown.** Universal.
10. **Scores are shown as five-digit numbers with a thousands separator.**
11. **Click/tap a tile to discard it.** No client surveyed uses drag-to-discard as the primary gesture; the
    only drag uses are manual sorting (天鳳 paid Windows) and the tabletop simulator (autotable).
12. **A call decision is presented as a row of labelled buttons, and passing is always available.**
13. **The active player is highlighted** — 電脳麻将 uses cyan `#0ff` on the score, MJ and MFC use panel or
    board emphasis.
14. **Sound effects are per-action and per-seat**, with distinct samples for chi/pon/kan/ron/tsumo/riichi
    and a separate yakuman sting (電脳麻将: 29 clips; 天鳳: ~40 slots).
15. **A result/summary screen showing final placements in score order, with a clickable replay.**
16. **Disconnected players are visually distinguished.** 電脳麻将 drops their name plate to 10 % opacity;
    天鳳 prints the name in red; MFC shows a 3-step connection indicator.

## 9. Genuine divergences

Places where established clients actively disagree — i.e. real design decisions, not accidents.

| Question | Divergence |
|---|---|
| **Tile aspect ratio** | 3 established answers: **Electronics 電脳麻将 5:7 (0.714)**, **天鳳 31:47 (0.66)**, **FluffyStuff art / Riichi Advanced / riichi-ui ≈ 3:4 (0.75)**. The project's current 32×43 (0.744) matches the art, not any client's layout maths. |
| **Pond tiles per row** | **Riichi Advanced: fixed 6 × 3** via CSS `order`. **電脳麻将: 6 per row for rows 1–3, then unwrapped** (`if (i < 6*3 && i % 6 == 0)`). 天鳳's pond is not a grid at all — the manual states non-tsumogiri discards are placed at **random** positions. MJ has a 捨て牌整列表示 toggle precisely because its default is *not* aligned. |
| **Pond orientation** | Riichi Advanced and 電脳麻将 rotate each seat's pond with the seat. MJ's 捨て牌整列表示 setting exists because the default renders some ponds "逆向き" — Sega treats per-seat rotation as a default and alignment as a preference. |
| **Flat vs 3D tiles** | 天鳳 is pseudo-3D (12-unit drawn thickness plus a neighbour-aware shading pass). MJ and MFC are fully 3D, MJ with real material work (ガラス牌). 電脳麻将, Riichi Advanced and riichi-ui are flat, and Riichi Advanced merely *fakes* extrusion with stacked box-shadows plus a `.flat` escape hatch. |
| **Where scores live** | **Centre**: 天鳳 (scores beside the centre), 電脳麻将 (a 280×160 centre panel holding all four), MJ and MFC (centre plate, press-and-hold for differences). **Per-player panels**: the more common modern arrangement. 電脳麻将's payment screen reusing the centre diamond shows how consistent the centre approach can be. |
| **Your hand vs theirs** | 電脳麻将 makes **your concealed tiles 56 px and every opponent's 42 px**, and your own melds 42 px. Everyone else keeps one size and gets hierarchy from camera perspective or shadow. |
| **Hand alignment** | 電脳麻将's own hand is **right-aligned to the board edge** (`translate(120, 620)` in an 800-wide board) with melds floating right; opponents are rotated equivalents. Riichi Advanced and 電脳麻将 both support a **rotatable viewpoint** (`viewpoint` in `lib/board.js`) so a spectator can sit at any seat. |
| **Called-tile treatment** | 電脳麻将 and Tenhou **dim or state-colour** the tiles; MFC **highlights** (`同牌検索` lights every matching visible tile green, including in ponds and melds); MJ turns held-tile copies **red** and taatsu acceptances **yellow**. Dimming is the older convention; highlight-on-demand is the newer one. |
| **Ankan rendering** | 電脳麻将 draws back–face–face–back (`pai('_')` on the outer two). **approximate** for other clients. |
| **Keyboard input** | **MFC's PC build is the only client with a real keyboard map** (A/S/D/F/Space/Z/X, right-click select-confirm, hold-to-auto). 天鳳 has **none** — `keyup`/`keypress`/`keyCode` never appear in its bundle. 雀魂 has none natively (hence third-party extensions). 電脳麻将 has 11 shortcuts but all for replay/analysis, plus a `<title>`-documented set and a focus-driven tile cursor. A new client can beat all of them here. |
| **Auto-discard / pass modes** | 天鳳: two orthogonal modes, 鳴かない and ツモ切り, self-clearing, plus 60-second inactivity fallback. MJ: 全鳴有 / 全鳴無 / ドラ鳴 plus per-tile permission. MFC: 鳴無 base tier, 対局アシスト tiered and disabled at high ranks. 電脳麻将 and しらぎく麻雀: none. |
| **Animation toggle** | **Only two clients have one.** しらぎく麻雀: 「麻雀牌等の動画効果」 = あり/無し, pre-game and in-game, persisted, added for performance. MJ: 演出設定 → 演出モード シンプル with ~9 named per-effect overrides. **No client anywhere implements `prefers-reduced-motion`.** |
| **Chonbo prevention** | しらぎく麻雀 **hides the 和了 button when the win is illegal** so chonbo cannot happen; 天鳳 simply states 「チョンボなし」 and validates server-side; 電脳麻将 shows the 和了 option and scores it. Two opposite philosophies. |
| **Action log** | **No client has a persistent in-table textual log.** 天鳳's density comes from the mjlog format plus a step-through replay; 電脳麻将's from DOM ARIA live regions plus a separate 牌譜 page; MJ narrates with commentary audio instead; MFC has a 牌譜 viewer with sharing and anonymisation. |
| **Accessibility** | **Only 電脳麻将 does any** — `aria-label` on 手牌/捨て牌/リーチ/ドラ/裏ドラ, per-meld labels like `"シモチャからポン"`, `aria-live="assertive"` per discard, and a `:focus` tile lifted by 1/7 of its height. Riichi Advanced ships a tile-number overlay for suit identification. |
| **Replay as a social object** | MJ Viewer hands out a URL **and an embed code**; MFC offers 牌譜ID sharing, name anonymisation, and a 牌譜→何切る quiz pipeline; 天鳳's replay URL supports `&tw=?` to anonymise. |

---

## 10. The ten most valuable ideas, ranked

Ranked by value to a new browser client, weighting (a) how much it improves play, (b) how cheap it is
given the current SVG-tile/DOM architecture, and (c) whether anyone else already does it well enough to copy.

1. **Size the table from one `--tile-size` token and make the playfield an exact integer tile grid.**
   Riichi Advanced computes `--tile-size: calc(100vh / 20)` in landscape and `min(100vw/19, 80vh/19)` in
   portrait, then expresses *everything* as a multiple of it, with the board as a `16 × 16` square via
   `aspect-ratio: 1/1`. Tenhou independently converged on the same idea with a solver that searches tile
   width downward and derives the centre panel as `2 tile-lengths + 1 tile-thickness`. This replaces
   breakpoint-by-breakpoint layout with two media queries and no JS measurement, and it is the single
   highest-leverage change available.

2. **Hide illegal actions instead of rejecting them.** しらぎく麻雀 does not render the 和了 command when
   the win is illegal, so chonbo is impossible by construction. Extend the principle to 立直 (hidden when
   not menzen or when no draws remain, and cancellable before the discard with no penalty) and to chi/pon
   where the call would be illegal. This removes an entire class of error, removes the need for
   confirmation dialogs, and costs nothing but a predicate on the model.

3. **Build a real keyboard layer — nobody in this survey has one.** MFC's PC map is the best template
   (`A` riichi, `S` chi, `D` pon, `F` kan, `Space` tsumo/ron, `Z` pass, `X` toggle never-call, with
   right-click as select-confirm), and 電脳麻将's `:focus { transform: translate(0, -h/7) }` shows how to
   make the hand itself a focusable, visibly-cursored row. Add arrow keys to move a hand cursor,
   Enter/Space to commit, and Esc to cancel. The demand is proven by third-party Chrome extensions
   existing for a client that lacks it.

4. **Make animation a named, persisted, in-game-toggleable option with a reduced-motion fallback —
   including `prefers-reduced-motion`.** しらぎく麻雀's 「麻雀牌等の動画効果」 (あり/無し, settable pre-game
   under 対局環境設定 and mid-game from the top bar, persisted across sessions) is the best precedent, and
   its dice fallback — keep the dice still, change only their faces — is the right shape: **drop the
   travel, keep the information**. MJ's 演出モード シンプル adds per-effect granularity worth copying for
   cut-ins specifically. No surveyed client honours `prefers-reduced-motion`, so doing so is free
   differentiation.

5. **Put animation timings in the server's state document, keyed by observer.** OpenRiichi's
   `AnimationTimings` + `GameRenderContext(server_times, tile_scale, tile_size, observer_index, dealer,
   wall_split)` means replay and spectating reproduce the original pacing, and one document can be scaled
   for a reduced-motion client instead of rewriting keyframes. For a server-authoritative browser client
   this is a small schema decision with disproportionate payoff.

6. **Rotate one seat component into four, and make your own hand the only large one.** 電脳麻将 positions a
   single `.he`/`.shoupai` element with `rotate(90/180/270deg)` and gives your concealed tiles 56 px against
   every opponent's 42 px, with your own melds dropping to 42 px. This buys a clear visual hierarchy with
   no perspective maths, no duplicated markup and no per-seat CSS beyond a transform — ideal for a DOM
   client using flat SVG tiles.

7. **Encode the called tile's origin in the model (`+`/`=`/`-`) and render the sideways tile with CSS on a
   wrapper, not a rotated asset.** 電脳麻将's `lib/mianzi.js` places the sideways tile leftmost for 下家,
   middle for 対面 and third for 上家, always leftmost for chi, and builds a precise `aria-label`
   (`"シモチャからポン"`) from the same marker. Its `.lizhi { transform: rotate(270deg) }` and
   `.rotate { transform-origin: 0% 0%; rotate(270deg) translate(-h, 0) }` mean the existing CC0 SVG set
   needs no rotated variants. This is both physically correct and the cheapest possible implementation.

8. **Make the table interrogative: hold or focus a tile to reveal relationships.** MJ's spatial assists —
   hold a tile and every visible copy turns red, hold two taatsu and acceptances turn yellow, hold a dora
   indicator and the dora turn yellow, hold a riichi stick and everything discarded after it turns red —
   deliver more information than any always-on highlight, at the moment it is wanted, with no extra
   screen space. MFC's 対局アシスト tiering (and its suppression at プラチナ/ダイヤモンド) shows how to
   package the same features as a beginner mode rather than a permanent crutch.

9. **Bring Tenhou's assist state model to a modern client, and add furiten from the server.** Tenhou
   computes exactly which hand tiles can participate in an available call, marks them green and dims the
   rest (`#060`/`#600` vs `#030`/`#300`), dims everything but the drawn tile in tsumogiri mode, and drives
   its furiten indicator from a server `FURITEN` message rather than recomputing it client-side. The green/
   red *state* per tile is a better primitive than a glow, and server-authoritative furiten removes a
   whole class of client/server disagreement.

10. **Add the ARIA layer now, while the table is DOM.** 電脳麻将 proves it is cheap: `aria-label` on
    手牌/捨て牌/リーチ/ドラ/裏ドラ, `aria-live="assertive"` on each discarded tile, and descriptive meld
    labels. Riichi Advanced's tile-number overlay — `div.tile.one::after { content: "1" }` with a
    four-way `text-shadow` halo, behind a checkbox — is the one suit-identification assist any client
    ships and is trivially portable. Tenhou, being canvas, cannot do any of this; a DOM client that skips
    it is throwing away its main structural advantage.

**Honourable mentions**, just outside the ten: Tenhou's **tap/drag threshold of <10 px and <1000 ms**
(one line, correct behaviour); Tenhou's **distance-proportional meld animation** (`sqrt(dx²+dy²)/4000`);
電脳麻将's **random-tile-marking for opponent discards** so the concealed count stays right without
leaking information; 電脳麻将's **hand-overflow absorbed by `margin-left: -overflow`** instead of resizing
tiles; しらぎく麻雀's **faint-text disabled controls**; 電脳麻将's **five-role font token system**; and
電脳麻将's **payment screen reusing the centre panel's four seat positions** so settlement is read in the
same spatial arrangement as the game.

---

## 11. Conventions we must not break

Experienced riichi players read these as correctness, not preference. Breaking any of them makes a client
feel wrong even to someone who cannot say why.

**Meld and pond geometry — this is physical-table correctness.**

- The **sideways called tile's position must encode its origin**: leftmost from 下家, middle from 対面,
  third from 上家; **chi always leftmost**. Getting this backwards is a rules-visible error.
- The **riichi declaration tile lies sideways in the pond**, and the **1000-point stick is rendered
  attached to the declarer's pond**, not floating in the centre.
- If the riichi declaration tile is **called away**, the sideways mark **moves to the next discard** so a
  declared riichi always has exactly one sideways tile. 電脳麻将's model marks this explicitly (Tenhou's
  log format encodes it as `254` in the kawa array).
- **Melds read in ascending tile order** within each set, left to right.
- **Ankan is a closed set**: do not reveal all four tiles as if it were open, and keep open/added kan
  visually distinct.
- **The pond's first three rows hold six tiles each**, then continue. The 4th-row behaviour is a genuine
  divergence so it can be chosen — but rows 1–3 at six per row is what players expect to see.

**Hand presentation.**

- Your hand is **one horizontal row at the bottom**; concealed tiles contiguous, **melds in a separate
  block** with a visible gap and a slight size or weight difference.
- The **drawn tile is always visually separated** — a gap, an offset, or both. Losing this separation makes
  the hand unreadable at speed.
- Tiles sort by suit in the fixed order 萬 → 筒 → 索 → 字, and within a suit by rank.

**Centre information.**

- **Round wind and hand number** (東一局 …), **honba** (本場) and **remaining wall count** must be
  continuously visible. Honba and riichi sticks are **counts of 100-point/1000-point units**, and a player
  will notice immediately if the number is off by one.
- **Dora indicators are face-up tiles in their own row**, visually separated from the hand and the ponds.
  Ura dora is revealed only after a win.
- **Scores are five-digit numbers with a thousands separator**, and the current player's turn must be
  visually marked.

**Interaction.**

- **Click or tap a tile to discard it.** No client surveyed uses drag-to-discard, and adding it as the
  primary gesture would be a regression.
- **Pass is always available and always obvious**, including during a call window.
- There must be a **timeout action and a way to opt into repeating it** — every established client has an
  auto-discard or pass mode, and players on a slow connection depend on it.
- **A round-end screen that names the yaku with their han, then the points, then who paid whom.** The yaku
  list is how players learn and verify; omitting or merging it destroys trust in the scorer.
- **A final results screen in placement order with the score deltas**, and a **clickable replay**.
- **Sound must be individually attributable** — distinct cues per action per seat, and a separate yakuma
  sting. Players navigate partly by ear.

**Non-negotiables that are really about trust.**

- **The client must never show an action that is illegal**, and must never silently drop one that is.
- **Furiten state must be authoritative and visible**, not inferred client-side.
- **Ari-ari / rule variants must be discoverable in-client** — 天鳳, MFC and しらぎく麻雀 all change rules
  between tables, and players expect the client to tell them which set is live.
