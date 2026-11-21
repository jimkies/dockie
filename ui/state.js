// Pure helpers: what a tile shows, and which warning the top bar shows. Tested by ui/state.test.js.

function tileState(acc, launch, running) {
  if (running) return { pill: "Running", tone: "ok", action: "stop", error: null };
  if (acc.status === "needs_sign_in") return { pill: "Needs sign-in", tone: "bad", action: "signin", error: null };
  if (launch?.state === "queued") return { pill: "Queued", tone: "busy", action: "stop", error: null };
  if (launch?.state === "launching") return { pill: "Launching", tone: "busy", action: "stop", error: null };
  if (launch?.state === "error") return { pill: "Error", tone: "bad", action: "launch", error: launch.message ?? null };
  return { pill: "Ready", tone: "idle", action: "launch", error: null };
}

function warnText(snap) {
  if (snap.lock_ok || !snap.roblox_running) return null;
  return snap.accounts.some((a) => a.running)
    ? "Dockie was reopened while its Roblox windows were still open, so it can't open more until they close."
    : "Roblox is already running — extra windows can't open.";
}

if (typeof module !== "undefined") module.exports = { tileState, warnText };
