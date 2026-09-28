# Addendum to `riichi-client-research.md`

This file **supersedes and extends** specific sections of `riichi-client-research.md`. Where the two
disagree, this file wins. It carries (A) measured layout data for しらぎく麻雀 that replaces the
"unverified" caveats in §6, (B) new material on 天鳳's app/viewer/mjai ecosystem, (C) five additional
small clients, (D) negative findings, and (E) the revised divergence table and ranked lists.

Evidence classes as in the main document: **source-verified** (the client's own shipped code or official
manual), **documented** (the operator's own how-to pages), **measured** (pixel analysis of an official
screenshot — new in this file), **approximate** (inferred). Nothing here is invented; unverifiable items
are named as such.

---

## A. しらぎく麻雀 — measured layout, colours and HUD (replaces §6.1–§6.4 caveats)

The game shell (`https://marguerite.gingerbeardman.com/Games/Marguerite_Mahjong/`) returns **HTTP 200 with
a zero-byte body** to both `web_fetch` and curl, so its HTML/JS/CSS cannot be read. The figures below come
from **measuring the publisher's own official screenshot** (422 × 322,
[ScreenShot.GIF](https://marguerite.gingerbeardman.com/Games/IMG/Mahjong/ScreenShot.GIF)) by pixel
histogram and connected-component analysis, plus the publisher's documentation. It is one downscaled GIF
of one mid-hand state, so **state-specific items remain approximate**; the measured geometry is solid.

### A.1 Layout

One flat, screen-filling top-down table. **No side panels, no avatars, no chrome.**

| Element | Measured value |
|---|---|
| Felt | **RGB(0,132,33) = `#008421`**; `#006B08` for edges and shadow |
| Menu bar | full-width strip **≈13 px** tall (≈4 % of a 322 px height), pinned top |
| Own hand | one row along the bottom edge, **roughly centred** (13 × 16 px ≈ 208 px ≈ 49 % of width) |
| Tile | **≈16 × 22 px** — ≈3.8 % of width, ≈6.8 % of height, aspect **≈1 : 1.38** |
| Side seats' tiles | drawn nearly **edge-on** as narrow vertical slivers with a yellow rim |
| Ponds | four-sided, one per player; side blocks ≈16 × 71–72 px, far seat ≈27 × 32–46 px |
| Centre | otherwise empty; a **red dashed rectangle ≈81 × 56 px** holds a black rounded plate (≈49 × 12 px) reading 「東場三局」 |
| Score plates | four black rectangles, **each 54 × 14 px**, white text: 「南 22,800」, 「西 28,500」, 「東 31,200」, 「北 21,700」 |
| Status box | light grey (198,198,198) **≈94 × 38 px**, bottom-right, one sentence (「下家の摸和です。」) |
| 進行 button | ≈30 × 16 px, just below-right of the status box |

Structural consequences worth copying or rejecting deliberately:

- **The score plates live inside the felt beside each seat's own pond, not in a fixed outer HUD**, and all
  four are drawn **upright** rather than rotated per seat. The scores therefore move *with* the table
  instead of framing it. Seating in that screenshot is 西 at the bottom, 北 right, 東 top, 南 left —
  standard counter-clockwise.
- **≈92 % of the height is table and 4 % is the menu bar.** This is the most aggressively information-first
  layout in the whole survey; it optimises for "a real table seen from above" plus rule configurability,
  not meta-progression.
- **The wall is drawn as physical tile backs**, not as a counter: a segmented vertical bar (~8 tiles,
  16 px wide) and a horizontal bar, sitting between the hand and the centre.
- Menu bar contents, left to right: 「無音モード」 (sound state), 「牌等の動画効果：切」 (animation state),
  「菜単に戻る」 (exit)
  ([基本的な操作方法](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E5%9F%BA%E6%9C%AC%E7%9A%84%E3%81%AA%E6%93%8D%E4%BD%9C%E6%96%B9%E6%B3%95.html)).
- Commands render **above the hand** when a discard choice or call decision is pending.
- **Disabled controls render as faint text** (「薄い文字で表示され」) rather than disappearing — the sound
  indicator greys out when audio is unavailable, and 菜単に戻る greys out on menu screens.

### A.2 Information design — and what is *absent*

Always on screen: own hand, all four ponds, all four wind+score plates, the centre round plate, the wall
as tile backs, and the top bar.

**Absent, and this is the important finding: no wall counter, no dora indicator, and no riichi-stick/供託
display are visible.** The rules page documents 供託点 and 「王牌は常に十四枚留め」, so both exist in the
model ([リーチ麻雀のルール](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%83%AA%E3%83%BC%E3%83%81%E9%BA%BB%E9%9B%80%E3%81%AE%E3%83%AB%E3%83%BC%E3%83%AB.html)) —
but I could not verify any on-screen representation. Treat the HUD as **approximate**.

**There is no action log and no match-record panel.** Instead a single grey box prints one sentence per
event, and the pond is the de-facto record. Round/hand is a black plate with white text; scores are text.
The only colour-coded base-HUD element is the red dashed centre frame.

Two ideas worth more than the missing HUD:

1. **Outcome is encoded as seat colour at hand end, not as a result panel**
   ([FAQ](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%82%88%E3%81%8F%E3%81%82%E3%82%8B%E3%81%94%E8%B3%AA%E5%95%8F.html)):
   the winner's wind turns **yellow**; the discarder **light blue**; a player beaten by 頭跳ね **ochre**;
   at an exhaustive draw tenpai **yellow** and noten **light blue**; 九種幺九倒牌 **light blue**. The table
   itself carries the outcome, so no overlay is needed.
2. **Tsumogiri can be greyed in the pond.** Since 2021-01-04 an option 摸切牌の表示 (台湾麻将除) with a
   「灰色で表示」 choice draws tsumogiri in grey so it reads differently from tedashi — added **because
   users of other online mahjong asked for it**, and **default off**
   ([開発メモ 2021-01-04](https://marguerite.gingerbeardman.com/Nihongo/Games/Memo/%E4%BB%A4%E5%92%8C03%E5%B9%B401%E6%9C%8804%E6%97%A5/1)).
   MFC encodes the same information by drawing tsumogiri slightly darker; 天鳳 gates it behind payment.

Also configurable, and unusual: **「副露門子を置く位置」** lets you put called sets left or right of the
hand (default **left**); self/opposite ankans always go left
([環境選択画面](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E7%92%B0%E5%A2%83%E9%81%B8%E6%8A%9E%E7%94%BB%E9%9D%A2.html)).

### A.3 Tile and table rendering — measured

- **Flat 2D sprites.** No perspective, no 3D, no face bevel. Side seats' tiles are drawn nearly edge-on as
  narrow vertical slivers with a yellow rim.
- **Tile backs default to pure yellow `#FFFF00`** and are selectable among seven schemes: 常時黄色 /
  常時青色 / 常時緑色 / 黄色と青色を一局交代 / 黄色と緑色を一局交代 / 青色と緑色を一局交代 / 常時青の濃淡.
  Mobile is fixed to yellow.
- **The felt is selectable among 黒色 / 青色 / 緑色**, and the same colour is the page background outside
  play ([FAQ](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%82%88%E3%81%8F%E3%81%82%E3%82%8B%E3%81%94%E8%B3%AA%E5%95%8F.html)).
- **Dimming, twice over.** (1) At hand end, hands of players not obliged to open are drawn washed-out —
  **measured (206,214,214)** against **(255,255,255)** for live faces; the FAQ explains this means "these
  would not normally be opened", i.e. しらぎく **dims rather than silently opening** them. (2) tsumogiri in
  the pond can be greyed (above).
- Tile glyphs are traditional (萬/筒/索/字). **There is no simplified or easy-read tile set** — contrast
  Kemono Mahjong, which ships one (below).
- **Sideways riichi tile: could not verify.** No riichi appears in the screenshot and no documentation page
  describes the rotated riichi discard.

### A.4 Settlement — the biggest gap

No operation, FAQ, rules or environment page describes a post-hand or post-game result/精算 screen, and the
only screenshot is mid-hand. **Unverified.** What *is* documented is the structure a settlement would
reflect: 東南半荘戦 by default, with 西入/北入 when nobody reaches 31,000, plus options for 延長戦, 荘家
and やめ, and 平局輪荘 handling
([リーチ麻雀のルール](https://marguerite.gingerbeardman.com/Nihongo/Games/%E3%81%97%E3%82%89%E3%81%8E%E3%81%8F%E9%BA%BB%E9%9B%80/%E3%83%AA%E3%83%BC%E3%83%81%E9%BA%BB%E9%9B%80%E3%81%AE%E3%83%AB%E3%83%BC%E3%83%AB.html)).
The always-visible 54 × 14 score plates are the only score UI I could actually see.

---

## B. 天鳳 — the app, the viewers and the mjai ecosystem (extends §1)

### B.1 The official app is a WebView shell, not a native UI

Tenhou's own top page advertises アプリ版 (iPhone/iPad/Android) alongside Web版/Desktop4K版
([tenhou.net](https://tenhou.net/)). The iOS listing resolves to **麻雀 天鳳 by C-EGG INC., version
1.69.13, released 2017-01-25, last updated 2024-01-25, 1.16 MB, free, 17+, rated 2.17 / 5 from 1,063
ratings**, with a bundle id byte-identical to the Android package id `net.tenhou.WebBrowser20161220`
([iTunes lookup API](https://itunes.apple.com/lookup?id=1187938169&country=jp)).

A **1.16 MB** binary with a package id literally containing "WebBrowser" is a thin WebView wrapper around
the web client — not a native mahjong UI — and the 2.17/5 rating is what players think of that. The lesson
for a new client is direct: **do not ship a WebView shell and call it an app.** Either build a real
touch-first UI or ship only the responsive web client.

### B.2 The web client is Canvas 2D, not WebGL

Independent analysis of the shipped bundle `/3/1911.js` (228,240 bytes) counted `getContext` × 7,
`drawImage` × 10, `requestAnimationFrame` × 12 and **`WebGL` × 0** — i.e. **Canvas 2D with nine-argument
sprite-sheet blits**. This corroborates the geometry findings in §1.1: the pseudo-3D look is achieved by
blitting pre-shaded sprite variants, not by a 3D pipeline. For a browser client that wants Tenhou's
performance profile without its rendering constraints, Canvas 2D worth of guarantees is enough.

### B.3 The official viewers, and a feature worth stealing

[tenhou.net/mjlog.html](https://tenhou.net/mjlog.html) documents the canonical log schema and three
renderers of the same log: the 牌譜エディタ at `/6/`, the HTML5+JS 牌譜ビューアβ at `/5/`, and `/0/` as the
alias. The stated viewer controls are:

- 「右クリック左クリックで進む戻る」 — right-click / left-click to step back / forward
- 「ホイールで進む戻る」 — wheel to step
- 「長押しでオートリピート」 — long-press for auto-repeat
- 「局一覧は「＃」から」 — hand list from the `#` key
- **「手牌をクリックすると牌理が開きます」 — clicking a hand tile opens 牌理, the tile-efficiency
  calculator**, on that position

That last one is the best single feature in Tenhou's entire replay layer and it belongs in every new
client: the replay viewer and the analysis tool are **the same surface**, so "why was this discard wrong?"
is one click away from the position in question. The log can also be embedded:
`<iframe src="//tenhou.net/5/?log=…" style="width:480px;height:320px;">`. The page notes 「PCはキー入力、
iPad他はタッチ入力を主としています」 and that it does not support サンマ or パオ.

### B.4 The JSON log format, precisely

The `/6/` format is `{"title":[…],"name":[…],"rule":{"disp":"般南喰赤","aka":1},"log":[[…]]}` with tiles
coded **11–19 萬 / 21–29 筒 / 31–39 索 / 41–47 字牌 / 51–53 赤五**, and — the part that matters for a table
renderer — **`60` means ツモ切り**. Actions are string tokens: `"r17"` 立直, `"c151416"` チー,
`"p252552"` ポン, `"m39393939"` 明槓, `"31k313131"` 加槓, `"121212a12"` 暗槓
([tenhou.net/mjlog.html](https://tenhou.net/mjlog.html)).

### B.5 mjai — the de-facto AI interchange protocol

The open-source centre of gravity is [gimite/mjai](https://api.github.com/repos/gimite/mjai) ("Game server
for Japanese Mahjong AI", Ruby, created 2012-04-30, last pushed 2021-04-07, 67★), whose protocol is **one
JSON object per line over TCP** using the same tile notation (`1m`–`9m`, `1p`, `1s`, `E/S/W/N/P/F/C`, reds
`5mr/5pr/5sr`, `"?"` for a hidden tile) and events `start_kyoku`, `dahai` (with an explicit `tsumogiri`
boolean), `pon`/`chi`/`kakan`/`daiminkan`/`ankan`, `reach`, `dora`, `hora` (with `yakus`, `fu`, `fan`,
`deltas`) and `ryukyoku`. Around it: `NikkeTryHard/tenhou-to-mjai` (Rust, 60★, Tenhou→mjai conversion) and
`shinkuan/Akagi` (Rust, 1,064★, a real-time AI overlay supporting Majsoul/Tenhou/Riichi City/Amatsuki).
Also reported but not re-opened: `Equim-chan/mjai-reviewer` (1,247★, mjai.ekyu.moe), `mjx-project/mjx`,
`vdanchenkov/tenhud` (an external HUD that marks tsumogiri with a coloured background and renders a
hidden-tile map), `mthrok/tenhou-log-utils`.

**Why this matters to a browser client:** mjai is what third-party analysis tools speak. A server that can
emit mjai gets Mortal/mjai-reviewer/tenhud compatibility for free, and a client that can *render* mjai gets
an entire tool ecosystem. Note that `dahai` carries `tsumogiri` as a **first-class boolean** — the same
distinction しらぎく added in 2021 and MFC draws with shading. It is the one piece of state every
serious implementation treats as explicit.

---

## C. Five more clients

### C.1 マルジャン (Maru-Jan) by シグナルトーク — the best keyboard model found

A **downloadable client, not a browser game**, with one account across PC/Mac/iOS/Android/Fire TV/Microsoft
Store ([maru-jan.com](https://www.maru-jan.com/)). Long-running: 22nd-anniversary badge on the top page,
and 「２００４年からサービスを開始し、会員数180万人を超える」 with 「永久に残る１８９１種類の個人成績」 —
including 平均打牌速度, 副露率 and 着順１位 ([mobile](https://www.maru-jan.com/mobile/)). A 2019 press
release said 120万 members against the site's current 180万 — growth, not contradiction
([SignalTalk 2019-12-11](https://www.signaltalk.com/press/20191211.php)).

Its rendering bet is **deliberate photo-realism rather than stylisation**: it models the real automatic
table 「NINJA」, ships two tile sets, claims 「細部までこだわりリアルな牌グラフィックを作り上げました。
ドットの部分から正確に描き上げています。」, and the discard and point-stick sounds were **recorded from an
actual NINJA table** ([game_saigen.html](https://www.maru-jan.com/game_saigen.html)). Resolution is
1024 × 768, or 2048 × 1536 in the 4K build ([game/4k.html](https://www.maru-jan.com/game/4k.html)).
Whether that is flat-2D or 3D is **approximate** — the site only claims 「リアル」.

**The input model is the most instructive in this study**
([game_sousa.html](https://www.maru-jan.com/game_sousa.html)):

- 「Maru-Janでは、マウス・キーボードのどちらでも操作が行えます。」 with separate 「マウス操作」 and
  **「キーボード操作(Windows版限定)」** sections — keyboard is first-class but platform-gated.
- Named on-table toggles: 「鳴きあり」「鳴きなし」「鳴き指定」「理牌オート」「手動理牌」「終了」「代走」
  「ポイント購入」「戦績表示」.
- Discard by clicking the tile that 「飛び出た」 (pops out) while hovering a call choice.
  鳴き指定 = 「牌の上で右クリックすると鳴き指定」 with modes 「１鳴き」「２鳴き」「赤５鳴き」 — per-tile call
  permission, like MJ's 特殊鳴き.
- **The cleanest auto-discard affordance found anywhere**: 「■右クリックツモ切り … 打牌選択時に卓内で
  右クリックを押すとツモ切りができます。」 — right-clicking **anywhere on the table** during discard
  selection is tsumogiri, and **after riichi, right-clicking anywhere is tsumogiri**, suppressed while the
  「ロン・ツモ」 prompt is up or リーチ/カン shows in red. One gesture, the whole table, mode-aware.
- Sorting: 「手動理牌を選んだ状態で牌をドラッグすると並び替えができます。(自動理牌を選択している場合は、
  打牌完了後に自動理牌します)」 — drag to sort when manual sorting is on, otherwise auto-sort fires after
  each discard.
- **iOS deliberately differs from desktop**: 「切りたい牌を２回タップで打牌」 — **double-tap, not drag** —
  tiles move by 「牌をタッチしたまま動かす」, and 鳴き指定 is 「ボタン設定後、牌の上で下フリック」
  (flick down on the tile).
- Clock: 四麻東南 標準 5 s + 30 s, 高速 3 s + 10 s, セット卓 30 s + 60 s, with
  「持ち時間が０秒になると自動的に打牌します」.
- Replay keys are textual even though the main bindings are images: コマ戻し = 右クリック or 「←」,
  コマ送り = 左クリック or 「→」.
- Progression: 段位 starts at 20級, auto-promotes, and **can demote above 10級**; 雀力 moves with finishing
  position and 「東南戦は追加雀力が倍になります」
  ([dani_system.html](https://www.maru-jan.com/dani_system.html)); rating starts at 1500, becomes visible
  in-game at Rt1550+ and to opponents at Rt1600+ ([rating_system.html](https://www.maru-jan.com/game/rating_system.html)).

**Unverified:** flat-2D vs 3D, any animation/sound/BGM toggle *names* (no in-game settings screen is
documented on any reachable page), and the 精算/結果 screen. The keyboard key bindings themselves are
rendered as **images**, so they could not be extracted as text.

### C.2 麻雀 雷神 -Rising- by Ateam — the "3D + CPU ladder" funnel

iOS release 2010-11-12, last updated 2024-10-15, version 6.0.14, 52 MB, free, **2.98 / 5 from 1,760
ratings**; explicitly 3D: 「累計800万ダウンロード突破」「【無料】で遊べる本格3D麻雀ゲームです。3Dグラフィック、
AIにこだわって、麻雀を実際に打っているかのようなリアルさを追求」, with 一般卓/上級卓 online, 友達対局, and a
single-player 雷神バトル of 96 staged battles, plus an unusually explicit rules list (アリアリ, 赤3枚,
トビあり, ダブロン/トリプルロン with 頭ハネ for 供託, 流し満貫, パオ, 数え役満, 食い替えなし)
([iTunes search API](https://itunes.apple.com/search?term=%E9%BA%BB%E9%9B%80%20%E9%9B%B7%E7%A5%9E&country=jp&entity=software&limit=3)).

An editorial review notes 「牌が見やすくグラフィックもきれい。ドラ牌の表示も分かりやすい。タッチがききにくいなど、
操作面における不具合もないため、ものすごく遊びやすい。なお、オンライン対局を見越したのか、細かいルールを設定できる
機能はない。」 ([appget.com](https://appget.com/appli/view/59344/)) — so: 3D, readable tiles, a clear dora
display, and **no fine-grained rule configuration**, the exact opposite trade-off to しらぎく and
Maru-Jan. There is no official 操作説明 page, so its discard gesture and layout are **unverified**. Note a
source discrepancy: the official site's meta says 700万DL while the App Store says 800万 — the site text is
stale.

### C.3 麻雀 和 -Nagomi- by Zoo Corporation

Steam appid 1356180, developer and publisher **Zoo Corporation** (not Yosemite — an aggregator's
attribution resolves via Apple's API to an unrelated map app), released 2020-08-06, 246 reviews. The
Japanese store copy promises 「本格派な3D麻雀を軽快な動作でサクサク楽しめます」 with 有名ローカル役,
**詳細なルール設定**, BGM, single-player against 3 CPUs, network play, and voice for 鳴き/ツモ and **yaku
read-aloud** ([Steam appdetails API](https://store.steampowered.com/api/appdetails?appids=1356180&l=japanese)).
No reviewer commentary on layout, input or hints was found — **approximate**.

### C.4 Kemono Mahjong by Cyberdog Software — the mobile-first small client

The best-documented small client and the most directly instructive for a browser client. Apple's
description states verbatim: "**Unique layout designed for mobile devices**", "Beautiful, easy-to-read
tiles (**with traditional and simplified tile sets**)", "Tutorials and in-game help, ideal for new
players!", 4-player vs 3 CPU or online, "3-player (Sanma) mode!", EMA riichi rules, and "No ads!". Data:
version 1.52.01, iOS release 2017-08-20, **$3.99**, **4.88 / 5 from 313 ratings**
([iTunes lookup API](https://itunes.apple.com/lookup?id=1207602683)). Steam (appid 1508430, 11 Jan 2021,
142 reviews) repeats the two-tile-set line and adds **full controller support** and cross-platform online
([Steam appdetails API](https://store.steampowered.com/api/appdetails?appids=1508430&l=english)).

Three transferable decisions: **a simplified tile set as a cheap legibility win** (the only client besides
Riichi Advanced to offer one), **a layout designed mobile-first rather than a shrunken desktop table**, and
**a tutorial/help layer advertised as a headline feature** rather than hidden in a menu. **Unverified:** any
named animation/speed setting, and the exact discard input (tap vs drag vs two-tap).

### C.5 OpenRiichi and Riichi Advanced — see main document §7.1–§7.2

Unchanged. Riichi Advanced remains the single most reusable layout artefact; OpenRiichi remains the best
timing-architecture precedent.

---

## D. Negative findings — do not spend time on these

- **しらぎくリーグ does not appear to exist.** Japanese searches for しらぎくリーグ / しらぎく杯 /
  しらぎく 麻雀 リーグ return nothing mahjong-related. しらぎく is a self-contained local HTML5 app.
- **雀SEED does not resolve to a client.** An iTunes Search API query for 雀SEED in the JP store returns
  exactly two apps, both by **CommSeed Corporation**, neither named 雀SEED. Every other hit is
  **雀荘 SEED, a real free-mahjong parlour chain, not a client**
  ([iTunes search API](https://itunes.apple.com/search?term=%E9%9B%80SEED&country=jp&entity=software);
  [mahj0ng.net](https://mahj0ng.net/parlor/seed/)). The brief's 「雀SEED」 is most likely a conflation of
  CommSeed (publisher of グリパチ pachinko-slot apps) or the parlour chain.
- **「Jong」 as an open-source browser riichi client does not exist.** GitHub API searches returned nothing;
  the only hits are a yanked 228-line crates.io crate and an unrelated 2013 Objective-C repo.
- **「奈良麻雀」 could not be verified** as any app or client — only Nara mahjong parlours and a one-off
  tournament.
- **麻雀格闘倶楽部 スリーサウザンド does not appear in Konami's own 20th-anniversary series history**; the
  nearest entries are 麻雀格闘倶楽部 DS Wi-Fi対応 and 麻雀格闘倶楽部 touch
  ([MFC 20th history](https://p.eagate.573.jp/game/mfc/20th/history/)).
- **No open-source 雀魂-equivalent client exists**, so its table rendering is not available to copy. The
  only hit was a private-server project, snippet-only.
- **`Euphyllia/…` mahjong, `Apaszke/…` mahjong, `saki-rs`, `riichi-mahjong-js` and `ninegate/…` were all
  searched for and do not exist.**
- **Wikipedia (ja/en) was unreachable** from the research environment, so several rule claims are cited to
  league/etiquette pages rather than the encyclopedia.
- An English-language index of clients worth checking is `chombo.club/en/help/`, which lists Tenhou,
  Mahjong Soul, Riichi City and Kemono Mahjong (surfaced but not opened).

---

## E. Revised divergence table and ranked lists

### E.1 Corrections to the main document's divergence table

| Row | Correction |
|---|---|
| **Keyboard input** | The main document says MFC is "the only client with a real keyboard map". **That is wrong — Maru-Jan documents 「キーボード操作(Windows版限定)」 as a first-class input path** with separate mouse and keyboard sections, and its replay viewer binds コマ戻し/コマ送り to 右クリック/左クリック or ←/→. So **two** commercial clients have keyboard play: **MFC PC (A/S/D/F/Space/Z/X)** and **Maru-Jan Windows** (keys documented as images, so the specific bindings remain unverified). 天鳳, 雀魂, 電脳麻将-play, しらぎく, 麻雀 雷神 and Nagomi have none. |
| **Tile-back design** | Add しらぎく: **default pure yellow `#FFFF00`**, seven selectable schemes including **per-hand alternation between two colours**, and a selectable felt (黒/青/緑). This is the most configurable tile-back system in the survey — ahead of Tenhou's RGB sliders. |
| **Called-tile / tsumogiri indication** | Add しらぎく's **grey tsumogiri in the pond** (option 摸切牌の表示 → 灰色で表示, default **off**, added 2021-01-04 at user request) and MFC's darker tsumogiri. Three agents agree the distinction matters; 天鳳 paywalls it. |
| **Where scores live** | Add しらぎく: **inside the felt beside each seat's own pond**, all four drawn **upright** rather than rotated per seat. A third answer to the question, and the only one where the score plates move with the table. |
| **Meld position** | New divergence: しらぎく makes **「副露門子を置く位置」** (left or right, default **left**) a user setting, with self/opposite ankans always left. 電脳麻将 floats melds **right** (`float: right` on `.fulou`). 天鳳 and 雀魂 put them right. This is a real convention split, not a preference. |
| **Tile sets** | Kemono Mahjong ships **traditional and simplified** tile sets; Riichi Advanced ships a **tile-number overlay** (`div.tile.one::after { content: "1" }`). しらぎく ships neither. These are the only two legibility aids found in any client. |
| **App distribution** | 天鳳's official iOS/Android app is a **1.16 MB WebView shell** rated 2.17/5. A cautionary data point about shipping a wrapper. |

### E.2 Additions to the ranked top-10

The main document's ten ideas stand. Two additions are strong enough to enter the list, and one existing
entry needs upgrading.

**New — enters at #4: make replay and analysis the same surface.** Tenhou's HTML5 viewer opens 牌理 (the
tile-efficiency calculator) when you **click a hand tile** in the log —
「手牌をクリックすると牌理が開きます」 ([tenhou.net/mjlog.html](https://tenhou.net/mjlog.html)) — and it
renders the log in-place via `<iframe src="//tenhou.net/5/?log=…" style="width:480px;height:320px;">`. No
other client surveyed connects "I lost this hand" to "what should I have discarded" with one click. For a
client that already has a rules engine and a 何切る/牌理 surface, this is a small join with outsized learning
value, and it pairs naturally with MFC's 牌譜→何切る quiz pipeline.

**New — enters at #9: one gesture for the whole table's auto-discard, made mode-aware.** Maru-Jan's
right-click tsumogiri — 「打牌選択時に卓内で右クリックを押すとツモ切りができます」, and after riichi
right-clicking *anywhere* is tsumogiri, automatically suppressed while a ロン/ツモ prompt is live or
リーチ/カン is highlighted — is the cleanest auto-discard affordance found: no dedicated button, no mode to
remember, and it cannot fire during a decision it would ruin
([game_sousa.html](https://www.maru-jan.com/game_sousa.html)). Tenhou's double-click is the same idea with
a worse failure mode: it is *the timeout action*, so a mis-timed double-click discards a tile you did not
choose, and the manual's own FAQ has an entry about accidentally tsumogiri-ing because of it.

**Upgrade to existing #1: adopt the measured tile proportion deliberately.** The survey now has four data
points — 天鳳 31:47 (0.66), しらぎく ≈16:22 (**1:1.38, i.e. 0.72**), 電脳麻将 5:7 (0.714), and the
FluffyStuff art / Riichi Advanced / riichi-ui at 3:4 (0.75). The project's current 32 × 43 = 0.744 is
within a rounding error of three of them and matches the art it already ships, so **do not change the tile
ratio for its own sake** — change the *derivation*, by making one `--tile-size` token the source of truth
for hand, pond, dora, meld previews and both dialogs, as Riichi Advanced and Tenhou both independently do.

### E.3 Additions to "conventions we must not break"

Two more, both now evidenced across at least three clients:

- **Tsumogiri must be distinguishable from tedashi in the pond.** 天鳳 (paid, 牌譜/観戦のツモ切り暗転表示),
  しらぎく (grey, optional, user-requested), MFC (darker), and MJ (「自動操作モード」 semantics) all encode
  it. It is the single most-read piece of information a pond carries about an opponent's hand.
- **The app must not be a WebView wrapper.** 天鳳's 1.16 MB shell rated 2.17/5 is the field evidence;
  a client should either commit to a real touch-first responsive UI or ship no app at all.
