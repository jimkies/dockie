# Dockie UI redesign: tiles

Date: 2026-11-21 · Status: approved · Size: small change (front end only)

## Intent
The user finds Dockie's current screen "terrible" and wants it redesigned before sharing Dockie with friends. It should apply the rules in the global `polishing-ui` skill.

Problems with the current screen:
- Every row shows Join/FPS/Note inputs all the time.
- Text is small (13–14 px) and everything has the same visual weight.
- There are two toolbars plus header buttons.
- Previews are tiny (120 px).
- Tabs look like plain buttons.
- There's no clear main action.

## Approaches (picked: A)
| | Layout | Trade-off |
|---|---|---|
| **A · Tiles (picked)** | A card per account, led by a big live preview, with one Launch/Stop button. Settings live behind ⋯ → Edit. | Calm, and shows every window at once. Fewer accounts fit on screen than in a list. |
| B · Compact list | A table row per account | Densest, but previews are tiny |
| C · Sidebar + detail | Accounts on the left, the selected one large on the right | Spacious, but one preview at a time |

Mockups were shown to the user, who picked A.

## Goals
- **Header:** the app mark plus a segmented control: **Accounts · Windows · Settings** ("Tools" is renamed to "Windows").
- **Accounts toolbar:**
  - The "Join selected into…" field.
  - A primary **Launch N** button for the ticked tiles. It's disabled when nothing is ticked.
  - **Stop all**, shown only while something is running or queued.
  - **+ Add account**, a menu with Sign in and Paste cookie.
  - A **⋯** overflow menu with Import and Export.
- **Tile** (responsive grid: `repeat(auto-fill, minmax(260px, 1fr))`):
  - **Preview area (16:9):** shows the live image, or a calm placeholder for each state: Not running · Launching… (shimmer) · Queued · Minimized. Clicking it focuses the window.
  - **On top of the preview:** a select checkbox (top-left) and a status pill (top-right): Running · Launching · Queued · Ready · Needs sign-in · Error.
  - **Body:** avatar, display name, @username and the note, if there is one. The primary button sits on the right: Launch / Stop / **Sign in again**.
  - **Meta line:** the Join label (game name, "Follow @x", "Home"), the FPS ("FPS 60" or "FPS default"), and an error message in red if there is one.
  - **⋯ menu:** Edit…, Copy username, Remove.
- **Edit sheet** (dialog): Join, with its live label or a red error, FPS cap, and Note. It saves on Save; Cancel discards.
- **Add tile:** the last tile, "+ Sign in another account", with a small "or paste a cookie" link.
- **Empty state:** a centred welcome with **Sign in** (primary) and Paste cookie.
- **Warning bar:** if the lock isn't held and Roblox is running:
  - If the running windows are Dockie's own tracked accounts: "Dockie was reopened while its Roblox windows were still open, so it can't open more until they close."
  - Otherwise, the existing text.
  - Both keep the **Close all Roblox** button.
- **Windows tab:** a list of running windows (title, account, PID) with Focus and Close, and Close all Roblox.
- **Settings tab:** grouped cards for the default FPS cap, a Free RAM switch, Open data folder, and the version.
- **Dark theme only,** as now. Window minimum size: 760×520.

## Polish rules that apply
These follow the `polishing-ui` checklist:
- OKLCH tokens and alpha borders.
- 3-layer shadows and concentric radii on tiles and previews.
- Lighter surfaces for menus, sheets and toasts in dark mode.
- `tabular-nums` on FPS and PIDs; `text-wrap: balance` on headings.
- Press `scale(.97)` on buttons, and menus and sheets entering from `scale(.97)` with `transform-origin` at their trigger.
- Durations ≤ 300 ms with a strong ease-out, a blur crossfade on "Copy username → Copied", and tiles that stagger in on first render.
- `prefers-reduced-motion` respected, and 1.5 px icon strokes.

## Non-goals
- No backend or command changes: every `invoke` and event stays as it is.
- No light theme, and no drag-to-reorder.

## Constraints
- **Keep existing behaviour:**
  - The deferred refresh while typing.
  - Preview elements reused across ticks.
  - Native `ask` dialogs, since `window.confirm` silently says yes in the webview.
- **Security:** `textContent` only, and nothing that the CSP would block (no inline scripts or handlers).
- **Files:** only `ui/index.html`, `ui/style.css` and `ui/app.js` change, plus `minWidth`/`minHeight` in `tauri.conf.json`.

## Left to the builder
Exact spacing, the icon set (inline SVG), and the shimmer and stagger timing within the rules above.
