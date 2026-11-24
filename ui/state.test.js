// Run: node ui/state.test.js
const assert = require("assert");
const { tileState, warnText, attachView } = require("./state.js");

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

assert.equal(attachView(undefined), null); // never said hello: no executor, nothing to show
assert.deepEqual([attachView("attached").text, attachView("stale").tone, attachView("disconnected").tone], ["Attached", "warn", "off"]);

assert.equal(warnText({ lock_ok: true, roblox_running: true, accounts: [] }), null);
const adminText = "A program running as administrator is holding Roblox's multi-instance lock, so extra windows can't open. Close that program, then try again.";
assert.equal(warnText({ lock_ok: false, roblox_running: true, accounts: [{ running: true }] }), adminText);
assert.equal(warnText({ lock_ok: false, roblox_running: false, accounts: [] }), adminText);
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

// Windows tab numbers and tab direction
const { fmtBytes, fmtUptime, tabStep } = require("./state.js");
assert.equal(fmtBytes(812 * 2 ** 20), "812 MB");
assert.equal(fmtBytes(1.25 * 2 ** 30), "1.3 GB");
assert.equal(fmtUptime(45), "45s");
assert.equal(fmtUptime(12 * 60 + 5), "12m");
assert.equal(fmtUptime(2 * 3600 + 14 * 60), "2h 14m");
assert.equal(tabStep("accounts", "settings"), 1);
assert.equal(tabStep("settings", "windows"), -1);
assert.equal(tabStep("windows", "windows"), 0);
console.log("format ok");
