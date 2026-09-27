use std::env;
use std::path::PathBuf;

use crate::catalog;
use crate::schema::{App, AppSource};
use crate::sys;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppResult {
    Installed,
    AlreadyPresent,
    InstalledRebootRequired,
    Failed(String),
}

pub fn winget_exe() -> Option<PathBuf> {
    if let Some(path) = find_in_path("winget.exe") {
        return Some(path);
    }
    let local = env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidate = local
        .join("Microsoft")
        .join("WindowsApps")
        .join("winget.exe");
    if candidate.is_file() {
        return Some(candidate);
    }
    return None;
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    return None;
}

pub fn available() -> bool {
    let Some(exe) = winget_exe() else {
        return false;
    };
    return match sys::run(&exe, &["--version"], sys::TIMEOUT_PROBE) {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };
}

pub fn ensure() -> Result<(), String> {
    if available() {
        return Ok(());
    }
    run_winget_script(false)?;
    if available() {
        return Ok(());
    }
    return Err(String::from("winget is still unavailable after bootstrap"));
}

pub fn repair() -> Result<(), String> {
    run_winget_script(true)?;
    return Ok(());
}

fn run_winget_script(force: bool) -> Result<(), String> {
    let Some(script) = catalog::script("winget.ps1") else {
        return Err(String::from("winget.ps1 is not embedded"));
    };
    let extra: &[&str] = if force { &["-Force"] } else { &[] };
    let out = sys::run_script_args(script, extra, sys::TIMEOUT_WINGET_SCRIPT)?;
    if !out.status.success() {
        return Err(format!(
            "winget bootstrap failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            sys::output_text(&out)
        ));
    }
    return Ok(());
}

pub fn install_app_quiet(app: &App) -> AppResult {
    let Some(exe) = winget_exe() else {
        return AppResult::Failed(String::from("winget not found"));
    };
    let args = install_args(app);
    let arg_refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
    let out = match sys::run(&exe, &arg_refs, sys::TIMEOUT_WINGET_INSTALL) {
        Ok(out) => out,
        Err(err) => return AppResult::Failed(err),
    };
    let code = out.status.code().unwrap_or(-1);
    sys::log(&format!("winget install {} exit {}", app.id, code));
    return classify(code);
}

fn install_args(app: &App) -> Vec<String> {
    let mut args: Vec<String> = vec![
        String::from("install"),
        String::from("--exact"),
        String::from("--id"),
        app.id.clone(),
        String::from("--silent"),
        String::from("--accept-package-agreements"),
        String::from("--accept-source-agreements"),
        String::from("--disable-interactivity"),
    ];
    if let Some(source) = app.source {
        args.push(String::from("--source"));
        args.push(String::from(match source {
            AppSource::Winget => "winget",
            AppSource::Msstore => "msstore",
        }));
    }
    if let Some(override_args) = app.override_.as_deref() {
        args.push(String::from("--override"));
        args.push(String::from(override_args));
    }
    return args;
}

fn classify(code: i32) -> AppResult {
    return match code as u32 {
        0 => AppResult::Installed,
        0x8A150061 | 0x8A15002B | 0x8A15010D | 0x8A15010E => AppResult::AlreadyPresent,
        0x8A150109 => AppResult::InstalledRebootRequired,
        other => AppResult::Failed(format!("0x{other:08X}")),
    };
}
