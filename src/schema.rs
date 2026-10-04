use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub meta: PackMeta,
    #[serde(default)]
    pub tweak: Vec<Tweak>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackMeta {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oneway: Option<bool>,
}

impl PackMeta {
    pub fn oneway(&self) -> bool {
        return self.oneway.unwrap_or(false);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tweak {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<Preset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    #[serde(default)]
    pub action: Vec<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Min,
    Std,
    Max,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Os {
    #[serde(rename = "10")]
    Win10,
    #[serde(rename = "11")]
    Win11,
}

impl Os {
    pub fn from_build(build: u32) -> Os {
        if build >= 22000 {
            return Os::Win11;
        }
        return Os::Win10;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Action {
    Registry(RegistryAction),
    Service(ServiceAction),
    Task(TaskAction),
    Appx(AppxAction),
    File(FileAction),
    Command(CommandAction),
}

impl Action {
    pub fn os(&self) -> Option<Os> {
        return match self {
            Action::Registry(act) => act.os,
            Action::Service(act) => act.os,
            Action::Task(act) => act.os,
            Action::Appx(act) => act.os,
            Action::File(act) => act.os,
            Action::Command(act) => act.os,
        };
    }
}

pub fn os_matches(want: Option<Os>, current: Os) -> bool {
    return want.is_none_or(|os| os == current);
}

impl Pack {
    pub fn for_os(mut self, current: Os) -> Option<Pack> {
        self.tweak.retain_mut(|tweak| {
            if !os_matches(tweak.os, current) {
                return false;
            }
            tweak.action.retain(|action| os_matches(action.os(), current));
            return !tweak.action.is_empty();
        });
        if self.tweak.is_empty() {
            return None;
        }
        return Some(self);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteWord {
    pub delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Delete(DeleteWord),
    Val(RegistryValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegType {
    Dword,
    Qword,
    Sz,
    ExpandSz,
    MultiSz,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub path: String,
    pub name: String,
    pub regtype: RegType,
    pub apply: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStart {
    Auto,
    Delayed,
    Manual,
    Disabled,
    Boot,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub name: String,
    pub apply: ServiceStart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<ServiceStart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub path: String,
    pub name: String,
    pub apply: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<TaskState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppxMode {
    Remove,
    Deprovision,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppxAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub pattern: String,
    pub apply: AppxMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    pub apply: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<String>,
    pub check: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegistryValue {
    U32(u32),
    U64(u64),
    Str(String),
    Multi(Vec<String>),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppGroup {
    pub meta: AppMeta,
    #[serde(default)]
    pub app: Vec<App>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppMeta {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppSource {
    Winget,
    Msstore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub name: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<AppSource>,
    #[serde(default, rename = "override", skip_serializing_if = "Option::is_none")]
    pub override_: Option<String>,
}
