// The window's shell: `show` sets in place the route the hash names, as the
// `page` command draws it, covers through the asset protocol and a chart's
// figures as tooltips; each `addEventListener` carries a click, search or drop.

const TAURI = window.__TAURI__;

const side = document.getElementById("side");
const main = document.getElementById("main");
const tip = document.getElementById("tip");
const veil = document.getElementById("veil");

/** `said` at the foot of the window for a while; `wrong` marks a failure. */
function flash(said, wrong = false) {
  const at = document.getElementById("flash");
  at.textContent = said;
  at.className = wrong ? "flash wrong" : "flash";
  at.hidden = false;
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => (at.hidden = true), wrong ? 9000 : 4000);
}

/** The command `name`'s answer; `err` is flashed and thrown on. */
async function call(name, args = {}) {
  try {
    return await TAURI.core.invoke(name, args);
  } catch (err) {
    flash(String(err), true);
    throw err;
  }
}

/** `run` once `wait` ms pass with no call. */
function settled(run, wait) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => run(...args), wait);
  };
}

/** The route and width last drawn. */
let drawn = { route: null, width: 0 };

/** The highlight row open in the editor, which only `show`, its own buttons and Escape close. */
let editing = null;

/** The route the hash names, drawn at the width of the page column. */
async function show() {
  editing = null;
  const route = location.hash.slice(1) || "/";
  const width = main.clientWidth;
  const typing = document.activeElement?.dataset?.search ? document.activeElement.selectionStart : null;
  const page = await call("page", { route, width });
  document.getElementById("vars").textContent = page.vars;
  document.documentElement.lang = page.lang;
  document.title = page.title;
  side.innerHTML = page.side;
  main.innerHTML = page.main;
  for (const img of document.querySelectorAll("img[data-cover]")) {
    img.src = TAURI.core.convertFileSrc(img.dataset.cover);
  }
  for (const title of main.querySelectorAll("svg title")) {
    title.parentElement.dataset.tip = title.textContent;
    title.remove();
  }
  const box = main.querySelector("[data-search]");
  if (typing !== null && box) {
    box.focus();
    box.setSelectionRange(typing, typing);
  }
  if (route !== drawn.route && typing === null) window.scrollTo(0, 0);
  drawn = { route, width };
}

/** What names `row`'s passage, or its note where `note`, as `edit_highlight` takes it. */
function named(row, note) {
  const field = (name) => row.dataset[note ? `note${name[0].toUpperCase()}${name.slice(1)}` : name];
  if (field("at") === undefined) return null;
  return {
    at: field("at"),
    kind: field("kind"),
    title: field("title"),
    location: Number(field("location")),
    start: Number(field("start")),
  };
}

/** A text area holding `text`, as tall as its lines. */
function area(part, text, lang) {
  const box = document.createElement("textarea");
  box.dataset.part = part;
  box.dataset.was = text;
  box.value = text;
  if (lang) box.lang = lang;
  box.addEventListener("input", () => fit(box));
  return box;
}

function fit(box) {
  box.style.height = "auto";
  box.style.height = `${box.scrollHeight + 2}px`;
}

/** `row` open in `#editor`: its passage, and its note where it carries one, in text areas. */
function open(row) {
  if (editing) close(editing);
  const editor = document.getElementById("editor").content.firstElementChild.cloneNode(true);
  const acts = editor.querySelector(".acts");
  const passage = row.querySelector("blockquote");
  const note = row.querySelector(".note");
  editor.insertBefore(area("passage", passage.textContent, passage.lang), acts);
  if (note) editor.insertBefore(area("note", note.textContent, note.lang), acts);
  if (!("edited" in row.dataset)) editor.querySelector('[data-act="revert"]').remove();
  row.classList.add("open");
  row.append(editor);
  for (const box of editor.querySelectorAll("textarea")) fit(box);
  editor.querySelector("textarea").focus();
  editing = row;
}

function close(row) {
  row.querySelector(".editor")?.remove();
  row.classList.remove("open");
  if (editing === row) editing = null;
}

/** Keep `row`'s words as its text areas hold them, or the Kindle's where `revert`, and draw again. */
async function keep(row, revert) {
  const changes = [];
  for (const box of row.querySelectorAll(".editor textarea")) {
    const id = named(row, box.dataset.part === "note");
    if (!id) continue;
    if (revert) changes.push({ id, text: null });
    else if (box.value !== box.dataset.was) changes.push({ id, text: box.value });
  }
  if (changes.length === 0) {
    close(row);
    return;
  }
  await call("edit_highlight", { kindle: row.dataset.kindle ?? "", changes });
  close(row);
  await show();
}

main.addEventListener("keydown", (event) => {
  const row = event.target.closest(".highlight.open");
  if (!row || event.target.tagName !== "TEXTAREA") return;
  if (event.key === "Escape") close(row);
  else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) keep(row, false).catch(console.error);
  else return;
  event.preventDefault();
});

/** Import `paths`, say what the record gained, and draw the route again. */
async function bring(paths) {
  if (paths.length === 0) return;
  flash(await call("import", { paths }));
  await show();
}

document.addEventListener("click", async (event) => {
  const act = event.target.closest("[data-act]");
  if (!act) return;
  event.preventDefault();
  switch (act.dataset.act) {
    case "pick": {
      const path = await call("pick_backup");
      if (path) await bring([path]);
      break;
    }
    case "kindle":
      flash(await call("import_kindle", { serial: act.dataset.serial }));
      await show();
      break;
    case "reveal":
      await call("reveal", { path: act.dataset.path });
      break;
    case "folder":
      if (await call("choose_folder")) await show();
      break;
    case "delete":
      if (await call("delete_backup", { path: act.dataset.path })) await show();
      break;
    case "delete-all":
      if (await call("delete_backups")) await show();
      break;
    case "reset": {
      const said = await call("reset");
      if (!said) break;
      flash(said);
      history.replaceState(null, "", "#/");
      await show();
      break;
    }
    case "edit":
      open(act.closest(".highlight"));
      break;
    case "save":
      await keep(act.closest(".highlight"), false);
      break;
    case "revert":
      await keep(act.closest(".highlight"), true);
      break;
    case "cancel":
      close(act.closest(".highlight"));
      break;
  }
});

document.addEventListener("click", async (event) => {
  const pick = event.target.closest("[data-set]");
  if (!pick) return;
  await call("set_setting", { key: pick.dataset.set, value: pick.dataset.value });
  await show();
});

/** Whether an input method is composing text, which `show` throws away with the box it is typed in. */
let composing = false;

// `[data-search]` narrows its own route: the hash takes `q`, in place, from
// what is typed, and from what an input method commits once `compositionend`
// says it has.
const search = settled((box) => {
  if (composing) return;
  const [path, query] = (location.hash.slice(1) || "/").split("?");
  const params = new URLSearchParams(query ?? "");
  if (box.value) params.set("q", box.value);
  else params.delete("q");
  const said = params.toString();
  history.replaceState(null, "", `#${path}${said ? `?${said}` : ""}`);
  show();
}, 220);
main.addEventListener("input", (event) => {
  const box = event.target.closest("[data-search]");
  if (box && !event.isComposing && !composing) search(box);
});
main.addEventListener("compositionstart", () => (composing = true));
main.addEventListener("compositionend", (event) => {
  composing = false;
  const box = event.target.closest("[data-search]");
  if (box) search(box);
});

main.addEventListener("mousemove", (event) => {
  const at = event.target.closest("[data-tip]");
  if (!at) {
    tip.hidden = true;
    return;
  }
  tip.textContent = at.dataset.tip;
  tip.hidden = false;
  const x = Math.min(event.clientX + 14, window.innerWidth - tip.offsetWidth - 8);
  const y = event.clientY + 18 + tip.offsetHeight > window.innerHeight
    ? event.clientY - tip.offsetHeight - 10
    : event.clientY + 18;
  tip.style.left = `${x}px`;
  tip.style.top = `${y}px`;
});
main.addEventListener("mouseleave", () => (tip.hidden = true));

TAURI.webview.getCurrentWebview().onDragDropEvent((event) => {
  const payload = event.payload;
  veil.hidden = !(payload.type === "enter" || payload.type === "over");
  if (payload.type === "drop") bring(payload.paths ?? []).catch(console.error);
});

// `ArrowLeft` and `ArrowRight` step a span or a day, through the page's `a[data-key]`.
document.addEventListener("keydown", (event) => {
  if (event.target.closest("input, textarea") || event.metaKey || event.ctrlKey || event.altKey) return;
  const key = { ArrowLeft: "prev", ArrowRight: "next" }[event.key];
  const arrow = key && main.querySelector(`a[data-key="${key}"]`);
  if (!arrow) return;
  event.preventDefault();
  location.hash = arrow.getAttribute("href");
});

window.addEventListener("hashchange", () => show().catch(console.error));
window.addEventListener(
  "resize",
  settled(() => {
    if (!composing && !editing && Math.abs(main.clientWidth - drawn.width) > 8) show().catch(console.error);
  }, 150),
);

// `poll` draws the page again where `kindle` changes, a Kindle plugged in or
// out, or `today` does, and asks nothing while `composing` or `editing`.
let kindle;
let today;
async function poll() {
  if (composing || editing) return;
  const [plugged, day] = await Promise.all([
    TAURI.core.invoke("kindle").catch(() => null),
    TAURI.core.invoke("today").catch(() => null),
  ]);
  const moved = (kindle !== undefined && plugged !== kindle) || (today !== undefined && day !== today);
  if (moved) show().catch(console.error);
  kindle = plugged;
  today = day;
}
setInterval(poll, 3000);
poll();

show().catch(console.error);
