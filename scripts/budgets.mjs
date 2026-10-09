// Measures the performance budgets inside the running app's webview.
//
// 1. Seed a throwaway database with a dev build (Ctrl+Alt+Shift+S in the popup):
//      QN_DATA_DIR=/tmp/qn-bench npm run tauri dev
// 2. Start the build to measure with remote debugging on the same data:
//      QN_DATA_DIR=/tmp/qn-bench WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
//        src-tauri/target/release/quick-notes.exe
// 3. node scripts/budgets.mjs
//
// Hotkey latency and idle memory are OS-level; see scripts/os-budgets.ps1 (Windows).
import { readFileSync } from "node:fs";
import { connect, ENTER, ESCAPE, MOD } from "./cdp.mjs";

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const stats = (a) => {
  const s = [...a].sort((x, y) => x - y);
  const at = (p) => +s[Math.min(s.length - 1, Math.floor(s.length * p))].toFixed(1);
  return { n: s.length, p50: at(0.5), p95: at(0.95), max: +s.at(-1).toFixed(1) };
};

const c = await connect();
const F = { code: "KeyF", modifiers: MOD.ctrl, windowsVirtualKeyCode: 70 };
const notes = await c.evaluate(`window.__TAURI_INTERNALS__.invoke('search', { query: 'e', limit: 1 }).then(() => true)`);
if (!notes) throw new Error("app not responding");

const coldStart = await c.evaluate(`window.__qnPerf.coldStart`);
await c.evaluate(`window.__qnPerf.search.length = 0; window.__qnPerf.enterToVisible.length = 0`);

// Search: type queries one key at a time, as a person would.
await c.evaluate(`document.querySelector('.composer .tiptap').focus()`);
for (const q of ["milk coffee", "#todo deploy", "rocket search", "e", "größe", "a\"b(c*-d"]) {
  await c.key("f", F);
  for (const ch of q) {
    await c.type(ch);
    await wait(40);
  }
  await c.key("Escape", ESCAPE);
}
await wait(200);
const searchErrors = await c.evaluate(`document.querySelector('.toast.error')?.textContent ?? null`);

// Enter to visible.
await c.evaluate(`document.querySelector('.composer .tiptap').focus()`);
for (let i = 0; i < 30; i++) {
  await c.type(`budget note ${i} #bench`);
  await c.key("Enter", ENTER);
  await wait(60);
}

// Scrolling through the timeline (loads older pages as it goes).
const scroll = readFileSync(new URL("./scroll-bench.js", import.meta.url), "utf8")
  .replace(/^\/\/.*\n/gm, "")
  .trim()
  .replace(/;$/, "");
const scrollSlow = await c.evaluate(`(${scroll})(40, 600)`);
const scrollFast = await c.evaluate(`(${scroll})(150, 600)`);

const perf = await c.evaluate(`({ search: window.__qnPerf.search, enter: window.__qnPerf.enterToVisible })`);
console.log(
  JSON.stringify(
    {
      coldStartToReadyMs: +coldStart.toFixed(1),
      searchMs: stats(perf.search),
      searchErrors,
      enterToVisibleMs: stats(perf.enter),
      scrollSlow,
      scrollFast,
    },
    null,
    2,
  ),
);
c.close();
