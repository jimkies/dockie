# Dockie Tabs, Buttons, Windows and Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Animate tab switching, give buttons life, and fill the Windows and Settings tabs with useful content.

**Architecture:** Rust extends `RobloxWindow` with memory, time running and minimized state. Pure formatting/direction helpers live in `ui/state.js` (Node-tested). DOM behaviour is checked in a throwaway browser harness with a fake backend; window data is checked in a real debug build with an isolated data folder.

**Tech Stack:** Tauri 2.12, Rust (`windows` 0.62.2), vanilla JS/CSS in WebView2.

**Spec:** `docs/superpowers/specs/2026-11-21-dockie-tabs-and-buttons-design.md`

## Global Constraints
- Follow `polishing-ui` (read `reference.md` before CSS/JS): OKLCH, alpha borders, layered shadows, concentric radii, motion ≤ 300 ms, strong ease-out, transform/opacity (the tab pill's `width` is the one ruled exception), press `scale(.96–.97)`, blur crossfade for label swaps, directional slides, honour `prefers-reduced-motion` (CSS and WAAPI).
- No new dependencies. No `innerHTML`. Data reaches the page via `textContent`/properties/attributes.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus
1. Windows tab polling (every 2 s) must keep hover, keyboard focus and preview images steady — no rebuild flicker — and must add/remove cards as windows open/close.
2. A window whose memory can't be read (access denied, process just exited) still shows its card, with memory left out and the total counting only readable ones.
3. Rapid tab clicks (click Windows then Settings within 100 ms) end with the right tab shown, the pill under it, and no half-faded page; reduced motion shows tabs instantly.
4. The label crossfade must not run on every per-second repaint — only when the label actually changes — and the button stays clickable during it.
5. "Copied ✓" keeps the menu open briefly, then closes it; clicking elsewhere meanwhile still closes it.

---

### Task 1: Window info (Rust) and formatting helpers (JS)

**Files:** Modify `src-tauri/src/procs.rs`, `src-tauri/src/launcher.rs`, `ui/state.js`, `ui/state.test.js`

**Interfaces:**
- Rust: `pub fn working_set(pid: u32) -> Option<u64>` (bytes; `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `K32GetProcessMemoryInfo`); `pub fn uptime_secs(created: u64, now: u64) -> u64` (FILETIME units, saturating); `pub fn filetime_now() -> u64`. `RobloxWindow` gains `mem_bytes: Option<u64>`, `uptime_s: u64`, `minimized: bool`.
- JS: `fmtBytes(n: number) -> string` ("812 MB", "1.2 GB"); `fmtUptime(s: number) -> string` ("45s", "12m", "2h 14m"); `tabStep(from: string, to: string) -> -1 | 0 | 1` over `["accounts", "windows", "settings"]`.

- [ ] **Step 1: failing tests.** Rust: `uptime_secs(c, c + 90 * 10_000_000) == 90`; `uptime_secs(c + 5, c) == 0`; `working_set(std::process::id()).unwrap() > 0`. JS: `fmtBytes(812 * 2**20) === "812 MB"`, `fmtBytes(1.25 * 2**30) === "1.3 GB"`, `fmtUptime(45) === "45s"`, `fmtUptime(12 * 60 + 5) === "12m"`, `fmtUptime(2 * 3600 + 14 * 60) === "2h 14m"`, `tabStep("accounts", "settings") === 1`, `tabStep("settings", "windows") === -1`, `tabStep("windows", "windows") === 0`.
- [ ] **Step 2:** run `node ui/state.test.js` and `cargo test` → Expected: FAIL (missing functions).
- [ ] **Step 3:** implement; fill the new fields in `launcher::windows` from `all` (process start time), `procs::working_set`, `procs::is_minimized`.
- [ ] **Step 4:** both suites → Expected: pass.
- [ ] **Step 5:** commit `feat: window memory, uptime and minimized state`.

### Task 2: Tab motion

**Files:** Modify `ui/index.html`, `ui/style.css`, `ui/app.js`

- [ ] **Step 1: failing harness checks:** `pill_follows_tab` (after `showTab("settings")` + 300 ms, pill's left/width match the Settings button within 1 px), `slide_direction` (showing Settings from Accounts starts an animation on `#tab-settings` whose first keyframe has a positive x translate; back to Accounts → negative), `rapid_clicks_settle` (Windows then Settings 50 ms apart → only `#tab-settings` visible, opacity 1 after 400 ms), `reduced_motion_instant` (with reduced motion forced → no animations on the section).
- [ ] **Step 2:** add `<span class="seg-pill">` inside `.seg`; position via `--x`/`--w` from the selected button (`offsetLeft`/`offsetWidth`), transition `translate`/`width` 250 ms ease-out, no transition on first placement; re-place on resize (`ResizeObserver` on `.seg`). Move the selected background/shadow from the button to the pill.
- [ ] **Step 3:** `showTab` cancels running section animations, shows the target and animates it from `translate: ${24 * step}px 0; opacity: 0` over 220 ms ease-out, unless reduced motion or step 0.
- [ ] **Step 4:** harness → pass. Commit `feat(ui): sliding tab pill and directional tab transitions`.

### Task 3: Buttons with life

**Files:** Modify `ui/style.css`, `ui/app.js`

**Interfaces:** Produces `setLabel(btn, iconPath | null, text)` in `app.js`.

- [ ] **Step 1: failing harness checks:** `label_swap_crossfades` (Launch → Stop on a tile starts a blur/opacity animation; a second paint with the same label starts none), `launch_button_icons` (Launch has a play icon, Stop a square, Launching… a spinner), `copied_in_place` (Copy username → item reads "Copied ✓", menu still open at 300 ms, closed by 1.2 s), `hover_lift_rule` and `release_spring_rule` (stylesheet contains the `.btn:hover` lift and the overshoot release curve).
- [ ] **Step 2:** CSS: hover `translate: 0 -1px` with a glow on a `::after` layer animated by opacity; primary sheen (`linear-gradient(in oklch, …)` + inset top highlight); press `scale(.97)` entering in 80 ms ease-out, release over 300 ms `cubic-bezier(.34, 1.56, .64, 1)`; spinner keyframes (stopped under reduced motion).
- [ ] **Step 3:** `setLabel` wraps content in a two-cell stack; when text differs from the current label, old fades/blurs out and new fades/blurs in over 150 ms (skipped on first paint and with reduced motion). Use it for the tile action button, `#launch-sel`, and Stop all. Copy username: `stopPropagation`, swap to "Copied ✓", close the menu after 900 ms.
- [ ] **Step 4:** harness → pass. Commit `feat(ui): livelier buttons with icons, springy press and label crossfade`.

### Task 4: Windows tab and Settings tab

**Files:** Modify `ui/index.html`, `ui/style.css`, `ui/app.js`

- [ ] **Step 1: failing harness checks:** `stats_show_totals` (2 windows of 1 GB and unknown memory → "2", "1.0 GB", minimized count), `cards_keyed_by_pid` (a hovered/focused card element survives the next poll; a closed pid's card disappears; a new pid's card appears), `unknown_memory_hidden`, `empty_state_button` ("Go to Accounts" switches tabs), `dockie_window_preview` (a Dockie window with a preview image shows it), `settings_cards` (four cards, About shows the version), `settings_centered`.
- [ ] **Step 2:** Windows tab markup: page header, `.stats` (3 cards), `#windows` grid, `#win-empty`. `renderWindows` keeps a `Map(pid → { el, paint })`: create missing cards, repaint existing ones, remove gone ones; Close all hidden when empty.
- [ ] **Step 3:** Settings: page header, `#tab-settings` centred (`max-width: 880px; margin-inline: auto`) with `grid-template-columns: repeat(auto-fit, minmax(320px, 1fr))`; tinted icon badges on card headers; About card with the big mark, name, `#version`, and the account count in the Data card.
- [ ] **Step 4:** harness (all tasks) → pass. Commit `feat(ui): fuller Windows and Settings tabs`.

### Task 5: Real-app check and review
- [ ] **Step 1:** debug build with a temporary single-instance lock name (never committed; delete the exe after), `LOCALAPPDATA` pointed at a scratch folder. Expected: tab pill slides and pages slide by direction; buttons lift/press/swap labels; Windows tab shows the user's running Roblox windows read-only with memory and time running (do not press Focus/Close on the user's windows); Settings centred with four cards.
- [ ] **Step 2:** `cargo test`, `node ui/state.test.js` green; fresh whole-branch review; fix Critical/Important with tests.
