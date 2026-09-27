use std::fs;
use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::catalog;
use crate::sys;

const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

pub fn run() -> Result<(), String> {
    let cmd_path = sys::temp_file(&format!("winforge-mas-{}.cmd", sys::rand_suffix()));
    fs::write(&cmd_path, catalog::MAS_CMD)
        .map_err(|err| format!("cannot write {}: {err}", cmd_path.display()))?;
    let mut command = Command::new(sys::cmd_exe());
    command
        .arg("/c")
        .arg(&cmd_path)
        .current_dir(cmd_path.parent().expect("temp path has parent"))
        .creation_flags(CREATE_NEW_CONSOLE);
    command
        .spawn()
        .map_err(|err| format!("cannot launch MAS: {err}"))?;
    sys::log("mas: massgravel MAS opened in a new window");
    sys::log("mas: https://github.com/massgravel/Microsoft-Activation-Scripts");
    return Ok(());
}
