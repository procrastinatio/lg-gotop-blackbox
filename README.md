# gotop-blackbox

A tiny "flight recorder" for GOTOP / ArcGIS Pro. When something goes wrong, the user clicks one icon,
types a sentence, and everything support needs lands in a timestamped folder on a network share.

## Why

GOTOP runs in a nested VDI (or on a workstation) where the clipboard is locked down, moving a file
across hops takes minutes, and e-mail is not available. "Just send us the logs" does not work.
Support also needs the *state at the moment of failure*, which users cannot reconstruct afterwards.

## What it collects

Into `<share>\2026-11-20_1130_HOSTNAME_user\`:

| File | Content |
|---|---|
| `description.txt` | What the user typed |
| `screenshot.png` | Optional, whole desktop: press "Take screenshot..." in the window (3/5/10 s delay; the window minimizes meanwhile so you can bring the problem on screen) |
| `report.json` | Timestamp, host, user, OS, RAM, ArcGIS/GOTOP process memory, warnings |
| `logs/<source>/*.tail.txt` | Last N lines of the most recent log files per configured source |
| `events-application.txt` | Recent Error/Warning entries from the Windows Application log |

If the share is unreachable the bundle is written locally (`%TEMP%\gotop-blackbox`) and the user is told.

## Why Rust (and not ArcGIS Python)

- **Starts instantly.** An `arcpy`-based script takes ~45 s (licence checks) before showing anything.
- **Survives ArcGIS upgrades.** The bundled Python directory changes with every ArcGIS release.
- **One static `.exe`**: no runtime, no installer, no admin rights, no third-party DLLs.
- Plain Win32 (GDI screenshot, native widgets): works inside RDP/Citrix/VDI sessions.

## Usage

```
gotop-blackbox.exe [--config FILE] [--headless --message TEXT] [--no-screenshot]
```

Copy `gotop-blackbox.example.toml` to `gotop-blackbox.toml` next to the exe and set `share_path`
and the log directories. Without a config file, defaults apply and bundles go to the local fallback.

## Build

```
cargo build --release --target x86_64-pc-windows-msvc    # on Windows
cargo test                                               # logic is cross-platform
```

CI builds the Windows exe on every push (artifact `gotop-blackbox-windows`).

## Troubleshooting

The normal exe is a GUI-subsystem program: no console, errors go to dialogs and to
`%TEMP%\gotop-blackbox.log`. If it "does nothing", run `gotop-blackbox-console.exe` (same code,
console subsystem) from PowerShell/cmd to see output directly. Compare the hash with
`SHA256SUMS.txt` shipped in the artifact: `Get-FileHash .\gotop-blackbox.exe`.

Exit code `0xC0000139` (-1073741511, "entry point not found") means the Windows loader rejected the
exe before `main`: historically a missing Common Controls v6 manifest (see `assets/`). CI runs the
console exe with `--version` to guard against this.

## Deployment note (AppLocker)

Unlike Esri's Python, a new `.exe` is not pre-whitelisted. Ask IT to allow it by path, hash or
publisher certificate (ideally sign the binary with the corporate code-signing certificate).
The tool is read-only towards the system: it reads logs, enumerates processes, takes a screenshot
and writes to one folder. It opens no network ports and sends nothing.

## Open items

- [ ] Real GOTOP log directories in the example config
- [ ] Code signing

## Licence

BSD 3-Clause, Copyright (c) 2026 Federal Office of Topography swisstopo. See [LICENSE](LICENSE).
