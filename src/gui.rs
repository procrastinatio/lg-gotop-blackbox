//! Win32 GUI in a rose colour scheme with a flower icon (embedded as resource id 1, see `assets/`).
//!
//! Flow: type what happened -> optionally "Take screenshot" (the window minimizes, counts down,
//! captures the whole desktop, then comes back) -> "Save & send to support".

// nwg::Timer is deprecated (irregular ticks). That is fine here: the countdown is wall-clock based.
#![allow(deprecated)]

use crate::bundle;
use crate::config::Config;
use crate::diag;
use crate::screenshot;
use crate::snapshot::Snapshot;
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, InvalidateRect, SetBkMode, SetTextColor, HDC, TRANSPARENT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, IsIconic, LoadImageW, SendMessageW, SetClassLongPtrW, SetForegroundWindow,
    SetProcessDPIAware, ShowWindow, GCLP_HBRBACKGROUND, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON,
    SM_CYICON, SM_CYSMICON, STM_SETICON, SW_MINIMIZE, SW_RESTORE, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CTLCOLORSTATIC, WM_SETICON, WS_CHILD, WS_VISIBLE,
};

/// COLORREF is 0x00BBGGRR.
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
const BG: u32 = rgb(255, 240, 246); // soft rose
const TEXT: u32 = rgb(110, 20, 70); // deep plum
const HEADING: u32 = rgb(200, 30, 120); // magenta
const ICON_RESOURCE_ID: usize = 1;
/// Static-control style SS_ICON.
const SS_ICON: u32 = 0x0000_0003;
const TITLE: &str = "GOTOP Blackbox";
/// Choices offered for the screenshot delay, in seconds (index 1 = default).
const DELAYS: [u32; 3] = [3, 5, 10];

#[derive(Default, NwgUi)]
pub struct App {
    data: RefCell<Option<(Config, Snapshot)>>,
    /// When the screenshot is due (None = idle). Wall-clock based because nwg timers tick irregularly.
    deadline: Cell<Option<Instant>>,

    #[nwg_control(size: (600, 540), position: (300, 150), title: "GOTOP Blackbox", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [App::exit])]
    window: nwg::Window,

    #[nwg_layout(parent: window, spacing: 6, margin: [12, 14, 12, 14], max_row: Some(11), max_column: Some(12))]
    layout: nwg::GridLayout,

    // Rows 0-2, columns 0-1 are left free for the flower (drawn in `add_flower`).
    #[nwg_control(text: "GOTOP Blackbox")]
    #[nwg_layout_item(layout: layout, row: 0, col: 2, col_span: 10)]
    heading: nwg::Label,

    #[nwg_control(text: "Collects versions, today's logs and an optional screenshot for support.")]
    #[nwg_layout_item(layout: layout, row: 1, col: 2, col_span: 10, row_span: 2)]
    subtitle: nwg::Label,

    #[nwg_control(text: "What happened?")]
    #[nwg_layout_item(layout: layout, row: 3, col: 0, col_span: 12)]
    prompt: nwg::Label,

    #[nwg_control(flags: "VISIBLE|VSCROLL|AUTOVSCROLL|TAB_STOP")]
    #[nwg_layout_item(layout: layout, row: 4, col: 0, col_span: 12, row_span: 4)]
    message: nwg::TextBox,

    #[nwg_control(text: "Take screenshot...")]
    #[nwg_layout_item(layout: layout, row: 8, col: 0, col_span: 6)]
    #[nwg_events(OnButtonClick: [App::start_shot])]
    shot_btn: nwg::Button,

    #[nwg_control(collection: vec!["after 3 s", "after 5 s", "after 10 s"], selected_index: Some(1))]
    #[nwg_layout_item(layout: layout, row: 8, col: 6, col_span: 3)]
    delay: nwg::ComboBox<&'static str>,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: layout, row: 8, col: 9, col_span: 3)]
    status: nwg::Label,

    #[nwg_control(text: "Include the screenshot in the bundle")]
    #[nwg_layout_item(layout: layout, row: 9, col: 0, col_span: 12)]
    screenshot: nwg::CheckBox,

    #[nwg_control(text: "Save && send to support")]
    #[nwg_layout_item(layout: layout, row: 10, col: 0, col_span: 9)]
    #[nwg_events(OnButtonClick: [App::save])]
    save_btn: nwg::Button,

    #[nwg_control(text: "Cancel")]
    #[nwg_layout_item(layout: layout, row: 10, col: 9, col_span: 3)]
    #[nwg_events(OnButtonClick: [App::exit])]
    cancel_btn: nwg::Button,

    /// Polls the countdown a few times per second.
    #[nwg_control(parent: window, interval: 250, stopped: true)]
    #[nwg_events(OnTimerTick: [App::tick])]
    timer: nwg::Timer,
}

impl App {
    fn hwnd(&self) -> Option<HWND> {
        self.window.handle.hwnd().map(|h| HWND(h as *mut _))
    }

    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn start_shot(&self) {
        if self.deadline.get().is_some() {
            return;
        }
        let secs = DELAYS[self.delay.selection().unwrap_or(1).min(DELAYS.len() - 1)];
        self.deadline.set(Some(Instant::now() + Duration::from_secs(u64::from(secs))));
        self.shot_btn.set_enabled(false);
        self.status.set_text("");
        self.window.set_text(&format!("{TITLE} - screenshot in {secs} s"));
        if let Some(h) = self.hwnd() {
            unsafe {
                let _ = ShowWindow(h, SW_MINIMIZE);
            }
        }
        self.timer.start();
        diag::log(&format!("screenshot countdown started ({secs} s)"));
    }

    fn tick(&self) {
        let Some(deadline) = self.deadline.get() else {
            self.timer.stop();
            return;
        };
        let now = Instant::now();
        if now < deadline {
            let left = (deadline - now).as_secs_f32().ceil() as u32;
            self.window.set_text(&format!("{TITLE} - screenshot in {left} s"));
            return;
        }

        // Time to shoot. If the user brought our window back, hide it again and retry shortly.
        let hwnd = self.hwnd();
        if let Some(h) = hwnd {
            if unsafe { !IsIconic(h).as_bool() } {
                unsafe {
                    let _ = ShowWindow(h, SW_MINIMIZE);
                }
                self.deadline.set(Some(now + Duration::from_millis(700)));
                return;
            }
        }
        self.timer.stop();
        self.deadline.set(None);
        let result = screenshot::capture_png();
        if let Some(h) = hwnd {
            unsafe {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
        }
        self.window.set_text(TITLE);
        self.shot_btn.set_enabled(true);

        let mut guard = self.data.borrow_mut();
        let Some((_, snap)) = guard.as_mut() else { return };
        match result {
            Ok(png) => {
                diag::log(&format!("screenshot captured, {} KB", png.len() / 1024));
                self.status.set_text(&format!("\u{2713} {} KB", png.len() / 1024));
                snap.screenshot_png = Some(png);
                snap.screenshot_error = None;
                snap.screenshot_taken_at = Some(chrono::Local::now().to_rfc3339());
                self.screenshot.set_enabled(true);
                self.screenshot.set_check_state(nwg::CheckBoxState::Checked);
            }
            Err(e) => {
                diag::log(&format!("screenshot failed: {e:#}"));
                self.status.set_text("failed");
                snap.screenshot_error = Some(format!("{e:#}"));
                nwg::modal_error_message(&self.window, "Screenshot failed", &format!("{e:#}"));
            }
        }
    }

    fn save(&self) {
        let message = self.message.text();
        let include_wanted = self.screenshot.check_state() == nwg::CheckBoxState::Checked;
        // Scope the borrow: the dialogs below run a nested message loop.
        let result = {
            let mut guard = self.data.borrow_mut();
            let Some((cfg, snap)) = guard.as_mut() else { return };
            snap.refresh(cfg); // memory/processes as they are now, not at launch
            let include = include_wanted && snap.screenshot_png.is_some();
            bundle::write(cfg, snap, &message, include)
        };

        match result {
            Ok(res) => {
                diag::log(&format!("bundle written: {}", res.path.display()));
                let mut text = format!("Bundle saved to:\n{}", res.path.display());
                if res.used_fallback {
                    text.push_str("\n\nThe network share was NOT reachable: tell support where the files are.");
                }
                if !res.warnings.is_empty() {
                    text.push_str("\n\nNotes:\n- ");
                    text.push_str(&res.warnings.join("\n- "));
                }
                nwg::modal_info_message(&self.window, "Saved", &text);
                self.exit();
            }
            Err(e) => {
                diag::log(&format!("bundle failed: {e:#}"));
                nwg::modal_error_message(&self.window, "Could not save bundle", &format!("{e:#}"));
            }
        }
    }
}

/// Load the embedded flower icon at the given size. Failures are non-fatal (cosmetic).
unsafe fn load_icon(cx: i32, cy: i32) -> Option<windows::Win32::Foundation::HANDLE> {
    let inst = HINSTANCE(GetModuleHandleW(None).ok()?.0);
    let name = PCWSTR(ICON_RESOURCE_ID as *const u16); // MAKEINTRESOURCE
    match LoadImageW(inst, name, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR) {
        Ok(icon) => Some(icon),
        Err(e) => {
            diag::log(&format!("icon {cx}x{cy} not loaded: {e}"));
            None
        }
    }
}

/// Title-bar / taskbar icon.
fn set_window_icon(hwnd: HWND) {
    unsafe {
        for (kind, cx, cy) in [(0usize, SM_CXSMICON, SM_CYSMICON), (1usize, SM_CXICON, SM_CYICON)] {
            if let Some(icon) = load_icon(GetSystemMetrics(cx), GetSystemMetrics(cy)) {
                SendMessageW(hwnd, WM_SETICON, WPARAM(kind), LPARAM(icon.0 as isize));
            }
        }
    }
}

/// The flower next to the heading: a 64 px static icon control at the top left.
fn add_flower(parent: HWND) {
    const SIZE: i32 = 64;
    unsafe {
        let Some(icon) = load_icon(SIZE, SIZE) else { return };
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let ctl = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_ICON),
            16,
            24,
            SIZE,
            SIZE,
            parent,
            None,
            HINSTANCE(inst.0),
            None,
        );
        match ctl {
            Ok(ctl) => {
                SendMessageW(ctl, STM_SETICON, WPARAM(icon.0 as usize), LPARAM(0));
            }
            Err(e) => diag::log(&format!("flower control not created: {e}")),
        }
    }
}

/// Rose background and plum text for static controls (labels, checkbox text).
fn apply_theme(app: &App, hwnd: HWND) {
    let (bg_brush, heading_hwnd) = unsafe {
        let brush = CreateSolidBrush(COLORREF(BG));
        SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brush.0 as isize);
        let _ = InvalidateRect(hwnd, None, true);
        (brush.0 as isize, app.heading.handle.hwnd().map(|h| h as isize))
    };

    let handler = nwg::bind_raw_event_handler(&app.window.handle, 0x10000, move |_hwnd, msg, wparam, lparam| {
        if msg == WM_CTLCOLORSTATIC {
            let is_heading = Some(lparam) == heading_hwnd;
            unsafe {
                let hdc = HDC(wparam as *mut _);
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, COLORREF(if is_heading { HEADING } else { TEXT }));
            }
            return Some(bg_brush);
        }
        None
    });
    if let Err(e) = handler {
        diag::log(&format!("theme handler not installed: {e:?}"));
    }
}

pub fn run(cfg: Config, snap: Snapshot, selftest: bool) -> anyhow::Result<()> {
    // Same DPI behaviour as before the screenshot moved into the window: system-DPI aware.
    unsafe {
        let _ = SetProcessDPIAware();
    }
    nwg::init().map_err(|e| anyhow::anyhow!("GUI init failed: {e}"))?;
    nwg::Font::set_global_family("Segoe UI").ok();
    let app = App::build_ui(Default::default()).map_err(|e| anyhow::anyhow!("GUI build failed: {e}"))?;
    *app.data.borrow_mut() = Some((cfg, snap));
    app.screenshot.set_enabled(false); // enabled once a screenshot exists

    // Big heading font; must outlive the window.
    let mut heading_font = nwg::Font::default();
    nwg::Font::builder().size(30).weight(700).family("Segoe UI").build(&mut heading_font).ok();
    app.heading.set_font(Some(&heading_font));

    if let Some(hwnd) = app.hwnd() {
        set_window_icon(hwnd);
        add_flower(hwnd);
        apply_theme(&app, hwnd);
    }

    if selftest {
        // Exercise the screenshot path too (no countdown); a runner without a desktop may fail: log only.
        match screenshot::capture_png() {
            Ok(png) => diag::log(&format!("gui self-test: screenshot ok, {} KB", png.len() / 1024)),
            Err(e) => diag::log(&format!("gui self-test: screenshot failed: {e:#}")),
        }
        diag::log("gui self-test: window built, quitting");
        nwg::stop_thread_dispatch();
    }
    nwg::dispatch_thread_events();
    Ok(())
}
