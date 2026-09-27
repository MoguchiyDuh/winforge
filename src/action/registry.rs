use std::borrow::Cow;
use std::io;

use winreg::enums::{
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_QUERY_VALUE,
    KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ,
};
use winreg::{HKEY, RegKey, RegValue};

use super::{Outcome, Status, Which};
use crate::schema::{DeleteWord, RegType, RegistryAction, RegistryValue, Value};

pub fn status(act: &RegistryAction, oneway: bool) -> Status {
    let cur = match read_current(&act.path, &act.name) {
        Ok(cur) => cur,
        Err(_) => return Status::Unknown,
    };
    let apply_hit = payload_matches(cur.as_ref(), act.regtype, &act.apply);
    if oneway {
        return if apply_hit {
            Status::Applied
        } else {
            Status::Default
        };
    }
    let Some(reset) = act.reset.as_ref() else {
        return Status::Unknown;
    };
    let reset_hit = payload_matches(cur.as_ref(), act.regtype, reset);
    return match (apply_hit, reset_hit) {
        (true, _) => Status::Applied,
        (false, true) => Status::Default,
        (false, false) => Status::Mixed,
    };
}

pub fn apply(act: &RegistryAction) -> Outcome {
    return change(act, Which::Apply);
}

pub fn reset(act: &RegistryAction) -> Outcome {
    if act.reset.is_none() {
        return Outcome::Skipped;
    }
    return change(act, Which::Reset);
}

fn change(act: &RegistryAction, which: Which) -> Outcome {
    let payload = match which {
        Which::Apply => &act.apply,
        Which::Reset => match act.reset.as_ref() {
            Some(payload) => payload,
            None => return Outcome::Skipped,
        },
    };
    let (hive, subkey) = match split_path(&act.path) {
        Ok(parts) => parts,
        Err(err) => return Outcome::Failed(err),
    };
    let root = RegKey::predef(hive);
    let key = match root.open_subkey_with_flags(subkey, KEY_QUERY_VALUE | KEY_SET_VALUE) {
        Ok(key) => key,
        Err(err) if err.kind() == io::ErrorKind::NotFound => match root.create_subkey(subkey) {
            Ok((key, _)) => key,
            Err(err) => {
                return Outcome::Failed(format!("cannot create registry key {}: {err}", act.path));
            }
        },
        Err(err) => {
            return Outcome::Failed(format!("cannot open registry key {}: {err}", act.path));
        }
    };
    match payload {
        Value::Delete(DeleteWord { delete: true }) => match key.delete_value(&act.name) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Outcome::Failed(format!(
                    "cannot delete registry value {}\\{}: {err}",
                    act.path, act.name
                ));
            }
        },
        Value::Delete(_) => {
            return Outcome::Failed(String::from("registry payload has delete = false"));
        }
        Value::Val(item) => {
            let value = match reg_value(act.regtype, item) {
                Ok(value) => value,
                Err(err) => return Outcome::Failed(err),
            };
            if let Err(err) = key.set_raw_value(&act.name, &value) {
                return Outcome::Failed(format!(
                    "cannot set registry value {}\\{}: {err}",
                    act.path, act.name
                ));
            }
        }
    }
    return Outcome::Done;
}

fn split_path(path: &str) -> Result<(HKEY, &str), String> {
    let Some((head, rest)) = path.split_once('\\') else {
        return Err(format!("registry path is missing a hive prefix: {path}"));
    };
    let hive = match head.to_ascii_uppercase().as_str() {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        "HKU" => HKEY_USERS,
        "HKCR" => HKEY_CLASSES_ROOT,
        other => return Err(format!("unknown registry hive prefix: {other}")),
    };
    return Ok((hive, rest));
}

fn read_current(path: &str, name: &str) -> Result<Option<RegValue<'static>>, String> {
    let (hive, subkey) = split_path(path)?;
    let key = match RegKey::predef(hive).open_subkey_with_flags(subkey, KEY_QUERY_VALUE) {
        Ok(key) => key,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("cannot open registry key {path}: {err}")),
    };
    return match key.get_raw_value(name) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("cannot read registry value {path}\\{name}: {err}")),
    };
}

fn payload_matches(cur: Option<&RegValue<'static>>, regtype: RegType, payload: &Value) -> bool {
    let expected = match payload {
        Value::Delete(DeleteWord { delete: true }) => return cur.is_none(),
        Value::Delete(_) => return false,
        Value::Val(item) => match reg_value(regtype, item) {
            Ok(value) => value,
            Err(_) => return false,
        },
    };
    return match cur {
        Some(cur) => *cur == expected,
        None => false,
    };
}

fn reg_value(regtype: RegType, v: &RegistryValue) -> Result<RegValue<'static>, String> {
    return match (regtype, v) {
        (RegType::Dword, RegistryValue::U32(n)) => Ok(dword(*n)),
        (RegType::Dword, RegistryValue::U64(n)) => u32::try_from(*n)
            .map(dword)
            .map_err(|_| format!("dword value out of range: {n}")),
        (RegType::Qword, RegistryValue::U64(n)) => Ok(qword(*n)),
        (RegType::Qword, RegistryValue::U32(n)) => Ok(qword(u64::from(*n))),
        (RegType::Sz, RegistryValue::Str(s)) => Ok(string_value(s, REG_SZ)),
        (RegType::ExpandSz, RegistryValue::Str(s)) => Ok(string_value(s, REG_EXPAND_SZ)),
        (RegType::MultiSz, RegistryValue::Multi(list)) => Ok(RegValue {
            bytes: Cow::Owned(multi_wide_bytes(list)),
            vtype: REG_MULTI_SZ,
        }),
        (RegType::Binary, RegistryValue::Binary(bytes)) => Ok(RegValue {
            bytes: Cow::Owned(bytes.clone()),
            vtype: REG_BINARY,
        }),
        (regtype, other) => Err(format!(
            "registry value {other:?} is not valid for regtype {regtype:?}"
        )),
    };
}

fn string_value(s: &str, vtype: winreg::enums::RegType) -> RegValue<'static> {
    return RegValue {
        bytes: Cow::Owned(wide_bytes(s)),
        vtype,
    };
}

fn dword(n: u32) -> RegValue<'static> {
    return RegValue {
        bytes: Cow::Owned(n.to_le_bytes().to_vec()),
        vtype: REG_DWORD,
    };
}

fn qword(n: u64) -> RegValue<'static> {
    return RegValue {
        bytes: Cow::Owned(n.to_le_bytes().to_vec()),
        vtype: REG_QWORD,
    };
}

fn wide_bytes(s: &str) -> Vec<u8> {
    return s
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
}

fn multi_wide_bytes(list: &[String]) -> Vec<u8> {
    let mut units: Vec<u16> = Vec::new();
    for item in list {
        units.extend(item.encode_utf16());
        units.push(0);
    }
    units.push(0);
    return units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
}
