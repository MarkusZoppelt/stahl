"use strict";

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
const clamp = (x, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, x));
const span = (x, from, to) => clamp((x - from) / (to - from));
const ease = (x) => x * x * (3 - 2 * x);
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
if (!isMac) $$("[data-mod-k]").forEach((el) => (el.textContent = "Ctrl K"));

/* The portfolio, shared by every interactive part of the page.
   The prices and targets are the demo's, from src/demo.rs, so the totals match the footage. */

const PRICES = { "BTC-USD": 60000, SPY: 500, TLT: 90, GLD: 200 };
const POLICY = [
  { name: "Stocks", target: 50, tolerance: 5 },
  { name: "Bonds", target: 15, tolerance: 5 },
  { name: "Gold", target: 5, tolerance: 2 },
  { name: "Crypto", target: 10, tolerance: 3 },
  { name: "Cash", target: 20, tolerance: 5 },
];
const CLASS_COLORS = { stocks: "#7aa2f7", cash: "#73daca", crypto: "#bb9af7", bonds: "#e0af68", gold: "#7dcfff" };
const SPARE_COLORS = ["#7aa2f7", "#73daca", "#bb9af7", "#e0af68", "#7dcfff", "#f7768e", "#9ece6a", "#ff9e64"];

function classColor(name) {
  const key = name.trim().toLowerCase();
  if (CLASS_COLORS[key]) return CLASS_COLORS[key];
  let sum = 0;
  for (const c of key) sum += c.charCodeAt(0);
  return SPARE_COLORS[sum % SPARE_COLORS.length];
}

const INITIAL_FILE = `[
  { "Name": "S&P 500", "Amount": 80, "AssetClass": "Stocks", "Ticker": "SPY" },
  { "Name": "Savings", "Amount": 30000, "AssetClass": "Cash" },
  { "Name": "Bitcoin", "Amount": 0.45, "AssetClass": "Crypto", "Ticker": "BTC-USD" },
  { "Name": "Long Bonds", "Amount": 60, "AssetClass": "Bonds", "Ticker": "TLT" },
  { "Name": "Gold", "Amount": 25, "AssetClass": "Gold", "Ticker": "GLD" }
]`;

const money = new Intl.NumberFormat("en-US", { style: "currency", currency: "EUR", minimumFractionDigits: 2, maximumFractionDigits: 2 });
const percent = (x) => `${x.toFixed(1)}%`;

/* A JSON reader that says where it went wrong, in the words of serde_json, which reads the real
   file. Browsers disagree on what JSON.parse reports, so it can't give a line and column. */

class FileError extends Error {
  constructor(message, line, column) {
    super(message);
    this.line = line;
    this.column = column;
  }
}

function parseFile(text) {
  let i = 0;

  function fail(message, at = i) {
    let line = 1;
    let column = 1;
    for (let k = 0; k < at && k < text.length; k++) {
      if (text[k] === "\n") {
        line++;
        column = 1;
      } else {
        column++;
      }
    }
    throw new FileError(message, line, column);
  }
  const space = () => {
    while (i < text.length && " \t\r\n".includes(text[i])) i++;
  };

  function value() {
    space();
    if (i >= text.length) fail("EOF while parsing a value");
    const c = text[i];
    if (c === "{") return object();
    if (c === "[") return array();
    if (c === '"') return string();
    if (c === "-" || (c >= "0" && c <= "9")) return number();
    for (const [word, result] of [["true", true], ["false", false], ["null", null]]) {
      if (text.startsWith(word, i)) {
        i += word.length;
        return result;
      }
    }
    fail("expected value");
  }

  function object() {
    const result = {};
    i++;
    space();
    if (text[i] === "}") {
      i++;
      return result;
    }
    for (;;) {
      space();
      if (i >= text.length) fail("EOF while parsing an object");
      if (text[i] !== '"') fail("key must be a string");
      const key = string();
      space();
      if (i >= text.length) fail("EOF while parsing an object");
      if (text[i] !== ":") fail("expected `:`");
      i++;
      result[key] = value();
      space();
      if (i >= text.length) fail("EOF while parsing an object");
      if (text[i] === ",") {
        i++;
        space();
        if (text[i] === "}") fail("trailing comma");
        continue;
      }
      if (text[i] === "}") {
        i++;
        return result;
      }
      fail("expected `,` or `}`");
    }
  }

  function array() {
    const result = [];
    i++;
    space();
    if (text[i] === "]") {
      i++;
      return result;
    }
    for (;;) {
      result.push(value());
      space();
      if (i >= text.length) fail("EOF while parsing a list");
      if (text[i] === ",") {
        i++;
        space();
        if (text[i] === "]") fail("trailing comma");
        continue;
      }
      if (text[i] === "]") {
        i++;
        return result;
      }
      fail("expected `,` or `]`");
    }
  }

  function string() {
    i++;
    let result = "";
    while (i < text.length) {
      const c = text[i];
      if (c === '"') {
        i++;
        return result;
      }
      if (c === "\n") fail("control character (\\u0000-\\u001F) found while parsing a string");
      if (c === "\\") {
        i++;
        const escapes = { '"': '"', "\\": "\\", "/": "/", n: "\n", t: "\t", r: "\r", b: "\b", f: "\f" };
        if (text[i] === "u") {
          result += String.fromCharCode(parseInt(text.slice(i + 1, i + 5), 16) || 0xfffd);
          i += 5;
          continue;
        }
        if (!(text[i] in escapes)) fail("invalid escape");
        result += escapes[text[i]];
        i++;
        continue;
      }
      result += c;
      i++;
    }
    fail("EOF while parsing a string");
  }

  function number() {
    const match = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?/.exec(text.slice(i));
    if (!match) fail("invalid number");
    i += match[0].length;
    return Number(match[0]);
  }

  const result = value();
  space();
  if (i < text.length) fail("trailing characters");
  if (!Array.isArray(result)) fail("invalid type, expected a sequence", 0);
  // The columns of a mistake in the data itself point at the entry's opening brace.
  let cursor = 0;
  result.forEach((entry) => {
    cursor = text.indexOf("{", cursor);
    const at = cursor;
    cursor++;
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) fail("invalid type, expected a position", at);
    if (typeof entry.AssetClass !== "string") fail("missing field `AssetClass`", at);
    if (typeof entry.Amount !== "number") fail("invalid type for `Amount`, expected a number", at);
  });
  return result;
}

/* Reads a parsed file the way the README describes: units when there's a ticker, otherwise a
   balance or a manual value. A ticker the demo has no price for is left out and reported. */
function value(positions) {
  const rows = positions.map((p) => {
    const hasTicker = typeof p.Ticker === "string" && p.Ticker !== "";
    const price = hasTicker ? PRICES[p.Ticker.toUpperCase()] : 1;
    return {
      name: p.Name || p.Ticker || p.AssetClass,
      assetClass: p.AssetClass,
      ticker: p.Ticker,
      value: price === undefined ? null : p.Amount * price,
    };
  });
  const total = rows.reduce((sum, r) => sum + (r.value ?? 0), 0);
  const byClass = new Map();
  for (const r of rows) {
    if (r.value === null) continue;
    byClass.set(r.assetClass, (byClass.get(r.assetClass) ?? 0) + r.value);
  }
  const classes = [...byClass.entries()]
    .map(([name, amount]) => ({ name, amount, share: total ? (amount / total) * 100 : 0 }))
    .sort((a, b) => b.amount - a.amount);
  return { rows, total, classes, unpriced: rows.filter((r) => r.value === null) };
}

let portfolio = value(parseFile(INITIAL_FILE));
let privacy = false;

const DOTS = "••••••";
const showAmount = (amount) => (privacy ? DOTS : money.format(amount));

/* The rolling total. Each digit is a column of 0-9, so a change turns it like the app's hero. */

function rollTo(host, text) {
  const previous = [...host.children];
  const chars = [...text];
  if (previous.length !== chars.length || previous.some((el, k) => el.dataset.kind !== kindOf(chars[k]))) {
    host.replaceChildren(...chars.map(makeCell));
  }
  chars.forEach((c, k) => {
    const cell = host.children[k];
    if (cell.dataset.kind === "digit") cell.firstElementChild.style.transform = `translateY(${-Number(c) * 10}%)`;
    else cell.textContent = c;
  });
  host.setAttribute("aria-label", text);
}
const kindOf = (c) => (c >= "0" && c <= "9" ? "digit" : "char");
function makeCell(c) {
  const cell = document.createElement("span");
  cell.dataset.kind = kindOf(c);
  if (cell.dataset.kind === "digit") {
    cell.className = "digit";
    const reel = document.createElement("span");
    reel.className = "digit__reel";
    reel.textContent = "0123456789";
    reel.style.transform = "translateY(0)";
    cell.append(reel);
  } else {
    cell.className = "char";
    cell.textContent = c;
  }
  return cell;
}

function drawBar(host, segments) {
  const existing = new Map($$(".alloc__seg", host).map((el) => [el.dataset.name, el]));
  const seen = new Set();
  segments.forEach(({ name, share }) => {
    let el = existing.get(name);
    if (!el) {
      el = document.createElement("span");
      el.className = "alloc__seg";
      el.dataset.name = name;
      el.style.background = classColor(name);
      el.style.flexGrow = "0";
      host.append(el);
    }
    seen.add(name);
    el.style.flexGrow = String(Math.max(share, 0));
    host.append(el);
  });
  existing.forEach((el, name) => {
    if (!seen.has(name)) el.remove();
  });
}

/* Department 01: the file. */

const fileText = $("#file-text");
const filePaint = $("#file-paint");
const fileLines = $("#file-lines");
const sketchOk = $("#sketch-ok");
const sketchBad = $("#sketch-bad");
const sketchTotal = $("#sketch-total");
let fileError = null;

const escapeHtml = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

function paintFile(text, error) {
  const painted = escapeHtml(text).replace(/("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b)/g, (m, str, colon, num) => {
    if (str) return colon ? `<span class="tok-k">${str}</span>${colon}` : `<span class="tok-s">${str}</span>`;
    return `<span class="tok-n" data-num>${num}</span>`;
  });
  const lines = painted.split("\n");
  if (error && lines[error.line - 1] !== undefined) {
    lines[error.line - 1] = `<span class="tok-err">${lines[error.line - 1] || " "}</span>`;
  }
  filePaint.innerHTML = lines.join("\n") + "\n";
  const count = text.split("\n").length;
  fileLines.textContent = Array.from({ length: count }, (_, k) => k + 1).join("\n");
}

function readFile() {
  const text = fileText.value;
  try {
    const positions = parseFile(text);
    fileError = null;
    portfolio = value(positions);
  } catch (error) {
    if (!(error instanceof FileError)) throw error;
    fileError = error;
  }
  paintFile(text, fileError);
  $("#file-restore").hidden = text === INITIAL_FILE;
  $("#file-break").hidden = Boolean(fileError);
  drawSketch();
  drawDesk();
}

function drawSketch() {
  sketchOk.hidden = Boolean(fileError);
  sketchBad.hidden = !fileError;
  if (fileError) {
    $("#bad-pos").textContent = `Line ${fileError.line}, column ${fileError.column}`;
    $("#bad-msg").textContent = fileError.message;
    return;
  }
  rollTo(sketchTotal, privacy ? DOTS : money.format(portfolio.total));
  sketchTotal.classList.toggle("is-hidden", privacy);
  drawBar($("#sketch-bar"), portfolio.classes);
  const list = $("#sketch-classes");
  list.replaceChildren(
    ...portfolio.classes.map((c) => {
      const li = document.createElement("li");
      const dot = document.createElement("i");
      dot.style.background = classColor(c.name);
      const name = document.createElement("span");
      name.textContent = c.name;
      const amount = document.createElement("span");
      amount.className = "num";
      amount.textContent = showAmount(c.amount);
      const share = document.createElement("b");
      share.className = "num";
      share.textContent = percent(c.share);
      li.append(dot, name, amount, share);
      return li;
    }),
    ...portfolio.unpriced.map((r) => {
      const li = document.createElement("li");
      li.className = "classes__warn";
      li.textContent = `No price for ${r.ticker}. It's left out of the total, never counted as zero without saying so.`;
      return li;
    }),
  );
}

fileText.value = INITIAL_FILE;
fileText.addEventListener("input", readFile);
fileText.addEventListener("scroll", () => {
  filePaint.scrollTop = fileText.scrollTop;
  filePaint.scrollLeft = fileText.scrollLeft;
  fileLines.scrollTop = fileText.scrollTop;
});
$("#file-break").addEventListener("click", () => {
  // The same mistake as the app's own parse-failure scene.
  fileText.value = INITIAL_FILE.replace('"Savings", "Amount"', '"Savings",, "Amount"');
  readFile();
});
$("#file-restore").addEventListener("click", () => {
  fileText.value = INITIAL_FILE;
  readFile();
});

/* Department 02: the desk. Same arithmetic as Stahl's Full Rebalance on the demo: the total times
   the target, minus what's held, ignoring anything already within tolerance. */

const targets = new Map(POLICY.map((p) => [p.name, p.target]));
const deskSliders = $("#desk-sliders");

POLICY.forEach((p) => {
  const row = document.createElement("label");
  row.className = "slider";
  row.innerHTML = `<span class="slider__name"><i></i></span><input type="range" min="0" max="100" step="1" /><output class="num"></output>`;
  row.querySelector("i").style.background = classColor(p.name);
  row.querySelector(".slider__name").append(p.name);
  const input = row.querySelector("input");
  input.setAttribute("aria-label", `${p.name} target`);
  input.value = String(p.target);
  input.addEventListener("input", () => setTarget(p.name, Number(input.value)));
  row.dataset.name = p.name;
  deskSliders.append(row);
});

// Moving one target scales the others, so they always add up to 100.
function setTarget(name, next) {
  const others = POLICY.filter((p) => p.name !== name);
  const rest = others.reduce((sum, p) => sum + targets.get(p.name), 0);
  const left = 100 - next;
  targets.set(name, next);
  others.forEach((p) => {
    targets.set(p.name, rest > 0 ? (targets.get(p.name) / rest) * left : left / others.length);
  });
  drawDesk();
}

function drawDesk() {
  const held = new Map(portfolio.classes.map((c) => [c.name, c]));
  $$(".slider", deskSliders).forEach((row) => {
    const target = targets.get(row.dataset.name);
    const input = row.querySelector("input");
    if (document.activeElement !== input) input.value = String(target);
    row.querySelector("output").textContent = `${Math.round(target)}%`;
  });

  const known = new Set(POLICY.map((p) => p.name));
  drawBar($("#desk-today"), portfolio.classes);
  drawBar($("#desk-after"), [
    ...POLICY.map((p) => ({ name: p.name, share: targets.get(p.name) })),
    ...portfolio.classes.filter((c) => !known.has(c.name)).map((c) => ({ name: c.name, share: 0 })),
  ]);

  const body = $("#desk-moves");
  const moves = POLICY.map((p) => {
    const target = targets.get(p.name);
    const amount = held.get(p.name)?.amount ?? 0;
    const share = held.get(p.name)?.share ?? 0;
    return { name: p.name, delta: (portfolio.total * target) / 100 - amount, within: Math.abs(share - target) <= p.tolerance && target === p.target };
  }).filter((m) => !m.within && Math.abs(m.delta) >= 0.005);

  if (!moves.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 3;
    cell.className = "moves__none";
    cell.textContent = "Nothing to do. Everything is within tolerance.";
    row.append(cell);
    body.replaceChildren(row);
  } else {
    body.replaceChildren(
      ...moves.map((m) => {
        const row = document.createElement("tr");
        const name = document.createElement("td");
        name.textContent = m.name;
        const action = document.createElement("td");
        const pill = document.createElement("span");
        pill.className = `pill ${m.delta > 0 ? "pill--up" : "pill--down"}`;
        pill.textContent = m.delta > 0 ? "Increase" : "Reduce";
        action.append(pill);
        const amount = document.createElement("td");
        amount.className = "num";
        amount.textContent = showAmount(Math.abs(m.delta));
        row.append(name, action, amount);
        return row;
      }),
    );
  }
}

const deskPanel = $("#desk-panel");
const shutter = $("#desk-shutter");
const deskTrade = $("#desk-trade");
const deskReopen = $("#desk-reopen");
deskTrade.addEventListener("click", () => {
  deskPanel.classList.add("is-closed");
  shutter.setAttribute("aria-hidden", "false");
  deskReopen.tabIndex = 0;
  deskReopen.focus({ preventScroll: true });
});
deskReopen.addEventListener("click", () => {
  deskPanel.classList.remove("is-closed");
  shutter.setAttribute("aria-hidden", "true");
  deskReopen.tabIndex = -1;
  deskTrade.focus({ preventScroll: true });
});

/* The tour. Chapters are the marks in media/tour.json. */

const CHAPTERS = [
  { label: "Overview", t: 0, page: "overview" },
  { label: "Holdings", t: 4.6, page: "holdings" },
  { label: "Allocation", t: 9.2, page: "allocation" },
  { label: "Performance", t: 12.4, page: "performance" },
  { label: "Review", t: 15.6, page: "review" },
  { label: "Rebalance", t: 18.8, page: "rebalance" },
  { label: "Policy", t: 22.0, page: "policy" },
  { label: "Briefing", t: 24.8, page: "briefing" },
  { label: "Palette", t: 27.6, page: "overview" },
];

const video = $("#tour-video");
const tourHidden = $("#tour-hidden");
const chapterHost = $("#chapters");
let wantsVideo = false;

CHAPTERS.forEach((chapter, k) => {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "chapter";
  button.innerHTML = `<kbd>${k + 1}</kbd>`;
  button.append(chapter.label);
  button.addEventListener("click", () => jumpToChapter(k));
  chapterHost.append(button);
});
const chapterButtons = $$(".chapter", chapterHost);

function currentChapter() {
  let index = 0;
  CHAPTERS.forEach((chapter, k) => {
    if (video.currentTime >= chapter.t - 0.01) index = k;
  });
  return index;
}
function markChapter() {
  const index = currentChapter();
  chapterButtons.forEach((button, k) => {
    button.classList.toggle("is-on", k === index);
    if (k === index) button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  });
}
function syncVideo() {
  tourHidden.hidden = !privacy;
  if (privacy) {
    tourHidden.src = `media/${CHAPTERS[currentChapter()].page}-hidden.webp`;
    video.pause();
  } else if (wantsVideo && !reducedMotion.matches) {
    video.play().catch(() => {});
  } else {
    video.pause();
  }
}
function jumpToChapter(k) {
  ensureVideoLoaded();
  video.currentTime = CHAPTERS[k].t + 0.05;
  markChapter();
  if (privacy) {
    syncVideo();
  } else {
    // With reduced motion the visitor asked for this frame, so it plays only when they say.
    video.play().catch(() => {});
  }
}
let videoLoaded = false;
function ensureVideoLoaded() {
  if (videoLoaded) return;
  videoLoaded = true;
  video.preload = "auto";
  video.load();
}
video.addEventListener("timeupdate", markChapter);
if (reducedMotion.matches) video.controls = true;
markChapter();

/* Hide amounts. Every amount in the page's own markup goes, and every real screenshot swaps to the
   same page rendered by Stahl with its own Hide Amounts on. */

const bigAmount = $("[data-amount]");
function setPrivacy(on) {
  privacy = on;
  document.documentElement.classList.toggle("privacy", on);
  $$('[data-action="privacy"]').forEach((el) => el.setAttribute("aria-pressed", String(on)));
  bigAmount.textContent = on ? DOTS : money.format(Number(bigAmount.dataset.amount));
  $$("img[data-page]").forEach((img) => {
    img.src = `media/${img.dataset.page}${on ? "-hidden" : ""}.webp`;
  });
  drawSketch();
  drawDesk();
  syncVideo();
  toast(on ? "Amounts hidden" : "Amounts shown");
}

let toastTimer = 0;
const toastEl = $("#toast");
function toast(message) {
  toastEl.textContent = message;
  toastEl.classList.add("is-on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.remove("is-on"), 1600);
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast("Copied");
  } catch {
    toast(text);
  }
}

$$("[data-copy]").forEach((el) => el.addEventListener("click", () => copyText(el.dataset.copy)));

/* Downloads: show the visitor's own platform first. */

const PLATFORMS = {
  mac: { label: "Download for macOS", small: "Version 0.2.0 for Apple silicon", href: "https://github.com/MarkusZoppelt/stahl/releases/latest/download/stahl-capital-macos-arm64.dmg" },
  linux: { label: "Download for Linux", small: "Version 0.2.0, .deb for x64", href: "https://github.com/MarkusZoppelt/stahl/releases/download/v0.2.0/stahl_0.2.0_amd64.deb" },
  windows: { label: "Download for Windows", small: "Version 0.2.0 for x64", href: "https://github.com/MarkusZoppelt/stahl/releases/latest/download/stahl-capital-windows-x64.zip" },
};
const agent = `${navigator.platform} ${navigator.userAgent}`;
const isPhone = /iPhone|iPad|Android/.test(agent) || (navigator.maxTouchPoints > 1 && /Mac/.test(agent));
const platform = /Win/.test(agent) ? "windows" : /Mac/.test(agent) && !isPhone ? "mac" : /Linux|X11/.test(agent) && !isPhone ? "linux" : null;

if (platform) {
  const chosen = PLATFORMS[platform];
  $("#hero-download").href = chosen.href;
  $("#hero-download-label").textContent = chosen.label;
  $("#hero-small").textContent = chosen.small;
} else {
  $("#hero-download-label").textContent = "For Mac, Linux and Windows";
}

const tabs = $$('[role="tab"]');
function selectTab(tab, focus) {
  tabs.forEach((other) => {
    const on = other === tab;
    other.setAttribute("aria-selected", String(on));
    other.tabIndex = on ? 0 : -1;
    $(`#${other.getAttribute("aria-controls")}`).hidden = !on;
  });
  if (focus) tab.focus();
}
tabs.forEach((tab, k) => {
  tab.addEventListener("click", () => selectTab(tab, false));
  tab.addEventListener("keydown", (event) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step) selectTab(tabs[(k + step + tabs.length) % tabs.length], true);
  });
});
if (platform) selectTab($(`#tab-${platform}`), false);

/* The command palette. Like the app's, it goes to places, runs actions and finds your holdings. */

const palette = $("#palette");
const paletteInput = $("#palette-input");
const paletteList = $("#palette-list");
let paletteRows = [];
let paletteIndex = 0;

function go(selector) {
  const target = $(selector);
  target.scrollIntoView({ behavior: reducedMotion.matches ? "auto" : "smooth" });
}
function goToTour(k) {
  const vault = $("#top");
  const top = pinned ? vault.offsetTop + (vault.offsetHeight - innerHeight) * 0.86 : $("#tour").getBoundingClientRect().top + scrollY - 70;
  scrollTo({ top, behavior: "auto" });
  jumpToChapter(k);
}

function paletteEntries() {
  const section = (label, selector) => ({ group: "Go to", label, run: () => go(selector) });
  const tour = CHAPTERS.map((chapter, k) => ({ group: "Watch", label: chapter.label, keys: String(k + 1), run: () => goToTour(k) }));
  const holdings = portfolio.rows.map((row) => ({
    group: "Holdings",
    label: row.name,
    hint: row.value === null ? row.assetClass : privacy ? DOTS : money.format(row.value),
    hiddenUntilSearch: true,
    run: () => go("#vault"),
  }));
  return [
    section("The Vault", "#vault"),
    section("The Trading Desk", "#desk"),
    section("Compliance", "#compliance"),
    section("The Analysts", "#analysts"),
    section("Client Discretion", "#discretion"),
    section("Schedule of Fees", "#terms"),
    section("Open Your Account", "#account"),
    { group: "Actions", label: "Hide or Show Amounts", keys: "H", run: () => setPrivacy(!privacy) },
    { group: "Actions", label: "Copy the nix run Command", run: () => copyText("nix run github:MarkusZoppelt/stahl") },
    { group: "Actions", label: "Download for macOS", run: () => (location.href = PLATFORMS.mac.href) },
    { group: "Actions", label: "Download for Linux", run: () => (location.href = PLATFORMS.linux.href) },
    { group: "Actions", label: "Download for Windows", run: () => (location.href = PLATFORMS.windows.href) },
    { group: "Actions", label: "Read the Source on GitHub", run: () => (location.href = "https://github.com/MarkusZoppelt/stahl") },
    ...tour,
    ...holdings,
  ];
}

function drawPalette() {
  const query = paletteInput.value.trim().toLowerCase();
  const entries = paletteEntries().filter((entry) => (query ? entry.label.toLowerCase().includes(query) : !entry.hiddenUntilSearch));
  paletteList.replaceChildren();
  paletteRows = [];
  paletteIndex = 0;
  let group = null;
  entries.forEach((entry) => {
    if (entry.group !== group) {
      group = entry.group;
      const heading = document.createElement("p");
      heading.className = "palette__group";
      heading.textContent = group;
      paletteList.append(heading);
    }
    const row = document.createElement("button");
    row.type = "button";
    row.className = "palette__row";
    row.setAttribute("role", "option");
    const label = document.createElement("span");
    label.textContent = entry.label;
    row.append(label);
    if (entry.keys) {
      const keys = document.createElement("kbd");
      keys.textContent = entry.keys;
      row.append(keys);
    }
    if (entry.hint) {
      const hint = document.createElement("span");
      hint.className = "palette__hint";
      hint.textContent = entry.hint;
      row.append(hint);
    }
    const index = paletteRows.length;
    row.addEventListener("click", () => runPalette(entry));
    row.addEventListener("pointermove", () => markPalette(index, false));
    paletteList.append(row);
    paletteRows.push({ row, entry });
  });
  if (!entries.length) {
    const none = document.createElement("p");
    none.className = "palette__none";
    none.textContent = "Nothing by that name. The trading desk is still closed.";
    paletteList.append(none);
  }
  markPalette(0, false);
}
function markPalette(index, scroll) {
  paletteIndex = index;
  paletteRows.forEach(({ row }, k) => {
    row.classList.toggle("is-on", k === index);
    row.setAttribute("aria-selected", String(k === index));
    if (k === index && scroll) row.scrollIntoView({ block: "nearest" });
  });
}
function runPalette(entry) {
  palette.close();
  entry.run();
}
function openPalette() {
  if (palette.open) {
    palette.close();
    return;
  }
  paletteInput.value = "";
  drawPalette();
  palette.showModal();
  paletteInput.focus();
}
paletteInput.addEventListener("input", drawPalette);
paletteInput.addEventListener("keydown", (event) => {
  if (!paletteRows.length) return;
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const step = event.key === "ArrowDown" ? 1 : -1;
    markPalette((paletteIndex + step + paletteRows.length) % paletteRows.length, true);
  } else if (event.key === "Enter") {
    event.preventDefault();
    runPalette(paletteRows[paletteIndex].entry);
  }
});
palette.addEventListener("click", (event) => {
  if (event.target === palette) palette.close();
});

/* Screenshots, larger. */

const zoom = $("#zoom");
const zoomImg = $("#zoom-img");
$$("[data-zoom]").forEach((button) => {
  button.addEventListener("click", () => {
    const img = $("img", button);
    zoomImg.src = img.src;
    zoomImg.alt = img.alt;
    zoom.showModal();
  });
});
zoom.addEventListener("click", () => zoom.close());

/* Keys and buttons. */

$$('[data-action="privacy"]').forEach((el) => el.addEventListener("click", () => setPrivacy(!privacy)));
$$('[data-action="palette"]').forEach((el) => el.addEventListener("click", openPalette));

addEventListener("keydown", (event) => {
  const typing = event.target.matches?.("input, textarea, select");
  if ((event.metaKey || event.ctrlKey) && !event.shiftKey && event.key.toLowerCase() === "k") {
    event.preventDefault();
    openPalette();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "h") {
    event.preventDefault();
    setPrivacy(!privacy);
    return;
  }
  if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
  if (palette.open || zoom.open) return;
  if (event.key === "h" || event.key === "H") {
    setPrivacy(!privacy);
  } else if (event.key >= "1" && event.key <= "9" && tourOnScreen()) {
    jumpToChapter(Number(event.key) - 1);
  }
});

function tourOnScreen() {
  const box = $("#tour").getBoundingClientRect();
  const visible = getComputedStyle($("#tour")).opacity !== "0";
  return visible && box.bottom > 80 && box.top < innerHeight - 80;
}

/* The door. While the stage is pinned, scrolling turns the tumblers, pushes in until the door
   fills the view, then parts it along the seam. */

const vault = $("#top");
const stage = $(".stage");
const hero = $("#hero");
const tour = $("#tour");
const cue = $("#scrollcue");
const canvas = $("#door");
const door = reducedMotion.matches ? null : window.createDoor(canvas);
const pinned = Boolean(door);
document.documentElement.classList.add("js");
document.documentElement.classList.toggle("pinned", pinned);

const light = { x: -0.55, y: -0.65, targetX: -0.55, targetY: -0.65 };
let layout = null;
let doorRunning = false;
let lastPointer = -Infinity;

function measure() {
  const headerBottom = $(".bar").getBoundingClientRect().bottom;
  const copyTop = hero.getBoundingClientRect().top - stage.getBoundingClientRect().top;
  const width = stage.clientWidth;
  const height = stage.clientHeight;
  const room = copyTop - headerBottom;
  layout = {
    width,
    height,
    restX: width / 2,
    restY: headerBottom + room / 2,
    restRadius: Math.max(60, Math.min(room / 2 - 20, width * 0.42)),
    openRadius: Math.hypot(width, height) * 0.56,
    scale: Math.min(devicePixelRatio || 1, 1.75),
  };
}

function scrollProgress() {
  const distance = vault.offsetHeight - innerHeight;
  return distance > 0 ? clamp((scrollY - vault.offsetTop) / distance) : 0;
}

function frame(time) {
  doorRunning = false;
  const p = scrollProgress();
  const onScreen = vault.getBoundingClientRect().bottom > 0;

  if (time - lastPointer > 2500) {
    // Nobody's steering, so the light drifts slowly over the steel.
    const t = time / 5200;
    light.targetX = Math.cos(t) * 0.75;
    light.targetY = Math.sin(t * 0.8) * 0.55 - 0.3;
  }
  light.x += (light.targetX - light.x) * 0.06;
  light.y += (light.targetY - light.y) * 0.06;

  const push = ease(span(p, 0.03, 0.44));
  const part = ease(span(p, 0.44, 0.82));
  const copy = 1 - span(p, 0.0, 0.1);

  hero.style.opacity = String(copy);
  hero.style.transform = `translateY(${(1 - copy) * -18}px)`;
  hero.style.visibility = copy <= 0 ? "hidden" : "visible";
  cue.style.opacity = String(1 - span(p, 0, 0.03));

  const show = span(p, 0.4, 0.46);
  tour.style.opacity = String(show);
  tour.style.visibility = show <= 0 ? "hidden" : "visible";
  tour.style.transform = `scale(${0.93 + 0.07 * ease(span(p, 0.44, 0.9))})`;
  $(".tour__note").style.opacity = String(span(p, 0.8, 0.9));
  chapterHost.style.opacity = String(span(p, 0.8, 0.9));

  const shouldPlay = p > 0.5 && onScreen;
  if (p > 0.22) ensureVideoLoaded();
  if (shouldPlay !== wantsVideo) {
    wantsVideo = shouldPlay;
    if (shouldPlay && video.currentTime < 0.2) video.currentTime = 0;
    syncVideo();
  }

  const doorGone = part >= 1;
  canvas.style.visibility = doorGone ? "hidden" : "visible";

  if (!doorGone) {
    door.draw({
      width: layout.width,
      height: layout.height,
      scale: layout.scale,
      x: layout.restX + (layout.width / 2 - layout.restX) * push,
      y: layout.restY + (layout.height / 2 - layout.restY) * push,
      radius: layout.restRadius + (layout.openRadius - layout.restRadius) * push,
      split: part * 1.08,
      spin: ease(span(p, 0, 0.44)) * 2.4,
      gain: 1 - 0.62 * push,
      lightX: light.x,
      lightY: light.y,
    });
  }
  // The steel sheen on the title follows the same light.
  hero.style.setProperty("--sheen", `${50 + light.x * 38}%`);

  if (onScreen && !doorGone) requestDoorFrame();
}
function requestDoorFrame() {
  if (doorRunning) return;
  doorRunning = true;
  requestAnimationFrame(frame);
}

if (pinned) {
  measure();
  addEventListener("resize", () => {
    measure();
    requestDoorFrame();
  });
  addEventListener("scroll", requestDoorFrame, { passive: true });
  addEventListener("pointermove", (event) => {
    if (!layout || scrollProgress() > 0.6) return;
    lastPointer = performance.now();
    const radius = layout.restRadius;
    light.targetX = clamp((event.clientX - layout.restX) / radius, -2, 2);
    light.targetY = clamp((event.clientY - layout.restY) / radius, -2, 2);
  }, { passive: true });
  document.fonts?.ready.then(() => {
    measure();
    requestDoorFrame();
  });
  requestDoorFrame();
} else {
  // No WebGL 2, or reduced motion: the page is a plain scroll, and the flat icon stands in.
  document.documentElement.classList.add("still");
  const watch = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) ensureVideoLoaded();
      wantsVideo = entry.isIntersecting;
      syncVideo();
    });
  }, { threshold: 0.4 });
  watch.observe(tour);
}

/* Sections rise in once, the first time they're seen. */

if (!reducedMotion.matches) {
  const rise = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.add("is-seen");
      rise.unobserve(entry.target);
    });
  }, { threshold: 0.06, rootMargin: "0px 0px -6% 0px" });
  $$(".dept__head, .ledger, .desk, .memo, .tile, .letter, .install, .natives").forEach((el) => {
    el.classList.add("rises");
    rise.observe(el);
  });
}

readFile();
