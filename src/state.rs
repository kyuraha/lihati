use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const STATE_KEY: &str = "lihati_state_v1";

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum ViewMode {
    #[default]
    Source,
    Preview,
}

impl<'de> Deserialize<'de> for ViewMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Legacy "Split" mode maps to Source.
        Ok(match s.to_ascii_lowercase().as_str() {
            "preview" => ViewMode::Preview,
            _ => ViewMode::Source,
        })
    }
}


impl ViewMode {
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Source => "Source",
            ViewMode::Preview => "Preview",
        }
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum ThemePref {
    #[default]
    Light,
    Dark,
}

impl<'de> Deserialize<'de> for ThemePref {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Legacy "System" preference maps to Light, the new default.
        Ok(match s.to_ascii_lowercase().as_str() {
            "dark" => ThemePref::Dark,
            _ => ThemePref::Light,
        })
    }
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
    #[serde(default)]
    pub outline_width: Option<f32>,
    #[serde(default)]
    pub outline_collapsed: HashMap<String, Vec<usize>>,
}

impl Default for PersistState {
    fn default() -> Self {
        PersistState {
            show_dir: true,
            show_outline: true,
            view: ViewMode::Source,
            theme: ThemePref::Light,
            zoom: 1.0,
            root: None,
            last_file: None,
            recents: Vec::new(),
            outline_width: None,
            outline_collapsed: HashMap::new(),
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
        s.view = ViewMode::Preview;
        s.theme = ThemePref::Dark;
        s.zoom = 1.4;
        s.root = Some("C:\\notes".into());
        s.recents = vec!["a.md".into(), "b.md".into()];
        s.outline_width = Some(300.0);
        s.outline_collapsed
            .insert("C:\\a.md".into(), vec![10, 42]);

        let json = serde_json::to_string(&s).unwrap();
        let back: PersistState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.view, ViewMode::Preview);
        assert_eq!(back.outline_width, Some(300.0));
        assert_eq!(back.theme, ThemePref::Dark);
        assert_eq!(back.zoom, 1.4);
        assert_eq!(back.recents.len(), 2);
        assert!(!back.show_dir);
        assert!(back.show_outline);
        assert_eq!(back.outline_collapsed.get("C:\\a.md").unwrap(), &vec![10, 42]);
    }

    #[test]
    fn outline_collapsed_per_file_roundtrip() {
        let mut s = PersistState::default();
        s.outline_collapsed
            .insert("C:\\a.md".into(), vec![3, 7]);
        s.outline_collapsed
            .insert("C:\\b.md".into(), vec![12]);
        let json = serde_json::to_string(&s).unwrap();
        let back: PersistState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outline_collapsed.len(), 2);
        assert_eq!(back.outline_collapsed["C:\\a.md"], vec![3, 7]);
    }

    #[test]
    fn old_state_without_collapsed_still_loads() {
        let raw = r#"{"show_dir":true,"show_outline":true,"view":"Source","theme":"Light","zoom":1.0,"recents":[]}"#;
        let back: PersistState = serde_json::from_str(raw).unwrap();
        assert!(back.outline_collapsed.is_empty());
    }

    #[test]
    fn corrupted_state_falls_back_to_default() {
        let bad = "{ not valid json";
        let parsed: Option<PersistState> = serde_json::from_str(bad).ok();
        assert!(parsed.is_none());
        let _ = PersistState::default();
    }
}
