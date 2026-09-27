use std::sync::Mutex;

use super::{Outcome, Status};
use crate::schema::{AppxAction, AppxMode};
use crate::sys;

#[derive(Debug, Clone, Default)]
struct PkgList {
    installed: Vec<String>,
    provisioned: Vec<String>,
}

static CACHE: Mutex<Option<PkgList>> = Mutex::new(None);

const MARKER: &str = "---WINFORGE-PROVISIONED---";

pub fn status(act: &AppxAction) -> Status {
    let Some(list) = installed_list() else {
        return Status::Unknown;
    };
    if list
        .installed
        .iter()
        .chain(list.provisioned.iter())
        .any(|name| pattern_matches(&act.pattern, name))
    {
        return Status::Default;
    }
    return Status::Applied;
}

pub fn apply(act: &AppxAction) -> Outcome {
    let result = match act.apply {
        AppxMode::Remove => remove_package(&act.pattern),
        AppxMode::Deprovision => deprovision_package(&act.pattern),
        AppxMode::Both => {
            remove_package(&act.pattern).and_then(|()| deprovision_package(&act.pattern))
        }
    };
    if let Err(err) = result {
        invalidate();
        return Outcome::Failed(err);
    }
    invalidate();
    let Some(list) = installed_list() else {
        return Outcome::Failed(String::from(
            "cannot verify appx removal: package list unavailable",
        ));
    };
    let installed_hit = list
        .installed
        .iter()
        .any(|name| pattern_matches(&act.pattern, name));
    let provisioned_hit = list
        .provisioned
        .iter()
        .any(|name| pattern_matches(&act.pattern, name));
    let still = match act.apply {
        AppxMode::Remove => installed_hit,
        AppxMode::Deprovision => provisioned_hit,
        AppxMode::Both => installed_hit || provisioned_hit,
    };
    if still {
        return Outcome::Failed(format!("appx {} still present after apply", act.pattern));
    }
    return Outcome::Done;
}

fn invalidate() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

fn installed_list() -> Option<PkgList> {
    let mut guard = CACHE.lock().ok()?;
    if guard.is_none() {
        *guard = probe();
    }
    return guard.clone();
}

fn probe() -> Option<PkgList> {
    let script = "try { Get-AppxPackage -AllUsers -ErrorAction Stop | Select-Object -ExpandProperty Name } catch { try { Get-AppxPackage -ErrorAction Stop | Select-Object -ExpandProperty Name } catch { exit 1 } }\nWrite-Output '---WINFORGE-PROVISIONED---'\ntry { Get-AppxProvisionedPackage -Online -ErrorAction Stop | Select-Object -ExpandProperty DisplayName } catch { }\n";
    let out = sys::run_script(script, sys::TIMEOUT_PROBE).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = sys::output_text(&out);
    let marker_pos = text.find(MARKER)?;
    let mut list = PkgList::default();
    for line in text[..marker_pos].lines() {
        let name = line.trim();
        if !name.is_empty() {
            list.installed.push(String::from(name));
        }
    }
    for line in text[marker_pos + MARKER.len()..].lines() {
        let name = line.trim();
        if !name.is_empty() {
            list.provisioned.push(String::from(name));
        }
    }
    return Some(list);
}

fn pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let first = parts[0];
    let last = parts[parts.len() - 1];
    let mut cursor = 0;
    if !first.is_empty() {
        if !name.starts_with(first) {
            return false;
        }
        cursor = first.len();
    }
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        let Some(pos) = name[cursor..].find(part) else {
            return false;
        };
        cursor += pos + part.len();
    }
    if !last.is_empty() && (name.len() < cursor + last.len() || !name[cursor..].ends_with(last)) {
        return false;
    }
    return true;
}

fn ps_quote(pattern: &str) -> String {
    return format!("'{}'", pattern.replace('\'', "''"));
}

fn remove_package(pattern: &str) -> Result<(), String> {
    let quoted = ps_quote(pattern);
    let script = format!(
        "try {{ Get-AppxPackage -AllUsers -Name {quoted} -ErrorAction Stop | Remove-AppxPackage -AllUsers -ErrorAction Stop }} catch {{ Get-AppxPackage -Name {quoted} -ErrorAction Stop | Remove-AppxPackage -ErrorAction Stop }}"
    );
    let out = sys::run_script(&script, sys::TIMEOUT_SCRIPT)?;
    if !out.status.success() {
        return Err(format!(
            "appx remove {pattern} failed: {}",
            sys::output_text(&out)
        ));
    }
    sys::log(&format!("appx remove {pattern} done"));
    return Ok(());
}

fn deprovision_package(pattern: &str) -> Result<(), String> {
    let quoted = ps_quote(pattern);
    let script = format!(
        "Get-AppxProvisionedPackage -Online -ErrorAction Stop | Where-Object DisplayName -like {quoted} | Remove-AppxProvisionedPackage -Online -ErrorAction Stop | Out-Null"
    );
    let out = sys::run_script(&script, sys::TIMEOUT_SCRIPT)?;
    if !out.status.success() {
        return Err(format!(
            "appx deprovision {pattern} failed: {}",
            sys::output_text(&out)
        ));
    }
    sys::log(&format!("appx deprovision {pattern} done"));
    return Ok(());
}
