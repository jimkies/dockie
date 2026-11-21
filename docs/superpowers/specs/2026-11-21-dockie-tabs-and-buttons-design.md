# Dockie: tab motion, livelier buttons, fuller Windows and Settings tabs

Approved by the user on 2026-11-21. Builds on `2026-11-21-dockie-window-and-search-design.md`.

## Why
The user wants Dockie to feel alive: buttons that respond, tabs that move, and Windows/Settings tabs that don't look empty. Apply the saved UI tips (polishing-ui) wherever they fit.

## Goals
1. **Tab switching.** The selected-tab pill slides to the clicked tab. The page slides in from the side you're moving toward (Settings from the right, Accounts from the left), short and quick; skipped with reduced motion.
2. **Buttons with life.** Hover lifts a button slightly with a soft glow; primary buttons get a light top sheen. Press shrinks with a small springy bounce on release. Buttons carry icons (▶ Launch, ■ Stop, + Add, a spinner while launching). Label changes (Launch → Launching… → Stop) crossfade with blur. "Copy username" turns into "Copied ✓" in place, with the menu staying open a moment so it's seen.
3. **Windows tab.** Three stat cards on top: windows open, total memory, minimized count. A grid of window cards: live preview (Dockie's own windows), account avatar and name (or the window title for others), memory, time running, Focus/Close. With nothing open: an empty screen with an icon, "No Roblox windows open", and a "Go to Accounts" button.
4. **Settings tab.** Centred, cards side by side when wide, each card with a small tinted icon, and a new About card (logo, name, version, one-line description).

## Non-goals
New settings, light theme, charts or history of memory use.

## Approaches
- **Windows tab info:** *list with more text* (rejected: still a thin column in a wide window) vs *stat cards + card grid with previews* (picked: fills the space with things the user cares about — they asked about RAM before).
- **Settings:** *add more settings* (rejected: nothing real to add) vs *centre + side-by-side cards + About card* (picked).
- **Tab pill:** *move it with transform + width* (picked; the pill is absolutely positioned, so its width change reflows nothing else) vs *transform scale* (rejected: warps the pill's rounded corners mid-move).

## Decided
- Backend adds to each listed Roblox window: memory (working set, read with the same limited process access Dockie already uses), time running (from the process start time Dockie already reads), and whether it's minimized.
- The Windows tab updates cards in place by process ID instead of rebuilding every 2 s, so hover, focus and previews don't flicker.

## Left to the builder
Exact icons, sizes, easing values and copy of secondary lines, within the polishing-ui rules.
