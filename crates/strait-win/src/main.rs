//! strait for Windows: a tray app that receives the Mac's keyboard, mouse
//! and clipboard.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod dib;
#[cfg(windows)]
mod clip;
#[cfg(windows)]
mod ddc;
#[cfg(windows)]
mod display;
#[cfg(windows)]
mod inject;
#[cfg(windows)]
mod logfile;
#[cfg(windows)]
mod net;
#[cfg(windows)]
mod sound;
#[cfg(windows)]
mod ui;

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("strait-win runs only on Windows");
    std::process::ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;

    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None => {}
        Some("--install") => return autostart::install(),
        Some("--uninstall") => return autostart::uninstall(),
        Some("--ddc") => return ddc_command(),
        Some(other) => {
            eprintln!("unknown argument {other}; expected --install, --uninstall or --ddc");
            return ExitCode::FAILURE;
        }
    }
    if !single_instance() {
        return ExitCode::SUCCESS;
    }
    let Some(dir) = std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("strait")) else {
        return ExitCode::FAILURE;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("create {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let log_path = dir.join("strait.log");
    logfile::init(&log_path);
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let identity = match strait::session::Identity::load_or_create(&dir.join("identity.key")) {
        Ok(id) => id,
        Err(e) => {
            log!("identity: {e}");
            return ExitCode::FAILURE;
        }
    };
    log!("strait {} started; this PC is {}", env!("CARGO_PKG_VERSION"), strait::session::fingerprint(&identity.public));
    ui::init(log_path);
    let net = net::Net::start(identity, dir.join("peer.key"), ui::notifier());
    ui::run(net);
    ExitCode::SUCCESS
}

/// `--ddc <feature> [value]` reads or sets a monitor feature and writes the
/// outcome to `ddc.txt` beside the log, since this app has no console. It
/// must run on the user's desktop, where Windows exposes the monitors.
#[cfg(windows)]
fn ddc_command() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let number = |s: &str| match s.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    };
    let Some(feature) = args.first().and_then(|a| number(a)) else {
        return std::process::ExitCode::FAILURE;
    };
    let Some(dir) = std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("strait")) else {
        return std::process::ExitCode::FAILURE;
    };
    let out = match args.get(1).and_then(|v| number(v)) {
        Some(value) => format!("set {feature:#04x} = {value:#x}: {}\n", ddc::write(feature as u8, value)),
        None => ddc::read(feature as u8).iter().map(|(m, v)| format!("{m}: {feature:#04x} = {v:?}\n")).collect(),
    };
    let _ = std::fs::write(dir.join("ddc.txt"), out);
    std::process::ExitCode::SUCCESS
}

/// A second copy would fight the first over the ports.
#[cfg(windows)]
fn single_instance() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\strait".encode_utf16().chain(Some(0)).collect();
    unsafe {
        // The mutex lives as long as the process.
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Starts strait at logon with full rights, so it can type into
/// administrator windows, without a UAC prompt each time.
#[cfg(windows)]
mod autostart {
    use std::process::{Command, ExitCode};

    const TASK: &str = "strait";

    fn schtasks(args: &[&str]) -> ExitCode {
        match Command::new("schtasks").args(args).status() {
            Ok(s) if s.success() => ExitCode::SUCCESS,
            Ok(s) => {
                eprintln!("schtasks {} failed: {s}", args[0]);
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("run schtasks: {e}");
                ExitCode::FAILURE
            }
        }
    }

    pub fn install() -> ExitCode {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("locate strait-win.exe: {e}");
                return ExitCode::FAILURE;
            }
        };
        let user = std::env::var("USERNAME").unwrap_or_default();
        let run = format!("\"{}\"", exe.display());
        let created = schtasks(&["/Create", "/F", "/TN", TASK, "/TR", &run, "/SC", "ONLOGON", "/RU", &user, "/IT", "/RL", "HIGHEST"]);
        if created != ExitCode::SUCCESS {
            return created;
        }
        schtasks(&["/Run", "/TN", TASK])
    }

    pub fn uninstall() -> ExitCode {
        schtasks(&["/Delete", "/F", "/TN", TASK])
    }
}
