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

// searchPlan: when a Join box should search Roblox games, and whether to offer "Follow @word"
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
console.log("search ok");
// A Roblox link is a link whatever its case (parse_join lowercases the host)
assert.equal(searchPlan("WWW.ROBLOX.COM?placeId=920587237"), null);
console.log("case ok");
