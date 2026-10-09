# Decisions

Places where the implementation departs from, or fills a gap in, the MVP spec, and why.

## Data and commands

**Notes are stored as editor JSON, not Markdown.** The spec has two models: `body_json` as the source of truth (data model), and a Markdown `body` rendered by pulldown-cmark (backend commands). They can't both hold. TipTap already produces JSON, HTML and text, so the JSON model is kept and pulldown-cmark is not used. `create_note` / `update_note` take `{ json, html, text }` instead of one `body` string. Cached HTML is still cleaned with DOMPurify before display.

**`Note` carries `text`, not `body`.** The timeline needs the cached HTML (display) and plain text (copy button); the JSON is fetched with `get_note_json` only when a note is opened for editing.

**Commands added beyond the spec's list**, each for a feature the spec asks for:

| Command | Why |
| --- | --- |
| `list_notes(…, after)` | Paging *newer* notes after jumping to an old search hit |
| `notes_around(…, tag)` | Same, inside a tag filter |
| `get_note_json` | Load the source of truth for in-place editing |
| `get_draft` / `save_draft` | "An unsent draft survives hiding the window and restarting the app", kept in `meta` |
| `app_ready` | Shows the popup once the UI has rendered on a normal launch; stays hidden with `--autostart` |
| `hide_popup`, `quit_app` | Esc and Ctrl+Q from the webview, which has no window permissions |
| `open_link` | "Clicking a link opens it in the default browser"; only `http`, `https`, `mailto` |
| `perf_since_hotkey` | Measures the hotkey budget |

**Search orders by id, not `created_at`.** Ids grow with creation time, and walking the FTS index by rowid descending lets `LIMIT` stop early. This keeps broad prefixes like `e*` fast on 100,000 notes.

**Tags need a word boundary.** `#[\p{L}\p{N}_-]+` as specified, but only after a non-word character, so `C#`, `issue#4` and `&#39;` aren't tags.

**An edit that empties a note deletes it**, with the usual Undo toast.

## Export

**Export converts from `body_json`, not `body_html`.** Converting HTML to Markdown would need a library the spec doesn't name. Walking the ProseMirror tree in Rust is a few dozen lines, exact, and has unit tests.

**Note headings are demoted two levels** (H1 → `###`) so they never collide with the `## HH:MM` section heading of each note. Text that merely looks like block syntax (a typed `## x`, `- x`, `1. x`) is escaped for the same reason. Hard breaks use the CommonMark backslash form.

**Export writes to a fixed folder.** `export_markdown(dir)` needs a folder, but the spec forbids native dialogs (they take focus and hide the popup) and frontend file-system access. Each export goes to a new `exports/<date> <time>/` folder in the app-data directory, which then opens in the file manager.

## Editor and UI

**Plain Svelte + Vite instead of SvelteKit.** `create-tauri-app`'s Svelte template uses SvelteKit; the spec asks for Svelte 5 + Vite and a handful of components, so routing and SSR are left out.

**Three components beyond the five listed:** `Toolbar` (floating format bar, link editor, colour palette), `EmojiPicker` and `Settings`. Each is a self-contained popover; folding them into `Composer` or `App` would make those hard to read.

**Optimistic insert.** A bridge round trip plus a SQLite write can't reliably fit in one frame, so the note is shown immediately and saved right after. A failed save removes it, puts the text back in the composer and shows a toast.

**Hand-written virtual list.** No virtual-list package is named in the spec. `Timeline.svelte` measures rows as they render and re-anchors the scroll position so nothing jumps when older pages are prepended.

**Emoji are inserted as plain Unicode characters**, not TipTap emoji nodes, so search, copy and export just work. The Emoji extension's data and the TipTap suggestion plugin drive `:name` autocomplete and the picker. The 600 KB data loads on first use, and emoji newer than the OS font supports (e.g. Emoji 13+ on Windows 10) are hidden.

**`#` autocompletes tags** in the editor, from `list_tags` (the spec lists it "for autocomplete" without saying where).

**Shift+Enter starts a new paragraph**, not a `<br>`, so Markdown shortcuts (`- `, `## `) work on the new line.

**Esc closes the innermost thing first:** popover → edit → search → tag filter → jumped-to view → hide the popup.

**Times use the OS locale** (`14:32` or `2:32 PM`).

**Checkboxes in the timeline are read-only**; tick them by editing the note. Toggling in place would need the editor schema outside an editor.

**The heading button cycles** H1 → H2 → H3 → paragraph. Ctrl+Alt+1–3 set a level directly.

**Pasting drops `style` and `class` attributes**, so foreign fonts and colours never enter a note; only the 8 palette colours exist.

## Window and platform

**"Close hides to tray" (milestone 3).** The popup has no title bar, so there's no close button. Esc, focus loss and the hotkey hide; Alt+F4 also only hides; Ctrl+Q and the tray's Quit exit.

**Windows: Alt+Tab and always-on-top.** `skipTaskbar` alone leaves the popup in Alt+Tab, and the windowing layer resets extended styles each time it shows the window. The popup reapplies `WS_EX_TOOLWINDOW` and topmost on every show (and once more ~80 ms later, after WebView2 settles).

**Windows: idle memory.** WebView2 runs ~6 processes. To meet "under 80 MB while idle", the GPU process is disabled (`--disable-gpu`; scrolling still holds 120 fps on the test machine) and the webview is set to `MemoryUsageTargetLevel = Low` while hidden. See the README for how this was measured.

**`tauri-plugin-window-state` is not used.** The popup is always centred on the monitor with the cursor, so restoring a saved position would contradict the spec.

**macOS NSPanel code is untested.** It follows the `tauri-nspanel` 2.1 API but was written on Windows; it needs a build and a check on a Mac. Linux is also untested.

## Dependencies not named in the spec

- `@tiptap/extension-list`: `TaskList`/`TaskItem` for checklists, which StarterKit doesn't include.
- `@tiptap/extensions`: where `Placeholder` lives in TipTap 3.
- `@tiptap/suggestion` (and its peer `@floating-ui/dom`), `@tiptap/pm`: required peers of the Emoji extension; Suggestion also drives `:` and `#` autocomplete.
- `tauri-plugin-opener` (official): used from Rust only, to open links and the export folder. The webview gets no opener permission.
- `windows`, `webview2-com`: already in the dependency tree via Tauri (same versions); used for the Windows popup behaviour and memory target above.

## Developer conveniences

- `QN_DATA_DIR` points the app at another data folder (benchmarks, tests).
- `QN_PERF=1` prints timing lines to stderr.
- Ctrl+Alt+Shift+S in a dev build seeds 100,000 notes (`seed_notes` refuses in release builds).
