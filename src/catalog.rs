include!(concat!(env!("OUT_DIR"), "/embeds.rs"));

use crate::schema::{AppGroup, Pack};

pub fn packs() -> Vec<Pack> {
    let mut out = Vec::new();
    for (name, text) in PACK_FILES {
        match toml::from_str::<Pack>(text) {
            Ok(pack) => out.push(pack),
            Err(err) => eprintln!("winforge: pack {name} failed to parse: {err}"),
        }
    }
    return out;
}

pub fn app_groups() -> Vec<AppGroup> {
    let mut out = Vec::new();
    for (name, text) in APP_FILES {
        match toml::from_str::<AppGroup>(text) {
            Ok(group) => out.push(group),
            Err(err) => eprintln!("winforge: app group {name} failed to parse: {err}"),
        }
    }
    return out;
}

pub fn asset(name: &str) -> Option<&'static [u8]> {
    return ASSET_FILES
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, data)| *data);
}

pub fn script(name: &str) -> Option<&'static str> {
    return SCRIPT_FILES
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, text)| *text);
}
