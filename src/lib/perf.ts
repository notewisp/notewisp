// Timing samples for the performance budgets. Exposed on `window.__qnPerf`
// so a measurement script attached to the webview can read them.
export const perf = {
  /** Enter keydown to the frame that shows the new note (ms). */
  enterToVisible: [] as number[],
  /** Search request round trip, keystroke to results (ms). */
  search: [] as number[],
  /** Hotkey press (in Rust) to editor focused (ms). */
  hotkeyToFocus: [] as number[],
  /** Process start to first render (ms). */
  coldStart: 0,
};

(window as unknown as { __qnPerf: typeof perf }).__qnPerf = perf;
