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

/** The commands changing the notes and corrections a Kindle plugged in takes at its next sync. */
const WRITING = new Set(["write_note", "reword_note", "delete_note", "edit_highlight"]);

/** The command `name`'s answer; `err` is flashed and thrown on. One of `WRITING` sets `resync` going. */
async function call(name, args = {}) {
  try {
    const answer = await TAURI.core.invoke(name, args);
    if (WRITING.has(name)) resync();
    return answer;
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

/** The row or note open in an editor, or the note field holding words: only `show`, their own buttons and Escape close it. */
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
  drawCard();
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
  watch();
}

/** What names the passage or the Kindle's note `holder` stands for, as `edit_highlight` takes it. */
function named(holder) {
  const field = holder.dataset;
  if (field.at === undefined) return null;
  return {
    at: field.at,
    kind: field.kind,
    title: field.title,
    location: Number(field.location),
    start: Number(field.start),
  };
}

/** `words`' own text, the buttons set in it left out. */
function textOf(words) {
  const copy = words.cloneNode(true);
  for (const button of copy.querySelectorAll("button")) button.remove();
  return copy.textContent;
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

/** A copy of the template `id`'s first element. */
function template(id) {
  return document.getElementById(id).content.firstElementChild.cloneNode(true);
}

/** Close whatever `editing` holds open. */
function closeAny() {
  if (!editing) return;
  if (editing.classList.contains("composer")) {
    editing.querySelector("textarea").value = "";
    editing.querySelector("textarea").blur();
    editing = null;
    return;
  }
  editing.querySelector(":scope > .editor")?.remove();
  editing.classList.remove("open");
  delete editing.dataset.open;
  editing = null;
}

/** `holder` open in `#editor` on its `part`: the passage, or the Kindle's note, in a text area in its place. */
function open(holder, part) {
  closeAny();
  const words = holder.querySelector(part === "passage" ? ":scope > blockquote" : ":scope > .note");
  if (!words || holder.dataset.at === undefined) return;
  const editor = template("editor");
  const box = area(part, textOf(words), words.lang);
  editor.insertBefore(box, editor.querySelector(".acts"));
  if (!("edited" in holder.dataset)) editor.querySelector('[data-act="revert"]').remove();
  holder.classList.add("open");
  holder.dataset.open = part;
  words.after(editor);
  fit(box);
  box.focus();
  editing = holder;
}

/** Keep the words `holder`'s editor holds, or the Kindle's where `revert`, and draw again. */
async function keep(holder, revert) {
  const id = named(holder);
  const box = holder.querySelector(":scope > .editor textarea");
  if (!id || !box || (!revert && box.value === box.dataset.was)) {
    closeAny();
    return;
  }
  const kindle = holder.closest(".highlight").dataset.kindle ?? "";
  await call("edit_highlight", { kindle, changes: [{ id, text: revert ? null : box.value }] });
  closeAny();
  await show();
}

/** `#note-editor` open on the note written here that `holder` holds, its Markdown in place of the note. */
function openWritten(holder) {
  closeAny();
  const editor = template("note-editor");
  const box = editor.querySelector("textarea");
  const note = holder.querySelector(":scope > .note");
  box.value = note.dataset.text ?? note.textContent;
  box.lang = note.lang;
  box.addEventListener("input", () => fit(box));
  holder.classList.add("open");
  holder.append(editor);
  fit(box);
  box.focus();
  editing = holder;
}

/** `#note-editor` open on `row` for a new note on its passage, above the row's label. */
function openAdding(row) {
  closeAny();
  const editor = template("note-editor");
  editor.querySelector('[data-act="delete-note"]').remove();
  editor.dataset.adding = "";
  const box = editor.querySelector("textarea");
  box.addEventListener("input", () => fit(box));
  row.insertBefore(editor, row.querySelector(":scope > .where"));
  box.focus();
  editing = row;
}

/** Keep what the note editor `editor` holds: a new note on its row's passage, or new words for the note it opened on. */
async function keepNote(editor) {
  const text = editor.querySelector("textarea").value;
  const row = editor.closest(".highlight");
  const kindle = row.dataset.kindle ?? "";
  if ("adding" in editor.dataset) {
    if (text.trim() === "") {
      editor.remove();
      editing = null;
      return;
    }
    await call("write_note", { kindle, on: { passage: named(row) }, text });
  } else {
    const holder = editor.closest("[data-written]");
    if (text.trim() === "") {
      if (!(await call("delete_note", { kindle, written: holder.dataset.written }))) return;
    } else {
      await call("reword_note", { kindle, written: holder.dataset.written, text });
    }
  }
  editing = null;
  await show();
}

/** Write what the field `composer` holds as a note on its book alone. */
async function keepComposed(composer) {
  const box = composer.querySelector("textarea");
  if (box.value.trim() === "") return;
  await call("write_note", {
    kindle: composer.dataset.kindle ?? "",
    on: { book: { key: composer.dataset.key ?? "", extent: Number(composer.dataset.extent) } },
    text: box.value,
  });
  box.value = "";
  editing = null;
  await show();
}

// Escape closes what `editing` holds open, and ⌘↩ keeps it: the note field's
// words, a note editor's, or a row's.
main.addEventListener("keydown", (event) => {
  if (event.target.tagName !== "TEXTAREA" || !editing) return;
  const editor = event.target.closest(".editor");
  if (event.key === "Escape") closeAny();
  else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    if (editing.classList.contains("composer")) keepComposed(editing).catch(console.error);
    else if (editor?.querySelector('[data-act="save-note"]')) keepNote(editor).catch(console.error);
    else keep(editing, false).catch(console.error);
  } else return;
  event.preventDefault();
});

main.addEventListener("focusin", (event) => {
  const composer = event.target.closest(".composer");
  if (composer && !editing) editing = composer;
});
// `mousedown` on a `.composer` button keeps the focus in its field, where its
// `.acts` stand only under `:focus-within`.
main.addEventListener("mousedown", (event) => {
  if (event.target.closest(".composer button")) event.preventDefault();
});
main.addEventListener("focusout", (event) => {
  const composer = event.target.closest(".composer");
  if (composer && editing === composer && composer.querySelector("textarea").value.trim() === "") {
    editing = null;
  }
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
      await sync(act.dataset.serial);
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
      if (act.dataset.part === "note") open(act.closest(".knote, .highlight"), "note");
      else open(act.closest(".highlight"), "passage");
      break;
    case "save":
      await keep(editing, false);
      break;
    case "revert":
      await keep(editing, true);
      break;
    case "cancel":
      closeAny();
      break;
    case "add-note":
      openAdding(act.closest(".highlight"));
      break;
    case "edit-note":
      openWritten(act.closest("[data-written]"));
      break;
    case "save-note":
      await keepNote(act.closest(".editor"));
      break;
    case "cancel-note": {
      const composer = act.closest(".composer");
      if (composer) {
        composer.querySelector("textarea").value = "";
        editing = null;
      } else if (act.closest(".editor")?.dataset.adding !== undefined) {
        act.closest(".editor").remove();
        editing = null;
      } else closeAny();
      break;
    }
    case "delete-note": {
      const holder = act.closest("[data-written]");
      const row = act.closest(".highlight");
      if (await call("delete_note", { kindle: row.dataset.kindle ?? "", written: holder.dataset.written })) {
        editing = null;
        await show();
      }
      break;
    }
    case "write-note":
      await keepComposed(act.closest(".composer"));
      break;
    case "jump":
      document.getElementById(act.dataset.to)?.scrollIntoView({ behavior: "smooth", block: "start" });
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

/** The observers keeping a book page's bar in step with the scroll. */
let watching = [];

// `watch` shows a book page's `.slim` bar once its head scrolls away, the
// chip of the part in view lit.
function watch() {
  for (const observer of watching) observer.disconnect();
  watching = [];
  const slim = main.querySelector(".slim");
  const hero = main.querySelector(".hero");
  if (!slim || !hero) return;
  const away = new IntersectionObserver(([entry]) => (slim.hidden = entry.isIntersecting));
  away.observe(hero);
  const lit = new IntersectionObserver(
    (entries) => {
      for (const entry of entries.filter((e) => e.isIntersecting)) {
        for (const chip of slim.querySelectorAll("[data-to]")) {
          chip.classList.toggle("on", chip.dataset.to === entry.target.id);
        }
      }
    },
    { rootMargin: "-30% 0px -60% 0px" },
  );
  for (const id of ["kindles", "journal"]) {
    const part = document.getElementById(id);
    if (part) lit.observe(part);
  }
  watching = [away, lit];
}

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
    if (!composing && !editing && Math.abs(main.clientWidth - drawn.width) > 8) {
      show().catch(console.error);
    }
  }, 150),
);

/** The Kindle `sync` last ran for, while it stays plugged in. */
let synced = null;

/** The Kindle card's line for the cut serial `serial`: `busy` while a sync runs, `wrong` once one stopped. */
let card = { serial: null, line: "", busy: false, wrong: false };

/** Set `card` on the Kindle card drawn for its serial; answers whether one is drawn. */
function drawCard() {
  const at = [...document.querySelectorAll(".kindle[data-serial]")].find((k) =>
    k.dataset.serial === card.serial
  );
  if (!at) return false;
  const state = at.querySelector(".state");
  state.textContent = card.line;
  state.classList.toggle("busy", card.busy);
  state.classList.toggle("wrong", card.wrong);
  at.querySelector("button[data-act=kindle]").hidden = !card.wrong;
  return true;
}

/** `line` on the card of the Kindle `serial` names, `how` being "busy" or "wrong"; flashed where it is wrong and no card is drawn. */
function tell(serial, line, how = "") {
  card = { serial, line, busy: how === "busy", wrong: how === "wrong" };
  if (!drawCard() && card.wrong) flash(line, true);
}

/** Sync the Kindle `serial` names, as `sync_kindle` does, its card told each half; flash what came in and draw again where it says anything, `quiet` saying nothing where nothing came in. */
async function sync(serial, quiet = false) {
  synced = serial;
  let running = true;
  const phase = new TAURI.core.Channel();
  phase.onmessage = (line) => running && tell(serial, line, "busy");
  let said;
  try {
    said = await TAURI.core.invoke("sync_kindle", { serial, quiet, phase });
  } catch (err) {
    tell(serial, String(err), "wrong");
    return;
  } finally {
    running = false;
  }
  tell(serial, said.card);
  if (!said.flash) return;
  flash(said.flash);
  await show();
}

/** A quiet `sync` of the Kindle plugged in, once the window rests after a change to the notes or corrections. */
const resync = settled(() => {
  if (!kindle) return;
  if (composing || editing) return resync();
  sync(kindle, true).catch(console.error);
}, 3000);

// `poll` draws the page again where `kindle` changes, a Kindle plugged in or
// out, or `today` does, then syncs a Kindle arriving; it asks nothing while
// `composing` or `editing`.
let kindle;
let today;
async function poll() {
  if (composing || editing) return;
  const [plugged, day] = await Promise.all([
    TAURI.core.invoke("kindle").catch(() => null),
    TAURI.core.invoke("today").catch(() => null),
  ]);
  const moved = (kindle !== undefined && plugged !== kindle) || (today !== undefined && day !== today);
  kindle = plugged;
  today = day;
  const arriving = plugged && plugged !== synced;
  if (!plugged) synced = null;
  if (arriving) synced = plugged;
  if (arriving || !plugged) card = { serial: plugged, line: "", busy: false, wrong: false };
  if (moved) await show().catch(console.error);
  if (arriving) sync(plugged).catch(console.error);
}
setInterval(poll, 3000);
poll();

show().catch(console.error);
