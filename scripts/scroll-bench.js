// Runs inside the webview (via scripts/budgets.mjs). Scrolls the timeline up
// by `step` px per frame for `frames` frames, recording frame times and any
// frame where rendered rows fail to cover the viewport ("blanking").
async (step, count) => {
  const v = document.querySelector(".timeline");
  v.scrollTop = v.scrollHeight;
  await new Promise((r) => setTimeout(r, 300));
  const times = [];
  let blank = 0;
  let last = performance.now();
  for (let i = 0; i < count; i++) {
    v.scrollTop -= step;
    await new Promise((r) => requestAnimationFrame(r));
    const now = performance.now();
    times.push(now - last);
    last = now;
    const vr = v.getBoundingClientRect();
    let top = Infinity;
    let bottom = -Infinity;
    for (const row of v.querySelectorAll(".timeline-row")) {
      const b = row.getBoundingClientRect();
      top = Math.min(top, b.top);
      bottom = Math.max(bottom, b.bottom);
    }
    if (top > vr.top + 1 || bottom < vr.bottom - 1) blank++;
  }
  times.sort((a, b) => a - b);
  const pct = (p) => +times[Math.floor((times.length - 1) * p)].toFixed(1);
  return {
    frames: times.length,
    pxPerFrame: step,
    p50: pct(0.5),
    p95: pct(0.95),
    p99: pct(0.99),
    max: +times.at(-1).toFixed(1),
    over_17ms: times.filter((t) => t > 17.5).length,
    fps: +(1000 / (times.reduce((a, b) => a + b, 0) / times.length)).toFixed(1),
    blankFrames: blank,
    timelineHeightPx: v.scrollHeight,
  };
};
