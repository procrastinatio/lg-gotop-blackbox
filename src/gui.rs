//! Win32 GUI: heading, one text box, one checkbox, two buttons, in a rose colour scheme
//! with a flower icon (embedded as resource id 1, see `assets/`).

use crate::bundle;
use crate::config::Config;
use crate::diag;
use crate::snapshot::Snapshot;
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;
use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, InvalidateRect, SetBkMode, SetTextColor, HDC, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, SetClassLongPtrW, GCLP_HBRBACKGROUND, IMAGE_ICON, LR_DEFAULTCOLOR,
    SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_CTLCOLORSTATIC, WM_SETICON,
};

/// COLORREF is 0x00BBGGRR.
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
const BG: u32 = rgb(255, 240, 246); // soft rose
const TEXT: u32 = rgb(110, 20, 70); // deep plum
const HEADING: u32 = rgb(200, 30, 120); // magenta
const ICON_RESOURCE_ID: usize = 1;

#[derive(Default, NwgUi)]
pub struct App {
    data: RefCell<Option<(Config, Snapshot)>>,

    #[nwg_control(size: (540, 470), position: (300, 150), title: "GOTOP Blackbox", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [App::exit])]
    window: nwg::Window,

    #[nwg_layout(parent: window, spacing: 6, margin: [12, 14, 12, 14])]
    layout: nwg::GridLayout,

    #[nwg_control(text: "GOTOP Blackbox")]
    #[nwg_layout_item(layout: layout, row: 0, col: 0, col_span: 4)]
    heading: nwg::Label,

    #[nwg_control(text: "Collects versions, logs and a screenshot for support.")]
    #[nwg_layout_item(layout: layout, row: 1, col: 0, col_span: 4)]
    subtitle: nwg::Label,

    #[nwg_control(text: "What happened?")]
    #[nwg_layout_item(layout: layout, row: 2, col: 0, col_span: 4)]
    prompt: nwg::Label,

    #[nwg_control(flags: "VISIBLE|VSCROLL|AUTOVSCROLL|TAB_STOP")]
    #[nwg_layout_item(layout: layout, row: 3, col: 0, col_span: 4, row_span: 4)]
    message: nwg::TextBox,

    #[nwg_control(text: "Include screenshot taken at launch", check_state: CheckBoxState::Checked)]
    #[nwg_layout_item(layout: layout, row: 7, col: 0, col_span: 4)]
    screenshot: nwg::CheckBox,

    #[nwg_control(text: "Save && send to support")]
    #[nwg_layout_item(layout: layout, row: 8, col: 0, col_span: 3)]
    #[nwg_events(OnButtonClick: [App::save])]
    save_btn: nwg::Button,

    #[nwg_control(text: "Cancel")]
    #[nwg_layout_item(layout: layout, row: 8, col: 3)]
    #[nwg_events(OnButtonClick: [App::exit])]
    cancel_btn: nwg::Button,
}

impl App {
    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn save(&self) {
        let guard = self.data.borrow();
        let Some((cfg, snap)) = guard.as_ref() else { return };
        let include = self.screenshot.check_state() == nwg::CheckBoxState::Checked;

        match bundle::write(cfg, snap, &self.message.text(), include) {
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

/// Title-bar / taskbar icon from the embedded resource. Failures are non-fatal (cosmetic).
fn set_window_icon(hwnd: HWND) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let inst = windows::Win32::Foundation::HINSTANCE(module.0);
        let name = windows::core::PCWSTR(ICON_RESOURCE_ID as *const u16); // MAKEINTRESOURCE
        for (kind, cx, cy) in [(0usize, SM_CXSMICON, SM_CYSMICON), (1usize, SM_CXICON, SM_CYICON)] {
            match LoadImageW(inst, name, IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_DEFAULTCOLOR) {
                Ok(icon) => {
                    SendMessageW(hwnd, WM_SETICON, WPARAM(kind), LPARAM(icon.0 as isize));
                }
                Err(e) => diag::log(&format!("window icon not loaded: {e}")),
            }
        }
    }
}

/// Rose background and plum text for static controls (labels, checkbox text).
fn apply_theme(app: &App, hwnd: HWND) {
    let (bg_brush, heading_hwnd) = unsafe {
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(BG));
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
                SetTextColor(hdc, windows::Win32::Foundation::COLORREF(if is_heading { HEADING } else { TEXT }));
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
    nwg::init().map_err(|e| anyhow::anyhow!("GUI init failed: {e}"))?;
    nwg::Font::set_global_family("Segoe UI").ok();
    let app = App::build_ui(Default::default()).map_err(|e| anyhow::anyhow!("GUI build failed: {e}"))?;
    *app.data.borrow_mut() = Some((cfg, snap));

    // Big heading font; must outlive the window.
    let mut heading_font = nwg::Font::default();
    nwg::Font::builder()
        .size(30)
        .weight(700)
        .family("Segoe UI")
        .build(&mut heading_font)
        .ok();
    app.heading.set_font(Some(&heading_font));

    if let Some(h) = app.window.handle.hwnd() {
        let hwnd = HWND(h as *mut _);
        set_window_icon(hwnd);
        apply_theme(&app, hwnd);
    }

    if selftest {
        diag::log("gui self-test: window built, quitting");
        nwg::stop_thread_dispatch();
    }
    nwg::dispatch_thread_events();
    Ok(())
}
