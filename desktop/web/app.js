// The window over the app's commands: Books, a book, Posts, Import and
// Publish. Every change goes to a command as it is made; nothing is held here
// but what the screen showing needs.

const TAURI = window.__TAURI__;

const main = document.getElementById("main");

const state = {
  ws: null,
  screen: "books",
  filter: "all",
  // The book open, as `books` listed it.
  book: null,
  // The post open, by its file's name.
  post: null,
  // What has been imported since the window opened, newest first.
  imports: [],
  // The last check of what becomes public.
  plan: null,
};

/** `tag` with `attrs` set and `kids` appended; a string kid is text. */
function el(tag, attrs = {}, ...kids) {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (name.startsWith("on")) node.addEventListener(name.slice(2), value);
    else if (name === "class") node.className = value;
    else if (name === "value") node.value = value;
    else if (value === true) node.setAttribute(name, "");
    else node.setAttribute(name, value);
  }
  for (const kid of kids.flat()) {
    if (kid === null || kid === undefined || kid === false) continue;
    node.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return node;
}

/** `said` at the foot of the window for a while; `wrong` marks a failure. */
function flash(said, wrong = false) {
  const at = document.getElementById("flash");
  at.textContent = said;
  at.className = wrong ? "wrong" : "";
  at.hidden = false;
  clearTimeout(flash.timer);
  flash.timer = setTimeout(() => (at.hidden = true), wrong ? 9000 : 3500);
}

/** The command `name`'s answer. What went wrong is flashed and thrown on. */
async function call(name, args = {}) {
  try {
    return await TAURI.core.invoke(name, args);
  } catch (err) {
    flash(String(err), true);
    throw err;
  }
}

/** A path on disk as a URL the window can load. */
function fileUrl(path) {
  return TAURI.core.convertFileSrc(path);
}

/** `count` and `one`, with an `s` past one. */
function counted(count, one) {
  return `${count} ${one}${count === 1 ? "" : "s"}`;
}

/** Each line of `problems` in a box, or nothing where there are none. */
function problemsBox(problems) {
  if (!problems || problems.length === 0) return null;
  return el("div", { class: "problems" }, problems.map((p) => el("p", {}, p)));
}

const screens = {
  books: showBooks,
  book: showBook,
  posts: showPosts,
  import: showImport,
  publish: showPublish,
};

async function go(screen) {
  state.screen = screen;
  const lit = screen === "book" ? "books" : screen;
  for (const button of document.querySelectorAll("#nav button")) {
    button.classList.toggle("on", button.dataset.screen === lit);
  }
  main.className = "";
  main.replaceChildren();
  try {
    await screens[screen]();
  } catch (err) {
    console.error(err);
  }
}

// ---- The workspace ----------------------------------------------------

function welcome() {
  document.body.classList.add("bare");
  document.getElementById("side").hidden = true;
  main.className = "";
  main.replaceChildren(
    el("h1", {}, "Reading Log"),
    el(
      "p",
      { class: "lede" },
      "Pick a folder to work in. It keeps every backup you import, the " +
        "record they fold into, the books and highlights you choose to show, " +
        "and your posts. Nothing in it is published: the site is written to " +
        "a folder of its own.",
    ),
    el("button", { type: "button", class: "primary", onclick: pickWorkspace }, "Choose a folder…"),
  );
}

async function pickWorkspace() {
  const dir = await call("pick_folder", { title: "A folder to work in" });
  if (!dir) return;
  state.ws = await call("open_workspace", { dir });
  enter();
}

function enter() {
  document.body.classList.remove("bare");
  document.getElementById("side").hidden = false;
  document.getElementById("where").textContent = state.ws.dir;
  go("books");
}

// ---- Books ----------------------------------------------------------------

async function showBooks() {
  const rows = await call("books");
  const shown = rows.filter((r) => r.shown).length;
  const unchosen = rows.filter((r) => !r.chosen).length;
  const filters = [
    ["all", "All", () => true],
    ["shown", "On the site", (r) => r.shown],
    ["hidden", "Off the site", (r) => !r.shown],
    ["new", "Not chosen yet", (r) => !r.chosen],
  ];
  const pass = (filters.find(([key]) => key === state.filter) ?? filters[0])[2];
  const chips = el(
    "div",
    { class: "row chips" },
    filters.map(([key, name]) =>
      el("button", {
        type: "button",
        class: key === state.filter ? "on" : "",
        onclick: () => {
          state.filter = key;
          go("books");
        },
      }, name)
    ),
  );
  const listed = rows.filter(pass);
  main.replaceChildren(
    el("h1", {}, "Books"),
    el(
      "p",
      { class: "lede" },
      `${counted(rows.length, "book")} in the record · ${shown} on the site · ` +
        `${unchosen} not chosen yet, which follow the site's default`,
    ),
    el(
      "div",
      { class: "row" },
      chips,
      el("button", { type: "button", class: "quiet", onclick: () => showAll(true) }, "Show them all"),
      el("button", { type: "button", class: "quiet", onclick: () => showAll(false) }, "Hide them all"),
    ),
    listed.length === 0 ? el("p", { class: "dim" }, "No books here.") : el(
      "table",
      { class: "books" },
      el(
        "thead",
        {},
        el(
          "tr",
          {},
          el("th", {}, "Shown"),
          el("th", {}, "Title"),
          el("th", {}, "Read"),
          el("th", {}, "Progress"),
          el("th", {}, "Highlights"),
        ),
      ),
      el("tbody", {}, listed.map(bookRow)),
    ),
  );
}

async function showAll(shown) {
  await call("show_all", { shown });
  go("books");
}

function progressOf(row) {
  if (row.finished) return el("span", { class: "badge ink" }, "Finished");
  return row.percent === null ? "" : `${row.percent}%`;
}

function bookRow(row) {
  return el(
    "tr",
    {},
    el(
      "td",
      {},
      el("input", {
        type: "checkbox",
        checked: row.shown,
        title: row.chosen ? null : "Not chosen yet: it follows the site's default",
        onchange: async (event) => {
          await call("change_book", {
            key: row.key,
            extent: row.extent,
            change: { shown: event.target.checked },
          });
          go("books");
        },
      }),
    ),
    el(
      "td",
      {},
      el("button", {
        type: "button",
        class: "title",
        lang: row.language || null,
        onclick: () => {
          state.book = row;
          go("book");
        },
      }, row.title),
      el("div", { class: "dim" }, row.author),
    ),
    el("td", { class: "figure" }, row.read),
    el("td", { class: "figure" }, progressOf(row)),
    el("td", { class: "figure" }, row.highlights ? String(row.highlights) : ""),
  );
}

// ---- A book -----------------------------------------------------------------

async function showBook() {
  const row = (await call("books")).find(
    (r) => r.key === state.book.key && r.extent === state.book.extent,
  );
  if (!row) return go("books");
  state.book = row;
  main.className = "split";
  const left = el("div", { class: "pane" });
  const right = el("div", { class: "pane" });
  main.replaceChildren(left, right);

  const change = async (what) => {
    await call("change_book", { key: row.key, extent: row.extent, change: what });
    await bookPage(right, row);
  };
  const title = el("input", {
    type: "text",
    value: row.title === row.record_title ? "" : row.title,
    placeholder: row.record_title,
  });
  const author = el("input", {
    type: "text",
    value: row.author === row.record_author ? "" : row.author,
    placeholder: row.record_author,
  });
  const rows = await call("highlights", { key: row.key, extent: row.extent });
  left.replaceChildren(
    el("button", { type: "button", class: "quiet", onclick: () => go("books") }, "‹ Books"),
    el("h1", { lang: row.language || null }, row.title),
    el("p", { class: "lede" }, row.author),
    el(
      "label",
      { class: "check" },
      el("input", {
        type: "checkbox",
        checked: row.shown,
        onchange: (event) => change({ shown: event.target.checked }),
      }),
      "On the site",
    ),
    el("h2", {}, "Its name on the site"),
    el("label", { class: "field" }, el("span", {}, "Title"), title),
    el("label", { class: "field" }, el("span", {}, "Author"), author),
    el("button", {
      type: "button",
      onclick: async () => {
        await change({ title: title.value, author: author.value });
        flash("Saved. An empty field gives the record's back.");
      },
    }, "Save the names"),
    el("h2", {}, `Highlights · ${rows.length}`),
    rows.length === 0 ? el("p", { class: "dim" }, "No highlights in this book.") : el(
      "label",
      { class: "field" },
      el("span", {}, "Show"),
      el(
        "select",
        {
          onchange: async (event) => {
            await change({ highlights: event.target.value });
            go("book");
          },
        },
        el("option", { value: "default", selected: row.highlights_shown !== true }, "Only those ticked"),
        el("option", { value: "shown", selected: row.highlights_shown === true }, "All but those unticked"),
      ),
    ),
    el("ol", { class: "highlights" }, rows.map((h) => highlightRow(h, row, right))),
  );
  await bookPage(right, row);
}

/** One highlight, its note, and the edits on both. */
function highlightRow(h, book, page) {
  const lang = book.language || null;
  const text = el("textarea", { rows: 3, placeholder: h.passage.body }, h.passage.text ?? "");
  const comment = el(
    "textarea",
    { rows: 2, placeholder: "Your words on it, in Markdown" },
    h.passage.comment ?? "",
  );
  const noteText = h.note ? el("textarea", { rows: 2, placeholder: h.note.body }, h.note.text ?? "") : null;
  const save = async () => {
    await call("change_highlight", {
      id: h.passage.id,
      change: { text: text.value, comment: comment.value },
    });
    if (h.note) {
      await call("change_highlight", { id: h.note.id, change: { text: noteText.value } });
    }
    flash("Saved. An empty field gives the record's words back.");
    await bookPage(page, book);
  };
  const show = async (event) => {
    const shown = event.target.checked ? "shown" : "hidden";
    await call("change_highlight", { id: h.passage.id, change: { shown } });
    if (h.note) await call("change_highlight", { id: h.note.id, change: { shown } });
    await bookPage(page, book);
  };
  return el(
    "li",
    {},
    el(
      "label",
      { class: "check" },
      el("input", { type: "checkbox", checked: h.passage.shown, onchange: show }),
      el("span", { class: "dim" }, `${h.label} · ${h.made}`),
    ),
    el("blockquote", { lang }, h.passage.text ?? h.passage.body),
    h.note ? el("blockquote", { class: "note" }, h.note.text ?? h.note.body) : null,
    el(
      "details",
      {},
      el("summary", {}, "Correct it, or comment"),
      el("label", { class: "field" }, el("span", {}, "The words, corrected"), text),
      noteText ? el("label", { class: "field" }, el("span", {}, "The note, corrected"), noteText) : null,
      el("label", { class: "field" }, el("span", {}, "Comment"), comment),
      el("button", { type: "button", onclick: save }, "Save"),
    ),
  );
}

/** The book's page as the site states it, built afresh into the preview. */
async function bookPage(pane, row) {
  const preview = await call("preview");
  const placed = preview.books.find((b) => b.key === row.key && b.extent === row.extent);
  pane.replaceChildren(
    problemsBox(preview.problems),
    placed
      ? el("iframe", {
        class: "page",
        title: "The book's page",
        src: fileUrl(`${preview.dir}/books/${placed.slug}/index.html`),
      })
      : el("p", { class: "dim" }, "Off the site: it has no page."),
  );
}

// ---- Posts --------------------------------------------------------------------

async function showPosts() {
  main.className = "split";
  const left = el("div", { class: "pane" });
  const right = el("div", { class: "pane" });
  main.replaceChildren(left, right);
  const { rows, problems } = await call("posts");
  const books = await call("books");
  const title = el("input", { type: "text", placeholder: "Its title" });
  const about = el(
    "select",
    {},
    el("option", { value: "" }, "About no one book"),
    books.filter((b) => b.shown).map((b) => el("option", { value: b.key || String(b.extent) }, b.title)),
  );
  left.replaceChildren(
    el("h1", {}, "Posts"),
    el(
      "p",
      { class: "lede" },
      "A review is a post about one book. A new post starts as a draft, which stays off the site.",
    ),
    el("label", { class: "field" }, el("span", {}, "Title"), title),
    el("label", { class: "field" }, el("span", {}, "About"), about),
    el("button", {
      type: "button",
      onclick: async () => {
        if (!title.value.trim()) return flash("A post needs a title.", true);
        state.post = await call("new_post", {
          title: title.value.trim(),
          books: about.value ? [about.value] : [],
        });
        go("posts");
      },
    }, "Start a draft"),
    problemsBox(problems),
    el("h2", {}, counted(rows.length, "post")),
    el(
      "ul",
      { class: "posts" },
      rows.map((p) =>
        el(
          "li",
          {},
          el(
            "button",
            {
              type: "button",
              class: p.name === state.post ? "on" : "",
              onclick: () => {
                state.post = p.name;
                go("posts");
              },
            },
            el("strong", {}, p.title),
            " ",
            el("span", { class: "dim" }, p.date),
            p.draft ? [" ", el("span", { class: "badge" }, "draft")] : null,
          ),
        )
      ),
    ),
  );
  if (state.post) await postEditor(right, state.post);
}

/** The post `name` in full, with the page its words make beside it. */
async function postEditor(pane, name) {
  const text = el("textarea", { class: "post", spellcheck: true }, await call("read_post", { name }));
  const page = el("iframe", { class: "page", title: "The post as the site states it" });
  const preview = await call("preview");
  const render = async () => {
    try {
      const done = await TAURI.core.invoke("render_post", { name, text: text.value });
      const style = fileUrl(`${preview.dir}/style.css`);
      page.srcdoc = `<!doctype html><html><head><meta charset="utf-8">` +
        `<link rel="stylesheet" href="${style}"></head><body><main>` +
        `<article class="post"><h1></h1><div class="text">${done.html}</div></article>` +
        `</main></body></html>`;
      page.onload = () => {
        const head = page.contentDocument?.querySelector("h1");
        if (head) head.textContent = done.title;
      };
    } catch (err) {
      page.srcdoc = `<p>${String(err).replace(/[&<>]/g, (c) => `&#${c.charCodeAt(0)};`)}</p>`;
    }
  };
  let timer = null;
  text.addEventListener("input", () => {
    clearTimeout(timer);
    timer = setTimeout(render, 400);
  });
  pane.replaceChildren(
    el(
      "div",
      { class: "row" },
      el("button", {
        type: "button",
        class: "primary",
        onclick: async () => {
          await call("write_post", { name, text: text.value });
          flash("Saved.");
          go("posts");
        },
      }, "Save"),
      el("span", { class: "dim" }, `posts/${name}.md · set draft = false to put it on the site`),
    ),
    text,
    page,
  );
  await render();
}

// ---- Import -------------------------------------------------------------------

function showImport() {
  main.replaceChildren(
    el("h1", {}, "Import"),
    el(
      "p",
      { class: "lede" },
      "A backup is kept whole under backups/, then folded into the record the " +
        "way Restore folds one on the Kindle: what the record holds already " +
        "stands, so nothing is counted twice and an import never takes a row away.",
    ),
    el("div", { class: "drop", id: "drop" }, "Drop a backup (.zip), or a Kindle's drive, here"),
    el(
      "div",
      { class: "row" },
      el("button", {
        type: "button",
        onclick: async () => {
          const path = await call("pick_backup");
          if (path) await importPath(path);
        },
      }, "Choose a backup…"),
      el("button", {
        type: "button",
        onclick: async () => {
          const path = await call("pick_folder", { title: "A Kindle's drive, or its readinglog folder" });
          if (path) await importPath(path);
        },
      }, "Choose a Kindle…"),
    ),
    el("h2", {}, "Imported"),
    state.imports.length === 0
      ? el("p", { class: "dim" }, "Nothing since the window opened.")
      : el("ul", { class: "plain" }, state.imports.map((done) => el("li", {}, importSaid(done)))),
  );
}

function importSaid(done) {
  const added = `${counted(done.sittings, "sitting")}, ${counted(done.books, "book")}, ` +
    `${counted(done.highlights, "highlight")} and ${counted(done.pictures, "cover")} added`;
  const about = [done.kindle, done.firmware, done.first && done.last ? `${done.first} to ${done.last}` : ""]
    .filter((s) => s)
    .join(" · ");
  return [
    el("strong", {}, done.file),
    done.again ? " (imported before)" : "",
    `: ${added}.`,
    about ? el("div", { class: "dim" }, about) : null,
  ];
}

async function importPath(path) {
  const done = await call("import", { path });
  state.imports.unshift(done);
  flash(`${done.file}: ${counted(done.sittings, "sitting")} added.`);
  if (state.screen === "import") showImport();
}

/** A file dropped on the window, while the Import screen stands. */
function dropped(event) {
  const zone = document.getElementById("drop");
  const payload = event.payload;
  if (!zone) return;
  if (payload.type === "over" || payload.type === "enter") zone.classList.add("over");
  else zone.classList.remove("over");
  if (payload.type !== "drop") return;
  (async () => {
    for (const path of payload.paths ?? []) {
      try {
        await importPath(path);
      } catch (err) {
        console.error(err);
      }
    }
  })();
}

// ---- Publish ------------------------------------------------------------------

async function showPublish() {
  state.ws = await call("workspace");
  const offered = await call("offered");
  const site = { ...state.ws.site };
  const input = (key, label, hint) =>
    el(
      "label",
      { class: "field" },
      el("span", {}, label),
      el("input", {
        type: "text",
        value: site[key] ?? "",
        placeholder: hint ?? null,
        oninput: (event) => (site[key] = event.target.value),
      }),
    );
  const select = (key, label, picks) =>
    el(
      "label",
      { class: "field" },
      el("span", {}, label),
      el(
        "select",
        { onchange: (event) => (site[key] = event.target.value) },
        picks.map((p) => el("option", { value: p.token, selected: p.token === site[key] }, p.label)),
      ),
    );
  const result = el("div", {});
  main.replaceChildren(
    el("h1", {}, "Publish"),
    el("p", { class: "lede" }, `The site is written to ${state.ws.out}.`),
    el("h2", {}, "The site"),
    el(
      "form",
      { class: "site", onsubmit: (event) => event.preventDefault() },
      input("title", "Its name"),
      input("author", "Whose reading it is"),
      el(
        "div",
        { class: "wide" },
        input("url", "Where it is served from, for the feed", "https://you.github.io/reading/"),
      ),
      el(
        "div",
        { class: "wide" },
        input("out", "The folder it is written to", "site"),
        el("button", {
          type: "button",
          class: "quiet",
          onclick: async () => {
            const dir = await call("pick_folder", { title: "The folder the site is written to" });
            if (dir) {
              site.out = dir;
              await saveSite(site);
              go("publish");
            }
          },
        }, "Choose a folder…"),
      ),
      select("new_books", "A book not chosen yet", [
        { token: "shown", label: "Is on the site" },
        { token: "hidden", label: "Stays off it" },
      ]),
      offered.map((setting) => select(setting.key, setting.label, setting.picks)),
      el(
        "div",
        { class: "wide" },
        el("button", {
          type: "button",
          onclick: async () => {
            await saveSite(site);
            go("publish");
          },
        }, "Save"),
      ),
    ),
    problemsBox(state.ws.problems),
    el("h2", {}, "What becomes public"),
    el(
      "p",
      { class: "warning" },
      "GitHub Pages serves a free account's site from a public repository, and git keeps " +
        "every version ever pushed: something public once stays in the history after it is hidden.",
    ),
    el(
      "div",
      { class: "row" },
      el("button", {
        type: "button",
        onclick: async () => {
          state.plan = await call("plan");
          result.replaceChildren(planned(state.plan));
        },
      }, "Check"),
      el("button", {
        type: "button",
        class: "primary",
        onclick: async () => {
          const done = await call("publish");
          state.plan = null;
          result.replaceChildren(published(done));
        },
      }, "Write the site"),
    ),
    result,
  );
  if (state.plan) result.replaceChildren(planned(state.plan));
}

async function saveSite(site) {
  state.ws = await call("save_site", { site });
  flash("Saved.");
}

function planned(plan) {
  const of = (kind) => plan.fresh.filter((f) => f.kind === kind);
  const group = (name, items) =>
    items.length === 0 ? null : [
      el("h2", {}, `${name} · ${items.length}`),
      el("ul", { class: "plain" }, items.map((i) => el("li", {}, i))),
    ];
  return el(
    "div",
    {},
    plan.first ? el("p", {}, `Nothing has been written to ${plan.out} yet: all of it becomes public.`) : null,
    plan.fresh.length === 0 ? el("p", {}, "Nothing becomes public that is not public already.") : null,
    group("Books", of("book").map((f) => f.said)),
    group("Highlights", of("highlight").map((f) => `${f.book}: ${f.said || "(no words: a sidecar's own)"}`)),
    group("Posts", of("post").map((f) => f.said)),
    plan.leaving
      ? el("p", {}, `${counted(plan.leaving, "thing")} on the site now will be taken off it.`)
      : null,
    problemsBox(plan.problems),
  );
}

function published(done) {
  return el(
    "div",
    {},
    el(
      "p",
      {},
      `Written to ${done.out}: ${counted(done.changed, "file")} written, ` +
        `${done.removed} taken away, ${done.unchanged} as they were.`,
    ),
    problemsBox(done.problems),
    el(
      "div",
      { class: "row" },
      el(
        "button",
        { type: "button", onclick: () => call("open_path", { path: done.index }) },
        "Open it in the browser",
      ),
      el(
        "button",
        { type: "button", class: "quiet", onclick: () => call("reveal", { path: done.index }) },
        "Show the folder",
      ),
    ),
  );
}

// ---- Start --------------------------------------------------------------------

document.getElementById("nav").addEventListener("click", (event) => {
  const screen = event.target.closest("button")?.dataset.screen;
  if (screen) go(screen);
});
document.getElementById("switch").addEventListener("click", pickWorkspace);
TAURI.webview.getCurrentWebview().onDragDropEvent(dropped);

(async () => {
  state.ws = await call("workspace");
  if (state.ws) enter();
  else welcome();
})();
