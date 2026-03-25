use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    pub name: String,
    pub keep_open: bool,
    pub binds: Vec<BindConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindConfig {
    pub key: String,
    pub description: String,
    pub options: Vec<BindOption>,
    pub action: BindAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BindOption {
    KeepOpen,
    Close,
    Alias(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BindAction {
    SpawnSh(String),
    Spawn(Vec<String>),
    SwitchMode(String),
    ScratchpadPick,
    ScratchpadToggle(Option<String>),
    ScratchpadHide,
    ScratchpadFloat(Option<String>),
    ScratchpadTile(Option<String>),
    ScratchpadToggleFloat,
    ScratchpadAdopt,
    ScratchpadDisown,
    /// Pass-through niri action: name + args
    NiriAction {
        name: String,
        args: Vec<String>,
    },
}
