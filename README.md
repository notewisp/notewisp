# NoteWisp

A desktop popup for capturing thoughts as fast as messaging yourself. Press a hotkey, type, press Enter, and you're back where you were. Tauri 2 + Svelte 5, with every note in one local SQLite file.

## Requirements

- Node.js 20+ and npm
- Rust (stable, 1.88+) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

## Run

```sh
npm install
npm run app:dev
```

`app:dev` runs as a separate app, **NoteWisp Dev** (`com.notewisp.desktop.dev`), with its own data folder, single-instance lock and autostart entry. It can run next to an installed NoteWisp without touching its notes.

The popup appears when the app is ready. After that, **Alt+N** shows and hides it from anywhere, and the tray icon offers Show, Settings and Quit.

## Build

```sh
npm run app:build        # installers in src-tauri/target/release/bundle/
```

`app:build` skips the signed update bundles, so it works without the signing key. Real releases are built and signed by GitHub Actions (see [Releases and updates](#releases-and-updates)).

## Test

```sh
cd src-tauri && cargo test   # database, export and backup tests (in-memory / temp SQLite)
npm run build                # svelte-check type check + frontend bundle
```

## Seed test data

In a dev build, press **Ctrl+Alt+Shift+S** inside the popup to add 100,000 random notes (about 7 s). To keep them out of your real notes, point the app at a scratch folder first:

```sh
NOTEWISP_DATA_DIR=/tmp/notewisp-bench npm run app:dev
```

## Data

| OS      | Folder                                                      |
| ------- | ----------------------------------------------------------- |
| Windows | `%APPDATA%\com.notewisp.desktop\`                     |
| macOS   | `~/Library/Application Support/com.notewisp.desktop/` |
| Linux   | `~/.local/share/com.notewisp.desktop/`                |

`npm run app:dev` uses the same paths with `com.notewisp.desktop.dev`.

- `notes.db`: all notes (SQLite, WAL mode)
- `backups/notes-YYYY-MM-DD.db`: daily backup, last 7 kept
- `exports/<date> <time>/YYYY-MM-DD.md`: Markdown exports (Ctrl+E)

Deleted notes can be undone for 5 seconds, stay hidden for 30 days, then are purged.

## Keyboard

| Shortcut | Where | Action |
| --- | --- | --- |
| Alt+N | Anywhere | Show / hide (configurable in Settings) |
| Enter / Shift+Enter | Editor | Save / new line |
| Ctrl+Enter | Editor | Save, even inside a list or code block |
| Up | Empty editor | Edit the most recent note |
| Esc | Anywhere | Close the innermost thing (popover, edit, search, filter), else hide |
| Ctrl+F | Popup | Search (`#tag` filters, combinable with words) |
| Up / Down, Enter | Search | Move between results, jump to note |
| Delete | Selected note | Delete (with Undo) |
| Ctrl+B / Ctrl+I / Ctrl+K | Editor | Bold / italic / link |
| Ctrl+Alt+1–3 | Editor | Heading 1–3 |
| `:` + name, `#` + tag | Editor | Emoji / tag autocomplete |
| Ctrl+E / Ctrl+, / Ctrl+Q | Popup | Export / settings / quit |

Cmd replaces Ctrl on macOS. Markdown shortcuts: `## `, `- `, `1. `, `[ ] `, `> `, `**bold**`, `` `code` ``.

## Performance budgets

Measured on the development machine (Windows 10, 120 Hz display) with 100,000 seeded notes, release build. The spec asks for a mid-range laptop; this machine's class wasn't checked, so treat the numbers as indicative. Each was measured from outside the code under test, against the running app:

| Budget | Target | Measured | How |
| --- | --- | --- | --- |
| Hotkey to visible and focused | < 50 ms | p50 11.5 ms, max 13.7 ms (20 presses) | `scripts/os-budgets.ps1`: real key events, polls until the popup is visible and the foreground window |
| Cold start to usable window | < 500 ms | 361–392 ms (6 launches) | same script: process launch until the popup (shown only after notes and draft are loaded) is visible and focused |
| Enter to note visible | < 16 ms | p50 3.4 ms, max 9.8 ms (30 notes) | `scripts/budgets.mjs`: keydown to the next animation frame after the note is in the DOM |
| Search after a keystroke | < 50 ms | p50 3.6 ms, max 16.7 ms (45 keystrokes); 13.6 ms worst in Rust alone | `scripts/budgets.mjs` (keystroke to results in the UI); `cargo test --release -- --ignored budgets` |
| Scrolling | 60 fps, no blanking | 120 fps median, 0 blank frames; 1 frame of 600 at 23.5 ms while a page of older notes loaded | `scripts/scroll-bench.js`: 1,200 frames, rows checked against the viewport every frame |
| Idle memory in tray | < 80 MB | 58.6 MB | private working set (Task Manager's "Memory") of the app and its 6 WebView2 processes; working set incl. shared pages is 255 MB |
| Installer size | < 15 MB | 2.4 MB (NSIS), 4.2 MB (MSI) | `npm run tauri build` |
| Queries with `"`, `*`, `(`, `-` | never error | no errors | `hostile_queries_never_error` test + `budgets.mjs` |
| Export round trip | each note once | 100,005 of 100,005 | `export_round_trip_every_note_exactly_once` test + a full export of the seeded database |

To reproduce on Windows:

```sh
# 1. Seed a scratch database with a dev build (Ctrl+Alt+Shift+S), then quit it.
NOTEWISP_DATA_DIR=C:/tmp/notewisp-bench npm run app:dev
# 2. OS-level budgets (cold start, hotkey, idle memory) on the release build.
NOTEWISP_DATA_DIR=C:/tmp/notewisp-bench powershell -File scripts/os-budgets.ps1 -Exe src-tauri/target/release/notewisp.exe
# 3. In-webview budgets (search, Enter, scrolling): start with remote debugging, then run the script.
NOTEWISP_DATA_DIR=C:/tmp/notewisp-bench WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 src-tauri/target/release/notewisp.exe &
node scripts/budgets.mjs
```

## Releases and updates

Production is [GitHub Releases](https://github.com/notewisp/notewisp/releases). Installed apps check `releases/latest/download/latest.json` a minute after starting and then every 6 hours, download a newer version in the background, and install it once the popup has been hidden for 2 minutes (`src-tauri/src/update.rs`). Only the Windows NSIS installer and the Linux AppImage update themselves; `.msi`, `.deb` and `.rpm` installs don't check. Dev builds (`app:dev`) and runs with `NOTEWISP_DATA_DIR` never update.

To release:

```sh
npm version patch            # or minor / major: bumps package.json, commits, tags v0.1.1
git push --follow-tags       # the tag starts .github/workflows/release.yml
```

1. GitHub Actions builds Windows and Linux, signs the update bundles and attaches everything to a **draft** release. Drafts are invisible to installed apps.
2. Staging: download the draft's installer from the release page, install it over your copy and try it.
3. Production: press **Publish release**. Installed apps pick it up within 6 hours.

There's no rolling back an update: if a release is broken, ship a fixed, higher version.

Updates are signed with the key in `~/.tauri/notewisp.key` (password and backup in the maintainer's password manager, copies in the repo's Actions secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). The public half is `plugins.updater.pubkey` in `tauri.conf.json`. **Losing the key or its password means installed apps can never update again.**

## Platform status

- **Windows:** built and tested end to end (hotkey, Esc returning focus, click-outside, Alt+Tab exclusion, tray, single instance, launch at login).
- **macOS:** the NSPanel code compiles only on macOS and has not been built or run yet.
- **Linux:** not tested. On Wayland, global hotkeys and always-on-top depend on the desktop.

See [DECISIONS.md](DECISIONS.md) for where the implementation departs from the spec.

## License

Copyright (C) 2026 The NoteWisp contributors.

NoteWisp is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. It is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; see [LICENSE](LICENSE) for details.

