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
    pub pattern: String,
    pub apply: AppxMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAction {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandAction {
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
