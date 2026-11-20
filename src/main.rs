#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bundle;
mod config;
mod events;
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
    -h, --help          Show this help
";

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    headless: bool,
    message: String,
    no_screenshot: bool,
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
            "-h" | "--help" => {
                print_or_dialog("gotop-blackbox", HELP);
                return Ok(None);
            }
            other => anyhow::bail!("unknown argument: {other}\n\n{HELP}"),
        }
    }
    Ok(Some(a))
}

/// Release builds on Windows have no console, so surface text via a dialog there.
fn print_or_dialog(title: &str, text: &str) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        native_windows_gui::init().ok();
        native_windows_gui::simple_message(title, text);
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    {
        let _ = title;
        println!("{text}");
    }
}

fn run() -> Result<()> {
    let Some(args) = parse_args()? else { return Ok(()) };
    let cfg = config::Config::load(args.config.as_deref())?;
    let snap = snapshot::Snapshot::capture(&cfg, !args.no_screenshot);

    #[cfg(windows)]
    if !args.headless {
        return gui::run(cfg, snap);
    }

    let res = bundle::write(&cfg, &snap, &args.message, !args.no_screenshot)?;
    let mut out = format!("Bundle saved to {}", res.path.display());
    if res.used_fallback {
        out.push_str("\nnote: network share not used; bundle is local");
    }
    for w in &res.warnings {
        out.push_str(&format!("\nwarning: {w}"));
    }
    print_or_dialog("gotop-blackbox", &out);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        print_or_dialog("gotop-blackbox error", &format!("{e:#}"));
        std::process::exit(1);
    }
}
