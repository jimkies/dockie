const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// window.confirm() silently returns true in Tauri's webview, so use the native dialog.
const ask = (msg) => window.__TAURI__.dialog.ask(msg, { title: "Dockie", kind: "warning" });
const $ = (id) => document.getElementById(id);

let snap = null;
const selected = new Set();
const launch = new Map();   // user_id -> { state, message }
const previews = new Map(); // user_id -> { minimized, image }
const running = new Set();
const cells = new Map();    // user_id -> paint() for that row

function el(tag, props = {}, ...kids) {
  const e = document.createElement(tag);
  Object.assign(e, props); // props are textContent/className/etc, never HTML
  e.append(...kids);
  return e;
}
function toast(message) {
  const t = el("div", { className: "toast", textContent: message });
  $("toasts").append(t);
  setTimeout(() => t.remove(), 5000);
}
const fail = (e) => toast(String(e));
const call = (cmd, args) => invoke(cmd, args).catch(fail);

// ---- shell ----
function showTab(name) {
  for (const b of document.querySelectorAll("[role=tab]")) b.setAttribute("aria-selected", b.dataset.tab === name);
  for (const t of ["accounts", "tools", "settings"]) $("tab-" + t).hidden = t !== name;
  clearInterval(showTab.timer);
  if (name === "tools") { renderWindows(); showTab.timer = setInterval(renderWindows, 2000); }
}
function renderBars() {
  $("warn").hidden = !(!snap.lock_ok && snap.roblox_running);
  $("loaderr").hidden = !snap.load_error;
  $("loaderr").textContent = snap.load_error ? `${snap.load_error}. Dockie will not save over it.` : "";
}
let stale = false;
// Re-rendering would wipe an in-progress edit, so wait until focus leaves rows/settings.
const editing = () => document.activeElement?.matches("#rows input:not([type=checkbox]), #tab-settings input:not([type=checkbox])");
async function refresh() {
  snap = await invoke("get_snapshot");
  if (editing()) { stale = true; return; }
  stale = false;
  running.clear();
  for (const a of snap.accounts) if (a.running) running.add(a.user_id);
  renderBars();
  renderAccounts();
  renderSettings();
}

// ---- accounts ----
// FPS control lives here only: swap for a dropdown later. Calls onSave(number | null).
function fpsInput(value, onSave) {
  const i = el("input", { type: "number", min: 1, className: "fps", placeholder: "default", value: value ?? "" });
  i.setAttribute("aria-label", "FPS cap");
  i.addEventListener("blur", () => onSave(i.value === "" ? null : Number(i.value)));
  return i;
}
function textInput(cls, label, placeholder, value, onSave) {
  const i = el("input", { type: "text", className: cls, placeholder, value });
  i.setAttribute("aria-label", label);
  i.addEventListener("blur", () => onSave(i.value));
  return i;
}

function renderAccounts() {
  $("empty").hidden = snap.accounts.length > 0;
  cells.clear();
  $("rows").replaceChildren(...snap.accounts.map(renderRow));
  for (const id of [...selected]) if (!cells.has(id)) selected.delete(id);
}

function renderRow(a) {
  const id = a.user_id;
  const needsSignIn = a.status === "needs_sign_in";

  const check = el("input", { type: "checkbox", checked: selected.has(id) });
  check.setAttribute("aria-label", "Select " + a.username);
  check.addEventListener("change", () => (check.checked ? selected.add(id) : selected.delete(id)));

  const av = el("img", { className: "av", alt: "" });
  if (a.avatar_url) av.src = a.avatar_url;

  const badge = el("span", { className: "badge" });
  const errText = el("span", { className: "err-text" });
  const img = el("img", { className: "preview", alt: "" });
  const label = el("span", { className: "minimized", textContent: "minimized" });
  const thumb = el("button", { className: "thumb" }, img, label);
  thumb.setAttribute("aria-label", `Focus ${a.username}'s window`);
  thumb.addEventListener("click", () => call("focus", { userId: id }));
  const btn = el("button");
  btn.addEventListener("click", () => (btn.dataset.act === "stop" ? call("stop", { userId: id }).then(() => unqueue(id)) : call("launch", { userIds: [id], target: null })));
  const signAgain = el("button", { textContent: "Sign in again", hidden: !needsSignIn });
  signAgain.addEventListener("click", () => signIn(id));

  const joinLabel = el("div", { className: "join-label" });
  const describe = async (text) => {
    joinLabel.className = "join-label";
    joinLabel.textContent = "";
    if (!text.trim()) return;
    try { joinLabel.textContent = await invoke("describe_join", { userId: id, text }); }
    catch (e) { joinLabel.textContent = String(e); joinLabel.classList.add("bad"); }
  };
  const join = textInput("join", "Join link, id or @user", "Join: link / id / @user", a.join, (v) => {
    call("set_join", { userId: id, join: v });
    describe(v);
  });
  describe(a.join);

  const summary = el("summary", { textContent: "⋯" });
  summary.setAttribute("aria-label", "More actions for " + a.username);
  const menu = el("details", { className: "menu" }, summary);
  const remove = el("button", { textContent: "Remove" });
  remove.addEventListener("click", async () => {
    menu.open = false;
    if (await ask(`Remove ${a.username}?`)) call("remove_account", { userId: id }).then(() => refresh().catch(fail));
  });
  const copy = el("button", { textContent: "Copy username" });
  copy.addEventListener("click", () => { navigator.clipboard.writeText(a.username).catch(fail); menu.open = false; });
  menu.append(el("div", { className: "pop" }, copy, remove));

  // Re-applies launch/preview/running state to this row.
  const paint = () => {
    const l = launch.get(id);
    const busy = l && (l.state === "queued" || l.state === "launching");
    const isRunning = running.has(id);
    let text = "Ready", cls = "badge";
    if (needsSignIn) { text = "Needs sign-in"; cls += " bad"; }
    else if (l?.state === "error") { text = "Error"; cls += " bad"; }
    else if (l?.state === "queued") { text = "Queued"; cls += " busy"; }
    else if (l?.state === "launching") { text = "Launching"; cls += " busy"; }
    else if (isRunning) { text = "Running"; cls += " running"; }
    else if (a.status === "unchecked") text = "Unchecked";
    badge.className = cls;
    badge.textContent = text;
    errText.textContent = l?.state === "error" ? l.message || "" : "";
    btn.dataset.act = busy || isRunning ? "stop" : "launch";
    btn.textContent = btn.dataset.act === "stop" ? "Stop" : "Launch";
    btn.hidden = needsSignIn;
    const p = previews.get(id);
    const mini = isRunning && !!p?.minimized;
    const image = isRunning && !mini && p?.image ? p.image : "";
    thumb.hidden = !mini && !image;
    label.hidden = !mini;
    img.hidden = !image;
    if (image && img.getAttribute("src") !== image) img.src = image;
  };
  cells.set(id, paint);
  paint();

  const fps = el("label", { className: "muted", textContent: "FPS " }, fpsInput(a.fps_cap, (cap) => call("set_fps_cap", { userId: id, cap })));
  const note = textInput("note", "Note", "Note", a.note, (v) => call("set_note", { userId: id, note: v }));

  return el("div", { className: "row" }, check, av,
    el("div", { className: "main" },
      el("div", { className: "line" },
        el("span", { className: "name", textContent: a.display_name }),
        el("span", { className: "muted", textContent: "@" + a.username }),
        badge, errText, signAgain, thumb),
      el("div", { className: "line" }, join, joinLabel),
      el("div", { className: "line" }, fps, note, btn, menu)));
}

async function signIn(refreshUserId) {
  try {
    const r = await invoke("begin_login", { refreshUserId });
    if (r.was_other_account) toast("Signed in as a different account — added it as a new one");
  } catch (e) {
    if (!String(e).includes("cancelled")) fail(e);
  }
  refresh().catch(fail);
}

// Stop / Stop all cancel queued and not-yet-started launches, which report nothing back.
function unqueue(id) {
  const s = launch.get(id)?.state;
  if (s === "queued" || s === "launching") { launch.delete(id); cells.get(id)?.(); }
}

function launchSelected(target) {
  const userIds = [...selected];
  if (!userIds.length) return toast("Tick at least one account first.");
  call("launch", { userIds, target });
}

// ---- tools ----
async function renderWindows() {
  let list;
  try { list = await invoke("roblox_windows"); } catch (e) { return fail(e); }
  const names = new Map((snap?.accounts || []).map((a) => [a.user_id, a.username]));
  const rows = list.map((w) => {
    const focusBtn = el("button", { textContent: "Focus" });
    focusBtn.addEventListener("click", () => call("focus_pid", { pid: w.pid }));
    const close = el("button", { textContent: "Close" });
    close.addEventListener("click", () => call("close_pid", { pid: w.pid }).then(renderWindows));
    const who = w.user_id != null ? "@" + (names.get(w.user_id) ?? w.user_id) : "unknown account";
    return el("div", { className: "win" },
      el("span", { textContent: who }), el("span", { className: "muted", textContent: `PID ${w.pid}` }),
      el("span", { className: "grow", textContent: w.title }), focusBtn, close);
  });
  $("windows").replaceChildren(...(rows.length ? rows : [el("p", { className: "muted", textContent: "No Roblox windows open." })]));
}

async function closeAll() {
  // check processes, not windows: a windowless Roblox still holds the multi-instance lock
  snap = (await call("get_snapshot")) ?? snap;
  if (!snap?.roblox_running) return toast("No Roblox windows are open.");
  if (!(await ask("Close all Roblox windows?"))) return;
  await call("close_all_roblox");
  await refresh().catch(fail);
  if (!$("tab-tools").hidden) renderWindows();
}

// ---- settings ----
function renderSettings() {
  $("set-fps").value = snap.settings.fps_cap_default ?? "";
  $("set-trim").checked = snap.settings.trim_minimized;
}
function saveSettings() {
  const v = $("set-fps").value;
  call("set_settings", { settings: { fps_cap_default: v === "" ? null : Number(v), trim_minimized: $("set-trim").checked } });
}

// ---- wiring ----
for (const b of document.querySelectorAll("[role=tab]")) b.addEventListener("click", () => showTab(b.dataset.tab));
$("warn-close").addEventListener("click", closeAll);
$("close-all").addEventListener("click", closeAll);
$("signin").addEventListener("click", () => signIn(null));
$("join-go").addEventListener("click", () => {
  const t = $("join-all").value.trim();
  t ? launchSelected(t) : toast("Enter a link, id or @user first.");
});
$("launch-sel").addEventListener("click", () => launchSelected(null));
$("stop-all").addEventListener("click", async () => {
  await call("stop_all");
  for (const id of [...launch.keys()]) unqueue(id);
});
$("import").addEventListener("click", async () => {
  const r = await call("import_accounts");
  if (r) toast(`Imported ${r[0]} new, ${r[1]} replaced.`);
  refresh().catch(fail);
});
$("export").addEventListener("click", () => call("export_accounts", { userIds: [...selected] }));
$("paste").addEventListener("click", () => { $("paste-dlg").returnValue = ""; $("paste-text").value = ""; $("paste-dlg").showModal(); });
$("paste-dlg").addEventListener("close", async () => {
  const cookie = $("paste-text").value.trim();
  $("paste-text").value = "";
  if ($("paste-dlg").returnValue !== "ok" || !cookie) return;
  try { await invoke("add_cookie", { cookie }); } catch (e) { fail(e); }
  refresh().catch(fail);
});
$("set-fps").addEventListener("blur", saveSettings);
$("set-trim").addEventListener("change", saveSettings);
$("open-data").addEventListener("click", () => call("open_data_folder"));

listen("accounts-changed", () => refresh().catch(fail));
document.addEventListener("focusout", () => setTimeout(() => stale && !editing() && refresh().catch(fail)));
listen("launch-status", ({ payload: p }) => { launch.set(p.user_id, p); cells.get(p.user_id)?.(); });
listen("preview", ({ payload: p }) => { previews.set(p.user_id, p); cells.get(p.user_id)?.(); });
listen("running", ({ payload: p }) => {
  running.clear();
  for (const id of p.user_ids) running.add(id);
  for (const [id, l] of [...launch]) {
    if (l.state === "running" && !running.has(id)) launch.delete(id);
    if (l.state === "error" && running.has(id)) launch.delete(id); // its window showed up after all
  }
  for (const paint of cells.values()) paint();
});
listen("notice", ({ payload: p }) => toast(p.message));
listen("confirm-quit", async () => {
  if (await ask("Roblox windows stay open, but no new extra windows can open until you reopen Dockie. Quit?")) invoke("quit");
});

window.__TAURI__.app?.getVersion?.().then((v) => ($("version").textContent = "Dockie " + v)).catch(() => {});
refresh().catch(fail);
