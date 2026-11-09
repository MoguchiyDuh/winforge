use super::{Outcome, Status};
use crate::schema::CommandAction;
use crate::sys;

pub fn status(act: &CommandAction) -> Status {
    let out = match sys::run_script(&act.check, sys::TIMEOUT_SCRIPT) {
        Ok(out) => out,
        Err(_) => return Status::Unknown,
    };
    let code = out.status.code().unwrap_or(-1);
    return match code {
        0 => Status::Applied,
        1 => Status::Default,
        _ => Status::Unknown,
    };
}

pub fn apply(act: &CommandAction) -> Outcome {
    let outcome = change(&act.apply);
    if outcome != Outcome::Done {
        return outcome;
    }
    return match status(act) {
        Status::Applied => Outcome::Done,
        Status::Default => Outcome::Failed("applied, but check still reports default".into()),
        _ => Outcome::Failed("applied, but check could not confirm".into()),
    };
}

pub fn reset(act: &CommandAction) -> Outcome {
    let Some(script) = act.reset.as_deref() else {
        return Outcome::Skipped;
    };
    return change(script);
}

fn change(script: &str) -> Outcome {
    let out = match sys::run_script(script, sys::TIMEOUT_SCRIPT) {
        Ok(out) => out,
        Err(err) => return Outcome::Failed(err),
    };
    if !out.status.success() {
        if out.status.code() == Some(3) {
            return Outcome::Skipped;
        }
        return Outcome::Failed(format!(
            "command failed ({}): {}",
            out.status.code().unwrap_or(-1),
            sys::output_text(&out)
        ));
    }
    return Outcome::Done;
}
