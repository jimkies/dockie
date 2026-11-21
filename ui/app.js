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
function svg(d, cls) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.setAttribute("aria-hidden", "true");
  if (cls) s.setAttribute("class", cls);
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", d);
  s.append(p);
  return s;
}
function toast(message) {
  const t = el("div", { className: "toast", textContent: message });
  $("toasts").append(t);
  setTimeout(() => t.animate([{}, { opacity: 0, translate: "0 4px" }], { duration: 150, easing: "ease-out" }).finished.then(() => t.remove()), 5000);
}
const fail = (e) => toast(String(e));

// Button labels with an icon. A changed label blur-crossfades into the new one, so
// Launch -> Cancel -> Stop reads as one button changing, not a jump. Same label: no-op.
const ICONS = { play: "M8 5.5v13l10.5-6.5Z", stop: "M7 7h10v10H7Z" };
function setLabel(btn, icon, text) {
  if (btn.dataset.label === text) return;
  const first = btn.dataset.label === undefined;
  btn.dataset.label = text;
  const glyph = icon === "spin" ? [el("span", { className: "spin" })] : icon ? [svg(ICONS[icon], "i-" + icon)] : [];
  const next = el("span", { className: "lbl" }, ...glyph, text);
  const old = btn.querySelector(":scope > .lbl:not(.out)");
  if (first || !old || reduceMotion()) return btn.replaceChildren(next);
  old.classList.add("out"); // stays in the same grid cell while it fades
  btn.append(next);
  const fade = (from, to) => [{ opacity: from, filter: `blur(${4 - 4 * from}px)` }, { opacity: to, filter: `blur(${4 - 4 * to}px)` }];
  next.animate(fade(0, 1), { duration: 150, easing: "ease-out" });
  old.animate(fade(1, 0), { duration: 150, easing: "ease-out" }).finished.then(() => old.remove(), () => old.remove());
}
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
    if (open) {
      // open upward when there's no room below (bottom row of tiles)
      menu.classList.remove("up");
      const floor = (menu.closest("main") ?? document.body).getBoundingClientRect().bottom;
      menu.classList.toggle("up", menu.getBoundingClientRect().bottom > floor - 8);
    }
    menu.classList.toggle("open", open);
    trigger.setAttribute("aria-expanded", String(open));
    if (open && e.detail === 0) menu.querySelector("[role=menuitem]")?.focus(); // opened from the keyboard
    return;
  }
  closeMenus(); // any other click, including a menu item, closes menus
});
document.addEventListener("keydown", (e) => {
  const menu = document.activeElement?.closest(".menu.open");
  if (e.key === "Escape") {
    menu?.previousElementSibling.focus(); // back to the button that opened it
    return closeMenus();
  }
  if (!menu) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const items = [...menu.querySelectorAll("[role=menuitem]")];
    const i = items.indexOf(document.activeElement) + (e.key === "ArrowDown" ? 1 : -1);
    items[(i + items.length) % items.length].focus();
  } else if (e.key === "Tab") closeMenus();
});
const reduceMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

// ---- window frame: Windows' title bar is off, so these buttons stand in for it ----
const appWindow = window.__TAURI__.window.getCurrentWindow();
async function syncMax() {
  const max = await appWindow.isMaximized();
  document.documentElement.classList.toggle("max", max); // square corners when maximized
  $("win-max").setAttribute("aria-label", max ? "Restore" : "Maximize");
  $("win-max").title = max ? "Restore" : "Maximize";
}
$("win-min").addEventListener("click", () => appWindow.minimize());
$("win-max").addEventListener("click", () => appWindow.toggleMaximize());
$("win-close").addEventListener("click", () => appWindow.close()); // the backend still asks before quitting with Roblox open
appWindow.onResized(() => syncMax().catch(() => {}));
syncMax().catch(() => {});

// ---- shell ----
// The selected tab's pill slides under whichever tab is picked.
function placePill() {
  const b = document.querySelector("[role=tab][aria-selected=true]");
  const pill = document.querySelector(".seg-pill");
  pill.style.setProperty("--x", b.offsetLeft + "px");
  pill.style.setProperty("--w", b.offsetWidth + "px");
}
new ResizeObserver(placePill).observe(document.querySelector(".seg"));
placePill();
// first placement jumps; only later moves slide
requestAnimationFrame(() => requestAnimationFrame(() => document.querySelector(".seg").classList.add("ready")));

let currentTab = "accounts";
function showTab(name) {
  const step = tabStep(currentTab, name); // the page slides in from the side you're heading to
  currentTab = name;
  for (const b of document.querySelectorAll("[role=tab]")) b.setAttribute("aria-selected", b.dataset.tab === name);
  placePill();
  for (const t of TABS) {
    for (const a of $("tab-" + t).getAnimations()) a.cancel(); // a fast second click never leaves a half-faded page
    $("tab-" + t).hidden = t !== name;
  }
  if (step && !reduceMotion()) {
    $("tab-" + name).animate([{ opacity: 0, translate: `${24 * step}px 0` }, { opacity: 1, translate: "0 0" }], { duration: 220, easing: "cubic-bezier(.22, 1, .36, 1)" });
  }
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
  copy.addEventListener("click", (e) => {
    e.stopPropagation(); // the menu stays open a moment so "Copied ✓" is seen in place
    navigator.clipboard.writeText(a.username).then(() => {
      copy.textContent = "Copied ✓";
      if (!reduceMotion()) copy.animate([{ opacity: 0, filter: "blur(3px)" }, { opacity: 1, filter: "blur(0)" }], { duration: 150, easing: "ease-out" });
      setTimeout(closeMenus, 900);
      setTimeout(() => (copy.textContent = "Copy username"), 1100); // after the menu has faded out
    }, fail);
  });
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
    const starting = s.pill === "Launching" || s.pill === "Queued"; // the button cancels; the pill says what's happening
    if (s.action === "stop") setLabel(btn, starting ? "spin" : "stop", starting ? "Cancel" : "Stop");
    else if (s.action === "signin") setLabel(btn, null, "Sign in again");
    else setLabel(btn, "play", "Launch");
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
    thumb.disabled = !isRunning; // nothing to focus, so no tab stop
    thumb.setAttribute("aria-label", isRunning ? `Focus ${a.username}'s window` : `${a.username} is not running`);
  };
  cells.set(id, paint);
  paint();

  tile.append(shot, body, meta, err);
  // Clicking the card ticks it. Its buttons, checkbox and menu keep their own jobs; a running preview focuses its window.
  tile.addEventListener("click", (e) => {
    if (!e.target.closest("button:not(:disabled), input, label, .menu-wrap")) check.click();
  });
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
  setLabel($("launch-sel"), "play", n ? `Launch ${n}` : "Launch");
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
  if (!userIds.length) return toast("Tick the accounts you want to launch first.");
  const target = $("join-all").value.trim() || null;
  if (target && searchPlan(target) && !searchPlan(target).follow) return toast(PICK_HINT); // words, not a link: pick a game first
  call("launch", { userIds, target });
}

// ---- Join boxes: words search Roblox games; a × clears the box ----
const GAMEPAD = "M6 11h4M8 9v4M15 12h.01M18 10h.01M17.3 5H6.7a4 4 0 0 0-4 3.6l-.7 6.4A2.5 2.5 0 0 0 4.5 18c.9 0 1.6-.5 2.1-1.2L8 15h8l1.4 1.8c.5.7 1.2 1.2 2.1 1.2a2.5 2.5 0 0 0 2.5-3l-.7-6.4a4 4 0 0 0-4-3.6Z";
const PERSON = "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM5 20a7 7 0 0 1 14 0";
function combo(input) {
  const list = $(input.getAttribute("aria-controls"));
  const clear = input.parentElement.querySelector(".clear");
  let timer, seq = 0, active = -1, rows = [], games = [];
  const close = () => {
    clearTimeout(timer);
    seq++; // a reply still on its way must not reopen the list
    rows = []; games = []; active = -1;
    list.hidden = true;
    list.removeAttribute("aria-busy");
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
  };
  const show = (kids) => {
    list.replaceChildren(...kids);
    list.hidden = false;
    input.setAttribute("aria-expanded", "true");
  };
  const status = (text) => el("div", { className: "status", textContent: text });
  const setActive = (n) => {
    active = n;
    rows.forEach((r, k) => r.el.setAttribute("aria-selected", String(k === n)));
    if (n < 0) return input.removeAttribute("aria-activedescendant");
    input.setAttribute("aria-activedescendant", rows[n].el.id);
    rows[n].el.scrollIntoView({ block: "nearest" });
  };
  const pick = (value) => {
    input.value = value;
    close();
    input.dispatchEvent(new Event("input", { bubbles: true })); // a link or @user never searches, so the list stays shut
    input.focus();
  };
  const row = (n, icon, title, sub, value) => {
    const o = el("div", { id: `${list.id}-${n}`, className: n === "f" ? "opt" : "opt game" }, icon,
      el("div", { className: "txt" }, el("div", { className: "t", textContent: title }), el("div", { className: "s num", textContent: sub })));
    o.setAttribute("role", "option");
    o.setAttribute("aria-selected", "false");
    o.addEventListener("click", () => pick(value));
    return { el: o, value };
  };
  const glyph = (d, round) => el("div", { className: "ic" + (round ? " round" : "") }, svg(d));
  const sync = () => { clear.hidden = !input.value; close(); };

  list.addEventListener("mousedown", (e) => e.preventDefault()); // clicking a row or the scrollbar keeps focus in the box
  input.addEventListener("blur", close);
  clear.addEventListener("click", () => {
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.focus();
  });
  input.addEventListener("input", () => {
    clear.hidden = !input.value;
    const plan = searchPlan(input.value);
    if (!plan) return close();
    clearTimeout(timer);
    const mine = ++seq; // any reply still on its way is now stale
    const follow = plan.follow ? [row("f", glyph(PERSON, true), `Follow @${plan.follow}`, "Join whatever they're playing", "@" + plan.follow)] : [];
    rows = [...follow, ...games]; // the last results stay up, dimmed, so the list doesn't collapse on every key
    setActive(-1);
    list.setAttribute("aria-busy", "true");
    show([...rows.map((r) => r.el), ...(games.length ? [] : [status("Searching…")])]);
    timer = setTimeout(async () => {
      let hits = [], failed = false;
      try { hits = await invoke("search_games", { query: plan.query }); } catch { failed = true; }
      if (mine !== seq) return; // the box changed, was cleared, picked or closed meanwhile
      games = hits.map((h, k) => row("g" + k,
        h.icon ? el("img", { className: "ic", alt: "", src: h.icon }) : glyph(GAMEPAD),
        h.name, `${h.players.toLocaleString()} playing`, h.url));
      rows = [...follow, ...games];
      list.removeAttribute("aria-busy");
      const note = failed ? "Couldn't search Roblox right now" : games.length ? null : "No games found";
      show([...rows.map((r) => r.el), ...(note ? [status(note)] : [])]);
      setActive(Math.min(active, rows.length - 1));
    }, 250);
  });
  input.addEventListener("keydown", (e) => {
    if (list.hidden) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = rows.length + 1; // the rows, plus "nothing highlighted"
      if (n > 1) setActive(((active + 1 + (e.key === "ArrowDown" ? 1 : -1) + n) % n) - 1);
    } else if (e.key === "Enter" && active >= 0) {
      e.preventDefault(); // not Save, not Launch: Enter picks the highlighted row
      e.stopImmediatePropagation();
      pick(rows[active].value);
    } else if (e.key === "Escape") {
      e.preventDefault(); // closes the list, not the Edit sheet
      e.stopPropagation();
      close();
    }
  });
  return { sync };
}
combo($("join-all"));
const editCombo = combo($("edit-join"));

// ---- edit sheet: holds its own copy of the values until Save ----
const PICK_HINT = "Pick a game from the list, or paste a link.";
let editing_acc = null;
let describeTimer;
function describeEditJoin() {
  const text = $("edit-join").value;
  const label = $("edit-join-label");
  label.className = "help";
  label.textContent = text.trim() ? "Checking…" : "Opens the Roblox home screen.";
  clearTimeout(describeTimer);
  if (!text.trim()) return;
  if (searchPlan(text) && !searchPlan(text).follow) return void (label.textContent = PICK_HINT); // words: a search, not a broken link
  describeTimer = setTimeout(async () => {
    let l, bad = false;
    try { l = await invoke("describe_join", { userId: editing_acc.user_id, text }); } catch (e) { l = String(e); bad = true; }
    if ($("edit-join").value !== text) return; // a slow answer for text that's since changed
    label.textContent = l;
    label.className = bad ? "help bad" : "help";
  }, 300);
}
function openEdit(a) {
  editing_acc = a;
  $("edit-title").textContent = `Edit ${a.display_name}`;
  $("edit-join").value = a.join;
  $("edit-fps").value = a.fps_cap ?? "";
  $("edit-note").value = a.note;
  editCombo.sync();
  describeEditJoin();
  $("edit-dlg").showModal();
}
$("edit-join").addEventListener("input", describeEditJoin);
$("edit-cancel").addEventListener("click", () => $("edit-dlg").close());
// Saves only the changed fields. set_join rejects a Join Dockie can't read: shake the field and keep the sheet open.
// Save is the form's default button, so Enter in any field lands here too.
$("edit-save").addEventListener("click", async (e) => {
  e.preventDefault();
  if (!e.target.form.reportValidity()) return;
  const a = editing_acc;
  const id = a.user_id;
  const join = $("edit-join").value.trim();
  const fpsRaw = $("edit-fps").value;
  const fps = fpsRaw === "" ? null : Number(fpsRaw);
  const note = $("edit-note").value.trim();
  if (join !== a.join) {
    try {
      await invoke("set_join", { userId: id, join });
    } catch (err) {
      $("edit-join-label").textContent = searchPlan(join) && !searchPlan(join).follow ? PICK_HINT : String(err);
      $("edit-join-label").className = "help bad";
      if (!reduceMotion()) $("edit-join").animate([{ translate: "0" }, { translate: "-8px" }, { translate: "8px" }, { translate: "-5px" }, { translate: "5px" }, { translate: "-2px" }, { translate: "0" }], { duration: 360, easing: "ease-out" });
      $("edit-join").focus();
      return;
    }
  }
  $("edit-dlg").close();
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

// ---- windows: one card per process, updated in place so the 2 s poll never flickers or steals focus ----
const winCards = new Map(); // pid -> { el, paint }
function winCard(pid) {
  const img = el("img", { alt: "" });
  const ph = el("span", { className: "ph" });
  const pill = el("span", { className: "pill" });
  const shot = el("div", { className: "shot" }, el("div", { className: "thumb" }, img, ph), pill);
  const av = el("div", {});
  const name = el("div", { className: "name" });
  const sub = el("div", { className: "handle" });
  const focusBtn = el("button", { className: "btn sm", textContent: "Focus" });
  focusBtn.addEventListener("click", () => call("focus_pid", { pid }));
  const close = el("button", { className: "btn sm danger", textContent: "Close" });
  close.addEventListener("click", () => call("close_pid", { pid }).then(renderWindows));
  const meta = el("div", { className: "meta" });
  const card = el("article", { className: "tile enter" }, shot, el("div", { className: "body" }, av, el("div", { className: "who" }, name, sub), focusBtn, close), meta);
  card.dataset.pid = pid;
  let face; // whose avatar is showing, so it's only rebuilt when that changes
  const paint = (w) => {
    const a = snap?.accounts.find((x) => x.user_id === w.user_id);
    const key = a ? `${a.user_id}|${a.avatar_url}` : "none";
    if (face !== key) { face = key; av.replaceChildren(a ? avatar(a) : el("div", { className: "av" }, svg(GAMEPAD))); }
    name.textContent = a ? a.display_name : w.title || "Roblox";
    sub.textContent = a ? "@" + a.username : w.user_id != null ? "Opened by Dockie" : "Not opened by Dockie";
    pill.className = "pill " + (w.minimized ? "busy" : "ok");
    pill.textContent = w.minimized ? "Minimized" : "Running";
    const p = w.user_id != null ? previews.get(w.user_id) : null;
    const image = !w.minimized && p?.image ? p.image : "";
    img.hidden = !image;
    if (image && img.getAttribute("src") !== image) img.src = image;
    ph.hidden = !!image;
    ph.textContent = w.minimized ? "Minimized" : w.user_id == null ? "No preview for windows Dockie didn't open" : "No preview yet";
    const bits = [w.mem_bytes != null ? fmtBytes(w.mem_bytes) : null, "Up " + fmtUptime(w.uptime_s), "PID " + w.pid].filter(Boolean);
    meta.replaceChildren(...bits.flatMap((t, i) => [...(i ? [el("span", { className: "sep", textContent: "·" })] : []), el("span", { className: "num", textContent: t })]));
  };
  return { el: card, paint };
}
async function renderWindows() {
  let list;
  try { list = await invoke("roblox_windows"); } catch (e) { return fail(e); }
  const pids = new Set(list.map((w) => w.pid));
  for (const [pid, c] of winCards) if (!pids.has(pid)) { c.el.remove(); winCards.delete(pid); }
  for (const w of list) {
    let c = winCards.get(w.pid);
    if (!c) { c = winCard(w.pid); winCards.set(w.pid, c); $("windows").append(c.el); }
    c.paint(w);
  }
  const known = list.filter((w) => w.mem_bytes != null); // memory Windows wouldn't report is left out, not counted as 0
  $("stat-count").textContent = list.length;
  $("stat-mem").textContent = known.length ? fmtBytes(known.reduce((s, w) => s + w.mem_bytes, 0)) : "–";
  $("stat-min").textContent = list.filter((w) => w.minimized).length;
  const none = !list.length;
  for (const id of ["stats", "windows", "close-all"]) $(id).hidden = none;
  $("win-empty").hidden = !none;
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
  const n = snap.accounts.length;
  $("data-help").textContent = `${n} account${n === 1 ? "" : "s"} saved on this PC. Sign-ins are encrypted to your Windows account.`;
}
function saveSettings() {
  const v = $("set-fps").value;
  call("set_settings", { settings: { fps_cap_default: v === "" ? null : Number(v), trim_minimized: $("set-trim").checked } });
}

// ---- wiring ----
for (const b of document.querySelectorAll("[role=tab]")) b.addEventListener("click", () => showTab(b.dataset.tab));
$("warn-close").addEventListener("click", closeAll);
$("close-all").addEventListener("click", closeAll);
$("win-go").addEventListener("click", () => showTab("accounts"));
for (const id of ["signin", "empty-signin"]) $(id).addEventListener("click", () => signIn(null));
for (const id of ["paste", "empty-paste"]) $(id).addEventListener("click", openPaste);
$("launch-sel").addEventListener("click", launchSelected);
setLabel($("stop-all"), "stop", "Stop all");
$("join-all").addEventListener("keydown", (e) => { if (e.key === "Enter") launchSelected(); });
$("stop-all").addEventListener("click", async () => {
  await call("stop_all");
  for (const id of [...launch.keys()]) unqueue(id);
});
for (const id of ["import", "empty-import"]) $(id).addEventListener("click", async () => {
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

window.__TAURI__.app?.getVersion?.().then((v) => ($("version").textContent = "Version " + v)).catch(() => {});
refresh().catch(fail);
