//! Minimal Win32 GUI: one text box, one checkbox, one button.

use crate::bundle;
use crate::diag;
use crate::config::Config;
use crate::snapshot::Snapshot;
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;
use std::cell::RefCell;

#[derive(Default, NwgUi)]
pub struct App {
    data: RefCell<Option<(Config, Snapshot)>>,

    #[nwg_control(size: (520, 380), position: (300, 200), title: "GOTOP Blackbox", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [App::exit])]
    window: nwg::Window,

    #[nwg_layout(parent: window, spacing: 6, margin: [10, 10, 10, 10])]
    layout: nwg::GridLayout,

    #[nwg_control(text: "What happened?")]
    #[nwg_layout_item(layout: layout, row: 0, col: 0, col_span: 4)]
    prompt: nwg::Label,

    #[nwg_control(flags: "VISIBLE|VSCROLL|AUTOVSCROLL|TAB_STOP")]
    #[nwg_layout_item(layout: layout, row: 1, col: 0, col_span: 4, row_span: 5)]
    message: nwg::TextBox,

    #[nwg_control(text: "Include screenshot taken at launch", check_state: CheckBoxState::Checked)]
    #[nwg_layout_item(layout: layout, row: 6, col: 0, col_span: 4)]
    screenshot: nwg::CheckBox,

    #[nwg_control(text: "Save && send to support")]
    #[nwg_layout_item(layout: layout, row: 7, col: 0, col_span: 3)]
    #[nwg_events(OnButtonClick: [App::save])]
    save_btn: nwg::Button,

    #[nwg_control(text: "Cancel")]
    #[nwg_layout_item(layout: layout, row: 7, col: 3)]
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

pub fn run(cfg: Config, snap: Snapshot) -> anyhow::Result<()> {
    nwg::init().map_err(|e| anyhow::anyhow!("GUI init failed: {e}"))?;
    nwg::Font::set_global_family("Segoe UI").ok();
    let app = App::build_ui(Default::default()).map_err(|e| anyhow::anyhow!("GUI build failed: {e}"))?;
    *app.data.borrow_mut() = Some((cfg, snap));
    nwg::dispatch_thread_events();
    Ok(())
}
