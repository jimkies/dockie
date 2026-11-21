# Dockie: custom window, game search and polish pass

Approved by the user on 2026-11-21. Builds on the tiles redesign (`2026-11-21-dockie-ui-redesign-design.md`).

## Why
The user wants Dockie to feel like a modern app, not a web page in a Windows frame: its own rounded window and title bar, a styled scrollbar, and less typing. Today a Join needs a link, place ID or @user; picking a game by name is the missing piece. Selecting accounts means hitting a small checkbox.

## Goals
1. **Own window frame.** Windows' title bar is gone. Dockie's top bar is the drag area (double-click maximizes) and holds minimize, maximize/restore and close at the right; close turns red on hover. Close keeps the existing "Roblox windows stay open… Quit?" prompt.
2. **Rounded window.** 14px corners and a hairline edge; square, borderless when maximized.
3. **Styled scrollbar.** Slim, rounded, in Dockie's colours, brighter on hover. No arrow buttons.
4. **Game search** in the Edit sheet's Join box and the toolbar's "Join selected into…" box. Typing words shows a dropdown of matching Roblox games (icon, name, players online). Click, or arrows + Enter, fills the game's link (`roblox.com/games/<placeId>/<slug>`). If the word could be a username, the top row is "Follow @word". Enter with no row highlighted keeps today's behaviour.
5. **Clear button** (×) inside both boxes whenever they hold text.
6. **Click to select.** Clicking anywhere on an account card ticks/unticks it, except its buttons, checkbox and menu. Clicking a running window's preview still brings that window forward.
7. **Polish pass** with the user's saved UI tips: press-scale on every button (tabs, add card, window buttons, clear), reduced motion respected by the error shake, card stagger capped, menus open upward near the bottom, arrow keys in menus, plus the deferred minors from the last review.

## Non-goals
Snap-layout flyout on the maximize button, light theme, drag to reorder, game search anywhere else.

## Approaches (window corners)
- **Native corners (not picked):** keep Windows' frame logic, 8px corners and drop shadow. Barely rounder than today.
- **See-through window with CSS corners (picked):** `decorations: false, transparent: true, shadow: false`, the page draws a 14px rounded body. Costs Windows' drop shadow and the snap-layout flyout; the user accepted that.

## Decided
- Search goes through the Rust backend (CSP blocks page fetches): Roblox omni-search (`apis.roblox.com/search-api/omni-search`, needs any UUID-shaped `sessionId`, no sign-in) plus one batched icon call (`thumbnails.roblox.com/v1/games/icons`). Icons load from `tr.rbxcdn.com`, already allowed by `img-src https://*.rbxcdn.com`.
- A bare 3–20 character word still means "follow that user" (existing `parse_join` rule), so saved Joins keep working. The Follow row makes that visible.
- Picking fills a link rather than a bare place ID so the box stays readable.

## Left to the builder
Exact sizes, icons, debounce timing, and empty/error rows of the dropdown, within the polishing-ui rules.
