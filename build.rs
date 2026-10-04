use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/schema.rs"]
#[allow(dead_code)]
mod schema;

use schema::{Action, AppGroup, Pack, RegType, RegistryValue, Value};

fn main() {
    let manifest =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| String::from(".")));
    let packs = list_files(&manifest.join("packs"), "toml");
    let apps = list_files(&manifest.join("apps"), "toml");
    let assets = list_files(&manifest.join("assets"), "");
    let scripts = list_files(&manifest.join("scripts"), "ps1");
    let mut errors = Vec::new();
    let mut pack_ids: HashMap<String, PathBuf> = HashMap::new();
    let mut tweak_ids: HashMap<String, PathBuf> = HashMap::new();
    for path in &packs {
        let Ok(text) = fs::read_to_string(path)
            .map_err(|err| errors.push(format!("{}: {err}", path.display())))
        else {
            continue;
        };
        let pack = match toml::from_str::<Pack>(&text) {
            Ok(pack) => pack,
            Err(err) => {
                errors.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        check_dupe(&mut pack_ids, &pack.meta.id, path, "pack id", &mut errors);
        if pack.tweak.is_empty() {
            errors.push(err(path, "-", "pack has no tweaks"));
        }
        for tweak in &pack.tweak {
            check_dupe(&mut tweak_ids, &tweak.id, path, "tweak id", &mut errors);
            if tweak.action.is_empty() {
                errors.push(err(path, &tweak.id, "tweak has no actions"));
            }
            if pack.meta.oneway() && tweak.preset.is_some() {
                errors.push(err(path, &tweak.id, "oneway tweak must not have preset"));
            }
            if !pack.meta.oneway() && tweak.preset.is_none() {
                errors.push(err(
                    path,
                    &tweak.id,
                    "tweak is missing preset (min|std|max|manual)",
                ));
            }
            for action in &tweak.action {
                if let (Some(tweak_os), Some(action_os)) = (tweak.os, action.os())
                    && tweak_os != action_os
                {
                    errors.push(err(path, &tweak.id, "action os contradicts tweak os"));
                }
                validate_action(
                    action,
                    pack.meta.oneway(),
                    path,
                    &tweak.id,
                    &manifest,
                    &mut errors,
                );
            }
        }
    }
    for path in &apps {
        let Ok(text) = fs::read_to_string(path)
            .map_err(|err| errors.push(format!("{}: {err}", path.display())))
        else {
            continue;
        };
        if let Err(err) = toml::from_str::<AppGroup>(&text) {
            errors.push(format!("{}: {err}", path.display()));
        }
    }
    let mas_cmd_path = manifest.join("mas").join("MAS_AIO.cmd");
    let mas_bytes = fs::read(&mas_cmd_path).unwrap_or_default();
    let crlf_ok = mas_bytes.windows(2).any(|pair| pair == [b'\r', b'\n']);
    let tail_ok = mas_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']);
    if !crlf_ok || !tail_ok {
        errors.push(format!(
            "{}: MAS_AIO.cmd must have CRLF line endings and end with an empty line",
            mas_cmd_path.display()
        ));
    }
    if !errors.is_empty() {
        errors
            .iter()
            .for_each(|error| eprintln!("winforge data error: {error}"));
        panic!(
            "winforge data validation failed with {} error(s)",
            errors.len()
        );
    }
    write_embeds(&packs, &apps, &assets, &scripts, &manifest.join("mas"));
    println!("cargo:rerun-if-changed=build.rs");
    for dir in ["packs", "apps", "assets", "scripts", "mas"] {
        println!("cargo:rerun-if-changed={dir}");
    }
}

fn list_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && (ext.is_empty() || path.extension().is_some_and(|e| e == ext))
        })
        .collect::<Vec<PathBuf>>();
    files.sort();
    return files;
}

fn err(path: &Path, tweak_id: &str, message: &str) -> String {
    return format!("{} tweak '{tweak_id}': {message}", path.display());
}

fn check_dupe(
    seen: &mut HashMap<String, PathBuf>,
    id: &str,
    path: &Path,
    kind: &str,
    errors: &mut Vec<String>,
) {
    if let Some(first) = seen.get(id) {
        errors.push(format!(
            "{}, duplicate {kind} '{id}' (also in {})",
            path.display(),
            first.display()
        ));
    } else {
        seen.insert(String::from(id), path.to_path_buf());
    }
}

fn reg_ok(regtype: RegType, value: &RegistryValue) -> bool {
    return match (regtype, value) {
        (RegType::Dword, RegistryValue::U32(_)) => true,
        (RegType::Dword, RegistryValue::U64(n)) => *n <= u32::MAX as u64,
        (RegType::Qword, RegistryValue::U32(_) | RegistryValue::U64(_)) => true,
        (RegType::Sz | RegType::ExpandSz, RegistryValue::Str(_)) => true,
        (RegType::MultiSz, RegistryValue::Multi(_)) => true,
        (RegType::Binary, RegistryValue::Binary(_)) => true,
        _ => false,
    };
}

fn validate_action(
    action: &Action,
    oneway: bool,
    path: &Path,
    tweak_id: &str,
    manifest: &Path,
    errors: &mut Vec<String>,
) {
    let mut fail = |message: String| errors.push(err(path, tweak_id, &message));
    let (kind, has_reset) = match action {
        Action::Registry(act) => {
            for (field, payload) in [("apply", Some(&act.apply)), ("reset", act.reset.as_ref())] {
                let Some(Value::Val(value)) = payload else {
                    continue;
                };
                if !reg_ok(act.regtype, value) {
                    fail(format!(
                        "registry {field} value does not match regtype {:?}",
                        act.regtype
                    ));
                }
            }
            ("registry", act.reset.is_some())
        }
        Action::Service(act) => ("service", act.reset.is_some()),
        Action::Task(act) => ("task", act.reset.is_some()),
        Action::Command(act) => ("command", act.reset.is_some()),
        Action::Appx(_) => {
            if !oneway {
                fail(String::from(
                    "appx action is forbidden in a non-oneway pack",
                ));
            }
            return;
        }
        Action::File(act) => {
            if !manifest.join("assets").join(&act.from).is_file() {
                fail(format!(
                    "file from '{}' does not exist in assets/",
                    act.from
                ));
            }
            return;
        }
    };
    if oneway && has_reset {
        fail(format!(
            "{kind} action must not have reset in a oneway pack"
        ));
    }
    if !oneway && !has_reset {
        fail(format!("{kind} action is missing reset"));
    }
}

fn write_embeds(
    packs: &[PathBuf],
    apps: &[PathBuf],
    assets: &[PathBuf],
    scripts: &[PathBuf],
    mas: &Path,
) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap_or_else(|_| String::from("out")));
    let mut code = String::new();
    for (name, kind, mac, files, stem) in [
        ("PACK_FILES", "&str", "include_str", packs, true),
        ("APP_FILES", "&str", "include_str", apps, true),
        ("ASSET_FILES", "&[u8]", "include_bytes", assets, false),
        ("SCRIPT_FILES", "&str", "include_str", scripts, false),
    ] {
        code.push_str(&format!("pub const {name}: &[(&str, {kind})] = &[\n"));
        for path in files {
            let key = if stem {
                path.file_stem()
            } else {
                path.file_name()
            }
            .unwrap()
            .to_string_lossy();
            code.push_str(&format!(
                "    ({}, {}!({})),\n",
                rust_str(&key),
                mac,
                rust_str(&path.to_string_lossy().replace('\\', "/"))
            ));
        }
        code.push_str("];\n");
    }
    for (name, file) in [("MAS_CMD", "MAS_AIO.cmd")] {
        let path = mas.join(file).to_string_lossy().replace('\\', "/");
        code.push_str(&format!(
            "pub const {name}: &[u8] = include_bytes!({});\n",
            rust_str(&path)
        ));
    }
    if fs::write(out_dir.join("embeds.rs"), code).is_err() {
        panic!("build.rs: cannot write embeds.rs to {}", out_dir.display());
    }
}

fn rust_str(value: &str) -> String {
    return format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""));
}
