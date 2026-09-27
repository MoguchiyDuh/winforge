pub mod appx;
pub mod command;
pub mod file;
pub mod registry;
pub mod service;
pub mod task;

use crate::schema::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Applied,
    Default,
    Mixed,
    Unknown,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Apply,
    Reset,
}

pub fn status(action: &Action, oneway: bool) -> Status {
    return match action {
        Action::Registry(act) => registry::status(act, oneway),
        Action::Service(act) => service::status(act, oneway),
        Action::Task(act) => task::status(act, oneway),
        Action::Appx(act) => appx::status(act),
        Action::File(act) => file::status(act),
        Action::Command(act) => command::status(act),
    };
}

pub fn apply(action: &Action) -> Outcome {
    return match action {
        Action::Registry(act) => registry::apply(act),
        Action::Service(act) => service::apply(act),
        Action::Task(act) => task::apply(act),
        Action::Appx(act) => appx::apply(act),
        Action::File(act) => file::apply(act),
        Action::Command(act) => command::apply(act),
    };
}

pub fn reset(action: &Action) -> Outcome {
    return match action {
        Action::Registry(act) => registry::reset(act),
        Action::Service(act) => service::reset(act),
        Action::Task(act) => task::reset(act),
        Action::File(act) => file::reset(act),
        Action::Command(act) => command::reset(act),
        Action::Appx(_) => Outcome::Skipped,
    };
}

pub fn describe(action: &Action) -> String {
    return match action {
        Action::Registry(act) => format!("registry {}\\{}", act.path, act.name),
        Action::Service(act) => format!("service {}", act.name),
        Action::Task(act) => format!("task {}{}", act.path, act.name),
        Action::Appx(act) => format!("appx {}", act.pattern),
        Action::File(act) => format!("file {} -> {}", act.from, act.to),
        Action::Command(act) => format!("command '{}'", first_line(&act.apply)),
    };
}

fn first_line(text: &str) -> &str {
    let line = text.lines().next().unwrap_or("");
    let end = line
        .char_indices()
        .nth(60)
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    return &line[..end];
}
