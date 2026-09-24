// The browser half of the finder demo. It follows internal/finder closely
// enough to show how gm behaves — the ranking is a port of score.go and the
// list hangs from the prompt the same way — but it only ever moves between
// rows of sample data: nothing is cloned, created, removed or opened.
import type { DemoCommit, DemoRepo } from "./demo";

interface Payload {
  root: string;
  repos: DemoRepo[];
  themes: Record<string, Record<string, string>>;
}

interface Item {
  label: string;
  path: string;
  score: number; // frecency; zero for worktrees
  repo: DemoRepo;
  branch: string;
  dirty: number;
  commits: DemoCommit[];
}

// ---- score.go ----------------------------------------------------------

const bonusBoundary = 8;
const bonusConsecutive = 6;
const bonusNameSeg = 10;
const bonusUserSeg = 3;
const penaltyHostSeg = 4;
const bonusWholeInName = 25;
const penaltyGapStart = 3;
const penaltyGapChar = 1;
const maxGapChars = 20;

const isBoundary = (c: string) => c === "/" || c === "-" || c === "_" || c === ".";

function matchScore(text: string, idx: number[]): number {
  if (idx.length === 0) return 0;
  const name = text.lastIndexOf("/") + 1;
  let user = 0;
  if (name > 1) user = text.lastIndexOf("/", name - 2) + 1;

  let score = 0;
  let prev = -2;
  let wholeInName = true;
  for (const i of idx) {
    const inHost = i < user;
    if (i >= name) score += bonusNameSeg;
    else if (i >= user) {
      score += bonusUserSeg;
      wholeInName = false;
    } else {
      score -= penaltyHostSeg;
      wholeInName = false;
    }
    if (!inHost && (i === 0 || isBoundary(text[i - 1]))) score += bonusBoundary;
    if (i === prev + 1) score += bonusConsecutive;
    else if (prev >= 0) score -= penaltyGapStart + penaltyGapChar * Math.min(i - prev - 1, maxGapChars);
    prev = i;
  }
  if (wholeInName) score += bonusWholeInName;
  return score;
}

function substringMatch(text: string, lowerQuery: string): number[] | null {
  const at = text.toLowerCase().indexOf(lowerQuery);
  if (at < 0) return null;
  return [...lowerQuery].map((_, i) => at + i);
}

// The candidate finder: every query character, in order, anywhere in the
// text. gm uses sahilm/fuzzy for this step; a left-to-right scan is the same
// idea and finds the same candidates.
function subsequence(text: string, lowerQuery: string): number[] | null {
  const lower = text.toLowerCase();
  const out: number[] = [];
  let from = 0;
  for (const ch of lowerQuery) {
    const at = lower.indexOf(ch, from);
    if (at < 0) return null;
    out.push(at);
    from = at + 1;
  }
  return out;
}

// ---- command.go ---------------------------------------------------------

const COMMANDS: { name: string; arg: string; what: string }[] = [
  { name: "/help", arg: "", what: "show this list" },
  { name: "/create", arg: "<repo>|<branch>", what: "create a repository, or a worktree" },
  { name: "/get", arg: "<repo>", what: "clone a repository, then go there" },
  { name: "/remove", arg: "", what: "remove the selected repository or worktree" },
  { name: "/worktrees", arg: "", what: "list the worktrees of the selected repository" },
  { name: "/branches", arg: "", what: "list the branches of the selected repository" },
  { name: "/remote", arg: "", what: "open the selected repository's remote in a browser" },
  { name: "/dirty", arg: "", what: "show only repositories with uncommitted work" },
];

// A command is the whole input, or follows the query after ";": "gm;/remote".
function splitInput(s: string): [query: string, cmd: string, ok: boolean] {
  if (s.trimStart().startsWith("/")) return ["", s, true];
  const i = s.indexOf(";");
  return i < 0 ? [s, "", false] : [s.slice(0, i), s.slice(i + 1), true];
}
const isCommand = (s: string) => splitInput(s)[2];

// ---- helpers ------------------------------------------------------------

const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const span = (cls: string, text: string) => `<span class="${cls}">${esc(text)}</span>`;

function browseURL(remote: string): string {
  const scp = remote.match(/^git@([^:]+):(.+?)(\.git)?$/);
  if (scp) return `https://${scp[1]}/${scp[2]}`;
  return remote.replace(/\.git$/, "");
}

// ---- the finder ---------------------------------------------------------

export function mount(root: HTMLElement) {
  const data: Payload = JSON.parse(root.querySelector("script[type='application/json']")!.textContent!);
  const listEl = root.querySelector<HTMLElement>(".list")!;
  const infoEl = root.querySelector<HTMLElement>(".info")!;
  const hintsEl = root.querySelector<HTMLElement>(".hints")!;
  const overlayEl = root.querySelector<HTMLElement>(".overlay")!;
  const input = root.querySelector<HTMLInputElement>(".box input")!;
  const select = root.querySelector<HTMLSelectElement>(".theme-pick select")!;
  const screen = root.querySelector<HTMLElement>(".screen")!;

  // Kept in step with --rows in FinderDemo.astro: the list draws as many rows
  // as the pane is tall.
  const ROWS = 22;

  const repoItems: Item[] = data.repos
    .map((r) => ({
      label: r.rel,
      path: `${data.root}/${r.rel}`,
      score: r.frecency,
      repo: r,
      branch: r.branch,
      dirty: r.dirty,
      commits: r.commits,
    }))
    .sort((a, b) => a.score - b.score || a.label.localeCompare(b.label));

  const st = {
    all: repoItems,
    view: [] as number[],
    matched: new Map<number, number[]>(),
    cursor: 0,
    query: "",
    mode: "repos" as "repos" | "worktrees",
    origin: "",
    saved: null as null | { view: number[]; matched: Map<number, number[]>; cursor: number; query: string },
    dirtyOnly: false,
    help: false,
    note: "",
    noteCls: "dirty",
  };

  function filter() {
    st.matched = new Map();
    st.note = "";
    const q = splitInput(input.value.trim())[0].trim();
    const hold = st.view.length > 0 && (q === st.query || q === "");
    const held = hold ? st.view[st.cursor] : -1;
    st.query = q;

    let view: number[];
    if (q === "") {
      view = st.all.map((_, i) => i);
    } else {
      const lower = q.toLowerCase();
      const score = new Map<number, number>();
      view = [];
      st.all.forEach((it, i) => {
        const pos = substringMatch(it.label, lower) ?? subsequence(it.label, lower);
        if (!pos) return;
        score.set(i, matchScore(it.label, pos));
        st.matched.set(i, pos);
        view.push(i);
      });
      // Ascending, so the strongest match lands at the bottom; frecency
      // breaks ties between equally good matches.
      view.sort((a, b) => score.get(a)! - score.get(b)! || st.all[a].score - st.all[b].score);
    }
    if (st.dirtyOnly && st.mode === "repos") view = view.filter((i) => st.all[i].dirty > 0);
    st.view = view;

    const at = held >= 0 ? view.indexOf(held) : -1;
    st.cursor = at >= 0 ? at : view.length - 1;
  }

  // ---- drawing ----

  let charW = 0;
  function measure() {
    const probe = document.createElement("span");
    probe.textContent = "x".repeat(40);
    probe.style.visibility = "hidden";
    probe.style.position = "absolute";
    listEl.appendChild(probe);
    charW = probe.getBoundingClientRect().width / 40 || 8;
    probe.remove();
  }

  function highlight(label: string, hits: number[], width: number): string {
    let off = 0;
    let prefix = "";
    let text = label;
    if (label.length > width) {
      off = label.length - width + 1;
      prefix = "…";
      text = label.slice(off);
    }
    const isHit = new Set(hits.map((h) => h - off));
    let out = esc(prefix);
    for (let i = 0; i < text.length; i++) out += isHit.has(i) ? span("hit", text[i]) : esc(text[i]);
    return out;
  }

  function drawList() {
    if (!charW) measure();
    const width = Math.max(Math.floor(listEl.clientWidth / charW) - 2, 8);
    const start = Math.max(0, Math.min(st.view.length - ROWS, st.cursor));
    const end = Math.min(start + ROWS, st.view.length);
    let html = "";
    for (let i = start; i < end; i++) {
      const sel = i === st.cursor;
      const idx = st.view[i];
      html +=
        `<div class="row${sel ? " sel" : ""}" role="option" aria-selected="${sel}" data-i="${i}">` +
        (sel ? span("mk", "▸ ") : "  ") +
        highlight(st.all[idx].label, st.matched.get(idx) ?? [], width) +
        `</div>`;
    }
    listEl.innerHTML = html;
  }

  function refs(c: DemoCommit): string {
    if (c.refs.length === 0) return "";
    const parts = c.refs.map((r) => span(`ref-${r.kind}`, r.name)).join(span("punct", ", "));
    return " " + span("punct", "(") + parts + span("punct", ")");
  }

  function drawInfo() {
    const idx = st.view[st.cursor];
    if (idx === undefined) {
      infoEl.innerHTML = span("dim", "no match");
      return;
    }
    const it = st.all[idx];
    const field = (k: string, v: string, cls: string) =>
      `<div>${span("lbl", k)}</div><div>${span(v ? cls : "dim", v || "-")}</div>`;

    let html = `<div>${span("name", it.label)}</div>`;
    if (st.mode === "worktrees") html += `<div>${span("dim", st.origin)}</div>`;
    html += "<div>&nbsp;</div>";
    html += field("path", it.path, "path");
    if (st.mode === "repos") html += field("remote", it.repo.remote, "remote");
    html += field("branch", it.branch, "branch");
    html += it.dirty > 0 ? field("status", `${it.dirty} changed`, "dirty") : field("status", "clean", "clean");
    if (st.mode === "repos") {
      html +=
        it.repo.visits > 0
          ? field("visits", `${it.repo.visits}, last ${it.repo.last}`, "visits")
          : field("visits", "never", "dim");
    }
    const label = it.commits.length === 1 ? "last commit" : `last ${it.commits.length} commits`;
    html += `<div>${span("lbl", label)}</div>`;
    // A commit that does not fit folds under itself, indented, the way
    // wrapSegs lays it out in the finder.
    for (const c of it.commits)
      html += `<div class="commit">${span("hash", c.hash)}${refs(c)} ${span("subject", c.subject)}</div>`;
    infoEl.innerHTML = html;
  }

  const hintKey = (key: string, what: string, action?: string) =>
    (action ? `<button type="button" class="key" data-act="${action}">${esc(key)}</button>` : span("key", key)) +
    esc(" " + what);

  function drawHints() {
    const sep = esc("  ·  ");
    if (st.note) {
      hintsEl.innerHTML = span(st.noteCls, st.note);
      return;
    }
    if (isCommand(input.value)) {
      hintsEl.innerHTML = esc("enter runs the command  ·  tab completes it  ·  ") + span("key", "/help") + esc(" lists them");
      return;
    }
    let out = "quit";
    if (input.value !== "") out = "clear";
    else if (st.dirtyOnly) out = "show all";

    const hints =
      st.mode === "worktrees"
        ? [hintKey("↑↓ ctrl-p/n", "move"), hintKey("enter", "jump"), hintKey("ctrl-w/g/esc", "repos", "back"), hintKey("ctrl-alt-b", "remote", "remote"), hintKey("ctrl-l", "branches")]
        : [
            hintKey("↑↓ ctrl-p/n", "move"),
            hintKey("enter", "jump"),
            hintKey("ctrl-w", "worktrees", "worktrees"),
            hintKey("esc", out, "esc"),
            hintKey("ctrl-alt-b", "remote", "remote"),
            hintKey("ctrl-l", "branches"),
          ];
    hintsEl.innerHTML = (st.dirtyOnly && st.mode === "repos" ? span("dirty", "dirty only") + sep : "") + hints.join(sep);
  }

  function drawOverlay() {
    overlayEl.hidden = !st.help;
    if (!st.help) return;
    const names = Math.max(...COMMANDS.map((c) => (c.name + (c.arg ? " " + c.arg : "")).length));
    let html = span("lbl", "commands") + "\n\n";
    for (const c of COMMANDS) {
      const l = c.name + (c.arg ? " " + c.arg : "");
      html += span("ref-local", l) + " ".repeat(names - l.length + 2) + span("subject", c.what) + "\n";
    }
    html += "\n" + span("dim", "q or esc closes this");
    overlayEl.innerHTML = `<div class="panel">${html}</div>`;
  }

  function draw() {
    drawList();
    drawInfo();
    drawHints();
    drawOverlay();
  }

  // ---- actions ----

  function say(note: string, cls = "dirty") {
    st.note = note;
    st.noteCls = cls;
  }

  function current(): Item | undefined {
    return st.all[st.view[st.cursor]];
  }

  function openWorktrees() {
    const it = current();
    if (!it || st.mode !== "repos") return;
    const r = it.repo;
    // Reversed, so git's first worktree — the main one — lands at the bottom.
    const wts: Item[] = [
      ...(r.worktrees ?? []).slice().reverse().map((w) => ({
        label: w.label,
        path: w.path,
        score: 0,
        repo: r,
        branch: w.branch,
        dirty: w.dirty,
        commits: w.commits,
      })),
      { label: r.branch, path: it.path, score: 0, repo: r, branch: r.branch, dirty: r.dirty, commits: r.commits },
    ];
    st.saved = { view: st.view, matched: st.matched, cursor: st.cursor, query: input.value };
    st.origin = it.label;
    st.mode = "worktrees";
    st.all = wts;
    input.value = "";
    st.view = [];
    filter();
  }

  function restore() {
    if (!st.saved) return;
    st.all = repoItems;
    st.view = st.saved.view;
    st.matched = st.saved.matched;
    st.cursor = st.saved.cursor;
    input.value = st.saved.query;
    st.query = st.saved.query.trim();
    st.mode = "repos";
    st.origin = "";
    st.saved = null;
  }

  function remote() {
    const it = current();
    if (it) say(`gm opens ${browseURL(it.repo.remote)} in your browser`, "remote");
  }

  function escape() {
    if (st.help) st.help = false;
    else if (st.mode === "worktrees") restore();
    else if (input.value !== "") {
      input.value = "";
      filter();
    } else if (st.dirtyOnly) {
      st.dirtyOnly = false;
      filter();
    } else say("gm would quit here, printing nothing", "dim");
  }

  function runCommand(line: string) {
    const [name, ...rest] = line.trim().split(/\s+/);
    const arg = rest.join(" ");
    input.value = "";
    filter();
    switch (name) {
      case "/help":
        st.help = true;
        break;
      case "/dirty":
        if (st.mode !== "repos") return say("/dirty works on the repository list");
        st.dirtyOnly = true;
        filter();
        break;
      case "/worktrees":
        openWorktrees();
        break;
      case "/remote":
        remote();
        break;
      case "/branches":
        say("the branch list asks git about a real repository, so it only runs in a real terminal", "dim");
        break;
      case "/create":
      case "/get":
      case "/remove":
        say(`${name}${arg ? " " + arg : ""} changes your disk, so it only runs in a real terminal`, "dim");
        break;
      default:
        say(`unknown command ${name}: /help lists them`);
    }
  }

  function enter() {
    const [, cmd, ok] = splitInput(input.value);
    if (ok) return runCommand(cmd);
    const it = current();
    if (it) say(`gm prints ${it.path} and your shell cds there`, "clean");
  }

  function complete() {
    const [query, cmd, ok] = splitInput(input.value);
    const typed = cmd.trim();
    if (!ok || typed.includes(" ")) return;
    const hits = COMMANDS.filter((c) => c.name.startsWith(typed));
    const before = query ? query + ";" : "";
    if (hits.length === 1) input.value = before + hits[0].name + (hits[0].arg ? " " : "");
  }

  function move(d: number) {
    st.cursor = Math.max(0, Math.min(st.view.length - 1, st.cursor + d));
    st.note = "";
  }

  // ---- wiring ----

  let touched = false;
  let autoplay: ReturnType<typeof setTimeout> | undefined;
  const touch = () => {
    touched = true;
    // Whatever the demo was about to type next, the visitor is driving now.
    clearTimeout(autoplay);
  };

  input.addEventListener("input", () => {
    touch();
    st.help = false;
    filter();
    draw();
  });

  input.addEventListener("keydown", (e) => {
    touch();
    if (st.help && (e.key === "q" || e.key === "Escape")) {
      e.preventDefault();
      st.help = false;
      return draw();
    }
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    let handled = true;
    if (e.key === "ArrowUp" || (ctrl && e.key === "p")) move(-1);
    else if (e.key === "ArrowDown" || (ctrl && e.key === "n")) move(1);
    else if (e.key === "Enter") enter();
    else if (e.key === "Escape") escape();
    else if (e.key === "Tab" && isCommand(input.value)) complete();
    else if (e.ctrlKey && e.altKey && e.key.toLowerCase() === "b") remote();
    else if (ctrl && e.key === "g" && st.mode === "worktrees") restore();
    else handled = false;
    if (handled) {
      e.preventDefault();
      draw();
    }
  });

  listEl.addEventListener("click", (e) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>(".row");
    if (!row) return;
    touch();
    st.cursor = Number(row.dataset.i);
    st.note = "";
    draw();
    input.focus({ preventScroll: true });
  });

  hintsEl.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!btn) return;
    touch();
    const act = btn.dataset.act;
    if (act === "worktrees") openWorktrees();
    else if (act === "back") restore();
    else if (act === "remote") remote();
    else if (act === "esc") escape();
    draw();
    input.focus({ preventScroll: true });
  });

  screen.addEventListener("click", (e) => {
    if ((e.target as HTMLElement).closest("button, .row")) return;
    input.focus({ preventScroll: true });
  });

  select.addEventListener("change", () => {
    const vars = data.themes[select.value];
    if (!vars) return;
    for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  });

  new ResizeObserver(() => {
    charW = 0;
    drawList();
  }).observe(listEl);

  filter();
  draw();

  // The demo types a query, rests on the result long enough to read it, wipes
  // itself and goes round again — so a visitor who looks up a moment too late
  // still sees it happen. It stops for good the first time they touch it, and
  // never runs at all for someone who asked for less motion.
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const script = "api";
  const typeDelay = 220; // between keystrokes
  const readDelay = 5000; // on the finished query, before starting over
  const startDelay = 900; // after it scrolls into view

  let i = 0;
  const tick = () => {
    if (touched) return;
    if (i < script.length) {
      input.value += script[i++];
      filter();
      draw();
      autoplay = setTimeout(tick, typeDelay);
      return;
    }
    autoplay = setTimeout(() => {
      if (touched) return;
      i = 0;
      input.value = "";
      filter();
      draw();
      autoplay = setTimeout(tick, typeDelay);
    }, readDelay);
  };

  // Only while it is on screen: a loop left running behind a scrolled-past
  // page is work nobody sees.
  const io = new IntersectionObserver((entries) => {
    if (touched) {
      io.disconnect();
      return;
    }
    if (entries.some((en) => en.isIntersecting)) {
      if (!autoplay) autoplay = setTimeout(tick, startDelay);
      return;
    }
    clearTimeout(autoplay);
    autoplay = undefined;
    i = 0;
    input.value = "";
    filter();
    draw();
  });
  io.observe(root);
}
