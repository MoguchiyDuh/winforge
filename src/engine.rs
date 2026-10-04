use crate::action::{self, Outcome, Status};
use crate::schema::{Pack, Tweak};
use crate::sys;

pub struct Report {
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    pub failures: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        return self.failed == 0;
    }
}

pub fn apply_tweaks(pack: &Pack, ids: &[String]) -> Report {
    return run_selected(pack, ids, true);
}

pub fn reset_tweaks(pack: &Pack, ids: &[String]) -> Report {
    return run_selected(pack, ids, false);
}

fn run_selected(pack: &Pack, ids: &[String], apply: bool) -> Report {
    let mut report = Report {
        done: 0,
        skipped: 0,
        failed: 0,
        failures: Vec::new(),
    };
    let verb = if apply { "apply" } else { "reset" };
    let past = if apply { "applied" } else { "reset" };
    sys::log(&format!("== {verb} {} begin", pack.meta.id));
    for tweak in &pack.tweak {
        if !ids.iter().any(|id| id == &tweak.id) {
            continue;
        }
        for action in &tweak.action {
            let desc = action::describe(action);
            let outcome = if apply {
                action::apply(action)
            } else {
                action::reset(action)
            };
            match outcome {
                Outcome::Done => {
                    report.done += 1;
                    sys::log(&format!("{past} {} {} :: {}", pack.meta.id, tweak.id, desc));
                }
                Outcome::Skipped => {
                    report.skipped += 1;
                    sys::log(&format!(
                        "skipped {} {} :: {} (target absent)",
                        pack.meta.id, tweak.id, desc
                    ));
                }
                Outcome::Failed(msg) => {
                    report.failed += 1;
                    report
                        .failures
                        .push(format!("{} {}: {}: {}", pack.meta.id, tweak.id, desc, msg));
                    sys::log(&format!(
                        "failed {} {} :: {}: {}",
                        pack.meta.id, tweak.id, desc, msg
                    ));
                }
            }
        }
    }
    sys::log(&format!(
        "== {verb} {} done: {} done, {} skipped, {} failed",
        pack.meta.id, report.done, report.skipped, report.failed
    ));
    return report;
}

pub fn tweak_statuses(pack: &Pack) -> Vec<(&Tweak, Status)> {
    let oneway = pack.meta.oneway();
    return pack
        .tweak
        .iter()
        .map(|tweak| (tweak, tweak_status(tweak, oneway)))
        .collect();
}

pub fn single_status(pack: &Pack, tweak_index: usize) -> Status {
    return tweak_status(&pack.tweak[tweak_index], pack.meta.oneway());
}

fn tweak_status(tweak: &Tweak, oneway: bool) -> Status {
    let mut any_unknown = false;
    let mut all_applied = true;
    let mut all_default = true;
    let mut any_present = false;
    let mut any_neutral = false;
    for action in &tweak.action {
        let status = action::status(action, oneway);
        if status == Status::Absent {
            continue;
        }
        if status == Status::Neutral {
            any_neutral = true;
            continue;
        }
        any_present = true;
        match status {
            Status::Applied => all_default = false,
            Status::Default => all_applied = false,
            Status::Unknown => any_unknown = true,
            _ => {
                all_applied = false;
                all_default = false;
            }
        }
    }
    if !any_present {
        if any_neutral {
            return Status::Applied;
        }
        return Status::Absent;
    }
    if any_unknown {
        return Status::Unknown;
    }
    if all_applied {
        return Status::Applied;
    }
    if all_default {
        return Status::Default;
    }
    return Status::Mixed;
}

pub struct Summary {
    pub applied: usize,
    pub total: usize,
    pub counts: [usize; 5],
}

pub fn summarize(statuses: &[Status]) -> Summary {
    let mut counts = [0usize; 5];
    for status in statuses {
        let index = match status {
            Status::Applied | Status::Neutral => 0,
            Status::Default => 1,
            Status::Mixed => 2,
            Status::Unknown => 3,
            Status::Absent => 4,
        };
        counts[index] += 1;
    }
    return Summary {
        applied: counts[0],
        total: statuses.len(),
        counts,
    };
}
