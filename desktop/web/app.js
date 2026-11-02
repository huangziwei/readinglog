// `show` sets in place the route `location.hash` names, as the `page` command
// draws it, covers through `convertFileSrc` and a chart's figures as `data-tip`;
// each `addEventListener` carries a click, search or drop.

const TAURI = window.__TAURI__;

const side = document.getElementById("side");
const main = document.getElementById("main");
const tip = document.getElementById("tip");
const veil = document.getElementById("veil");

/** `said` at the foot of the window for a while; `wrong` marks a failure, and `act`, `{ label, run }`, stands as a button after it. */
function flash(said, wrong = false, act = null) {
  const at = document.getElementById("flash");
  at.textContent = said;
  if (act) {
    const button = document.createElement("button");
    button.textContent = act.label;
    button.addEventListener("click", () => {
      at.hidden = true;
      act.run().catch(console.error);
    });
    at.append(" ", button);
  }
  at.className = wrong ? "flash wrong" : "flash";
  at.hidden = false;
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => (at.hidden = true), wrong || act ? 9000 : 4000);
}

/** `template` with `{d}` set to `count`, and text in brackets kept only where `count` is not one, as `lang::counted` sets it. */
function counted(template, count) {
  let out = "";
  let dropping = false;
  for (const ch of template) {
    if (ch === "[") dropping = count === 1;
    else if (ch === "]") dropping = false;
    else if (!dropping) out += ch;
  }
  return out.replaceAll("{d}", String(count));
}

/** `WRITING`: the commands writing rows a Kindle plugged in takes at its next sync. */
const WRITING = new Set([
  "write_note",
  "reword_note",
  "delete_note",
  "edit_highlight",
  "edit_metadata",
  "edit_metadata_each",
  "rename_metadata",
  "rename_type",
  "retag_every",
  "undo_metadata",
]);

/** `call`: the command `name`'s answer, `err` flashed and thrown on, and `resync` set going where `WRITING` holds `name`. */
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

/** `drawn`: the route and width last drawn. */
let drawn = { route: null, width: 0 };

/** `editing`: what holds an editor open (a row, a note, the note field holding words, a metadata editor or a `.types` field), which `show`, its own buttons and Escape close. */
let editing = null;

/** `drawing`: whether `show` is setting a page in place, its fields losing the focus keeping nothing. */
let drawing = false;

/** `show`: the route `location.hash` names, drawn at `main`'s width. */
async function show() {
  editing = null;
  const route = location.hash.slice(1) || "/";
  const width = main.clientWidth;
  const typing = document.activeElement?.dataset?.search ? document.activeElement.selectionStart : null;
  const page = await call("page", { route, width });
  document.getElementById("vars").textContent = page.vars;
  document.documentElement.lang = page.lang;
  document.title = page.title;
  drawing = true;
  side.innerHTML = page.side;
  main.innerHTML = page.main;
  drawing = false;
  for (const chart of main.querySelectorAll("[data-chart]")) {
    if (whole.has(chart.dataset.chart)) openWhole(chart, true);
  }
  for (const box of main.querySelectorAll("input[data-fit]")) fitted(box);
  if (refocus) {
    const key = refocus.toLowerCase();
    refocus = null;
    [...main.querySelectorAll("input[data-key]")].find((b) => b.dataset.key.toLowerCase() === key)?.focus();
  }
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

/** `named`: what names the passage or the Kindle's note `holder` stands for, as `edit_highlight` takes it. */
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

/** `area`: a text area holding `text`, as tall as its lines. */
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

/** `template`: a copy of the template `id`'s first element. */
function template(id) {
  return document.getElementById(id).content.firstElementChild.cloneNode(true);
}

/** `closeAny` closes whatever `editing` holds open. */
function closeAny() {
  if (!editing) return;
  if (editing.closest(".meta, .sheet, .renaming")) {
    closeMeta();
    return;
  }
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

/** `keep` keeps the words `holder`'s editor holds, or the Kindle's where `revert`, and draws again. */
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
  holder.dataset.open = "note";
  note.after(editor);
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

/** `keepNote` keeps what the note editor `editor` holds: a new note on its row's passage, or new words for the note it opened on. */
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

/** `keepComposed` writes what the field `composer` holds as a note on its book alone. */
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

// `Escape` closes what `editing` holds open, and `Enter` with `metaKey` or
// `ctrlKey` keeps it: the note field's words, a note editor's, or a row's.
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

/** `bring` imports `paths`, flashes what the record gained, and draws the route again. */
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

// `whole` … `rename`: the ranked charts shown whole, the head's editor and
// `.types`, the pick lists, the Books selection and its bulk editor, Rename…,
// and the suggestions.

/** `whole`: the ranked charts shown whole, by their `data-chart`, which `show` opens again on every page drawn. */
const whole = new Set();

/** `chart`'s bars past the first ten shown where `open`, hidden where not, its `[data-act="whole"]` naming what a click does next. */
function openWhole(chart, open) {
  for (const row of chart.querySelectorAll(".r.past")) row.hidden = !open;
  const button = chart.querySelector('[data-act="whole"]');
  if (button) button.textContent = open ? button.dataset.top : button.dataset.all;
}

/** `metaWords`: the words the metadata editors flash, `undo` and `changed` with `{d}` books. */
function metaWords() {
  return document.getElementById("metawords")?.dataset ?? { undo: "Undo", changed: "{d}" };
}

/** `bookOf`: the book `at` names in its `data-book`, as the commands name one. */
function bookOf(at) {
  return JSON.parse(at.closest("[data-book]").dataset.book);
}

/** `closeMeta` closes the metadata editor, the bulk editor or Rename… open, every field of the head back as it was. */
function closeMeta() {
  for (const open of main.querySelectorAll(".metaedit, .renaming")) open.hidden = true;
  for (const box of main.querySelectorAll(".meta input[data-was]")) {
    box.value = box.dataset.was;
    if ("fit" in box.dataset) fitted(box);
  }
  main.querySelector(".hero")?.classList.remove("editing");
  main.querySelector(".meta")?.classList.remove("open");
  const sheet = main.querySelector(".sheet");
  if (sheet && !sheet.hidden) {
    sheet.hidden = true;
    main.querySelector(".grid")?.classList.remove("under");
  }
  for (const pick of main.querySelectorAll(".pick")) pick.hidden = true;
  editing = null;
}

/** `patched` sets what `patch` changes on `books` and draws again, flashing Undo where `offer` asks for it or the edit reached past `books`. */
async function patched(books, patch, offer = false) {
  const undone = await call("edit_metadata", { books, patch });
  closeMeta();
  await show();
  if (undone.length && (offer || reached(undone) > books.length)) offerUndo(undone);
}

/** `reached`: how many books the rows `undone` sets back are of. */
function reached(undone) {
  const books = new Set();
  for (const each of undone) {
    for (const row of each.rows) books.add(`${each.kindle}\t${row.key}\t${row.key ? "" : row.extent}`);
  }
  return books.size;
}

/** `offerUndo` flashes how many books `undone` reaches, with Undo taking it back. */
function offerUndo(undone) {
  flash(counted(metaWords().changed, reached(undone)), false, {
    label: metaWords().undo,
    run: async () => {
      await call("undo_metadata", { undone });
      await show();
    },
  });
}

/** `changeOf`: the change `box` asks for against `was`, `back` where emptied, else its words, `null` where it stands as it was. */
function changeOf(box, was) {
  const value = box.value.trim();
  if (value === was) return null;
  return value ? { to: value } : "back";
}

/** `saveMeta` saves the head's editor, each field changed. */
async function saveMeta(meta) {
  const patch = {};
  for (const box of meta.querySelectorAll(".metaedit input[data-field]")) {
    const change = changeOf(box, box.dataset.was);
    if (change) patch[box.dataset.field] = change;
  }
  if (Object.keys(patch).length === 0) return closeMeta();
  await patched([bookOf(meta)], patch);
}

/** `retagsOf`: what the bulk editor's tag fields in `holder` ask for, `retag` each tag whose group's type or own words changed and `remove` each marked gone, a blank field standing as it was. */
function retagsOf(holder) {
  const retag = [];
  const remove = [];
  for (const group of holder?.querySelectorAll(".group") ?? []) {
    const kind = group.querySelector('input[data-field="type"]');
    if (!kind) continue;
    const type = kind.value.trim() || kind.dataset.was;
    for (const box of group.querySelectorAll('.tagged input[data-field="tag"]')) {
      if (box.closest(".tagged").classList.contains("new")) continue;
      const from = { type: kind.dataset.was, tag: box.dataset.was };
      const tag = box.value.trim() || from.tag;
      if (box.closest(".tagged").classList.contains("gone")) remove.push(from);
      else if (type !== from.type || tag !== from.tag) retag.push({ from, to: { type, tag } });
    }
  }
  const out = {};
  if (retag.length) out.retag = retag;
  if (remove.length) out.remove = remove;
  return out;
}

/** `ruler`: a canvas's 2D context, measuring words in a field's face; `null` where the page has none. */
function ruler() {
  try {
    return document.createElement("canvas").getContext("2d") ?? null;
  } catch {
    return null;
  }
}

/** `words`' width in `em`, reckoned by the letter's breadth as `screen::metadata::width` reckons it. */
function reckoned(words) {
  let em = 0;
  for (const ch of words) {
    if (ch.codePointAt(0) >= 0x2e80) em += 1;
    else if ("ijlI.,:;'!|".includes(ch)) em += 0.26;
    else if ("frt -()/".includes(ch)) em += 0.36;
    else if ("mwMW".includes(ch)) em += 0.85;
    else if (ch >= "A" && ch <= "Z") em += 0.68;
    else em += 0.57;
  }
  return em;
}

/** `fitted`: `box` as wide as its words and no narrower than its placeholder, measured in its face or `reckoned` where no `ruler` measures, a caret's room past them. */
function fitted(box) {
  const words = [box.value, box.placeholder ?? ""];
  fitted.ruler ??= ruler() ?? false;
  const style = fitted.ruler && globalThis.getComputedStyle?.(box);
  if (style?.fontSize) {
    fitted.ruler.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const px = Math.max(...words.map((w) => fitted.ruler.measureText(w).width));
    box.style.width = `${Math.ceil(px) + 2}px`;
    return;
  }
  box.style.width = `calc(${Math.max(...words.map(reckoned)).toFixed(2)}em + 2px)`;
}

/** `tagOf`: the tag the form `adding` holds, `null` where either field is blank. */
function tagOf(adding) {
  const type = adding.querySelector('input[data-field="type"]').value.trim();
  const tag = adding.querySelector('input[data-field="tag"]').value.trim();
  return type && tag ? { type, tag } : null;
}

/** `pickOf`: the pick list `box` offers, its own or the editor's list of types or tags moved under it. */
function pickOf(box) {
  const own = box.parentElement.querySelector(".pick");
  if (own) return own;
  const held = box.closest(".meta, .sheet")?.querySelector(`.pick[data-for="${box.dataset.pick}"]`);
  if (held) box.parentElement.append(held);
  return held ?? null;
}

/** `narrowPick`: the pick list `box` offers, its options narrowed to its words, and a tag field's to the type beside it (the languages too under a type `data-languages` names), less the tags a `.types` row holds. */
function narrowPick(box) {
  const pick = pickOf(box);
  if (!pick) return;
  const typed = box.value.trim().toLowerCase();
  const row = box.closest(".adding, .group, .kind");
  const type = box.dataset.field === "tag"
    ? (row?.querySelector('input[data-field="type"]')?.value ?? "").trim().toLowerCase()
    : "";
  const spoken = type !== "" && (pick.dataset.languages ?? "").split("|").includes(type);
  const held = [...(row?.querySelectorAll(".tag[data-tag]") ?? [])].map((t) => t.dataset.tag.toLowerCase());
  let shown = 0;
  for (const option of pick.children) {
    const typeFits = !type || option.dataset.type === type || (spoken && option.dataset.lang !== undefined);
    const fits = (!typed || option.dataset.words.includes(typed)) && typeFits &&
      !held.includes(option.dataset.name.toLowerCase());
    option.hidden = !fits;
    if (fits) shown++;
  }
  pick.hidden = shown === 0;
}

main.addEventListener("focusin", (event) => {
  const box = event.target.closest("input[data-pick]");
  if (box) narrowPick(box);
});
main.addEventListener("input", (event) => {
  const box = event.target.closest("input");
  if (!box) return;
  if ("fit" in box.dataset) fitted(box);
  if (box.closest(".fresh") && box.classList.contains("name")) {
    box.closest(".fresh").querySelector(".more input").hidden = box.value.trim() === "";
  }
  if (!box.dataset.pick) return;
  narrowPick(box);
});
main.addEventListener("focusout", (event) => {
  const box = event.target.closest("input[data-pick]");
  const pick = box?.parentElement.querySelector(".pick");
  if (pick) setTimeout(() => (pick.hidden = true), 120);
});
// `mousedown` takes an option, before its field loses the focus; under a
// `.types` row it adds the tag at once.
main.addEventListener("mousedown", (event) => {
  const option = event.target.closest(".pick > [data-name]");
  if (!option) return;
  event.preventDefault();
  const box = option.parentElement.parentElement.querySelector("input");
  box.value = option.dataset.name;
  if ("fit" in box.dataset) fitted(box);
  option.parentElement.hidden = true;
  if (box.closest(".types")) keepType(box, againOf(box)).catch(console.error);
  else if (box.dataset.field === "type") box.closest(".adding")?.querySelector('input[data-field="tag"]').focus();
});

// `Enter` keeps the head's editor or a `.types` field, adds a tag, or renames,
// never while `isComposing`; `Escape` closes, or puts a `.types` field back.
main.addEventListener("keydown", async (event) => {
  const box = event.target.closest(".meta input, .sheet input, .renaming input, .types input");
  if (!box || event.isComposing || event.keyCode === 229) return;
  if (box.closest(".types")) typeKey(event, box);
  else if (event.key === "Escape") {
    event.preventDefault();
    closeMeta();
  } else if (event.key === "Enter") {
    event.preventDefault();
    if (box.closest(".adding")) addTag(box.closest(".adding"));
    else if (box.closest(".renaming")) await rename(box.closest(".renaming"));
    else if (box.closest(".metaedit")) await saveMeta(box.closest(".meta"));
  }
});

// `.types`: a book page's tags under every type in use, each opening Books narrowed to it,
// `.more` adding one; over Books as `.editing`, every tag a field. Each field is kept on
// `Enter` or `focusout`, a `.name` renaming its type on every book, a tag field its tag.

/** `refocus`: the `data-key` of the field `show` focuses once it has drawn the page. */
let refocus = null;

/** `box`, a field of `.types`, back as it was: its words, or empty where it held none. */
function putBack(box) {
  box.value = box.dataset.was ?? "";
  if ("fit" in box.dataset) fitted(box);
}

/** `askOf`: what `box`, a field of `.types`, asks for, as a command and its arguments; `null` where it asks nothing, a blank field standing as it was. */
function askOf(box) {
  const words = box.value.trim();
  const row = box.closest(".kind");
  const name = row.querySelector(".name");
  const fresh = row.classList.contains("fresh");
  const type = fresh ? name.value.trim() : name.dataset.was;
  if (box === name) return fresh || !words || words === type ? null : ["rename_type", { from: type, to: words }];
  if (box.closest(".more")) {
    return words && type ? ["edit_metadata", { books: [bookOf(box)], patch: { add: [{ type, tag: words }] } }] : null;
  }
  if (!words || words === box.dataset.was) return null;
  return ["retag_every", { from: { type, tag: box.dataset.was }, to: { type, tag: words } }];
}

/** `follow` sets in place the tag the route narrows Books to, `from` (a type, or a type and a tag), as `to`, the filter dropped where `to` is `null`. */
function follow(from, to) {
  const [path, query] = (location.hash.slice(1) || "/").split("?");
  const params = new URLSearchParams(query ?? "");
  const named = (held, words) => (held ?? "").toLowerCase() === words.toLowerCase();
  if (params.get("by") !== "tag" || !named(params.get("type"), from.type)) return;
  if (from.tag !== undefined && !named(params.get("is"), from.tag)) return;
  if (to === null) {
    for (const key of ["by", "type", "is"]) params.delete(key);
  } else {
    params.set("type", to.type);
    if (to.tag !== undefined) params.set("is", to.tag);
  }
  const said = params.toString();
  history.replaceState(null, "", `#${path}${said ? `?${said}` : ""}`);
}

/** `keepType` keeps what `box`, a field of `.types`, asks for, once, and draws again, `then` naming the field focused after, flashing Undo for a type or a tag renamed on every book or an edit reaching other books. */
async function keepType(box, then = null) {
  if ("kept" in box.dataset) return;
  const ask = askOf(box);
  if (!ask) return putBack(box);
  box.dataset.kept = "";
  const [name, args] = ask;
  refocus = then;
  let undone;
  try {
    undone = await call(name, args);
  } catch {
    delete box.dataset.kept;
    refocus = null;
    return;
  }
  if (name === "rename_type") follow({ type: args.from }, { type: args.to });
  if (name === "retag_every") follow(args.from, args.to);
  closeMeta();
  await show();
  if (undone.length && (name !== "edit_metadata" || reached(undone) > 1)) offerUndo(undone);
}

/** `typeKey`: `Enter` or `Escape` in `box`, a field of `.types`, `Enter` in `.fresh`'s type going on to its tag and in `.more` keeping the focus there for another. */
function typeKey(event, box) {
  if (event.key !== "Enter" && event.key !== "Escape") return;
  event.preventDefault();
  const row = box.closest(".kind");
  if (event.key === "Escape") {
    putBack(box);
    box.blur();
  } else if (row.classList.contains("fresh") && box === row.querySelector(".name")) {
    const tag = row.querySelector(".more input");
    tag.hidden = box.value.trim() === "";
    if (!tag.hidden) tag.focus();
  } else if (!askOf(box)) {
    putBack(box);
    box.blur();
  } else {
    keepType(box, box.closest(".more") ? againOf(box) : null).catch(console.error);
  }
}

/** `againOf`: the `data-key` of the `.more` field a tag added from `box` leaves the focus in, for another under its type. */
function againOf(box) {
  const row = box.closest(".kind");
  if (!row.classList.contains("fresh")) return box.dataset.key;
  return JSON.stringify(["more", row.querySelector(".name").value.trim()]);
}

main.addEventListener("focusin", (event) => {
  if (event.target.closest(".types input")) editing = event.target.closest(".types");
});
// `focusout` keeps a field of `.types`, focusing `relatedTarget` once the page is
// drawn again; between `.fresh`'s two fields it keeps nothing.
main.addEventListener("focusout", (event) => {
  const box = event.target.closest(".types input");
  if (!box || drawing || !box.isConnected) return;
  const into = event.relatedTarget;
  const row = box.closest(".kind");
  if (row.classList.contains("fresh") && into && row.contains(into)) return;
  editing = null;
  keepType(box, into?.closest?.(".types input")?.dataset.key ?? null).catch(console.error);
});
// `mousedown` on the `.off` of the `.tag` holding `document.activeElement` keeps
// the focus there, the tag taken off as it stood.
main.addEventListener("mousedown", (event) => {
  const tag = event.target.closest(".types .tag");
  if (tag && event.target.closest(".off") && tag.contains(document.activeElement)) event.preventDefault();
});

/** `addTag` adds the tag `adding` holds to the bulk editor, waiting there. */
function addTag(adding) {
  const tag = tagOf(adding);
  if (!tag) return;
  sheetTag(adding.closest(".sheet"), tag, 0, "new");
  for (const box of adding.querySelectorAll("input")) box.value = "";
  adding.querySelector("input").focus();
}

/** `picked`: the tiles picked on Books. */
function picked() {
  return [...main.querySelectorAll("[data-pickable] .tile.picked")];
}

/** `pickedLast`: the tile last picked, which a click with `shiftKey` picks from. */
let pickedLast = null;

/** `drawPicked` sets `.chosen` in place of the shelf chips, stating how many are picked. */
function drawPicked() {
  const chosen = main.querySelector(".chosen");
  if (!chosen) return;
  const n = picked().length;
  chosen.hidden = n === 0;
  chosen.previousElementSibling.hidden = n > 0;
  chosen.querySelector("b").textContent = counted(chosen.dataset.said.replace("{n}", n), Number(chosen.dataset.total));
}

main.addEventListener("click", (event) => {
  const tile = event.target.closest("[data-pickable] .tile");
  if (!tile || !(event.metaKey || event.ctrlKey || event.shiftKey)) return;
  event.preventDefault();
  const tiles = [...main.querySelectorAll("[data-pickable] .tile")];
  if (event.shiftKey && pickedLast && tiles.includes(pickedLast)) {
    const [a, b] = [tiles.indexOf(pickedLast), tiles.indexOf(tile)].sort((x, y) => x - y);
    for (const each of tiles.slice(a, b + 1)) each.classList.add("picked");
  } else {
    tile.classList.toggle("picked");
  }
  pickedLast = tile;
  drawPicked();
});

document.addEventListener("keydown", (event) => {
  if (event.target.closest("input, textarea") || !main.querySelector("[data-pickable]")) return;
  if (event.key === "a" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    for (const tile of main.querySelectorAll("[data-pickable] .tile")) tile.classList.add("picked");
    drawPicked();
  } else if (event.key === "Escape" && picked().length) {
    for (const tile of picked()) tile.classList.remove("picked");
    drawPicked();
  }
});

/** `tagBox`: a field of the bulk editor's tags, `field` `type` or `tag`, holding `words`. */
function tagBox(field, words) {
  const box = document.createElement("input");
  box.className = "box";
  box.dataset.field = field;
  box.dataset.pick = `${field}s`;
  box.dataset.was = words;
  box.value = words;
  box.dataset.fit = "";
  box.autocomplete = "off";
  box.spellcheck = false;
  return box;
}

/** `sheetTag` sets `tag` in `sheet` under its type's `.group`, a field held by `n` of the books picked, or with `"new"` in `how` one waiting to be added. */
function sheetTag(sheet, tag, n, how = "") {
  const list = sheet.querySelector(".retag");
  const total = picked().length;
  let group = [...list.querySelectorAll(".group")].find((g) => g.dataset.type === tag.type);
  if (!group) {
    group = document.createElement("div");
    group.className = "group";
    group.dataset.type = tag.type;
    const kind = document.createElement("span");
    kind.className = "kind";
    if (how === "new") kind.textContent = tag.type;
    else kind.append(tagBox("type", tag.type));
    const under = document.createElement("span");
    under.className = "under";
    group.append(kind, under);
    list.append(group);
  }
  const tagged = document.createElement("span");
  tagged.className = `tagged ${how}`.trim();
  tagged.dataset.type = tag.type;
  tagged.dataset.tag = tag.tag;
  const box = tagBox("tag", tag.tag);
  box.disabled = how === "new";
  fitted(box);
  tagged.append(box);
  if (n && n < total) {
    const some = document.createElement("small");
    some.textContent = counted(sheet.dataset.some.replace("{n}", n), total);
    tagged.append(some);
  }
  const off = document.createElement("button");
  off.className = "off";
  off.dataset.act = "drop-tag";
  off.dataset.tip = list.dataset.off;
  off.textContent = "×";
  tagged.append(off);
  group.querySelector(".under").append(tagged);
}

/** `openSheet` opens the bulk editor on the books picked, each field's books stated and their tags listed. */
function openSheet() {
  const sheet = main.querySelector(".sheet");
  const tiles = picked();
  if (!sheet || tiles.length === 0) return;
  closeMeta();
  const values = tiles.map((t) => JSON.parse(t.dataset.values));
  const said = sheet.dataset;
  sheet.querySelector("h3").textContent = counted(said.head, tiles.length);
  sheet.querySelector('[data-act="apply-bulk"]').textContent = counted(said.apply, tiles.length);
  const fields = {
    authors: values.map((v) => v.authors.join(" & ")),
    publisher: values.map((v) => v.publisher ?? ""),
  };
  for (const [field, held] of Object.entries(fields)) {
    const box = sheet.querySelector(`input[data-field="${field}"]`);
    box.value = "";
    box.disabled = false;
    const distinct = [...new Set(held.filter((v) => v))];
    box.placeholder = distinct.length === 0
      ? said.none
      : distinct.length === 1
      ? `${said.leave} · ${distinct[0]}`
      : counted(said.values, distinct.length);
  }
  for (const back of sheet.querySelectorAll('[data-act="bulk-back"]')) back.classList.remove("on");
  for (const group of sheet.querySelectorAll(".retag .group")) group.remove();
  const counts = new Map();
  for (const v of values) {
    for (const tag of v.tags) {
      const key = JSON.stringify([tag.type, tag.tag]);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  }
  for (const [key, n] of counts) {
    const [type, tag] = JSON.parse(key);
    sheetTag(sheet, { type, tag }, n);
  }
  sheet.hidden = false;
  main.querySelector(".grid").classList.add("under");
  editing = sheet;
  sheet.scrollIntoView({ block: "nearest" });
}

/** `applySheet` applies `sheet` to the books picked, Undo offered. */
async function applySheet(sheet) {
  const patch = {};
  for (const box of sheet.querySelectorAll(":scope > .field input[data-field]")) {
    if (box.closest(".retag")) continue;
    const field = box.dataset.field;
    const back = sheet.querySelector(`[data-act="bulk-back"][data-field="${field}"]`).classList.contains("on");
    const value = box.value.trim();
    if (back) patch[field] = "back";
    else if (value) patch[field] = { to: value };
  }
  const list = sheet.querySelector(".retag");
  Object.assign(patch, retagsOf(list));
  const add = [...list.querySelectorAll(".tagged.new")].map((t) => {
    const kind = t.closest(".group").querySelector('input[data-field="type"]');
    return { type: kind?.value.trim() || t.dataset.type, tag: t.dataset.tag };
  });
  if (add.length) patch.add = add;
  const books = picked().map((t) => JSON.parse(t.dataset.book));
  if (Object.keys(patch).length === 0) return closeMeta();
  await patched(books, patch, true);
}

/** `rename` renames the value Books is narrowed to on every book shown, then follows it to its new name. */
async function rename(renaming) {
  const to = renaming.querySelector("input").value.trim();
  const by = JSON.parse(renaming.dataset.by);
  if (!to) return;
  const books = [...main.querySelectorAll("[data-pickable] .tile")].map((t) => JSON.parse(t.dataset.book));
  const undone = await call("rename_metadata", { books, by, to });
  closeMeta();
  const [path, query] = (location.hash.slice(1) || "/").split("?");
  const params = new URLSearchParams(query ?? "");
  params.set("is", to);
  history.replaceState(null, "", `#${path}?${params.toString()}`);
  await show();
  if (undone.length) offerUndo(undone);
}

document.addEventListener("click", async (event) => {
  const act = event.target.closest("[data-act]");
  if (!act) return;
  switch (act.dataset.act) {
    case "edit-meta": {
      closeAny();
      const meta = act.closest(".meta");
      meta.classList.add("open");
      meta.querySelector(".metaedit").hidden = false;
      main.querySelector(".hero")?.classList.add("editing");
      meta.querySelector(".metaedit input").focus();
      editing = meta;
      break;
    }
    case "cancel-meta":
    case "cancel-rename":
    case "cancel-bulk":
      closeMeta();
      break;
    case "save-meta":
      await saveMeta(act.closest(".meta"));
      break;
    case "revert-field":
      await patched([bookOf(act)], { [act.dataset.field]: "back" });
      break;
    case "save-tag":
      addTag(act.closest(".adding"));
      break;
    case "untag":
      await patched([bookOf(act)], { remove: [{ type: act.dataset.type, tag: act.dataset.tag }] });
      break;
    case "untag-every": {
      const from = { type: act.dataset.type, tag: act.dataset.tag };
      const undone = await call("retag_every", { from, to: null });
      follow(from, null);
      await show();
      if (undone.length) offerUndo(undone);
      break;
    }
    case "unpick":
      for (const tile of picked()) tile.classList.remove("picked");
      drawPicked();
      break;
    case "bulk":
      openSheet();
      break;
    case "bulk-back": {
      act.classList.toggle("on");
      const box = act.closest(".field").querySelector("input");
      box.disabled = act.classList.contains("on");
      if (box.disabled) box.value = "";
      break;
    }
    case "drop-tag": {
      const tagged = act.closest(".tagged");
      const group = tagged.closest(".group");
      if (!tagged.classList.contains("new")) tagged.classList.toggle("gone");
      else tagged.remove();
      if (!group.querySelector(".tagged")) group.remove();
      break;
    }
    case "whole": {
      const chart = act.closest("[data-chart]");
      const open = !whole.has(chart.dataset.chart);
      if (open) whole.add(chart.dataset.chart);
      else whole.delete(chart.dataset.chart);
      openWhole(chart, open);
      break;
    }
    case "apply-bulk":
      await applySheet(act.closest(".sheet"));
      break;
    case "rename": {
      closeAny();
      const renaming = main.querySelector(".renaming");
      renaming.hidden = false;
      const box = renaming.querySelector("input");
      box.focus();
      box.select();
      editing = renaming;
      break;
    }
    case "save-rename":
      await rename(act.closest(".renaming"));
      break;
    case "suggest": {
      const each = JSON.parse(act.dataset.each);
      const undone = await call("edit_metadata_each", { each });
      await show();
      if (undone.length) offerUndo(undone);
      break;
    }
  }
});

/** `composing`: whether an input method is composing text, which `show` throws away with the box it is typed in. */
let composing = false;

// `[data-search]` narrows its own route, `location.hash` taking `q` in place from
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

/** `watching`: the observers keeping a book page's `.slim` bar in step with the scroll. */
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

/** `synced`: the Kindle `sync` last ran for, while it stays plugged in. */
let synced = null;

/** `card`: the Kindle card's line for the cut serial `serial`, `busy` while a sync runs, `wrong` once one stopped. */
let card = { serial: null, line: "", busy: false, wrong: false };

/** `drawCard` sets `card` on the Kindle card drawn for its serial, and answers whether one is drawn. */
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

/** `sync` syncs the Kindle `serial` names as `sync_kindle` does, its card told each half, flashing what came in and drawing again where it says anything; `quiet` says nothing where nothing came in. */
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

/** `resync`: a quiet `sync` of the Kindle plugged in, once the window rests after a command of `WRITING`. */
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
