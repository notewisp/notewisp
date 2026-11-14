//! Silent auto-update from GitHub Releases.
//!
//! A minute after startup and then every 6 hours, ask the endpoint in tauri.conf.json
//! (`latest.json` of the latest published release) whether there's a newer version. If
//! there is, download it in the background (the updater plugin checks its signature
//! against the public key in tauri.conf.json), wait until the popup has been hidden for a
//! while so nobody is mid-note, then install and restart.
//!
//! Draft releases are invisible to the endpoint, which is what makes them a safe staging
//! step: only publishing a release reaches installed apps.

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::popup;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How long the popup must have been hidden before an update may restart the app.
/// Drafts are saved 300 ms after the last keystroke, so this is plenty.
const IDLE_BEFORE_INSTALL: Duration = Duration::from_secs(2 * 60);
const IDLE_POLL: Duration = Duration::from_secs(30);

/// Whether this copy of the app should update itself.
fn enabled() -> bool {
    // `npm run app:dev` must never replace itself with a release.
    if cfg!(debug_assertions) {
        return false;
    }
    // Benchmarks and tests run release builds against a throwaway database; an update would
    // install over the real app instead.
    if std::env::var_os("NOTEWISP_DATA_DIR").is_some() {
        return false;
    }
    // On Linux only the AppImage can replace itself; .deb and .rpm installs update through the
    // package manager. The AppImage runtime sets APPIMAGE to the file's path.
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none() {
        return false;
    }
    true
}

/// Starts the background update loop (a no-op when updates are disabled).
pub fn spawn(app: AppHandle) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            match tauri::async_runtime::block_on(fetch(&app)) {
                Ok(Some((update, bytes))) => install_when_idle(&app, &update, &bytes),
                Ok(None) => {}
                // Offline, GitHub down, bad signature...: try again next round.
                Err(e) => eprintln!("update check failed: {e}"),
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// Checks for a newer version and downloads it. `None` when already up to date.
async fn fetch(app: &AppHandle) -> tauri_plugin_updater::Result<Option<(Update, Vec<u8>)>> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(None);
    };
    let bytes = update.download(|_, _| {}, || {}).await?;
    Ok(Some((update, bytes)))
}

fn install_when_idle(app: &AppHandle, update: &Update, bytes: &[u8]) {
    while !popup::hidden_for(app, IDLE_BEFORE_INSTALL) {
        std::thread::sleep(IDLE_POLL);
    }
    // On Windows this starts the installer and exits; the installer restarts the app with the
    // same arguments (so an autostarted copy stays in the tray). On Linux it replaces the
    // AppImage file, and we restart ourselves.
    if let Err(e) = update.install(bytes) {
        eprintln!("installing update {} failed: {e}", update.version);
        return;
    }
    app.restart();
}
