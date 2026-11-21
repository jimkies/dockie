// Run: node ui/state.test.js
const assert = require("assert");
const { tileState, warnText } = require("./state.js");

assert.deepEqual(tileState({ status: "ok" }, undefined, false), { pill: "Ready", tone: "idle", action: "launch", error: null });
assert.deepEqual(tileState({ status: "ok" }, undefined, true), { pill: "Running", tone: "ok", action: "stop", error: null });
assert.deepEqual(tileState({ status: "ok" }, { state: "queued" }, false), { pill: "Queued", tone: "busy", action: "stop", error: null });
assert.deepEqual(tileState({ status: "ok" }, { state: "launching" }, false), { pill: "Launching", tone: "busy", action: "stop", error: null });
assert.deepEqual(tileState({ status: "needs_sign_in" }, undefined, false), { pill: "Needs sign-in", tone: "bad", action: "signin", error: null });
assert.deepEqual(
  tileState({ status: "ok" }, { state: "error", message: "Window didn't appear within 60 s" }, false),
  { pill: "Error", tone: "bad", action: "launch", error: "Window didn't appear within 60 s" },
);
// A window that appeared after a timeout is running, not an error.
assert.deepEqual(tileState({ status: "ok" }, { state: "error", message: "x" }, true), { pill: "Running", tone: "ok", action: "stop", error: null });

assert.equal(warnText({ lock_ok: true, roblox_running: true, accounts: [] }), null);
assert.equal(
  warnText({ lock_ok: false, roblox_running: true, accounts: [{ running: true }] }),
  "Dockie was reopened while its Roblox windows were still open, so it can't open more until they close.",
);
assert.equal(warnText({ lock_ok: false, roblox_running: true, accounts: [{ running: false }] }), "Roblox is already running — extra windows can't open.");
assert.equal(warnText({ lock_ok: false, roblox_running: false, accounts: [] }), null);
console.log("state ok");
