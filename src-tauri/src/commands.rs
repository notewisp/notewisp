//! The Tauri commands: the only way the webview talks to the core.
//! Every command returns `Result<T, String>`; the frontend shows errors as a toast.

use std::fmt::Display;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::db::{self, Note, NoteBody, SearchHit, Settings};
use crate::{AppState, export, popup};

type CmdResult<T> = Result<T, String>;

fn err(e: impl Display) -> String {
    e.to_string()
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// --- Timeline -----------------------------------------------------------------

#[tauri::command]
pub fn list_notes(
    state: State<AppState>,
    before: Option<i64>,
    after: Option<i64>,
    limit: u32,
    tag: Option<String>,
) -> CmdResult<Vec<Note>> {
    let conn = state.db.lock().unwrap();
    db::list_notes(&conn, before, after, limit.min(1000), tag.as_deref()).map_err(err)
}

#[tauri::command]
pub fn notes_around(state: State<AppState>, id: i64, radius: u32, tag: Option<String>) -> CmdResult<Vec<Note>> {
    let conn = state.db.lock().unwrap();
    db::notes_around(&conn, id, radius.min(500), tag.as_deref()).map_err(err)
}

#[tauri::command]
pub fn get_note_json(state: State<AppState>, id: i64) -> CmdResult<String> {
    db::get_note_json(&state.db.lock().unwrap(), id).map_err(err)
}

// --- Writes -------------------------------------------------------------------

/// Saves a new note and clears the stored draft in the same step.
#[tauri::command]
pub fn create_note(state: State<AppState>, body: NoteBody) -> CmdResult<Note> {
    let mut conn = state.db.lock().unwrap();
    let note = db::create_note(&mut conn, &body, now_ms()).map_err(err)?;
    db::set_draft(&conn, None).map_err(err)?;
    Ok(note)
}

#[tauri::command]
pub fn update_note(state: State<AppState>, id: i64, body: NoteBody) -> CmdResult<Note> {
    db::update_note(&mut state.db.lock().unwrap(), id, &body, now_ms()).map_err(err)
}

#[tauri::command]
pub fn delete_note(state: State<AppState>, id: i64) -> CmdResult<()> {
    db::delete_note(&state.db.lock().unwrap(), id, now_ms()).map_err(err)
}

#[tauri::command]
pub fn restore_note(state: State<AppState>, id: i64) -> CmdResult<Note> {
    db::restore_note(&state.db.lock().unwrap(), id).map_err(err)
}

// --- Search -------------------------------------------------------------------

#[tauri::command]
pub fn search(state: State<AppState>, query: String, limit: u32) -> CmdResult<Vec<SearchHit>> {
    db::search(&state.db.lock().unwrap(), &query, limit.min(500)).map_err(err)
}

#[tauri::command]
pub fn list_tags(state: State<AppState>) -> CmdResult<Vec<(String, u32)>> {
    db::list_tags(&state.db.lock().unwrap()).map_err(err)
}

// --- Draft --------------------------------------------------------------------

#[tauri::command]
pub fn get_draft(state: State<AppState>) -> CmdResult<Option<String>> {
    db::get_draft(&state.db.lock().unwrap()).map_err(err)
}

#[tauri::command]
pub fn save_draft(state: State<AppState>, json: Option<String>) -> CmdResult<()> {
    db::set_draft(&state.db.lock().unwrap(), json.as_deref()).map_err(err)
}

// --- Settings -----------------------------------------------------------------

#[derive(Serialize)]
pub struct SettingsView {
    #[serde(flatten)]
    settings: Settings,
    /// Set when the hotkey could not be registered (e.g. another app owns it).
    hotkey_error: Option<String>,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> CmdResult<SettingsView> {
    let settings = db::get_settings(&state.db.lock().unwrap()).map_err(err)?;
    Ok(SettingsView { settings, hotkey_error: state.hotkey_error.lock().unwrap().clone() })
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<AppState>, s: Settings) -> CmdResult<()> {
    let old = db::get_settings(&state.db.lock().unwrap()).map_err(err)?;
    if s.hotkey != old.hotkey
        && let Err(e) = crate::register_hotkey(&app, &s.hotkey)
    {
        // Keep the old shortcut working.
        let _ = crate::register_hotkey(&app, &old.hotkey);
        return Err(format!("Couldn't use {}: {e}", s.hotkey));
    }
    if s.launch_at_login != old.launch_at_login {
        crate::set_launch_at_login(&app, s.launch_at_login).map_err(err)?;
    }
    db::set_settings(&state.db.lock().unwrap(), &s).map_err(err)
}

// --- Window and app -----------------------------------------------------------

/// Called once the UI has rendered. Shows the popup unless launched at login.
#[tauri::command]
pub fn app_ready(app: AppHandle, state: State<AppState>) -> f64 {
    let ms = state.started.elapsed().as_secs_f64() * 1000.0;
    crate::perf_log(&format!("cold start to ready: {ms:.1} ms"));
    if !state.autostarted {
        popup::show(&app);
    }
    ms
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    popup::hide(&app, true);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Milliseconds since the last hotkey press, for the latency budget.
#[tauri::command]
pub fn perf_since_hotkey(state: State<AppState>) -> Option<f64> {
    let t = state.hotkey_at.lock().unwrap().take()?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    crate::perf_log(&format!("hotkey to focused: {ms:.1} ms"));
    Some(ms)
}

#[tauri::command]
pub fn open_link(url: String) -> CmdResult<()> {
    let lower = url.trim().to_ascii_lowercase();
    if !["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
        return Err("Only web and mail links can be opened".into());
    }
    tauri_plugin_opener::open_url(url.trim(), None::<&str>).map_err(err)
}

// --- Maintenance --------------------------------------------------------------

/// Exports to a new folder under `exports/` in the app-data directory and
/// opens it. Returns the number of files written.
#[tauri::command]
pub fn export_markdown(state: State<AppState>) -> CmdResult<u32> {
    let conn = state.db.lock().unwrap();
    let (day, time) = db::local_now(&conn).map_err(err)?;
    let dir = state.data_dir.join("exports").join(format!("{day} {time}"));
    let n = export::export_markdown(&conn, &dir)?;
    drop(conn);
    let _ = tauri_plugin_opener::open_path(&dir, None::<&str>);
    Ok(n)
}

#[tauri::command]
pub fn seed_notes(state: State<AppState>, count: u32) -> CmdResult<()> {
    if cfg!(debug_assertions) {
        db::seed_notes(&mut state.db.lock().unwrap(), count.min(1_000_000), now_ms()).map_err(err)
    } else {
        Err("seed_notes is only available in dev builds".into())
    }
}
