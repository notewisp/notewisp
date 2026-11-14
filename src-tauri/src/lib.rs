mod commands;
mod db;
mod export;
mod popup;
mod update;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const AUTOSTART_ARG: &str = "--autostart";
const BACKUPS_KEPT: usize = 7;

pub struct AppState {
    /// The single database connection. Only the Rust core touches SQLite.
    pub db: Mutex<rusqlite::Connection>,
    pub data_dir: PathBuf,
    pub started: Instant,
    pub autostarted: bool,
    pub hotkey_at: Mutex<Option<Instant>>,
    pub hotkey_error: Mutex<Option<String>>,
    hotkey: Mutex<Option<String>>,
}

/// Timing lines for the performance budgets, printed when NOTEWISP_PERF is set.
pub fn perf_log(line: &str) {
    if std::env::var_os("NOTEWISP_PERF").is_some() {
        eprintln!("[perf] {line}");
    }
}

/// Replaces the global hotkey that toggles the popup.
pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let shortcuts = app.global_shortcut();
    if let Some(old) = state.hotkey.lock().unwrap().take() {
        let _ = shortcuts.unregister(old.as_str());
    }
    let result = shortcuts.register(hotkey).map_err(|e| e.to_string());
    if result.is_ok() {
        *state.hotkey.lock().unwrap() = Some(hotkey.to_owned());
    }
    *state.hotkey_error.lock().unwrap() = result.clone().err();
    result
}

pub fn set_launch_at_login(app: &AppHandle, on: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let r = if on { launcher.enable() } else { launcher.disable() };
    r.map_err(|e| e.to_string())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("tray")
        .tooltip(format!("NoteWisp {}", app.package_info().version))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => popup::show(app),
            "settings" => {
                popup::show(app);
                let _ = app.emit("open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                popup::toggle_from_tray(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Daily backup (keeping the last 7) and purge of notes deleted 30+ days ago.
/// Runs on its own connection so the UI never waits on it.
fn maintenance(db_path: &Path, backups: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::purge_deleted(&conn, commands::now_ms()).map_err(|e| e.to_string())?;

    std::fs::create_dir_all(backups).map_err(|e| e.to_string())?;
    let (today, _) = db::local_now(&conn).map_err(|e| e.to_string())?;
    let target = backups.join(format!("notes-{today}.db"));
    if !target.exists() {
        let tmp = backups.join(format!("notes-{today}.db.tmp"));
        let _ = std::fs::remove_file(&tmp);
        db::backup_to(&conn, &tmp).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    }

    // Names sort by date, so the oldest come first.
    let mut files: Vec<PathBuf> = std::fs::read_dir(backups)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("notes-") && name.ends_with(".db")
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(BACKUPS_KEPT);
    for old in &files[..excess] {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let started = Instant::now();
    let autostarted = std::env::args().any(|a| a == AUTOSTART_ARG);

    tauri::Builder::default()
        // Must be first: a second launch just shows the running instance's popup.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| popup::show(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        *app.state::<AppState>().hotkey_at.lock().unwrap() = Some(Instant::now());
                        popup::toggle(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                app.handle().plugin(tauri_nspanel::init())?;
            }

            // NOTEWISP_DATA_DIR points benchmarks and tests at a throwaway database.
            let dir = match std::env::var_os("NOTEWISP_DATA_DIR") {
                Some(d) => PathBuf::from(d),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("notes.db");
            let conn = db::open(&db_path)?;
            let settings = db::get_settings(&conn)?;
            app.manage(AppState {
                db: Mutex::new(conn),
                data_dir: dir.clone(),
                started,
                autostarted,
                hotkey_at: Mutex::new(None),
                hotkey_error: Mutex::new(None),
                hotkey: Mutex::new(None),
            });
            app.manage(popup::PopupState::default());

            let handle = app.handle();
            popup::init(handle)?;
            if let Err(e) = register_hotkey(handle, &settings.hotkey) {
                eprintln!("hotkey {} unavailable: {e}", settings.hotkey);
            }
            build_tray(handle)?;
            update::spawn(handle.clone());

            std::thread::spawn(move || {
                let backups = dir.join("backups");
                loop {
                    if let Err(e) = maintenance(&db_path, &backups) {
                        eprintln!("maintenance failed: {e}");
                    }
                    std::thread::sleep(Duration::from_secs(60 * 60));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The popup is never destroyed; closing just hides it.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                popup::hide(window.app_handle(), true);
            }
            WindowEvent::Focused(false) => popup::on_focus_lost(window.app_handle()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_notes,
            commands::notes_around,
            commands::get_note_json,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::restore_note,
            commands::search,
            commands::list_tags,
            commands::get_draft,
            commands::save_draft,
            commands::get_settings,
            commands::set_settings,
            commands::app_ready,
            commands::hide_popup,
            commands::quit_app,
            commands::perf_since_hotkey,
            commands::open_link,
            commands::export_markdown,
            commands::seed_notes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_backs_up_once_a_day_and_keeps_seven() {
        let dir = std::env::temp_dir().join(format!("notewisp-maint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let backups = dir.join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for d in 1..=9 {
            std::fs::write(backups.join(format!("notes-2020-01-0{d}.db")), "old").unwrap();
        }
        let db_path = dir.join("notes.db");
        drop(db::open(&db_path).unwrap());

        maintenance(&db_path, &backups).unwrap();
        maintenance(&db_path, &backups).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), BACKUPS_KEPT);
        assert_eq!(names[0], "notes-2020-01-04.db");
        // Today's backup is a real database.
        let today = backups.join(names.last().unwrap());
        assert!(db::open(&today).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
