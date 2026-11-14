//! The popup window: created hidden at startup, never destroyed, shown and
//! hidden by the hotkey, Esc, the tray, and focus loss.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

pub const LABEL: &str = "main";

/// Focus loss this soon after showing is the OS settling focus, not the user leaving.
const FOCUS_GRACE: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct PopupState {
    shown_at: Mutex<Option<Instant>>,
    hidden_at: Mutex<Option<Instant>>,
    /// The window that was in front before we showed, to hand focus back on hide.
    #[cfg(windows)]
    previous: Mutex<isize>,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// One-time platform setup of the hidden window.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = window(app) else { return Ok(()) };
    win.set_always_on_top(true)?;
    #[cfg(windows)]
    windows_impl::hide_from_alt_tab(&win)?;
    #[cfg(target_os = "macos")]
    macos::init(app, &win)?;
    let _ = win;
    Ok(())
}

pub fn is_visible(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(visible) = macos::is_visible(app) {
        return visible;
    }
    window(app).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

/// True when the popup is hidden and has been for at least `d` (or was never shown).
pub fn hidden_for(app: &AppHandle, d: Duration) -> bool {
    if is_visible(app) {
        return false;
    }
    let hidden_at = *app.state::<PopupState>().hidden_at.lock().unwrap();
    hidden_at.is_none_or(|t| t.elapsed() >= d)
}

pub fn toggle(app: &AppHandle) {
    if is_visible(app) {
        hide(app, true);
    } else {
        show(app);
    }
}

/// Tray click: clicking the tray takes focus from the popup, which hides it
/// just before the click arrives. Treat that as "hide", not "show again".
pub fn toggle_from_tray(app: &AppHandle) {
    let just_hidden = app
        .state::<PopupState>()
        .hidden_at
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(400));
    if !just_hidden {
        toggle(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let state = app.state::<PopupState>();
    #[cfg(windows)]
    {
        *state.previous.lock().unwrap() = windows_impl::foreground();
    }
    center_on_cursor_monitor(&win);
    *state.shown_at.lock().unwrap() = Some(Instant::now());

    #[cfg(target_os = "macos")]
    if macos::show(app) {
        return;
    }
    #[cfg(windows)]
    windows_impl::set_memory_low(&win, false);
    let _ = win.show();
    let _ = win.set_focus();
    // Showing rewrites the window's extended styles, and the topmost bit is
    // dropped again a few ms later while the webview settles. Reapply ours on
    // the event-loop thread right after the show and once more shortly after.
    #[cfg(windows)]
    {
        let (w, app) = (win.clone(), app.clone());
        std::thread::spawn(move || {
            for delay in [0, 80] {
                std::thread::sleep(Duration::from_millis(delay));
                let w = w.clone();
                let _ = app.run_on_main_thread(move || {
                    if w.is_visible().unwrap_or(false) {
                        let _ = windows_impl::hide_from_alt_tab(&w);
                    }
                });
            }
        });
    }
}

/// Hides the popup. `restore_focus` hands focus back to the app that was in
/// front before (Esc, hotkey); a click on another app keeps that app in front.
pub fn hide(app: &AppHandle, restore_focus: bool) {
    let Some(win) = window(app) else { return };
    let state = app.state::<PopupState>();
    *state.shown_at.lock().unwrap() = None;
    *state.hidden_at.lock().unwrap() = Some(Instant::now());

    #[cfg(target_os = "macos")]
    {
        let _ = restore_focus;
        if macos::hide(app) {
            return;
        }
    }
    let _ = win.hide();
    #[cfg(windows)]
    {
        if restore_focus {
            windows_impl::set_foreground(*state.previous.lock().unwrap());
        }
        windows_impl::set_memory_low(&win, true);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = restore_focus;
}

/// Called when the popup loses keyboard focus (click outside, Alt+Tab).
pub fn on_focus_lost(app: &AppHandle) {
    let shown_at = *app.state::<PopupState>().shown_at.lock().unwrap();
    match shown_at {
        Some(t) if t.elapsed() < FOCUS_GRACE => {}
        Some(_) => hide(app, false),
        None => {}
    }
}

fn center_on_cursor_monitor(win: &WebviewWindow) {
    let Ok(cursor) = win.cursor_position() else { return };
    let monitors = win.available_monitors().unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| {
            let (p, s) = (m.position(), m.size());
            cursor.x >= p.x as f64
                && cursor.y >= p.y as f64
                && cursor.x < (p.x + s.width as i32) as f64
                && cursor.y < (p.y + s.height as i32) as f64
        })
        .cloned()
        .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, win.outer_size()) else { return };

    // The window's physical size follows the target monitor's scale factor once moved.
    let scale = monitor.scale_factor() / win.scale_factor().unwrap_or(1.0);
    let (w, h) = (size.width as f64 * scale, size.height as f64 * scale);
    let (p, s) = (monitor.position(), monitor.size());
    let x = p.x as f64 + (s.width as f64 - w) / 2.0;
    let y = p.y as f64 + (s.height as f64 - h) / 2.0;
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

#[cfg(windows)]
mod windows_impl {
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, HWND_TOPMOST, IsWindow,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    /// A topmost tool window stays above other windows and out of the taskbar and Alt+Tab.
    pub fn hide_from_alt_tab(win: &WebviewWindow) -> tauri::Result<()> {
        let hwnd = HWND(win.hwnd()?.0);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let want = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            if want != ex {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
            }
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    /// Asks WebView2 to trim memory while hidden (caches, heaps), and to go
    /// back to normal before showing.
    pub fn set_memory_low(win: &WebviewWindow, low: bool) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            ICoreWebView2_19,
        };
        use windows::core::Interface;
        let _ = win.with_webview(move |wv| unsafe {
            let Ok(core) = wv.controller().CoreWebView2() else { return };
            if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                let level = if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                };
                let _ = core.SetMemoryUsageTargetLevel(level);
            }
        });
    }

    pub fn foreground() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn set_foreground(hwnd: isize) {
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            if !hwnd.is_invalid() && IsWindow(Some(hwnd)).as_bool() {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

/// macOS: an NSPanel so the popup appears over full-screen apps and on the
/// current Space without switching to it. Not compiled on Windows/Linux.
#[cfg(target_os = "macos")]
mod macos {
    use tauri::{AppHandle, WebviewWindow};
    use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt, tauri_panel};

    tauri_panel! {
        panel!(PopupPanel {
            config: {
                can_become_key_window: true,
                is_floating_panel: true
            }
        })

        panel_event!(PopupPanelEvents {
            window_did_resign_key(notification: &NSNotification) -> ()
        })
    }

    pub fn init(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
        let panel = win.to_panel::<PopupPanel>()?;
        panel.set_level(PanelLevel::Status.value());
        let _ = panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
        panel.set_collection_behavior(
            CollectionBehavior::new().can_join_all_spaces().full_screen_auxiliary().into(),
        );
        let events = PopupPanelEvents::new();
        let handle = app.clone();
        events.window_did_resign_key(move |_| super::on_focus_lost(&handle));
        panel.set_event_handler(Some(events.as_ref()));
        Ok(())
    }

    pub fn is_visible(app: &AppHandle) -> Option<bool> {
        app.get_webview_panel(super::LABEL).ok().map(|p| p.is_visible())
    }

    pub fn show(app: &AppHandle) -> bool {
        app.get_webview_panel(super::LABEL).map(|p| p.show_and_make_key()).is_ok()
    }

    pub fn hide(app: &AppHandle) -> bool {
        app.get_webview_panel(super::LABEL).map(|p| p.hide()).is_ok()
    }
}
