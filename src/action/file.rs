use std::fs;
use std::io::Read;
use std::path::Path;

use super::{Outcome, Status};
use crate::catalog::asset;
use crate::schema::FileAction;

pub fn status(act: &FileAction) -> Status {
    let Some(bytes) = asset(&act.from) else {
        return Status::Unknown;
    };
    let target = Path::new(&act.to);
    let Ok(mut file) = fs::File::open(target) else {
        return Status::Default;
    };
    let mut current = Vec::new();
    if file.read_to_end(&mut current).is_err() {
        return Status::Unknown;
    }
    return if current == bytes {
        Status::Applied
    } else {
        Status::Mixed
    };
}

pub fn apply(act: &FileAction) -> Outcome {
    let Some(bytes) = asset(&act.from) else {
        return Outcome::Failed(format!("asset {} is not embedded", act.from));
    };
    let target = Path::new(&act.to);
    let dirs = match target.parent() {
        Some(parent) => fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display())),
        None => Ok(()),
    };
    if let Err(message) = dirs {
        return Outcome::Failed(message);
    }
    if let Err(err) = fs::write(target, bytes) {
        return Outcome::Failed(format!("cannot write {}: {err}", target.display()));
    }
    return Outcome::Done;
}

pub fn reset(act: &FileAction) -> Outcome {
    let target = Path::new(&act.to);
    match fs::remove_file(target) {
        Ok(()) => return Outcome::Done,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Outcome::Done,
        Err(err) => {
            return Outcome::Failed(format!("cannot delete {}: {err}", target.display()));
        }
    }
}
