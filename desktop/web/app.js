// The window's shell: `show` sets in place the route the hash names, as the
// `page` command draws it, covers through the asset protocol and a chart's
// figures as tooltips; the listeners carry the clicks, searches and drops.

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

/** The route the hash names, drawn at the width of the page column. */
async function show() {
  const route = location.hash.slice(1) || "/";
  const width = main.clientWidth;
  const typing = document.activeElement?.dataset?.search ? document.activeElement.selectionStart : null;
  const page = await call("page", { route, width });
  document.getElementById("vars").textContent = page.vars;
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
      await bring([act.dataset.root]);
      break;
    case "reveal":
      await call("reveal", { path: act.dataset.path });
      break;
    case "folder":
      if (await call("choose_folder")) await show();
      break;
  }
});

document.addEventListener("click", async (event) => {
  const pick = event.target.closest("[data-set]");
  if (!pick) return;
  await call("set_setting", { key: pick.dataset.set, value: pick.dataset.value });
  await show();
});

// `[data-search]` narrows its own route: the hash takes `q`, in place.
main.addEventListener(
  "input",
  settled((event) => {
    const box = event.target.closest("[data-search]");
    if (!box) return;
    const [path, query] = (location.hash.slice(1) || "/").split("?");
    const params = new URLSearchParams(query ?? "");
    if (box.value) params.set("q", box.value);
    else params.delete("q");
    const said = params.toString();
    history.replaceState(null, "", `#${path}${said ? `?${said}` : ""}`);
    show();
  }, 220),
);

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

// ← and → step a span or a day, through the arrows the page draws.
document.addEventListener("keydown", (event) => {
  if (event.target.closest("input") || event.metaKey || event.ctrlKey || event.altKey) return;
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
    if (Math.abs(main.clientWidth - drawn.width) > 8) show().catch(console.error);
  }, 150),
);

// `poll` draws the sidebar again where `kindle` changes: a Kindle plugged in or out.
let kindle;
async function poll() {
  const now = await TAURI.core.invoke("kindle").catch(() => null);
  if (kindle !== undefined && now !== kindle) show().catch(console.error);
  kindle = now;
}
setInterval(poll, 3000);
poll();

show().catch(console.error);
