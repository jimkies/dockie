# Dockie UI Redesign (Tiles) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Dockie's row-based screen with the approved tiles layout, applying the `polishing-ui` rules, without changing any backend command or event.

**Architecture:**
- Plain HTML/CSS/JS in `ui/` as now, with no build step.
- The status and warning decisions move into a tiny `ui/state.js`: pure functions, unit-tested with Node.
- `ui/app.js` keeps its existing wiring patterns:
  - the `call`/`ask`/`toast` helpers
  - the deferred `refresh` while typing
  - the `cells` paint map that reuses preview elements
  - the existing event listeners

  It swaps row rendering for tiles plus an Edit sheet.

**Tech Stack:** vanilla JS, CSS (OKLCH, `interpolate-size`), Tauri 2 webview, Node 26 for the state test.

**Spec:** `docs/superpowers/specs/2026-11-21-dockie-ui-redesign-design.md`

## Global Constraints
- **Files:** only `ui/index.html`, `ui/style.css`, `ui/app.js` and a new `ui/state.js`, `ui/state.test.js`, plus `app.windows[0].minWidth: 760, minHeight: 520` in `src-tauri/tauri.conf.json`.
- **Backend calls:** use exactly the commands, args and events already used in `ui/app.js` (camelCase args), with no new commands.
- **Safety:** `textContent` only for data, and no inline scripts or `on*=` attributes (CSP). Confirmations use `ask(...)` (dialog plugin), never `window.confirm`.
- **Rules:** every item in `~/.claude/skills/polishing-ui/SKILL.md` that the UI touches; exact CSS is in its `reference.md`.
- **Exact copy:**
  - Pills: `Running` · `Launching` · `Queued` · `Ready` · `Needs sign-in` · `Error`.
  - Primary button: `Launch` · `Stop` · `Sign in again`.
  - Toolbar button: `Launch N` (N = ticked count).
  - Tabs: `Accounts` · `Windows` · `Settings`.
  - Own-windows warning: `Dockie was reopened while its Roblox windows were still open, so it can't open more until they close.`
  - Other warning: `Roblox is already running — extra windows can't open.`
- **Commits:** every commit ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus
1. **Typing in the Edit sheet while a refresh lands** must not lose input. The sheet holds its own values until Save (Task 3 manual check).
2. **An account with an error AND running** (the window appeared after a timeout) shows Running, not Error (Task 1 test).
3. **No accounts** shows the welcome state, not an empty grid (Task 2 check).
4. **The tracked-window warning** shows when `!lock_ok && roblox_running` and any account is running; the generic text shows otherwise (Task 1 test).
5. **Many accounts (10+)** wrap into the grid without horizontal scroll at 760 px width (Task 4 live check).

---

### Task 1: Pure state helpers

**Files:** Create `ui/state.js`, `ui/state.test.js`.

**Interfaces:**
- Produces, as globals and via `module.exports` when present:
  - `tileState(acc, launch, running) -> { pill: string, tone: "ok"|"busy"|"bad"|"idle", action: "launch"|"stop"|"signin"|"none", error: string|null }`
    - `acc` is an AccountView, `launch` is `{state, message}|undefined`, `running` is a bool.
  - `warnText(snap) -> string|null`

- [ ] **Step 1: Write the failing test** in `ui/state.test.js` (Node `assert`, loads `./state.js` with `require`):
```js
assert.deepEqual(tileState({status:"ok"}, undefined, false), {pill:"Ready", tone:"idle", action:"launch", error:null});
assert.deepEqual(tileState({status:"ok"}, undefined, true), {pill:"Running", tone:"ok", action:"stop", error:null});
assert.deepEqual(tileState({status:"ok"}, {state:"queued"}, false), {pill:"Queued", tone:"busy", action:"stop", error:null});
assert.deepEqual(tileState({status:"ok"}, {state:"launching"}, false), {pill:"Launching", tone:"busy", action:"stop", error:null});
assert.deepEqual(tileState({status:"needs_sign_in"}, undefined, false), {pill:"Needs sign-in", tone:"bad", action:"signin", error:null});
assert.deepEqual(tileState({status:"ok"}, {state:"error", message:"Window didn't appear within 60 s"}, false), {pill:"Error", tone:"bad", action:"launch", error:"Window didn't appear within 60 s"});
assert.deepEqual(tileState({status:"ok"}, {state:"error", message:"x"}, true), {pill:"Running", tone:"ok", action:"stop", error:null}); // Review Focus 2
assert.equal(warnText({lock_ok:true, roblox_running:true, accounts:[]}), null);
assert.equal(warnText({lock_ok:false, roblox_running:true, accounts:[{running:true}]}), "Dockie was reopened while its Roblox windows were still open, so it can't open more until they close.");
assert.equal(warnText({lock_ok:false, roblox_running:true, accounts:[{running:false}]}), "Roblox is already running — extra windows can't open.");
assert.equal(warnText({lock_ok:false, roblox_running:false, accounts:[]}), null);
console.log("state ok");
```
- [ ] **Step 2:** Run `node ui/state.test.js`. Expected: FAIL (`tileState` not defined).
- [ ] **Step 3: Implement** both functions in `ui/state.js`, written as a plain script with no ES module syntax. End it with `if (typeof module !== "undefined") module.exports = { tileState, warnText };`.
- [ ] **Step 4:** Run `node ui/state.test.js`. Expected: `state ok`.
- [ ] **Step 5: Commit** `feat(ui): tile state and warning helpers`.

### Task 2: Shell, tokens and tiles

**Files:**
- Rewrite `ui/index.html` and `ui/style.css`.
- Modify `ui/app.js`: replace `renderAccounts`/`renderRow`, the toolbar wiring and `renderBars`.
- Modify `src-tauri/tauri.conf.json` (min size).

**Interfaces:**
- Consumes `tileState`, `warnText` (Task 1).
- `index.html` loads `state.js` before `app.js`.
- Produces `renderTile(acc) -> HTMLElement`, which registers `cells.set(id, paint)`, and `openEdit(acc)`, a stub in this task that is filled in by Task 3.

- [ ] **Step 1: Build the markup.**
  - **Header:** the app mark plus a segmented tablist (`Accounts`, `Windows`, `Settings`), driven by the existing `showTab`.
  - **Toolbar:** the Join-selected field, `Launch N` (primary, disabled at 0), `Stop all` (hidden unless any tile is running, queued or launching), `+ Add account` (a menu with Sign in and Paste cookie), and a `⋯` menu with Import and Export.
  - **Grid:** `#grid`, the add tile, and the `#empty` welcome with Sign in (primary) and Paste cookie.
  - **Warning bar:** the `#warn` text comes from `warnText(snap)`.
- [ ] **Step 2: Write the CSS.**
  - OKLCH tokens, with surfaces lighter per layer: page < tile < menu/sheet/toast.
  - Alpha borders and 3-layer shadows.
  - Tile radius 16 px with padding 6 px, so the preview radius is 10 px.
  - `grid-template-columns: repeat(auto-fill, minmax(260px, 1fr))`.
  - Buttons press with `scale(.97)` over 150 ms ease-out.
  - Menus enter from `scale(.97)` + opacity with `transform-origin` at the trigger, ≤ 200 ms.
  - Tiles stagger in on first render only (`animation-delay: calc(var(--i) * 40ms)`, 250 ms).
  - The launching shimmer is a `transform`-animated gradient.
  - `tabular-nums` on FPS and PIDs, `text-wrap: balance` on headings, 1.5 px icon strokes, and a `prefers-reduced-motion` block.
- [ ] **Step 3: Write `renderTile`.**
  - Keep the preview `<button class="thumb">` reuse and its `src`-only updates from the current code.
  - Place the select checkbox top-left and the pill top-right over the preview.
  - Add the body (avatar, names, note) and the primary button, mapped from `tileState`: launch / stop (+`unqueue`) / `signIn(id)`.
  - Add the meta line (join label from `describe_join`, the FPS text and the error) and the `⋯` menu (Edit…, Copy username, Remove via `ask`).
- [ ] **Step 4: Set the window size.** Set `minWidth: 760, minHeight: 520` in `tauri.conf.json`.
- [ ] **Step 5: Verify.** `node --check ui/app.js` and `node ui/state.test.js` pass, and `cargo build` in `src-tauri` succeeds.
- [ ] **Step 6: Commit** `feat(ui): tiles layout`.

### Task 3: Edit sheet

**Files:** Modify `ui/index.html` (add `<dialog id="edit">`), `ui/style.css`, `ui/app.js` (`openEdit`).

**Interfaces:** Consumes the `openEdit(acc)` call sites from Task 2.

- [ ] **Step 1: Build the sheet.** It contains Join (text, with a live label under it from `describe_join` on input after 300 ms of idle, red on reject), FPS cap (number, min 1, empty = default) and Note, with Cancel and Save buttons.
- [ ] **Step 2: Save.**
  - Save calls `set_join`, `set_fps_cap` and `set_note` only for fields that changed, then `refresh()`.
  - Cancel closes without calls.
  - The sheet copies values when it opens and is not touched by refresh (Review Focus 1).
- [ ] **Step 3: Style the sheet.** It enters from `scale(.97)` + opacity at 200 ms with a strong ease-out, and uses the lighter sheet surface.
- [ ] **Step 4: Verify.** `node --check ui/app.js`.
- [ ] **Step 5: Commit** `feat(ui): edit sheet for join, FPS and note`.

### Task 4: Windows and Settings tabs, live check

**Files:** Modify `ui/index.html`, `ui/style.css`, `ui/app.js` (`renderWindows`, `renderSettings`).

- [ ] **Step 1: Windows tab.** Rows show the window title, the account's @username (or "Not from Dockie") and the PID in tabular numbers, with Focus and Close buttons and a `Close all Roblox` header button. Keep the existing 2 s refresh only while the tab is visible.
- [ ] **Step 2: Settings tab.** Use grouped cards: the default FPS cap, a Free RAM switch (`role="switch"` on the checkbox, styled as a toggle), Open data folder, and the version.
- [ ] **Step 3: Live check.**
  - Build with `npx --yes @tauri-apps/cli@2 build --no-bundle` and run `dockie.exe`.
  - Screenshot every tab, the Edit sheet, a menu, and the empty state (use a temporary `LOCALAPPDATA`).
  - At 760 px width, check that there's no horizontal scroll with 10+ accounts (Review Focus 5). Inject them into the mocked `__TAURI__` page if no real accounts are available.
- [ ] **Step 4: Checklist.** Re-read the `polishing-ui` checklist against the code and fix any misses.
- [ ] **Step 5: Commit** `feat(ui): windows and settings tabs`.
