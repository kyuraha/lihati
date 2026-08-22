use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Document {
    pub path: Option<PathBuf>,
    pub text: String,
    pub dirty: bool,
    pub had_bom: bool,
    pub version: u64,
    disk_meta: Option<(SystemTime, u64)>,
}

pub enum ExternalChange {
    None,
    Conflicted(String),
    Vanished,
}

fn read_meta(path: &Path) -> Option<(SystemTime, u64)> {
    let md = fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

impl Document {
    pub fn new() -> Self {
        Document {
            path: None,
            text: String::new(),
            dirty: false,
            had_bom: false,
            version: 0,
            disk_meta: None,
        }
    }

    pub fn file_name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled.md".into()),
            None => "untitled.md".into(),
        }
    }

    pub fn is_empty_doc(&self) -> bool {
        self.path.is_none() && !self.dirty && self.text.is_empty()
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        let bytes =
            fs::read(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        let had_bom = text.starts_with('\u{feff}');
        if had_bom {
            text.remove(0);
        }
        self.path = Some(path.to_path_buf());
        self.text = text;
        self.dirty = false;
        self.had_bom = had_bom;
        self.disk_meta = read_meta(path);
        self.version = self.version.wrapping_add(1);
        Ok(())
    }

    pub fn on_text_changed(&mut self) {
        self.dirty = true;
        self.version = self.version.wrapping_add(1);
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => return Err("no file path".into()),
        };
        let mut data = String::with_capacity(self.text.len() + 3);
        if self.had_bom {
            data.push('\u{feff}');
        }
        data.push_str(&self.text);
        fs::write(&path, data.as_bytes())
            .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        self.disk_meta = read_meta(&path);
        self.dirty = false;
        Ok(())
    }

    pub fn adopt_path(&mut self, path: &Path) {
        self.path = Some(path.to_path_buf());
    }

    pub fn check_external(&mut self) -> ExternalChange {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => return ExternalChange::None,
        };
        if !path.exists() {
            return ExternalChange::Vanished;
        }
        let meta = match read_meta(&path) {
            Some(m) => m,
            None => return ExternalChange::None,
        };
        if self.disk_meta.as_ref() == Some(&meta) {
            return ExternalChange::None;
        }
        let disk_text = match fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => return ExternalChange::None,
        };
        let disk_stripped = disk_text
            .strip_prefix('\u{feff}')
            .map(str::to_string)
            .unwrap_or(disk_text);
        self.disk_meta = Some(meta);
        if disk_stripped == self.text {
            ExternalChange::None
        } else if self.dirty {
            ExternalChange::Conflicted(disk_stripped)
        } else {
            self.text = disk_stripped;
            self.version = self.version.wrapping_add(1);
            ExternalChange::None
        }
    }

    pub fn reload_from_disk_text(&mut self, disk: String, path: &Path) {
        self.text = disk;
        self.dirty = false;
        self.disk_meta = read_meta(path);
        self.version = self.version.wrapping_add(1);
    }

    pub fn keep_mine(&mut self) {
        if let Some(p) = self.path.clone() {
            self.disk_meta = read_meta(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_roundtrip() {
        let dir = std::env::temp_dir().join("lihati_test_bom");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bom.md");
        fs::write(&p, "\u{feff}# hello".as_bytes()).unwrap();

        let mut doc = Document::new();
        doc.load(&p).unwrap();
        assert!(doc.had_bom);
        assert_eq!(doc.text, "# hello");
        doc.on_text_changed();
        doc.save().unwrap();

        let raw = fs::read(&p).unwrap();
        assert_eq!(&raw[0..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(String::from_utf8_lossy(&raw), "\u{feff}# hello");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn invalid_utf8_is_lossy_not_fatal() {
        let dir = std::env::temp_dir().join("lihati_test_bad");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.md");
        fs::write(&p, [0x68, 0x69, 0xFF, 0xFE]).unwrap();

        let mut doc = Document::new();
        doc.load(&p).unwrap();
        assert_eq!(doc.text, "hi\u{fffd}\u{fffd}");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn missing_file_reports_error() {
        let mut doc = Document::new();
        let err = doc.load(Path::new("Z:/definitely/not/here/x.md"));
        assert!(err.is_err());
        assert!(doc.path.is_none());
    }

    #[test]
    fn version_bumps_on_edit_and_load() {
        let dir = std::env::temp_dir().join("lihati_test_ver");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("v.md");
        fs::write(&p, "# v").unwrap();

        let mut doc = Document::new();
        let v0 = doc.version;
        doc.on_text_changed();
        assert_ne!(doc.version, v0);
        doc.load(&p).unwrap();
        let v2 = doc.version;
        assert_ne!(doc.version, v0);
        doc.reload_from_disk_text("# w".into(), &p);
        assert_ne!(doc.version, v2);
        let _ = fs::remove_file(&p);
    }
}
