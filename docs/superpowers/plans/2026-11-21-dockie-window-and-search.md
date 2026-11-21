# Dockie Window, Game Search and Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Dockie its own rounded, frameless window with a styled scrollbar, a game search with clear buttons in both Join boxes, click-to-select cards, and a polish pass.

**Architecture:** Rust adds one command (`search_games`) and window config/capabilities; everything else is the plain HTML/CSS/JS in `ui/`. Pure decision logic lives in `ui/state.js` (Node-tested); DOM behaviour is checked in a throwaway browser harness with a fake backend; window behaviour is checked in a real debug build with an isolated data folder.

**Tech Stack:** Tauri 2.12, Rust (reqwest 0.13, serde_json), vanilla JS/CSS in WebView2.

**Spec:** `docs/superpowers/specs/2026-11-21-dockie-window-and-search-design.md`

## Global Constraints
- Follow the `polishing-ui` checklist; read its `reference.md` before writing CSS/JS. OKLCH colours, alpha borders, motion ≤ 300 ms on transform/opacity, press `scale(.96–.97)`, honour `prefers-reduced-motion`.
- Window corner radius `14px`; square and borderless when maximized.
- No new dependencies (Rust or JS). No `innerHTML`; build DOM with `el()`/`svg()`.
- Account data and search results reach the page only through `textContent`, properties and attributes.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus
1. Stale search replies: typing fast never shows results for an older query, and a reply never reopens the list after a pick, a clear, or the sheet closing.
2. Enter/Escape in a search box: with no highlighted row, Enter keeps today's behaviour (Save / launch selected); Escape closes the list without closing the Edit sheet.
3. Search failure (offline, rate-limited, nothing found) shows one quiet row, no toasts; typing and Save keep working.
4. Maximize by any route (button, double-click, Win+Up, snap) squares the corners and drops the border; restore brings them back; dialog backdrops match.
5. Click-to-select never fires from buttons, the checkbox label, menus, or a running preview (which focuses the window).

---

### Task 1: Search plumbing (pure logic + Rust command)

**Files:**
- Modify: `ui/state.js`, `ui/state.test.js`
- Modify: `src-tauri/src/roblox_api.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/join.rs` (test only)

**Interfaces:**
- Produces (JS): `searchPlan(text: string) -> { query: string, follow: string | null } | null`
- Produces (Rust): `#[derive(Serialize)] pub struct GameHit { place_id: u64, universe_id: u64, name: String, players: u64, url: String, icon: Option<String> }`; `fn parse_game_search(v: &Value) -> Vec<GameHit>`; `Api::search_games(&self, query: &str) -> Result<Vec<GameHit>, ApiError>`; command `search_games(query: String) -> Res<Vec<GameHit>>`.

- [ ] **Step 1: failing JS tests** in `ui/state.test.js`:
```js
const { searchPlan } = require("./state.js");
assert.equal(searchPlan(""), null);
assert.equal(searchPlan("a"), null);
assert.equal(searchPlan("123"), null);
assert.equal(searchPlan("@jake"), null);
assert.equal(searchPlan("https://www.roblox.com/games/1"), null);
assert.equal(searchPlan("roblox.com/games/1/x"), null);
assert.equal(searchPlan("123:abc-def"), null);
assert.deepEqual(searchPlan("blox fruits"), { query: "blox fruits", follow: null });
assert.deepEqual(searchPlan("  Arsenal "), { query: "Arsenal", follow: "Arsenal" });
assert.deepEqual(searchPlan("ab"), { query: "ab", follow: null });
```
- [ ] **Step 2:** `node ui/state.test.js` → Expected: FAIL (`searchPlan is not a function`).
- [ ] **Step 3:** implement `searchPlan` in `ui/state.js` and export it. Search when trimmed length ≥ 2 and the text is not digits, not `digits:job`, does not start with `@`, and contains neither `/` nor `roblox.com`. `follow` = the text when it matches `^[A-Za-z0-9_]{3,20}$` (same rule as `parse_join`).
- [ ] **Step 4:** `node ui/state.test.js` → Expected: `state ok`.
- [ ] **Step 5: failing Rust tests.** In `roblox_api.rs` tests: `parse_game_search` on a fixture with one `"contentGroupType":"Game"` group holding two games (one with `canonicalUrlPath`, one without) and one non-game group → two hits; first `url == "roblox.com/games/2753915549/Blox-Fruits"`, second falls back to `"roblox.com/games/<rootPlaceId>"`; `icon == None`; entries without `rootPlaceId` skipped; at most 8 hits. In `join.rs` tests: `parse_join("roblox.com/games/2753915549/Blox-Fruits") == Ok(Game { place_id: 2753915549 })` (guards the fill format).
- [ ] **Step 6:** `cargo test` in `src-tauri` → Expected: FAIL to compile (`parse_game_search` missing). The join test may already pass; that is fine.
- [ ] **Step 7:** implement `GameHit`, `parse_game_search`, `Api::search_games` (GET omni-search with URL-encoded `searchQuery`, `pageType=all`, and a `sessionId` made once per run from the clock in UUID shape; then one GET `thumbnails.roblox.com/v1/games/icons?universeIds=<csv>&size=150x150&format=Png&isCircular=false` to fill `icon`; an icon failure keeps the hits without icons). Add the `search_games` command and register it in `lib.rs`.
- [ ] **Step 8:** `cargo test` → Expected: all pass.
- [ ] **Step 9:** commit `feat: game search command and search rules`.

### Task 2: Frameless rounded window + scrollbar

**Files:** Modify `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `ui/index.html`, `ui/style.css`, `ui/app.js`

**Interfaces:** Produces `syncMax()` in `app.js` (sets `html.max` and the max button's label).

- [ ] **Step 1:** window config: `"decorations": false, "transparent": true, "shadow": false`. Capabilities add `core:window:allow-minimize`, `core:window:allow-toggle-maximize`, `core:window:allow-internal-toggle-maximize`, `core:window:allow-close`, `core:window:allow-start-dragging`.
- [ ] **Step 2:** header gets `data-tauri-drag-region` (and on the brand and a flex spacer), plus `.winctl` with `#win-min` ("Minimize"), `#win-max` ("Maximize"/"Restore", two stacked icons crossfading with blur), `#win-close` ("Close").
- [ ] **Step 3:** `app.js`: `getCurrentWindow()`; min → `minimize()`, max → `toggleMaximize()`, close → `close()` (backend `CloseRequested` keeps the quit prompt); `syncMax()` on load and on `onResized`.
- [ ] **Step 4:** CSS: `html` transparent; `body` radius 14px, `1px` `--line-2` border, `overflow: hidden`; `html.max body` radius 0, no border; `dialog.sheet::backdrop` radius 14px (0 when maximized); window buttons 34×30, radius 8, close hover red with white icon, press scale .96; `user-select: none` on the app with text fields exempt; `::-webkit-scrollbar` 12px, transparent track, thumb `rgb(255 255 255/.12)` with 3px transparent border + `background-clip: padding-box`, hover `.22`, no buttons. Do not set `scrollbar-width`/`scrollbar-color` (they disable the webkit styling).
- [ ] **Step 5:** commit `feat(ui): frameless rounded window with own title bar and scrollbar`. Checked live in Task 5.

### Task 3: Search dropdown + clear buttons

**Files:** Modify `ui/index.html`, `ui/style.css`, `ui/app.js`

**Interfaces:** Consumes `searchPlan` and `search_games` (Task 1). Produces `combo(input)` in `app.js`.

- [ ] **Step 1: failing harness checks** (throwaway `.superpowers/harness/`, fake backend with a slow `search_games`): `stale_reply_ignored` (type "blox", then "arsenal" before the first reply lands → only arsenal results), `no_reopen_after_pick`, `enter_without_row_saves` (Edit sheet closes, `set_join` not called with search text unless typed), `enter_on_row_picks` (value becomes the hit's `url`, sheet stays open), `escape_keeps_sheet` (list closes, dialog open), `failure_row` (rejecting `search_games` → one row "Couldn't search Roblox right now", no toast), `clear_button` (× hidden when empty, clears and refocuses), `follow_row_first` ("arsenal" → first option "Follow @arsenal"). Run → Expected: all FAIL.
- [ ] **Step 2:** markup: wrap `#join-all` and `#edit-join` each in `.combo` with a `.clear` button (`type="button"`, `aria-label="Clear"`) and a `role="listbox"` `.results`. Inputs get `role="combobox"`, `aria-autocomplete="list"`, `aria-expanded`, `aria-controls`, `autocomplete="off"`, `spellcheck="false"`. Move `#edit-join-label` out of the `<label>`; link it with `aria-describedby`, `aria-live="polite"`.
- [ ] **Step 3:** `combo(input)`: 250 ms debounce; drop replies whose query no longer matches `searchPlan(input.value)`; rows = Follow row (if `follow`) then games (icon 28px radius 7px, name, `N playing` in `.num` with `toLocaleString`); "Searching…" while waiting; "No games found" when empty; arrows move `aria-activedescendant`; Enter on a highlighted row picks (`preventDefault` + `stopImmediatePropagation`) and dispatches `input`; Escape with the list open closes it (`preventDefault`); `mousedown` on rows keeps focus; blur closes. Register before the existing Enter→launch listener.
- [ ] **Step 4:** CSS: `.results` like `.menu` (pop surface, radius 12, rows radius 8, enter from `scale(.97)` at `transform-origin: top`), `max-height: 320px` with the styled scrollbar; `.clear` fades/scales in; `dialog.sheet { overflow: visible }` so the list isn't clipped.
- [ ] **Step 5:** re-run harness → Expected: all pass. `node ui/state.test.js` → `state ok`.
- [ ] **Step 6:** commit `feat(ui): game search and clear buttons in Join boxes`.

### Task 4: Click-to-select + polish sweep

**Files:** Modify `ui/style.css`, `ui/app.js`

- [ ] **Step 1: failing harness checks:** `card_click_selects` (click `.name` toggles selection and "Launch 1"), `buttons_dont_select` (Launch, ⋯, menu items, checkbox label: selection count unchanged), `idle_preview_selects`, `live_preview_focuses` (calls `focus`, selection unchanged), `menu_opens_up_near_bottom`, `menu_arrow_keys` (keyboard-opened menu focuses first item, ArrowDown moves, Escape returns focus to trigger), `enter_nothing_ticked_toasts`, `join_label_race` (late describe reply for an old value is ignored). Run → Expected: FAIL.
- [ ] **Step 2:** tile click handler ignoring `button, input, label, .menu-wrap`; `.thumb:disabled { pointer-events: none }`; `.tile:not(.add):hover` border `--line-2`.
- [ ] **Step 3:** press scale on `.seg button`, `.tile.add .main`, `button.link`, `.clear`, `.winctl button`; shake skipped under reduced motion; stagger `calc(min(var(--i, 0), 8) * 40ms)`; `.menu.up` when the menu would pass the bottom of `main`; menu arrow keys / Escape-to-trigger / visible focus (`rgb(255 255 255/.1)`); Join label race guard; Enter in `#join-all` with nothing ticked → toast "Tick the accounts you want to launch first."; `.win` columns `minmax(0, 1fr) auto` with `min-width: 0` on the title cell; toasts fade out over 150 ms.
- [ ] **Step 4:** harness → Expected: all pass (Task 3's too).
- [ ] **Step 5:** commit `feat(ui): click to select and polish pass`.

### Task 5: Real-app check

- [ ] **Step 1:** `npx --yes @tauri-apps/cli@2 build --no-bundle --debug` → `target/debug/dockie.exe` (the user's running release exe is untouched).
- [ ] **Step 2:** run it with `LOCALAPPDATA` pointed at a scratch folder holding a test `accounts.json` (no BOM). Expected: rounded corners, drag by the top bar, double-click maximizes and squares corners, Win+Up / snap do the same, min/max/close work, edges resize, scrollbar styled, game search returns real Roblox games with icons, Follow row shows for single words.
- [ ] **Step 3:** `cargo test` and `node ui/state.test.js` green; then the final whole-branch review.
