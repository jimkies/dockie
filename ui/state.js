// Pure helpers: what a tile shows, and which warning the top bar shows. Tested by ui/state.test.js.

function tileState(acc, launch, running) {
  if (running) return { pill: "Running", tone: "ok", action: "stop", error: null };
  if (acc.status === "needs_sign_in") return { pill: "Needs sign-in", tone: "bad", action: "signin", error: null };
  if (launch?.state === "queued") return { pill: "Queued", tone: "busy", action: "stop", error: null };
  if (launch?.state === "launching") return { pill: "Launching", tone: "busy", action: "stop", error: null };
  if (launch?.state === "error") return { pill: "Error", tone: "bad", action: "launch", error: launch.message ?? null };
  return { pill: "Ready", tone: "idle", action: "launch", error: null };
}

// Dockie takes the lock over from Roblox and ordinary programs, so this only shows when a program
// running as administrator holds it.
function warnText(snap) {
  if (snap.lock_ok) return null;
  return "A program running as administrator is holding Roblox's multi-instance lock, so extra windows can't open. Close that program, then try again.";
}

// Words in a Join box search Roblox games; links, place IDs, servers and @user don't.
// A word that could be a username also gets "Follow @word" (parse_join treats it as one).
function searchPlan(text) {
  const t = text.trim();
  if (t.length < 2 || /^\d+(:\S*)?$/.test(t) || t.startsWith("@") || t.includes("/") || t.toLowerCase().includes("roblox.com")) return null;
  return { query: t, follow: /^[A-Za-z0-9_]{3,20}$/.test(t) ? t : null };
}

// Windows tab: memory and time running, short and readable.
function fmtBytes(n) {
  const mb = n / 2 ** 20;
  return mb < 1000 ? `${Math.round(mb)} MB` : `${(mb / 1024).toFixed(1)} GB`;
}
function fmtUptime(s) {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

// Executor attach status for a tile's meta line (from the localhost server); null before the
// script in that window ever said hello, so accounts without an executor show nothing extra.
function attachView(s) {
  if (s === "attached") return { text: "Attached", tone: "ok", title: "The executor's script is running in this window" };
  if (s === "stale") return { text: "Stale", tone: "warn", title: "No heartbeat from the executor's script for over 30 s; the game may be frozen or loading" };
  if (s === "disconnected") return { text: "Disconnected", tone: "off", title: "The executor's script lost its connection, usually because the window closed" };
  return null;
}

// Which way a tab switch moves: 1 = to a tab on the right, -1 = left.
const TABS = ["accounts", "windows", "settings"];
const tabStep = (from, to) => Math.sign(TABS.indexOf(to) - TABS.indexOf(from));

if (typeof module !== "undefined") module.exports = { tileState, warnText, searchPlan, fmtBytes, fmtUptime, tabStep, attachView };
