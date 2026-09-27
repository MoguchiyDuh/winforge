use std::io;

use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE};
use winreg::{RegKey, RegValue};

use super::{Outcome, Status, Which};
use crate::schema::{ServiceAction, ServiceStart};
use crate::sys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    Boot,
    System,
    Auto,
    AutoDelayed,
    Demand,
    Disabled,
}

pub fn status(act: &ServiceAction, oneway: bool) -> Status {
    let cur = match query_start(&act.name) {
        Ok(Some(cur)) => cur,
        Ok(None) => return Status::Absent,
        Err(_) => return Status::Unknown,
    };
    let apply_hit = to_start(act.apply) == cur;
    if oneway {
        return if apply_hit {
            Status::Applied
        } else {
            Status::Default
        };
    }
    let reset_hit = act.reset.map(to_start) == Some(cur);
    return match (apply_hit, reset_hit) {
        (true, _) => Status::Applied,
        (false, true) => Status::Default,
        (false, false) => Status::Mixed,
    };
}

pub fn apply(act: &ServiceAction) -> Outcome {
    return change(act, Which::Apply);
}

pub fn reset(act: &ServiceAction) -> Outcome {
    return change(act, Which::Reset);
}

fn to_start(word: ServiceStart) -> Start {
    return match word {
        ServiceStart::Auto => Start::Auto,
        ServiceStart::Delayed => Start::AutoDelayed,
        ServiceStart::Manual => Start::Demand,
        ServiceStart::Disabled => Start::Disabled,
        ServiceStart::Boot => Start::Boot,
        ServiceStart::System => Start::System,
    };
}

fn sc_word(start: Start) -> &'static str {
    return match start {
        Start::Boot => "boot",
        Start::System => "system",
        Start::Auto => "auto",
        Start::AutoDelayed => "delayed-auto",
        Start::Demand => "demand",
        Start::Disabled => "disabled",
    };
}

fn query_start(name: &str) -> Result<Option<Start>, String> {
    let key_path = format!(r"SYSTEM\CurrentControlSet\Services\{name}");
    let key = match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(&key_path, KEY_QUERY_VALUE)
    {
        Ok(key) => key,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!("cannot open service key {key_path}: {err}"));
        }
    };
    let image = key.get_raw_value("ImagePath");
    if matches!(&image, Err(err) if err.kind() == io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let start_raw: Result<RegValue, io::Error> = key.get_raw_value("Start");
    let start = match start_raw {
        Ok(value) => match value {
            RegValue { bytes, vtype } if vtype == winreg::enums::REG_DWORD && bytes.len() == 4 => {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            }
            _ => return Err(format!("service {name} Start is not a dword")),
        },
        Err(err) => return Err(format!("cannot read service {name} Start: {err}")),
    };
    let delayed = key
        .get_raw_value("DelayedAutostart")
        .ok()
        .and_then(|value| match value {
            RegValue { bytes, vtype } if vtype == winreg::enums::REG_DWORD && bytes.len() == 4 => {
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            _ => None,
        })
        .unwrap_or(0);
    let parsed = match start {
        0 => Start::Boot,
        1 => Start::System,
        2 if delayed == 1 => Start::AutoDelayed,
        2 => Start::Auto,
        3 => Start::Demand,
        4 => Start::Disabled,
        _ => return Err(format!("service {name} has unknown Start value {start}")),
    };
    return Ok(Some(parsed));
}

fn change(act: &ServiceAction, which: Which) -> Outcome {
    let start = match which {
        Which::Apply => to_start(act.apply),
        Which::Reset => match act.reset.map(to_start) {
            Some(start) => start,
            None => return Outcome::Skipped,
        },
    };
    let current = match query_start(&act.name) {
        Ok(Some(current)) => Some(current),
        Ok(None) => None,
        Err(err) => return Outcome::Failed(err),
    };
    if current.is_none() {
        sys::log(&format!("service {} not found, skipped", act.name));
        return Outcome::Skipped;
    }
    let out = match sys::run(
        &sys::system32("sc.exe"),
        &["config", &act.name, "start=", sc_word(start)],
        sys::TIMEOUT_PROBE,
    ) {
        Ok(out) => out,
        Err(err) => return Outcome::Failed(err),
    };
    if !out.status.success() {
        return Outcome::Failed(format!(
            "sc config {} failed: {}",
            act.name,
            sys::output_text(&out)
        ));
    }
    if start == Start::Demand || start == Start::Disabled {
        match sys::run(
            &sys::system32("sc.exe"),
            &["stop", &act.name],
            sys::TIMEOUT_PROBE,
        ) {
            Ok(stop) if !stop.status.success() => {
                sys::log(&format!("sc stop {} failed, ignored", act.name));
            }
            Err(err) => sys::log(&format!("sc stop {} failed, ignored: {err}", act.name)),
            Ok(_) => {}
        }
    }
    return Outcome::Done;
}
