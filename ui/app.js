const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// window.confirm() silently returns true in Tauri's webview, so use the native dialog.
const ask = (msg) => window.__TAURI__.dialog.ask(msg, { title: "Dockie", kind: "warning" });
const $ = (id) => document.getElementById(id);

let snap = null;
let firstRender = true;
const selected = new Set();
const launch = new Map();   // user_id -> { state, message }
const previews = new Map(); // user_id -> { minimized, image }
const running = new Set();
const cells = new Map();    // user_id -> paint() for that tile

function el(tag, props = {}, ...kids) {
  const e = document.createElement(tag);
  Object.assign(e, props); // props are textContent/className/etc, never HTML
  e.append(...kids);
  return e;
}
function svg(d) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.setAttribute("aria-hidden", "true");
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", d);
  s.append(p);
  return s;
}
function toast(message) {
  const t = el("div", { className: "toast", textContent: message });
  $("toasts").append(t);
  setTimeout(() => t.remove(), 5000);
}
const fail = (e) => toast(String(e));
const call = (cmd, args) => invoke(cmd, args).catch(fail);

// ---- menus: a trigger with aria-haspopup opens the .menu next to it ----
function closeMenus(except) {
  for (const m of document.querySelectorAll(".menu.open")) {
    if (m === except) continue;
    m.classList.remove("open");
    m.previousElementSibling?.setAttribute("aria-expanded", "false");
  }
}
document.addEventListener("click", (e) => {
  const trigger = e.target.closest("[aria-haspopup=menu]");
  if (trigger) {
    const menu = trigger.nextElementSibling;
    const open = !menu.classList.contains("open");
    closeMenus(menu);
    menu.classList.toggle("open", open);
    trigger.setAttribute("aria-expanded", String(open));
    return;
  }
  closeMenus(); // any other click, including a menu item, closes menus
});
document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenus(); });

// ---- shell ----
function showTab(name) {
  for (const b of document.querySelectorAll("[role=tab]")) b.setAttribute("aria-selected", b.dataset.tab === name);
  for (const t of ["accounts", "windows", "settings"]) $("tab-" + t).hidden = t !== name;
  clearInterval(showTab.timer);
  if (name === "windows") { renderWindows(); showTab.timer = setInterval(renderWindows, 2000); }
}
function renderBars() {
  const w = warnText(snap);
  $("warn").hidden = !w;
  $("warn-text").textContent = w ?? "";
  $("loaderr").hidden = !snap.load_error;
  $("loaderr").textContent = snap.load_error ? `${snap.load_error}. Dockie will not save over it.` : "";
}
let stale = false;
// Re-rendering would wipe an in-progress settings edit, so wait until focus leaves it.
const editing = () => document.activeElement?.matches("#tab-settings input:not([type=checkbox])");
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
function renderAccounts() {
  const has = snap.accounts.length > 0;
  $("empty").hidden = has;
  $("grid").hidden = !has;
  for (const b of $("tab-accounts").querySelectorAll(".toolbar")) b.hidden = !has;
  cells.clear();
  const tiles = snap.accounts.map((a, i) => renderTile(a, i));
  for (const id of [...selected]) if (!cells.has(id)) selected.delete(id);
  $("grid").replaceChildren(...tiles, addTile(tiles.length));
  firstRender = !has; // stagger only the first time tiles appear
  updateToolbar();
}

function avatar(a) {
  if (a.avatar_url) return el("img", { className: "av", alt: "", src: a.avatar_url });
  return el("div", { className: "av", textContent: (a.display_name || a.username || "?").slice(0, 1).toUpperCase() });
}

function renderTile(a, i) {
  const id = a.user_id;
  const tile = el("article", { className: "tile" });
  tile.setAttribute("aria-label", a.display_name);
  if (firstRender) { tile.classList.add("enter"); tile.style.setProperty("--i", i); }

  // preview area: one reused <button> so per-second updates only swap the src
  const img = el("img", { alt: "" });
  const ph = el("span", { className: "ph" });
  const thumb = el("button", { className: "thumb" }, img, ph);
  thumb.addEventListener("click", () => running.has(id) && call("focus", { userId: id }));
  const check = el("input", { type: "checkbox", checked: selected.has(id) });
  check.setAttribute("aria-label", "Select " + a.username);
  check.addEventListener("change", () => {
    check.checked ? selected.add(id) : selected.delete(id);
    tile.classList.toggle("sel", check.checked);
    updateToolbar();
  });
  tile.classList.toggle("sel", check.checked);
  const pill = el("span", { className: "pill" });
  const shot = el("div", { className: "shot" }, thumb, el("label", { className: "pick" }, check), pill);

  // body
  const who = el("div", { className: "who" },
    el("div", { className: "name", textContent: a.display_name }),
    el("div", { className: "handle", textContent: "@" + a.username },
      ...(a.note ? [el("span", { className: "note", textContent: " · " + a.note })] : [])));
  const btn = el("button", { className: "btn" });
  btn.addEventListener("click", () => {
    const act = btn.dataset.act;
    if (act === "stop") call("stop", { userId: id }).then(() => unqueue(id));
    else if (act === "signin") signIn(id);
    else call("launch", { userIds: [id], target: null });
  });
  const more = el("button", { className: "btn icon ghost" }, svg("M5 12h.01M12 12h.01M19 12h.01"));
  more.setAttribute("aria-haspopup", "menu");
  more.setAttribute("aria-expanded", "false");
  more.setAttribute("aria-label", "More actions for " + a.username);
  const edit = el("button", { textContent: "Edit…" });
  edit.setAttribute("role", "menuitem");
  edit.addEventListener("click", () => openEdit(a));
  const copy = el("button", { textContent: "Copy username" });
  copy.setAttribute("role", "menuitem");
  copy.addEventListener("click", () => navigator.clipboard.writeText(a.username).then(() => toast(`Copied @${a.username}`), fail));
  const remove = el("button", { className: "danger", textContent: "Remove" });
  remove.setAttribute("role", "menuitem");
  remove.addEventListener("click", async () => {
    if (await ask(`Remove ${a.username}?`)) call("remove_account", { userId: id }).then(() => refresh().catch(fail));
  });
  const menu = el("div", { className: "menu right" }, edit, copy, remove);
  menu.setAttribute("role", "menu");
  const body = el("div", { className: "body" }, avatar(a), who, btn, el("div", { className: "menu-wrap" }, more, menu));

  // meta line: where it joins and its FPS cap
  const game = el("span", { textContent: a.join.trim() ? "…" : "Roblox home" });
  if (a.join.trim()) {
    invoke("describe_join", { userId: id, text: a.join })
      .then((label) => (game.textContent = label))
      .catch((e) => { game.textContent = String(e); game.className = "bad"; });
  }
  const fps = el("span", { className: "num", textContent: a.fps_cap ? `FPS ${a.fps_cap}` : "FPS default" });
  const meta = el("div", { className: "meta" }, game, el("span", { className: "sep", textContent: "·" }), fps);
  const err = el("div", { className: "err" });

  // Re-applies launch/preview/running state to this tile.
  const paint = () => {
    const isRunning = running.has(id);
    const s = tileState(a, launch.get(id), isRunning);
    pill.className = "pill " + s.tone;
    pill.textContent = s.pill;
    err.textContent = s.error ?? "";
    err.hidden = !s.error;
    btn.dataset.act = s.action;
    btn.textContent = s.action === "stop" ? "Stop" : s.action === "signin" ? "Sign in again" : "Launch";
    btn.className = "btn" + (s.action === "stop" ? "" : " primary");
    const p = previews.get(id);
    const image = isRunning && !p?.minimized && p?.image ? p.image : "";
    img.hidden = !image;
    if (image && img.getAttribute("src") !== image) img.src = image;
    ph.hidden = !!image;
    ph.textContent = isRunning ? (p?.minimized ? "Minimized" : "Starting…")
      : s.pill === "Launching" ? "Launching…" : s.pill === "Queued" ? "Queued"
      : a.status === "needs_sign_in" ? "Signed out" : "Not running";
    shot.classList.toggle("busy", s.tone === "busy" || (isRunning && !image && !p?.minimized));
    thumb.classList.toggle("live", isRunning);
    thumb.setAttribute("aria-label", isRunning ? `Focus ${a.username}'s window` : `${a.username} is not running`);
  };
  cells.set(id, paint);
  paint();

  tile.append(shot, body, meta, err);
  return tile;
}

function addTile(i) {
  const main = el("button", { className: "main", textContent: "Sign in another account" });
  main.addEventListener("click", () => signIn(null));
  const paste = el("button", { className: "link", textContent: "or paste a cookie" });
  paste.addEventListener("click", openPaste);
  const plus = el("div", { className: "big-plus" }, svg("M12 5v14M5 12h14"));
  const t = el("div", { className: "tile add" }, plus, main, paste);
  if (firstRender) { t.classList.add("enter"); t.style.setProperty("--i", i); }
  return t;
}

function updateToolbar() {
  const n = selected.size;
  $("launch-sel").disabled = n === 0;
  $("launch-sel").textContent = n ? `Launch ${n}` : "Launch";
  $("launch-sel").title = n ? "" : "Tick accounts to launch them together";
  const busy = [...launch.values()].some((l) => l.state === "queued" || l.state === "launching");
  $("stop-all").hidden = running.size === 0 && !busy;
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
  if (s === "queued" || s === "launching") { launch.delete(id); cells.get(id)?.(); updateToolbar(); }
}

function launchSelected() {
  const userIds = [...selected];
  if (!userIds.length) return;
  call("launch", { userIds, target: $("join-all").value.trim() || null });
}

// ---- edit sheet: holds its own copy of the values until Save ----
let editing_acc = null;
let describeTimer;
function describeEditJoin() {
  const text = $("edit-join").value;
  const label = $("edit-join-label");
  label.className = "help";
  label.textContent = text.trim() ? "Checking…" : "Opens the Roblox home screen.";
  clearTimeout(describeTimer);
  if (!text.trim()) return;
  describeTimer = setTimeout(async () => {
    try { label.textContent = await invoke("describe_join", { userId: editing_acc.user_id, text }); }
    catch (e) { label.textContent = String(e); label.className = "help bad"; }
  }, 300);
}
function openEdit(a) {
  editing_acc = a;
  $("edit-title").textContent = `Edit ${a.display_name}`;
  $("edit-join").value = a.join;
  $("edit-fps").value = a.fps_cap ?? "";
  $("edit-note").value = a.note;
  describeEditJoin();
  $("edit-dlg").returnValue = "";
  $("edit-dlg").showModal();
}
$("edit-join").addEventListener("input", describeEditJoin);
// Save only a Join that Dockie can read; otherwise shake the field and keep the sheet open.
$("edit-save").addEventListener("click", async (e) => {
  const text = $("edit-join").value.trim();
  if (!text) return;
  e.preventDefault();
  try {
    $("edit-join-label").textContent = await invoke("describe_join", { userId: editing_acc.user_id, text });
    $("edit-join-label").className = "help";
    $("edit-dlg").close("save");
  } catch (err) {
    $("edit-join-label").textContent = String(err);
    $("edit-join-label").className = "help bad";
    $("edit-join").animate([{ translate: "0" }, { translate: "-8px" }, { translate: "8px" }, { translate: "-5px" }, { translate: "5px" }, { translate: "-2px" }, { translate: "0" }], { duration: 360, easing: "ease-out" });
    $("edit-join").focus();
  }
});
$("edit-dlg").addEventListener("close", async () => {
  const a = editing_acc;
  if ($("edit-dlg").returnValue !== "save" || !a) return;
  const id = a.user_id;
  const join = $("edit-join").value.trim();
  const fpsRaw = $("edit-fps").value;
  const fps = fpsRaw === "" ? null : Number(fpsRaw);
  const note = $("edit-note").value.trim();
  if (join !== a.join) await call("set_join", { userId: id, join });
  if (fps !== (a.fps_cap ?? null)) await call("set_fps_cap", { userId: id, cap: fps });
  if (note !== a.note) await call("set_note", { userId: id, note });
  refresh().catch(fail);
});

// ---- paste a cookie ----
function openPaste() {
  $("paste-dlg").returnValue = "";
  $("paste-text").value = "";
  $("paste-dlg").showModal();
}
$("paste-dlg").addEventListener("close", async () => {
  const cookie = $("paste-text").value.trim();
  $("paste-text").value = "";
  if ($("paste-dlg").returnValue !== "ok" || !cookie) return;
  try { await invoke("add_cookie", { cookie }); } catch (e) { fail(e); }
  refresh().catch(fail);
});

// ---- windows ----
async function renderWindows() {
  let list;
  try { list = await invoke("roblox_windows"); } catch (e) { return fail(e); }
  const names = new Map((snap?.accounts || []).map((a) => [a.user_id, a.username]));
  const rows = list.map((w) => {
    const focusBtn = el("button", { className: "btn sm", textContent: "Focus" });
    focusBtn.addEventListener("click", () => call("focus_pid", { pid: w.pid }));
    const close = el("button", { className: "btn sm danger", textContent: "Close" });
    close.addEventListener("click", () => call("close_pid", { pid: w.pid }).then(renderWindows));
    const who = w.user_id != null ? "@" + (names.get(w.user_id) ?? w.user_id) : "Not from Dockie";
    return el("div", { className: "win" },
      el("div", {},
        el("div", { className: "t", textContent: w.title || "Roblox" }),
        el("div", { className: "s" }, who + " · ", el("span", { className: "num", textContent: `PID ${w.pid}` }))),
      el("div", { className: "acts" }, focusBtn, close));
  });
  $("windows").replaceChildren(...(rows.length ? rows : [el("div", { className: "none", textContent: "No Roblox windows are open." })]));
}

async function closeAll() {
  // check processes, not windows: a windowless Roblox still holds the multi-instance lock
  snap = (await call("get_snapshot")) ?? snap;
  if (!snap?.roblox_running) return toast("No Roblox windows are open.");
  if (!(await ask("Close all Roblox windows?"))) return;
  await call("close_all_roblox");
  await refresh().catch(fail);
  if (!$("tab-windows").hidden) renderWindows();
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
for (const id of ["signin", "empty-signin"]) $(id).addEventListener("click", () => signIn(null));
for (const id of ["paste", "empty-paste"]) $(id).addEventListener("click", openPaste);
$("launch-sel").addEventListener("click", launchSelected);
$("join-all").addEventListener("keydown", (e) => { if (e.key === "Enter") launchSelected(); });
$("stop-all").addEventListener("click", async () => {
  await call("stop_all");
  for (const id of [...launch.keys()]) unqueue(id);
});
$("import").addEventListener("click", async () => {
  const r = await call("import_accounts");
  if (r && r[0] + r[1] > 0) toast(`Imported ${r[0]} new, ${r[1]} replaced.`);
  refresh().catch(fail);
});
$("export").addEventListener("click", () => call("export_accounts", { userIds: [...selected] }));
$("set-fps").addEventListener("blur", saveSettings);
$("set-trim").addEventListener("change", saveSettings);
$("open-data").addEventListener("click", () => call("open_data_folder"));

listen("accounts-changed", () => refresh().catch(fail));
document.addEventListener("focusout", () => setTimeout(() => stale && !editing() && refresh().catch(fail)));
listen("launch-status", ({ payload: p }) => { launch.set(p.user_id, p); cells.get(p.user_id)?.(); updateToolbar(); });
listen("preview", ({ payload: p }) => { previews.set(p.user_id, p); cells.get(p.user_id)?.(); });
listen("running", ({ payload: p }) => {
  running.clear();
  for (const id of p.user_ids) running.add(id);
  for (const [id, l] of [...launch]) {
    if (l.state === "running" && !running.has(id)) launch.delete(id);
    if (l.state === "error" && running.has(id)) launch.delete(id); // its window showed up after all
  }
  for (const paint of cells.values()) paint();
  updateToolbar();
});
listen("notice", ({ payload: p }) => toast(p.message));
listen("confirm-quit", async () => {
  if (await ask("Roblox windows stay open, but no new extra windows can open until you reopen Dockie. Quit?")) invoke("quit");
});

window.__TAURI__.app?.getVersion?.().then((v) => ($("version").textContent = "Dockie " + v)).catch(() => {});
refresh().catch(fail);
