use super::{Outcome, Status, Which};
use crate::schema::{TaskAction, TaskState};
use crate::sys;

pub fn status(act: &TaskAction, oneway: bool) -> Status {
    let id = task_id(&act.path, &act.name);
    let cur = match query_state(&id) {
        Ok(Some(cur)) => cur,
        Ok(None) => return Status::Absent,
        Err(_) => return Status::Unknown,
    };
    let apply_hit = act.apply == cur;
    if oneway {
        return if apply_hit {
            Status::Applied
        } else {
            Status::Default
        };
    }
    let reset_hit = act.reset == Some(cur);
    return match (apply_hit, reset_hit) {
        (true, _) => Status::Applied,
        (false, true) => Status::Default,
        (false, false) => Status::Mixed,
    };
}

pub fn apply(act: &TaskAction) -> Outcome {
    return change(act, Which::Apply);
}

pub fn reset(act: &TaskAction) -> Outcome {
    return change(act, Which::Reset);
}

fn task_id(path: &str, name: &str) -> String {
    if path.ends_with('\\') {
        return format!("{path}{name}");
    }
    return format!("{path}\\{name}");
}

fn query_state(id: &str) -> Result<Option<TaskState>, String> {
    let out = sys::run(
        &sys::system32("schtasks.exe"),
        &["/Query", "/TN", id, "/XML"],
        sys::TIMEOUT_PROBE,
    )?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let settings = match_settings_block(&text);
    if settings
        .to_ascii_lowercase()
        .contains("<enabled>false</enabled>")
    {
        return Ok(Some(TaskState::Disabled));
    }
    return Ok(Some(TaskState::Enabled));
}

fn match_settings_block(text: &str) -> String {
    let lowered = text.to_ascii_lowercase();
    let Some(start) = lowered.find("<settings>") else {
        return String::new();
    };
    let after = &lowered[start..];
    let Some(end) = after.find("</settings>") else {
        return String::new();
    };
    return text[start..start + end].to_owned();
}

fn change(act: &TaskAction, which: Which) -> Outcome {
    let flag = match which {
        Which::Apply => act.apply,
        Which::Reset => match act.reset {
            Some(flag) => flag,
            None => return Outcome::Skipped,
        },
    };
    let id = task_id(&act.path, &act.name);
    match query_state(&id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            sys::log(&format!("task {id} not found, skipped"));
            return Outcome::Skipped;
        }
        Err(err) => return Outcome::Failed(err),
    }
    let flag_arg = if flag == TaskState::Enabled {
        "/ENABLE"
    } else {
        "/DISABLE"
    };
    let out = match sys::run(
        &sys::system32("schtasks.exe"),
        &["/Change", "/TN", &id, flag_arg],
        sys::TIMEOUT_PROBE,
    ) {
        Ok(out) => out,
        Err(err) => return Outcome::Failed(err),
    };
    if !out.status.success() {
        return Outcome::Failed(format!(
            "schtasks /Change {id} failed: {}",
            sys::output_text(&out)
        ));
    }
    return Outcome::Done;
}
