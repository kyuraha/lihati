use serde::{Deserialize, Serialize};

pub const STATE_KEY: &str = "lihati_state_v1";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum ViewMode {
    #[default]
    Source,
    Split,
    Preview,
}


impl ViewMode {
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Source => "Source",
            ViewMode::Split => "Split",
            ViewMode::Preview => "Preview",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistState {
    pub show_dir: bool,
    pub show_outline: bool,
    pub view: ViewMode,
    pub theme: ThemePref,
    pub zoom: f32,
    pub root: Option<String>,
    pub last_file: Option<String>,
    pub recents: Vec<String>,
}

impl Default for PersistState {
    fn default() -> Self {
        PersistState {
            show_dir: true,
            show_outline: true,
            view: ViewMode::Source,
            theme: ThemePref::System,
            zoom: 1.0,
            root: None,
            last_file: None,
            recents: Vec::new(),
        }
    }
}

pub fn load(cc: &eframe::CreationContext<'_>) -> PersistState {
    cc.storage
        .and_then(|s| s.get_string(STATE_KEY))
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn store(state: &PersistState, storage: &mut dyn eframe::Storage) {
    if let Ok(json) = serde_json::to_string(state) {
        storage.set_string(STATE_KEY, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let mut s = PersistState::default();
        s.show_dir = false;
        s.view = ViewMode::Split;
        s.theme = ThemePref::Dark;
        s.zoom = 1.4;
        s.root = Some("C:\\notes".into());
        s.recents = vec!["a.md".into(), "b.md".into()];

        let json = serde_json::to_string(&s).unwrap();
        let back: PersistState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.view, ViewMode::Split);
        assert_eq!(back.theme, ThemePref::Dark);
        assert_eq!(back.zoom, 1.4);
        assert_eq!(back.recents.len(), 2);
        assert!(!back.show_dir);
        assert!(back.show_outline);
    }

    #[test]
    fn corrupted_state_falls_back_to_default() {
        let bad = "{ not valid json";
        let parsed: Option<PersistState> = serde_json::from_str(bad).ok();
        assert!(parsed.is_none());
        let _ = PersistState::default();
    }
}
