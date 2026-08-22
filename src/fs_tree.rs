use std::path::Path;

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_dir: bool,
}

const MD_EXTS: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mkdn"];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MD_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn list_dir(path: &Path) -> Vec<Entry> {
    let mut dirs: Vec<Entry> = Vec::new();
    let mut mds: Vec<Entry> = Vec::new();
    let Ok(read) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    for entry in read.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        let p = entry.path();
        if ft.is_dir() {
            dirs.push(Entry { name, path: p, is_dir: true });
        } else if is_markdown(&p) {
            mds.push(Entry { name, path: p, is_dir: false });
        }
    }
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    mds.sort_by(by_name);
    dirs.extend(mds);
    dirs
}
