#![cfg_attr(all(windows, not(debug_assertions), not(feature = "console")), windows_subsystem = "windows")]

mod bundle;
mod config;
mod diag;
mod events;
mod gotop_info;
#[cfg(windows)]
mod gui;
mod logs;
mod screenshot;
mod snapshot;

use anyhow::Result;
use std::path::PathBuf;

const HELP: &str = "\
gotop-blackbox - collect context, logs and a screenshot into a support bundle

USAGE:
    gotop-blackbox [OPTIONS]

OPTIONS:
    --config <FILE>     Config file (default: gotop-blackbox.toml next to the exe)
    --headless          No window: write the bundle immediately and print its path
    --message <TEXT>    Description to store (headless mode)
    --no-screenshot     Do not capture a screenshot
    --version           Show version
    -h, --help          Show this help

Diagnostics: progress and crashes are logged to %TEMP%\\gotop-blackbox.log
";

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    headless: bool,
    message: String,
    no_screenshot: bool,
    gui_selftest: bool,
}

fn parse_args() -> Result<Option<Args>> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => a.config = Some(it.next().ok_or_else(|| anyhow::anyhow!("--config needs a value"))?.into()),
            "--message" => a.message = it.next().ok_or_else(|| anyhow::anyhow!("--message needs a value"))?,
            "--headless" => a.headless = true,
            "--no-screenshot" => a.no_screenshot = true,
            "--gui-selftest" => a.gui_selftest = true, // build the window, then quit (CI)
            "--version" => {
                diag::notify("gotop-blackbox", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-h" | "--help" => {
                diag::notify("gotop-blackbox", HELP);
                return Ok(None);
            }
            other => anyhow::bail!("unknown argument: {other}\n\n{HELP}"),
        }
    }
    Ok(Some(a))
}

fn run() -> Result<()> {
    diag::log(&format!("start v{} args={:?}", env!("CARGO_PKG_VERSION"), std::env::args().skip(1).collect::<Vec<_>>()));
    let Some(args) = parse_args()? else { return Ok(()) };
    let cfg = config::Config::load(args.config.as_deref())?;
    diag::log("config loaded");
    // The GUI takes its (delayed) screenshot on request; headless mode captures right away.
    let headless = args.headless || cfg!(not(windows));
    let snap = snapshot::Snapshot::capture(&cfg, headless && !args.no_screenshot);
    diag::log(&format!(
        "snapshot captured on {} ({}); screenshot: {}",
        snap.host,
        snap.machine_kind,
        match (&snap.screenshot_png, &snap.screenshot_error) {
            (Some(p), _) => format!("ok, {} KB", p.len() / 1024),
            (None, Some(e)) => format!("failed: {e}"),
            (None, None) => "skipped".into(),
        }
    ));

    #[cfg(windows)]
    if !args.headless {
        diag::log("opening window");
        let r = gui::run(cfg, snap, args.gui_selftest);
        diag::log(&format!("window closed: {}", if r.is_ok() { "ok".to_string() } else { format!("{r:?}") }));
        return r;
    }

    let res = bundle::write(&cfg, &snap, &args.message, !args.no_screenshot)?;
    diag::log(&format!("bundle written: {}", res.path.display()));
    let mut out = format!("Bundle saved to {}", res.path.display());
    if res.used_fallback {
        out.push_str("\nnote: network share not used; bundle is local");
    }
    for w in &res.warnings {
        out.push_str(&format!("\nwarning: {w}"));
    }
    diag::notify("gotop-blackbox", &out);
    Ok(())
}

fn main() {
    diag::install_panic_hook();
    if let Err(e) = run() {
        diag::log(&format!("error: {e:#}"));
        diag::notify("gotop-blackbox error", &format!("{e:#}"));
        std::process::exit(1);
    }
}
