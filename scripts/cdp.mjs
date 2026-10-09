// Minimal Chrome DevTools Protocol client for driving the app's webview in
// measurements. WebView2 exposes CDP when the app runs with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
// Uses only Node's built-in fetch and WebSocket.

export async function connect(port = 9222, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  let target;
  while (!target) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      target = list.find((t) => t.type === "page");
    } catch {
      // Not up yet.
    }
    if (!target) {
      if (Date.now() > deadline) throw new Error("webview debug port not reachable");
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (msg) => {
    const data = JSON.parse(msg.data);
    if (data.id && pending.has(data.id)) {
      const { resolve, reject } = pending.get(data.id);
      pending.delete(data.id);
      data.error ? reject(new Error(data.error.message)) : resolve(data.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      pending.set(n, { resolve, reject });
      ws.send(JSON.stringify({ id: n, method, params }));
    });

  /** Evaluates an expression (awaiting promises) in the page and returns its value. */
  const evaluate = async (expression) => {
    const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };

  /** Sends a real key press through the input pipeline. */
  const key = async (keyName, { code, text, modifiers = 0, windowsVirtualKeyCode } = {}) => {
    const base = { key: keyName, code: code ?? keyName, modifiers, windowsVirtualKeyCode };
    await send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", text, ...base });
    await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  };

  /** Types one key press per character, so editor input rules (Markdown shortcuts) fire. */
  const type = async (text) => {
    for (const ch of text) {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: ch, text: ch, unmodifiedText: ch });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: ch });
    }
  };

  return { send, evaluate, key, type, close: () => ws.close() };
}

export const ENTER = { code: "Enter", text: "\r", windowsVirtualKeyCode: 13 };
export const ESCAPE = { code: "Escape", windowsVirtualKeyCode: 27 };
export const MOD = { ctrl: 2, shift: 8, alt: 1 };
