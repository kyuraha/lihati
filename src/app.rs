use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Key, Label, Layout, Margin, Modifiers,
    RichText, ScrollArea, Sense, TextEdit, TextStyle, TopBottomPanel, UiBuilder, ViewportCommand,
};

use crate::document::{Document, ExternalChange};
use crate::fs_tree::{list_dir, Entry};
use crate::markdown::{self, Heading};
use crate::preview;
use crate::state::{self, PersistState, ThemePref, ViewMode};
use crate::theme;

const WATCH_INTERVAL: Duration = Duration::from_millis(1000);
const WATCH_INTERVAL_BACKGROUND: Duration = Duration::from_millis(5000);
const STORE_INTERVAL: Duration = Duration::from_secs(5);
const DIR_TTL: Duration = Duration::from_millis(2000);
const LARGE_FILE_BYTES: u64 = 10 * 1024 * 1024;

enum Pending {
    Open(PathBuf),
    New,
    Quit,
}

struct Toast {
    msg: String,
    born: Instant,
    err: bool,
}

struct Find {
    query: String,
    matches: Vec<usize>,
    idx: usize,
    jumped: bool,
    version: u64,
    last_query: String,
}

/// Scroll memory per view (pixels): offset, content height, viewport height.
#[derive(Clone, Copy, Default)]
struct ViewScroll {
    offset: f32,
    content: f32,
    viewport: f32,
}

pub struct App {
    state: PersistState,
    doc: Document,
    root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    outline_collapsed: HashSet<usize>,
    files_filter: String,
    outline_filter: String,
    active_outline: Option<usize>,
    src_scroll: ViewScroll,
    prev_scroll: ViewScroll,
    last_view: ViewMode,
    scroll_override: Option<f32>,
    files_rect: Option<egui::Rect>,
    outline_rect: Option<egui::Rect>,
    outline_width: f32,
    dir_cache: HashMap<PathBuf, (Instant, Arc<Vec<Entry>>)>,
    preview: preview::Preview,
    toasts: Vec<Toast>,
    pending: Option<Pending>,
    conflict_disk_text: Option<String>,
    vanished_banner: bool,
    jump: Option<usize>,
    editor_pitch: f32,
    find: Option<Find>,
    find_focus_next: bool,
    headings: Vec<Heading>,
    words: usize,
    lines: usize,
    derived_version: u64,
    last_watch: Instant,
    last_store: Instant,
    last_scroll: Instant,
    cur_dark: bool,
    title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        theme::init_fonts(&cc.egui_ctx);
        preview::warmup();
        let state = state::load(cc);
        theme::apply(&cc.egui_ctx);
        theme::set_pref(&cc.egui_ctx, state.theme);

        let now = Instant::now();
        let last_view = state.view;
        let outline_width = state.outline_width.unwrap_or(250.0).clamp(170.0, 420.0);
        let mut app = App {
            state,
            doc: Document::new(),
            root: None,
            expanded: HashSet::new(),
            outline_collapsed: HashSet::new(),
            files_filter: String::new(),
            outline_filter: String::new(),
            active_outline: None,
            src_scroll: ViewScroll::default(),
            prev_scroll: ViewScroll::default(),
            last_view,
            scroll_override: None,
            files_rect: None,
            outline_rect: None,
            outline_width,
            dir_cache: HashMap::new(),
            preview: preview::Preview::new(),
            toasts: Vec::new(),
            pending: None,
            conflict_disk_text: None,
            vanished_banner: false,
            jump: None,
            editor_pitch: 0.0,
            find: None,
            find_focus_next: false,
            headings: Vec::new(),
            words: 0,
            lines: 0,
            derived_version: u64::MAX,
            last_watch: now,
            last_store: now,
            last_scroll: now - Duration::from_secs(60),
            cur_dark: true,
            title: String::new(),
        };

        if let Some(f) = &file {
            match app.doc.load(f) {
                Ok(()) => {
                    app.push_recent(f);
                    app.state.last_file = Some(f.to_string_lossy().to_string());
                }
                Err(e) => app.toast(e, true),
            }
        } else if let Some(last) = app.state.last_file.clone() {
            let p = PathBuf::from(&last);
            if p.is_file() {
                app.doc.load(&p).ok();
            }
        }

        app.root = app
            .state
            .root
            .clone()
            .map(PathBuf::from)
            .or_else(|| app.doc.path.clone().and_then(|p| parent_of(&p)));
        if let Some(r) = app.root.clone() {
            if r.is_dir() {
                app.expanded.insert(r);
            }
        }
        app.ensure_root_for_current();
        app.refresh_derived();
        app
    }

    fn pal(&self) -> theme::Palette {
        theme::palette(self.cur_dark)
    }

    fn toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toasts.push(Toast { msg: msg.into(), born: Instant::now(), err });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn push_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.state.recents.retain(|r| r != &s);
        self.state.recents.insert(0, s);
        self.state.recents.truncate(10);
    }

    fn ensure_root_for_current(&mut self) {
        let Some(p) = self.doc.path.clone() else { return };
        let Some(parent) = parent_of(&p) else { return };
        match &self.root {
            None => self.set_root(parent),
            Some(r) => {
                if !p.starts_with(r) {
                    self.set_root(parent);
                }
            }
        }
    }

    fn set_root(&mut self, path: PathBuf) {
        self.expanded.clear();
        self.dir_cache.clear();
        self.expanded.insert(path.clone());
        self.root = Some(path);
        self.state.root = self.root.as_ref().map(|p| p.to_string_lossy().to_string());
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "mdown", "mkd", "mkdn", "txt"])
            .pick_file()
        {
            self.open_path(p);
        }
    }

    fn choose_root(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder() {
            self.set_root(p);
        }
    }

    fn refresh_derived(&mut self) {
        let v = self.doc.version;
        if v == self.derived_version {
            return;
        }
        self.derived_version = v;
        self.headings = markdown::outline(&self.doc.text);
        self.words = markdown::word_count(&self.doc.text);
        self.lines = self.doc.text.lines().count();
    }

    fn note_large_file(&mut self) {
        if let Some(p) = &self.doc.path {
            if let Ok(md) = std::fs::metadata(p) {
                if md.len() > LARGE_FILE_BYTES {
                    self.toast(
                        format!(
                            "Large file ({:.1} MB) \u{2014} editing may be slow",
                            md.len() as f64 / 1_048_576.0
                        ),
                        true,
                    );
                }
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.doc.dirty {
            self.pending = Some(Pending::Open(path));
            return;
        }
        match self.doc.load(&path) {
            Ok(()) => {
                self.push_recent(&path);
                self.conflict_disk_text = None;
                self.vanished_banner = false;
                self.jump = None;
                self.active_outline = None;
                self.note_large_file();
                self.ensure_root_for_current();
                self.state.last_file = Some(path.to_string_lossy().to_string());
            }
            Err(e) => self.toast(e, true),
        }
    }

    fn new_doc(&mut self) {
        if self.doc.dirty {
            self.pending = Some(Pending::New);
            return;
        }
        self.do_new();
    }

    fn do_new(&mut self) {
        self.doc = Document::new();
        self.conflict_disk_text = None;
        self.vanished_banner = false;
        self.jump = None;
        self.active_outline = None;
    }

    fn save_current(&mut self) {
        if self.doc.path.is_some() {
            match self.doc.save() {
                Ok(()) => {
                    self.vanished_banner = false;
                    self.toast("Saved", false);
                }
                Err(e) => self.toast(e, true),
            }
        } else {
            self.save_as_dialog();
        }
    }

    fn save_as_dialog(&mut self) -> bool {
        let start_name = self.doc.file_name();
        let picked = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(&start_name)
            .save_file();
        match picked {
            Some(mut p) => {
                if p.extension().is_none() {
                    p.set_extension("md");
                }
                self.doc.adopt_path(&p);
                match self.doc.save() {
                    Ok(()) => {
                        self.push_recent(&p);
                        self.ensure_root_for_current();
                        self.vanished_banner = false;
                        self.state.last_file = Some(p.to_string_lossy().to_string());
                        true
                    }
                    Err(e) => {
                        self.doc.path = None;
                        self.toast(e, true);
                        false
                    }
                }
            }
            None => false,
        }
    }

    fn complete_pending(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.pending.take() {
            match p {
                Pending::Open(path) => self.open_path(path),
                Pending::New => self.do_new(),
                Pending::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }

    fn watch_tick(&mut self) {
        if self.doc.path.is_none() || self.conflict_disk_text.is_some() {
            return;
        }
        if self.last_watch.elapsed() < WATCH_INTERVAL {
            return;
        }
        self.last_watch = Instant::now();
        match self.doc.check_external() {
            ExternalChange::None => {}
            ExternalChange::Conflicted(disk) => self.conflict_disk_text = Some(disk),
            ExternalChange::Vanished => self.vanished_banner = true,
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use eframe::egui::Modifiers;
        #[derive(PartialEq)]
        enum Cmd {
            None,
            Save,
            SaveAs,
            Open,
            New,
            ToggleView,
            Find,
            ZoomIn,
            ZoomOut,
            ZoomReset,
        }
        let mut cmd = Cmd::None;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::S) {
                cmd = Cmd::SaveAs;
            } else if i.consume_key(Modifiers::CTRL, Key::S) {
                cmd = Cmd::Save;
            } else if i.consume_key(Modifiers::CTRL, Key::O) {
                cmd = Cmd::Open;
            } else if i.consume_key(Modifiers::CTRL, Key::N) {
                cmd = Cmd::New;
            } else if i.consume_key(Modifiers::CTRL, Key::E) {
                cmd = Cmd::ToggleView;
            } else if i.consume_key(Modifiers::CTRL, Key::F) {
                cmd = Cmd::Find;
            } else if i.consume_key(Modifiers::CTRL, Key::Equals) {
                cmd = Cmd::ZoomIn;
            } else if i.consume_key(Modifiers::CTRL, Key::Minus) {
                cmd = Cmd::ZoomOut;
            } else if i.consume_key(Modifiers::CTRL, Key::Num0) {
                cmd = Cmd::ZoomReset;
            }
        });
        match cmd {
            Cmd::Save => self.save_current(),
            Cmd::SaveAs => {
                self.save_as_dialog();
            }
            Cmd::Open => self.open_dialog(),
            Cmd::New => self.new_doc(),
            Cmd::ToggleView => {
                self.state.view = if self.state.view == ViewMode::Source {
                    ViewMode::Preview
                } else {
                    ViewMode::Source
                };
            }
            Cmd::Find => {
                if self.find.take().is_none() {
                    self.find = Some(Find {
                        query: String::new(),
                        matches: Vec::new(),
                        idx: 0,
                        jumped: false,
                        version: u64::MAX,
                        last_query: String::new(),
                    });
                    self.find_focus_next = true;
                }
            }
            Cmd::ZoomIn => self.state.zoom = (self.state.zoom + 0.1).min(2.2),
            Cmd::ZoomOut => self.state.zoom = (self.state.zoom - 0.1).max(0.6),
            Cmd::ZoomReset => self.state.zoom = 1.0,
            Cmd::None => {}
        }
    }

    fn sync_theme_and_zoom(&mut self, ctx: &egui::Context) {
        let now_dark = ctx.style().visuals.dark_mode;
        if now_dark != self.cur_dark {
            self.cur_dark = now_dark;
        }
        if (ctx.zoom_factor() - self.state.zoom).abs() > 0.01 {
            ctx.set_zoom_factor(self.state.zoom);
        }
    }

    fn cycle_theme(&mut self, ctx: &egui::Context) {
        self.state.theme = match self.state.theme {
            ThemePref::Light => ThemePref::Dark,
            ThemePref::Dark => ThemePref::Light,
        };
        theme::set_pref(ctx, self.state.theme);
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        TopBottomPanel::top("toolbar")
            .frame(Frame::default().fill(pal.panel).inner_margin(Margin::symmetric(10, 5)))
            .show(ctx, |ui| {
                // One spacing language for every control in the header.
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
                ui.columns(3, |cols| {
                    // ---- left: panels + file actions ----
                    cols[0].with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if toggle_btn(ui, "Files", self.state.show_dir, &pal).clicked() {
                            self.state.show_dir = !self.state.show_dir;
                        }
                        ui.separator();
                        if small_btn(ui, "Open").on_hover_text("Open a Markdown file (Ctrl+O)").clicked() {
                            self.open_dialog();
                        }
                        if small_btn(ui, "Save").on_hover_text("Save (Ctrl+S)").clicked() {
                            self.save_current();
                        }
                        if small_btn(ui, "Save As").on_hover_text("Save as new file (Ctrl+Shift+S)").clicked() {
                            self.save_as_dialog();
                        }
                    });
                    // ---- center: document title, centered on the window ----
                    cols[1].centered_and_justified(|ui| {
                        let name = self.doc.file_name();
                        let dot = if self.doc.dirty { "\u{25cf} " } else { "" };
                        let tip = self
                            .doc
                            .path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unsaved document".to_string());
                        ui.add(
                            Label::new(
                                RichText::new(format!("{}{}", dot, name))
                                    .size(13.0)
                                    .color(if self.doc.dirty { pal.warn } else { pal.text }),
                            )
                            .truncate(),
                        )
                        .on_hover_text(tip)
                    });
                    // ---- right: view + theme + outline ----
                    cols[2].with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if toggle_btn(ui, "Outline", self.state.show_outline, &pal).clicked() {
                            self.state.show_outline = !self.state.show_outline;
                        }
                        ui.separator();
                        let theme_label = match self.state.theme {
                            ThemePref::Light => "Theme: Light",
                            ThemePref::Dark => "Theme: Dark",
                        };
                        if small_btn(ui, theme_label).on_hover_text("Cycle theme").clicked() {
                            self.cycle_theme(ui.ctx());
                        }
                        if let Some(m) = segmented_view(ui, self.state.view, self.cur_dark, &pal) {
                            self.state.view = m;
                        }
                    });
                });
            });
    }

    fn entries_for(&mut self, dir: &Path) -> Arc<Vec<Entry>> {
        let now = Instant::now();
        let stale = match self.dir_cache.get(dir) {
            Some((t, _)) => now.duration_since(*t) > DIR_TTL,
            None => true,
        };
        if stale {
            let v = Arc::new(list_dir(dir));
            self.dir_cache.insert(dir.to_path_buf(), (now, v));
        }
        match self.dir_cache.get(dir) {
            Some((_, v)) => v.clone(),
            None => Arc::new(Vec::new()),
        }
    }

    fn draw_files_panel(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        egui::SidePanel::left("files-panel")
            .resizable(true)
            .default_width(240.0)
            .width_range(170.0..=420.0)
            .show_separator_line(false)
            .frame(
                Frame::default()
                    .fill(pal.panel)
                    .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                    .inner_margin(Margin::symmetric(8, 6)),
            )
            .show(ctx, |ui| {
                self.files_rect = Some(ui.max_rect());
                // ---- vault card ----
                if let Some(root) = self.root.clone() {
                    let name = root
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Notes")
                        .to_string();
                    vault_card(ui, &pal, &name, &root.to_string_lossy());
                    ui.add_space(4.0);
                }

                // ---- search ----
                ui.horizontal(|ui| {
                    let clear_w = if self.files_filter.is_empty() { 0.0 } else { 26.0 };
                    let w = (ui.available_width() - clear_w).max(40.0);
                    ui.add_sized(
                        [w, 24.0],
                        TextEdit::singleline(&mut self.files_filter)
                            .hint_text("Search files\u{2026}"),
                    );
                    if !self.files_filter.is_empty()
                        && small_btn(ui, "\u{00d7}").on_hover_text("Clear search").clicked()
                    {
                        self.files_filter.clear();
                    }
                });
                ui.add_space(2.0);
                ui.separator();

                // ---- tree ----
                if !self.files_filter.trim().is_empty() {
                    let q = self.files_filter.trim().to_lowercase();
                    let mut hits: Vec<PathBuf> = Vec::new();
                    if let Some(root) = self.root.clone() {
                        collect_md(&root, &mut hits, 800, 0);
                    }
                    hits.retain(|p| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_lowercase().contains(q.as_str()))
                            .unwrap_or(false)
                    });
                    hits.sort();
                    ScrollArea::vertical()
                        .id_salt("files-search-scroll")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            if hits.is_empty() {
                                tree_empty(ui, &pal, "No matching notes");
                            } else {
                                for p in hits {
                                    let name = p
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    let selected =
                                        self.doc.path.as_deref() == Some(p.as_path());
                                    let dirty = selected && self.doc.dirty;
                                    if file_row(ui, &pal, 0, selected, dirty, &name, &p.to_string_lossy())
                                    {
                                        self.open_path(p);
                                    }
                                }
                            }
                        });
                    return;
                }

                match self.root.clone() {
                    Some(root) if root.is_dir() => {
                        let entries = self.entries_for(&root);
                        if entries.is_empty() {
                            tree_empty(ui, &pal, "No notes yet");
                            ui.add_space(2.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Markdown files will appear here")
                                        .size(11.0)
                                        .color(pal.weak),
                                );
                            });
                        } else {
                            ScrollArea::vertical()
                                .id_salt("files-scroll")
                                .auto_shrink(false)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    self.draw_dir_entries(ui, &root, 0);
                                });
                        }
                        let (d, f) = dir_counts(&self.entries_for(&root));
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "{} folders \u{00b7} {} notes",
                                d, f
                            ))
                            .size(11.0)
                            .color(pal.weak),
                        );
                    }
                    Some(_) => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Folder not found").weak().size(12.5));
                            ui.add_space(6.0);
                            if small_btn(ui, "Choose folder\u{2026}").clicked() {
                                self.choose_root();
                            }
                        });
                    }
                    None => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("No folder open").weak().size(12.5));
                            ui.add_space(6.0);
                            if small_btn(ui, "Choose folder\u{2026}").clicked() {
                                self.choose_root();
                            }
                        });
                    }
                }
            });
    }

    fn draw_dir_entries(&mut self, ui: &mut egui::Ui, dir: &Path, depth: usize) {
        let entries = self.entries_for(dir);
        for e in entries.iter() {
            if e.is_dir {
                let expanded = self.expanded.contains(&e.path);
                let tip = e.path.to_string_lossy().to_string();
                if folder_row(ui, &self.pal(), depth, expanded, &e.name, &tip).clicked() {
                    if expanded {
                        self.expanded.remove(&e.path);
                    } else {
                        self.expanded.insert(e.path.clone());
                    }
                } else if expanded {
                    let path = e.path.clone();
                    self.draw_dir_entries(ui, &path, depth + 1);
                }
            } else {
                let selected = self.doc.path.as_deref() == Some(e.path.as_path());
                let dirty = selected && self.doc.dirty;
                let tip = e.path.to_string_lossy().to_string();
                if file_row(ui, &self.pal(), depth, selected, dirty, &e.name, &tip) {
                    let path = e.path.clone();
                    self.open_path(path);
                }
            }
        }
    }

    fn draw_center(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        egui::CentralPanel::default()
            .frame(Frame::default().fill(pal.bg))
            .show(ctx, |ui| {
                if let Some(disk) = self.conflict_disk_text.clone() {
                    let action = banner(ui, &pal, "\u{26a0}  This file changed on disk while you had unsaved changes.", ("Load from disk", "Keep mine"));
                    match action {
                        Some(true) => {
                            if let Some(p) = self.doc.path.clone() {
                                self.doc.reload_from_disk_text(disk, &p);
                                self.toast("Reloaded from disk", false);
                            }
                            self.conflict_disk_text = None;
                        }
                        Some(false) => {
                            self.doc.keep_mine();
                            self.conflict_disk_text = None;
                        }
                        None => {}
                    }
                    ui.add_space(4.0);
                }
                if self.vanished_banner {
                    let action = banner(ui, &pal, "\u{26a0}  The file no longer exists on disk.", ("Save anyway", "Dismiss"));
                    match action {
                        Some(true) => {
                            self.vanished_banner = false;
                            self.save_current();
                        }
                        Some(false) => self.vanished_banner = false,
                        None => {}
                    }
                    ui.add_space(4.0);
                }

                if self.doc.is_empty_doc() {
                    empty_state(ui, &pal, &mut |which| match which {
                        0 => self.open_dialog(),
                        _ => self.new_doc(),
                    });
                    return;
                }

                // Keep the reading position across Source/Preview switches:
                // map the old view's scroll fraction onto the new view.
                if self.state.view != self.last_view {
                    let (from, to) = if self.last_view == ViewMode::Source {
                        (self.src_scroll, self.prev_scroll)
                    } else {
                        (self.prev_scroll, self.src_scroll)
                    };
                    let frac = (from.offset / (from.content - from.viewport).max(1.0))
                        .clamp(0.0, 1.0);
                    self.scroll_override =
                        Some(frac * (to.content - to.viewport).max(0.0));
                }
                self.last_view = self.state.view;

                match self.state.view {
                    ViewMode::Source => {
                        let ov = self.scroll_override.take();
                        let Self { doc, jump, editor_pitch, src_scroll, .. } = self;
                        *src_scroll = editor_pane(ui, doc, jump, editor_pitch, ov);
                    }
                    ViewMode::Preview => {
                        let ov = self.scroll_override.take();
                        let Self { doc, preview, cur_dark, prev_scroll, .. } = self;
                        *prev_scroll = preview_pane(ui, doc, preview, *cur_dark, ov);
                    }
                }
            });
    }

    fn draw_outline_panel(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        egui::SidePanel::right("outline-panel")
            .resizable(false)
            .exact_width(self.outline_width.clamp(170.0, 420.0))
            .show_separator_line(false)
            .frame(
                Frame::default()
                    .fill(pal.panel)
                    .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                    .inner_margin(Margin::symmetric(8, 6)),
            )
            .show(ctx, |ui| {
                self.outline_rect = Some(ui.max_rect());
                // Splitter metrics first: available height shrinks as content
                // is laid out, so measure before adding widgets. The handle
                // itself is allocated at the very end (allocating it up-front
                // consumed the panel's layout space and pushed content out).
                let htop = ui.max_rect().min.y;
                let hfull = ui.available_height();
                let hedge = ui.max_rect().min.x - 8.0;
                // ---- search ----
                ui.horizontal(|ui| {
                    let clear_w = if self.outline_filter.is_empty() { 0.0 } else { 26.0 };
                    let w = (ui.available_width() - clear_w).max(40.0);
                    ui.add_sized(
                        [w, 24.0],
                        TextEdit::singleline(&mut self.outline_filter)
                            .hint_text("Search outline\u{2026}"),
                    );
                    if !self.outline_filter.is_empty()
                        && small_btn(ui, "\u{00d7}").on_hover_text("Clear search").clicked()
                    {
                        self.outline_filter.clear();
                    }
                });
                ui.add_space(2.0);
                ui.separator();

                // ---- list ----
                let q = self.outline_filter.trim().to_lowercase();
                let filtering = !q.is_empty();
                let headings = self.headings.clone();
                let shown: Vec<(usize, Heading)> = headings
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| {
                        filtering && h.title.to_lowercase().contains(q.as_str()) || !filtering
                    })
                    .map(|(i, h)| (i, h.clone()))
                    .collect();

                ScrollArea::vertical()
                    .id_salt("outline-scroll")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        if headings.is_empty() {
                            tree_empty(ui, &pal, "No headings yet");
                            ui.add_space(2.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Use # headings to build an outline")
                                        .size(11.0)
                                        .color(pal.weak),
                                );
                            });
                            return;
                        }
                        if shown.is_empty() {
                            tree_empty(ui, &pal, "No matching headings");
                            return;
                        }

                        let min_lvl =
                            shown.iter().map(|(_, h)| h.level).min().unwrap_or(1);
                        let mut open_ancestors: Vec<(u8, usize)> = Vec::new();
                        for (pos, (_, h)) in shown.iter().enumerate() {
                            while let Some(&(lvl, _)) = open_ancestors.last() {
                                if lvl >= h.level {
                                    open_ancestors.pop();
                                } else {
                                    break;
                                }
                            }
                            let visible = filtering
                                || open_ancestors
                                    .iter()
                                    .all(|(_, line)| !self.outline_collapsed.contains(line));
                            let has_children = !filtering
                                && shown
                                    .get(pos + 1)
                                    .map(|(_, n)| n.level > h.level)
                                    .unwrap_or(false);
                            let open = !self.outline_collapsed.contains(&h.line);
                            if !visible {
                                // Push even when collapsed: descendants must still
                                // see this ancestor to know they stay hidden.
                                if has_children {
                                    open_ancestors.push((h.level, h.line));
                                }
                                continue;
                            }

                            let depth =
                                h.level.saturating_sub(min_lvl) as usize;
                            let active = self.active_outline == Some(h.line);
                            let (rect, resp) =
                                tree_row_frame(ui, &pal, OUTLINE_ROW_H, active, true);
                            paint_guides(ui, &pal, rect, depth, OUTLINE_STEP);
                            let lx = level_x(rect, depth, OUTLINE_STEP);

                            let mut caret_toggle = false;
                            let content_rect = egui::Rect::from_min_max(
                                egui::pos2(lx - 10.0, rect.min.y),
                                egui::pos2(rect.max.x - 4.0, rect.max.y),
                            );
                            let size = match h.level {
                                1 => 13.5,
                                2 => 13.0,
                                3 => 12.5,
                                4 => 12.0,
                                _ => 11.5,
                            };
                            let display = if h.title.trim().is_empty() {
                                "(untitled)".to_string()
                            } else {
                                h.title.clone()
                            };
                            let mut rt = RichText::new(display).size(size);
                            if h.level == 1 {
                                rt = rt.font(FontId::new(size, theme::family_bold()));
                            }
                            rt = rt.color(if active { pal.accent } else { pal.text });
                            let tip = format!("Line {} \u{2014} click to jump", h.line + 1);
                            ui.allocate_new_ui(
                                UiBuilder::new().max_rect(content_rect),
                                |ui| {
                                    ui.with_layout(
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            if has_children {
                                                let (cr, cresp) = ui.allocate_exact_size(
                                                    egui::vec2(20.0, 22.0),
                                                    Sense::click(),
                                                );
                                                if cresp.hovered() {
                                                    ui.painter().rect_filled(
                                                        cr,
                                                        4.0,
                                                        pal.hover,
                                                    );
                                                }
                                                paint_caret_tri(
                                                    ui,
                                                    cr.center(),
                                                    open,
                                                    if cresp.hovered() {
                                                        pal.text
                                                    } else {
                                                        pal.weak
                                                    },
                                                );
                                                if cresp.clicked() {
                                                    caret_toggle = true;
                                                }
                                                let _ = cresp.on_hover_text(if open {
                                                    "Collapse section"
                                                } else {
                                                    "Expand section"
                                                });
                                            } else {
                                                ui.add_space(20.0);
                                            }
                                            ui.add(Label::new(rt).truncate());
                                        },
                                    );
                                },
                            );
                            let resp = resp.on_hover_text(tip);
                            if caret_toggle {
                                if open {
                                    self.outline_collapsed.insert(h.line);
                                } else {
                                    self.outline_collapsed.remove(&h.line);
                                }
                            } else if resp.clicked() {
                                self.jump = Some(h.line);
                                self.active_outline = Some(h.line);
                                if self.state.view == ViewMode::Preview {
                                    self.state.view = ViewMode::Source;
                                }
                            }

                            if has_children {
                                open_ancestors.push((h.level, h.line));
                            }
                        }
                    });
                // Explicit splitter handle LAST: same metrics as captured
                // above. Hit-test only (no allocation): allocating the strip
                // would stretch the panel content rect past its frame, push
                // the central panel away and open a clear-color seam.
                // Drag-only strip just inside the edge (rows keep clicks).
                let hr = egui::Rect::from_min_max(
                    egui::pos2(hedge + 2.0, htop),
                    egui::pos2(hedge + 14.0, htop + hfull),
                );
                let mut w = self.outline_width;
                let resp = ui.interact(hr, egui::Id::new("outline-splitter"), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if resp.dragged() {
                    w = (w - resp.drag_delta().x).clamp(170.0, 420.0);
                }
                self.outline_width = w;
            });
    }

    /// Visible splitter grips: three dots exactly on each panel edge so the
    /// eye can acquire the (invisible, generous) drag zone without hunting.
    fn paint_splitter_grips(&self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        let pal = self.pal();
        let pointer = ctx.pointer_hover_pos();
        let panels = [
            (self.state.show_dir, self.files_rect, true),
            (self.state.show_outline, self.outline_rect, false),
        ];
        for (shown, rect_opt, is_left) in panels {
            if !shown {
                continue;
            }
            let Some(r) = rect_opt else { continue };
            // Dots sit just inside the panel, in the middle of the EFFECTIVE
            // grab band: the center scrollbar covers the zone part left of the
            // edge, so grabbing exactly on the line would catch the scrollbar.
            let edge = if is_left { r.max.x + 8.0 } else { r.min.x - 8.0 };
            let x = if is_left { r.max.x } else { r.min.x };
            let y = (r.min.y + r.max.y) * 0.5;
            let hot = pointer
                .map(|p| (p.x - edge).abs() <= 14.0 && r.y_range().contains(p.y))
                .unwrap_or(false);
            let col = if hot {
                pal.accent
            } else {
                pal.weak.gamma_multiply(0.55)
            };
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("splitter-grips"),
            ));
            for dy in [-7.0, 0.0, 7.0] {
                painter.circle_filled(egui::pos2(x, y + dy), 1.6, col);
            }
        }
    }

    fn draw_status(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        TopBottomPanel::bottom("status-bar")
            .frame(Frame::default().fill(pal.panel).inner_margin(Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let path = self
                        .doc
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unsaved document".to_string());
                    ui.add(Label::new(RichText::new(path).size(11.5).color(pal.weak)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} words \u{00b7} {} lines \u{00b7} {:.0}%",
                                self.words, self.lines, self.state.zoom * 100.0
                            ))
                            .size(11.5)
                            .color(pal.weak),
                        );
                    });
                });
            });
    }

    fn draw_modal(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let pal = self.pal();
        ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("modal-dim")))
            .rect_filled(ctx.screen_rect(), 0.0, Color32::from_black_alpha(110));

        let mut choice: u8 = 0;
        egui::Area::new(egui::Id::new("modal"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .interactable(false)
            .show(ctx, |ui| {
                Frame::default()
                    .fill(pal.panel)
                    .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                    .corner_radius(6)
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(
                            RichText::new("Unsaved changes")
                                .font(FontId::new(17.0, theme::family_bold())),
                        );
                        ui.add_space(6.0);
                        ui.add(Label::new(RichText::new(
                            "Your document has unsaved edits. Save before continuing?",
                        ))
                        .wrap());
                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if small_btn(ui, "Cancel").clicked() {
                                choice = 3;
                            }
                            if small_btn(ui, "Don't save").clicked() {
                                choice = 2;
                            }
                            if ui
                                .add(Button::new(RichText::new("Save").color(Color32::WHITE)).fill(pal.accent))
                                .clicked()
                            {
                                choice = 1;
                            }
                        });
                    });
            });

        match choice {
            1 => {
                let saved = if self.doc.path.is_some() {
                    matches!(self.doc.save(), Ok(()))
                } else {
                    self.save_as_dialog()
                };
                if saved {
                    self.complete_pending(ctx);
                } else {
                    self.pending = None;
                }
            }
            2 => {
                self.doc.dirty = false;
                self.complete_pending(ctx);
            }
            3 => self.pending = None,
            _ => {}
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| now.duration_since(t.born) < Duration::from_millis(2600));
        if self.toasts.is_empty() {
            return;
        }
        let pal = self.pal();
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -34.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for t in self.toasts.iter().rev() {
                    let color = if t.err { pal.err } else { pal.accent };
                    Frame::default()
                        .fill(pal.panel)
                        .stroke(egui::Stroke::new(1.0_f32, color.gamma_multiply(0.7)))
                        .corner_radius(6)
                        .inner_margin(Margin::symmetric(12, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.add(Label::new(RichText::new(t.msg.clone()).size(13.0)).wrap());
                        });
                    ui.add_space(4.0);
                }
            });
    }

    fn jump_to_byte(&mut self, byte: usize) {
        let line = self.doc.text[..byte.min(self.doc.text.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        self.jump = Some(line);
        if self.state.view == ViewMode::Preview {
            self.state.view = ViewMode::Source;
        }
    }

    fn find_step(&mut self, forward: bool) {
        let Some(f) = &mut self.find else { return };
        if f.matches.is_empty() {
            return;
        }
        f.idx = if forward {
            (f.idx + 1) % f.matches.len()
        } else {
            f.idx.checked_sub(1).unwrap_or(f.matches.len() - 1)
        };
        f.jumped = true;
    }

    fn refresh_find(&mut self) {
        let Some(f) = &mut self.find else { return };
        if f.version == self.doc.version && f.last_query == f.query {
            return;
        }
        let query_changed = f.query != f.last_query;
        f.version = self.doc.version;
        if query_changed {
            f.idx = 0;
            f.jumped = false;
        }
        f.last_query.clone_from(&f.query);
        f.matches = markdown::find_matches(&self.doc.text, &f.query);
        if f.idx >= f.matches.len() {
            f.idx = 0;
        }
    }

    fn draw_find(&mut self, ctx: &egui::Context) {
        if self.find.is_none() || self.pending.is_some() {
            return;
        }
        self.refresh_find();

        let query_focused =
            ctx.memory(|m| m.focused()) == Some(egui::Id::new("find-input"));
        let enter_pressed = query_focused
            && ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
        let shift_enter_pressed = query_focused
            && ctx.input(|i| i.key_pressed(Key::Enter) && i.modifiers.shift);

        if self.find_focus_next {
            self.find_focus_next = false;
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("find-input")));
        }

        let pal = self.pal();
        let mut action: Option<bool> = None;
        let mut close = false;
        egui::Area::new(egui::Id::new("find-bar"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_TOP, [-14.0, 44.0])
            .show(ctx, |ui| {
                Frame::default()
                    .fill(pal.panel)
                    .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                    .corner_radius(6)
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut query =
                                std::mem::take(&mut self.find.as_mut().unwrap().query);
                            let resp = TextEdit::singleline(&mut query)
                                .id_salt("find-input")
                                .desired_width(200.0)
                                .hint_text("Find\u{2026}")
                                .show(ui);
                            self.find.as_mut().unwrap().query = query;
                            let _ = resp;

                            let count =
                                self.find.as_ref().map(|f| f.matches.len()).unwrap_or(0);
                            let cur = if count == 0 {
                                0
                            } else {
                                self.find.as_ref().unwrap().idx + 1
                            };
                            ui.label(
                                RichText::new(if count == 0 {
                                    "0/0".to_string()
                                } else {
                                    format!("{}/{}", cur, count)
                                })
                                .size(12.5)
                                .color(pal.weak),
                            );
                            if small_btn(ui, "\u{2193}")
                                .on_hover_text("Next match (Enter)")
                                .clicked()
                            {
                                action = Some(true);
                            }
                            if small_btn(ui, "\u{2191}")
                                .on_hover_text("Previous match (Shift+Enter)")
                                .clicked()
                            {
                                action = Some(false);
                            }
                            if small_btn(ui, "\u{00d7}").on_hover_text("Close (Esc)").clicked() {
                                close = true;
                            }
                        });
                    });
            });
        if let Some(forward) = action {
            self.find_step(forward);
            self.jump_to_current();
        } else if enter_pressed || shift_enter_pressed {
            if shift_enter_pressed {
                self.find_step(false);
            } else {
                let already = self.find.as_ref().map(|f| f.jumped).unwrap_or(false);
                if already {
                    self.find_step(true);
                } else if let Some(f) = &mut self.find {
                    f.jumped = true;
                }
            }
            self.jump_to_current();
        }
        if close {
            self.find = None;
        }
    }

    fn jump_to_current(&mut self) {
        self.refresh_find();
        let byte = match &self.find {
            Some(f) if !f.matches.is_empty() => f.matches[f.idx.min(f.matches.len() - 1)],
            _ => return,
        };
        self.jump_to_byte(byte);
    }

    fn handle_drop(&mut self, ctx: &egui::Context) {
        let has_drops = ctx.input(|i| !i.raw.dropped_files.is_empty());
        if has_drops {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            if let Some(f) = dropped.into_iter().find_map(|d| d.path) {
                self.open_path(f);
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.sync_theme_and_zoom(ctx);
        self.refresh_derived();

        if ctx.input(|i| i.viewport().close_requested()) && self.doc.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if !matches!(self.pending, Some(Pending::Quit)) {
                self.pending = Some(Pending::Quit);
            }
        }

        let escape_pressed =
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if escape_pressed {
            if self.pending.is_some() {
                self.pending = None;
            } else {
                self.find = None;
            }
        }

        self.handle_drop(ctx);
        let scroll_pending = ctx.input(|i| {
            i.raw_scroll_delta != egui::Vec2::ZERO || i.smooth_scroll_delta != egui::Vec2::ZERO
        });
        if scroll_pending {
            self.last_scroll = Instant::now();
        }
        if self.pending.is_none() {
            self.shortcuts(ctx);
        }
        self.watch_tick();

        self.draw_toolbar(ctx);
        self.draw_status(ctx);
        if self.state.show_dir {
            self.draw_files_panel(ctx);
        }
        if self.state.show_outline {
            self.draw_outline_panel(ctx);
        }
        self.draw_center(ctx);
        self.draw_modal(ctx);
        self.draw_find(ctx);
        self.draw_toasts(ctx);
        self.paint_splitter_grips(ctx);

        if self.last_store.elapsed() > STORE_INTERVAL {
            sync_state(self);
            if let Some(storage) = frame.storage_mut() {
                state::store(&self.state.clone(), storage);
                storage.flush();
            }
            self.last_store = Instant::now();
        }

        let wake = if !self.toasts.is_empty() {
            Some(Duration::from_millis(120))
        } else if self.last_scroll.elapsed() < Duration::from_millis(500) {
            Some(Duration::from_millis(8))
        } else if self.doc.path.is_some() {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            Some(if focused { WATCH_INTERVAL } else { WATCH_INTERVAL_BACKGROUND })
        } else {
            None
        };
        if let Some(d) = wake {
            ctx.request_repaint_after(d);
        }

        let t = format!(
            "{}{}\u{2003}\u{2014}\u{2003}Lihati",
            if self.doc.dirty { "\u{2731} " } else { "" },
            self.doc.file_name()
        );
        if t != self.title {
            self.title = t.clone();
            ctx.send_viewport_cmd(ViewportCommand::Title(t));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        sync_state(self);
        state::store(&self.state, storage);
    }
}

fn sync_state(app: &mut App) {
    app.state.last_file = app
        .doc
        .path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    app.state.root = app.root.as_ref().map(|p| p.to_string_lossy().to_string());
    app.state.outline_width = Some(app.outline_width);
}

fn parent_of(path: &Path) -> Option<PathBuf> {
    path.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf)
}

fn toggle_btn(ui: &mut egui::Ui, label: &str, active: bool, pal: &theme::Palette) -> egui::Response {
    let text = RichText::new(label).size(13.0);
    let btn = if active {
        Button::new(text.color(pal.accent))
    } else {
        Button::new(text)
    };
    ui.add(btn)
}

fn small_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(Button::new(RichText::new(label).size(13.0)))
}

fn segmented_view(
    ui: &mut egui::Ui,
    current: ViewMode,
    dark: bool,
    pal: &theme::Palette,
) -> Option<ViewMode> {
    let mut picked = None;
    Frame::default()
        .fill(pal.faint_fill)
        .corner_radius(5)
        .inner_margin(Margin::symmetric(3, 1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for m in [ViewMode::Source, ViewMode::Preview] {
                    let sel = m == current;
                    let txt = RichText::new(m.label())
                        .size(12.5)
                        .color(if sel { pal.text } else { pal.weak });
                    let mut btn = Button::new(txt);
                    if sel {
                        btn = btn.fill(if dark { pal.active } else { pal.extreme })
                            .stroke(egui::Stroke::new(1.0_f32, pal.stroke));
                    }
                    if ui.add(btn).clicked() {
                        picked = Some(m);
                    }
                }
            });
        });
    picked
}

fn banner(
    ui: &mut egui::Ui,
    pal: &theme::Palette,
    msg: &str,
    buttons: (&str, &str),
) -> Option<bool> {
    let mut action = None;
    Frame::default()
        .fill(pal.warn_soft)
        .corner_radius(4)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_btn(ui, buttons.1).clicked() {
                    action = Some(false);
                }
                if small_btn(ui, buttons.0).clicked() {
                    action = Some(true);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(msg).size(13.5).color(pal.warn));
                });
            });
        });
    action
}

fn empty_state(ui: &mut egui::Ui, pal: &theme::Palette, act: &mut dyn FnMut(u8)) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.22);
        ui.label(RichText::new("Lihati").font(FontId::new(36.0, theme::family_bold())));
        ui.add_space(4.0);
        ui.label(RichText::new("A calm place to write Markdown.").weak());
        ui.add_space(20.0);
        if ui
            .add_sized([220.0, 30.0], Button::new("Open Markdown file\u{2026}").fill(pal.faint_fill))
            .clicked()
        {
            act(0);
        }
        ui.add_space(6.0);
        if ui.add_sized([220.0, 26.0], Button::new("Start a new note")).clicked() {
            act(1);
        }
        ui.add_space(24.0);
        kbd_chip(ui, "Ctrl + O", "open file", pal);
        kbd_chip(ui, "Ctrl + E", "toggle source / preview", pal);
        kbd_chip(ui, "Ctrl + S", "save", pal);
    });
}

fn kbd_chip(ui: &mut egui::Ui, keys: &str, desc: &str, pal: &theme::Palette) {
    ui.add_space(2.0);
    Frame::default()
        .fill(pal.faint_fill)
        .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(keys).font(FontId::monospace(11.5)).color(pal.text));
            ui.label(RichText::new(desc).size(11.5).weak());
        });
}

const TREE_ROW_H: f32 = 26.0;
const TREE_STEP: f32 = 14.0;
const OUTLINE_ROW_H: f32 = 24.0;
const OUTLINE_STEP: f32 = 12.0;

/// Full-width clickable row background with rounded hover / selection fill.
/// `tinted` selects the accent-tinted fill (outline active row) instead of the
/// neutral fill + accent bar (files active row).
fn tree_row_frame(
    ui: &mut egui::Ui,
    pal: &theme::Palette,
    h: f32,
    selected: bool,
    tinted: bool,
) -> (egui::Rect, egui::Response) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let hovered = resp.hovered();
    if selected || hovered {
        let fill = if selected {
            if tinted {
                pal.accent.gamma_multiply(0.20)
            } else {
                pal.active
            }
        } else {
            pal.hover
        };
        ui.painter().rect_filled(rect, 5.0, fill);
        if selected && !tinted {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    rect.min + egui::vec2(1.5, 5.0),
                    egui::vec2(2.5, h - 10.0),
                ),
                1.5,
                pal.accent,
            );
        }
    }
    // Tree rows deliberately do NOT set a hand cursor: like Explorer, VS Code
    // and Obsidian they signal clickability with hover highlight only, so the
    // panel splitter stays the single cursor-changing spot near the edge.
    (rect, resp)
}

fn level_x(rect: egui::Rect, depth: usize, step: f32) -> f32 {
    rect.min.x + 12.0 + depth as f32 * step
}

/// Faint vertical indent guides, one per ancestor level.
fn paint_guides(
    ui: &mut egui::Ui,
    pal: &theme::Palette,
    rect: egui::Rect,
    depth: usize,
    step: f32,
) {
    for d in 0..depth {
        let x = level_x(rect, d, step);
        ui.painter().line_segment(
            [egui::pos2(x, rect.min.y - 1.0), egui::pos2(x, rect.max.y + 1.0)],
            egui::Stroke::new(1.0_f32, pal.stroke),
        );
    }
}

fn paint_caret_tri(ui: &mut egui::Ui, c: egui::Pos2, open: bool, col: Color32) {
    let s = 3.4f32;
    let pts: Vec<egui::Pos2> = if open {
        vec![
            egui::pos2(c.x - s, c.y - s * 0.55),
            egui::pos2(c.x + s, c.y - s * 0.55),
            egui::pos2(c.x, c.y + s * 0.9),
        ]
    } else {
        vec![
            egui::pos2(c.x - s * 0.55, c.y - s),
            egui::pos2(c.x - s * 0.55, c.y + s),
            egui::pos2(c.x + s * 0.9, c.y),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
}

fn paint_folder_icon(ui: &mut egui::Ui, c: egui::Pos2, col: Color32) {
    let p = ui.painter();
    p.rect_filled(
        egui::Rect::from_min_size(egui::pos2(c.x - 6.0, c.y - 4.5), egui::vec2(5.0, 3.5)),
        1.0,
        col,
    );
    p.rect_filled(
        egui::Rect::from_min_size(egui::pos2(c.x - 6.0, c.y - 2.5), egui::vec2(12.0, 7.0)),
        1.8,
        col,
    );
}

fn paint_doc_icon(ui: &mut egui::Ui, c: egui::Pos2, col: Color32) {
    let (w, h, f) = (8.5, 11.0, 2.8);
    let x0 = c.x - w / 2.0;
    let y0 = c.y - h / 2.0;
    let pts = vec![
        egui::pos2(x0, y0),
        egui::pos2(x0 + w - f, y0),
        egui::pos2(x0 + w, y0 + f),
        egui::pos2(x0 + w, y0 + h),
        egui::pos2(x0, y0 + h),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
}

/// Vault switcher card: accent avatar tile with the folder initial,
/// bold vault name and muted full path.
fn vault_card(ui: &mut egui::Ui, pal: &theme::Palette, name: &str, path: &str) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), Sense::hover());
    ui.painter().rect_filled(rect, 6.0, pal.faint_fill);
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 10.0, rect.min.y + 5.0),
        egui::pos2(rect.max.x - 8.0, rect.max.y - 5.0),
    );
    ui.allocate_new_ui(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(
                Label::new(
                    RichText::new(name)
                        .size(13.0)
                        .font(FontId::new(13.0, theme::family_bold()))
                        .color(pal.text),
                )
                .truncate(),
            );
            ui.add(Label::new(RichText::new(path).size(10.5).color(pal.weak)).truncate());
        });
    });
    resp.on_hover_text(path.to_string())
}

fn tree_empty(ui: &mut egui::Ui, pal: &theme::Palette, msg: &str) {
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("\u{25cb}").size(22.0).color(pal.weak));
        ui.add_space(4.0);
        ui.label(RichText::new(msg).size(12.5).color(pal.weak));
    });
}

fn folder_row(
    ui: &mut egui::Ui,
    pal: &theme::Palette,
    depth: usize,
    expanded: bool,
    name: &str,
    tip: &str,
) -> egui::Response {
    let (rect, resp) = tree_row_frame(ui, pal, TREE_ROW_H, false, false);
    paint_guides(ui, pal, rect, depth, TREE_STEP);
    let cy = rect.center().y;
    let lx = level_x(rect, depth, TREE_STEP);
    paint_caret_tri(ui, egui::pos2(lx, cy), expanded, pal.weak);
    paint_folder_icon(
        ui,
        egui::pos2(lx + 16.0, cy),
        if expanded { pal.text } else { pal.weak },
    );
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(lx + 28.0, rect.min.y),
        egui::pos2(rect.max.x - 6.0, rect.max.y),
    );
    ui.allocate_new_ui(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                Label::new(
                    RichText::new(name)
                        .size(13.0)
                        .font(FontId::new(13.0, theme::family_bold()))
                        .color(pal.text),
                )
                .truncate(),
            );
        });
    });
    resp.on_hover_text(tip)
}

/// Returns true when the row was clicked.
fn file_row(
    ui: &mut egui::Ui,
    pal: &theme::Palette,
    depth: usize,
    selected: bool,
    dirty: bool,
    name: &str,
    tip: &str,
) -> bool {
    let (rect, resp) = tree_row_frame(ui, pal, TREE_ROW_H, selected, false);
    paint_guides(ui, pal, rect, depth, TREE_STEP);
    let cy = rect.center().y;
    let lx = level_x(rect, depth, TREE_STEP);
    paint_doc_icon(
        ui,
        egui::pos2(lx + 16.0, cy),
        if selected { pal.accent } else { pal.weak },
    );
    if dirty {
        ui.painter()
            .circle_filled(egui::pos2(rect.max.x - 12.0, cy), 3.0, pal.accent);
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(lx + 28.0, rect.min.y),
        egui::pos2(rect.max.x - if dirty { 22.0 } else { 6.0 }, rect.max.y),
    );
    ui.allocate_new_ui(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(Label::new(RichText::new(name).size(13.0).color(pal.text)).truncate());
        });
    });
    resp.on_hover_text(tip).clicked()
}

fn dir_counts(entries: &[Entry]) -> (usize, usize) {
    let mut dirs = 0;
    let mut files = 0;
    for e in entries {
        if e.is_dir {
            dirs += 1;
        } else {
            files += 1;
        }
    }
    (dirs, files)
}

/// Recursive markdown collection for the files search box (depth + total capped).
fn collect_md(root: &Path, out: &mut Vec<PathBuf>, limit: usize, depth: usize) {
    if out.len() >= limit || depth > 5 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        let p = ent.path();
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(p);
        } else if crate::fs_tree::is_markdown(&p) {
            if out.len() < limit {
                out.push(p);
            }
        }
    }
    dirs.sort();
    for d in dirs {
        if out.len() >= limit {
            break;
        }
        collect_md(&d, out, limit, depth + 1);
    }
}

fn editor_pane(
    ui: &mut egui::Ui,
    doc: &mut Document,
    jump: &mut Option<usize>,
    pitch: &mut f32,
    scroll_override: Option<f32>,
) -> ViewScroll {
    // NOTE: vertical() is load-bearing. Inside ScrollArea::both the horizontal
    // axis is unbounded, and multiline TextEdit (clip_text:false) sizes itself
    // to galley.max(wrap_width) — i.e. ~infinitely wide — sliding UNDER the
    // side panels. With vertical-only scroll the width stays viewport-bound.
    // Hide the scrollbar TRACK on this inner edge: with the global frozen
    // 10px floating width the full-height rail reads as a thick dark bar
    // against the outline panel. Thumb stays (position + drag); geometry,
    // hit-testing and cursors are untouched.
    ui.style_mut().spacing.scroll.dormant_background_opacity = 0.0;
    ui.style_mut().spacing.scroll.active_background_opacity = 0.0;
    ui.style_mut().spacing.scroll.interact_background_opacity = 0.0;
    let mut area = ScrollArea::vertical()
        .id_salt("editor-scroll")
        .auto_shrink(false)
        .drag_to_scroll(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
    if let Some(y) = scroll_override {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show(ui, |ui| {
            let margin: f32 = 8.0;
            ui.add_space(2.0);
            let prev_pitch = *pitch;
            let mut buf = std::mem::take(&mut doc.text);
            let out = TextEdit::multiline(&mut buf)
                .id_salt("main-editor")
                .font(TextStyle::Monospace)
                .desired_width(ui.available_width() - margin * 2.0)
                .frame(false)
                .hint_text("Start writing Markdown\u{2026}")
                .lock_focus(true)
                .show(ui);
            let changed = out.response.changed();
            doc.text = buf;
            if changed {
                doc.on_text_changed();
            }
            let lines = doc.text.lines().count().max(1);
            let h = out.response.rect.height() - margin * 2.0;
            if h > 4.0 {
                let p = (h / lines as f32).max(1.0);
                *pitch = if prev_pitch > 1.0 { prev_pitch * 0.6 + p * 0.4 } else { p };
            }
            if let Some(line) = jump.take() {
                let y = line as f32 * (*pitch).max(1.0);
                let top = out.response.rect.min + egui::vec2(margin, margin);
                let marker = egui::Rect::from_min_size(
                    top + egui::vec2(0.0, (y - 40.0).max(0.0)),
                    egui::vec2(1.0, 40.0),
                );
                let inner = ui.allocate_new_ui(UiBuilder::new().max_rect(marker), |ui| {
                    ui.allocate_exact_size(egui::Vec2::ZERO, Sense::hover())
                });
                inner.inner.1.scroll_to_me(Some(Align::Center));
            }
            // Trailing canvas: lets the last line scroll up from the viewport
            // bottom (Obsidian-style scroll-past-end). View-only padding,
            // never written to the file.
            let pad = (ui.clip_rect().height() - 40.0).max(120.0);
            ui.add_space(pad);
        });
    ViewScroll {
        offset: out.state.offset.y,
        content: out.content_size.y,
        viewport: out.inner_rect.height(),
    }
}

fn preview_pane(
    ui: &mut egui::Ui,
    doc: &mut Document,
    pv: &mut preview::Preview,
    dark: bool,
    scroll_override: Option<f32>,
) -> ViewScroll {
    // Same trackless treatment as the editor pane: hide the full-height
    // rail on the outline boundary, keep the position thumb + dragging.
    ui.style_mut().spacing.scroll.dormant_background_opacity = 0.0;
    ui.style_mut().spacing.scroll.active_background_opacity = 0.0;
    ui.style_mut().spacing.scroll.interact_background_opacity = 0.0;
    let mut area = ScrollArea::vertical()
        .id_salt("preview-scroll")
        .auto_shrink(false)
        .drag_to_scroll(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
    if let Some(y) = scroll_override {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show(ui, |ui| {
            let base = doc.path.as_ref().and_then(|p| parent_of(p));
            let version = doc.version;
            preview::show(ui, pv, &doc.text, base.as_deref(), dark, version);
            // Same trailing canvas as the editor: view-only, never saved.
            let pad = (ui.clip_rect().height() - 40.0).max(120.0);
            ui.add_space(pad);
        });
    ViewScroll {
        offset: out.state.offset.y,
        content: out.content_size.y,
        viewport: out.inner_rect.height(),
    }
}

/// Diagnostic harness (kept as a regression test): sweep a virtual pointer
/// horizontally across the outline splitter and record which cursor icon
/// egui reports. Replicates the real layout 1:1 (frames, margins, spacing,
/// rows with tooltips + hand cursor, editor with always-visible scrollbars).
#[cfg(test)]
mod splitter_cursor_probe {
    use eframe::egui::{
        self, Align, CentralPanel, CursorIcon, Event, Frame, Layout, Margin, Pos2, RawInput,
        Rect, RichText, ScrollArea, Sense, TextEdit, UiBuilder,
    };
    use super::{OUTLINE_ROW_H, OUTLINE_STEP, paint_guides, tree_row_frame};

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1536.0, 937.0))
    }

    fn probe_row(ui: &mut egui::Ui, h: f32, _tip: String) {
        // Mirrors tree_row_frame: hover highlight only, plain arrow cursor.
        let _ = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    }

    fn build_ui(ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .frame(Frame::default().inner_margin(Margin::symmetric(10, 5)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                ui.add_sized([ui.available_width(), 26.0], egui::Label::new("toolbar"));
            });
        egui::SidePanel::left("files-panel")
            .resizable(true)
            .default_width(259.0)
            .width_range(170.0..=420.0)
            .show_separator_line(false)
            .frame(Frame::default().inner_margin(Margin::symmetric(8, 6)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for i in 0..30 {
                    probe_row(ui, 26.0, format!("C:\\notes\\file{i:02}.md"));
                }
            });
        let mut edge = 1141.0;
        egui::SidePanel::right("outline-panel")
            .resizable(false)
            .exact_width(395.0)
            .show_separator_line(false)
            .frame(Frame::default().inner_margin(Margin::symmetric(8, 6)))
            .show(ctx, |ui| {
                edge = ui.max_rect().min.x - 8.0;
                // Explicit handle mirror: drag-only strip just inside the edge.
                let htop = ui.max_rect().min.y;
                let hfull = ui.available_height();
                let hr = egui::Rect::from_min_max(
                    egui::pos2(edge + 2.0, htop),
                    egui::pos2(edge + 14.0, htop + hfull),
                );
                ui.allocate_new_ui(UiBuilder::new().max_rect(hr), |ui| {
                    let (_, resp) = ui.allocate_exact_size(hr.size(), Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    }
                });
                ui.spacing_mut().item_spacing.y = 1.0;
                for i in 0..40 {
                    probe_row(ui, 24.0, format!("Line {} \u{2014} click to jump", i * 7 + 1));
                }
            });
        // Central panel LAST: side panels must reserve space first,
        // otherwise it spans full width underneath them.
        CentralPanel::default()
            .frame(Frame::default())
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .id_salt("editor-scroll")
                    .auto_shrink(false)
                    .drag_to_scroll(false)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        let mut text = "lorem ipsum dolor sit amet consectetur\n".repeat(300);
                        let _ = TextEdit::multiline(&mut text)
                            .id_salt("main-editor")
                            .desired_width(ui.available_width() - 16.0)
                            .frame(false)
                            .show(ui);
                        ui.add_space(800.0);
                    });
            });
    }

    fn run_frame(ctx: &egui::Context, x: f32, y: f32, t: f64) -> CursorIcon {
        let mut raw = RawInput::default();
        raw.screen_rect = Some(screen());
        raw.time = Some(t);
        raw.events.push(Event::PointerMoved(Pos2::new(x, y)));
        let out = ctx.run(raw, build_ui);
        out.platform_output.cursor_icon
    }

    fn sweep(ctx: &egui::Context, y: f32, t: &mut f64, x0: i32, x1: i32) -> Vec<(i32, CursorIcon)> {
        let mut map = Vec::new();
        let mut x = x0;
        while x <= x1 {
            let mut cur = CursorIcon::Default;
            for _ in 0..5 {
                *t += 1.0 / 60.0;
                cur = run_frame(ctx, x as f32, y, *t);
            }
            map.push((x, cur));
            x += 2;
        }
        map
    }

    /// Diagnostic: does exact_width constrain the panel?
    #[test]
    fn splitter_exact_width_debug() {
        let ctx = egui::Context::default();
        crate::theme::init_fonts(&ctx);
        crate::theme::apply(&ctx);
        let mut raw = RawInput::default();
        raw.screen_rect = Some(screen());
        raw.time = Some(1.0);
        raw.events.push(Event::PointerMoved(Pos2::new(10.0, 10.0)));
        let _ = ctx.run(raw, |ctx| {
            egui::SidePanel::right("outline-panel")
                .resizable(false)
                .exact_width(344.0)
                .show_separator_line(false)
                .frame(Frame::default().inner_margin(Margin::symmetric(8, 6)))
                .show(ctx, |ui| {
                    println!("outline content max_rect={:?}", ui.max_rect());
                    println!("outline avail w={:.0}", ui.available_width());
                });
        });
    }

    /// Diagnostic: tessellate one frame and histogram dark vertices by x.
    /// Finds any tall dark painted column (mystery thick line audit).
    #[test]
    fn paint_audit_dark_columns() {
        use std::collections::BTreeMap;
        let ctx = egui::Context::default();
        crate::theme::init_fonts(&ctx);
        crate::theme::apply(&ctx);
        ctx.set_theme(egui::ThemePreference::Light);
        let pal = crate::theme::palette(false);
        let mut raw = RawInput::default();
        raw.screen_rect = Some(screen());
        raw.time = Some(1.0);
        raw.events.push(Event::PointerMoved(Pos2::new(100.0, 500.0)));
        let mut edge = 0.0f32;
        let out = ctx.run(raw, |ctx| {
            egui::TopBottomPanel::top("toolbar")
                .frame(Frame::default().inner_margin(Margin::symmetric(10, 5)))
                .show(ctx, |ui| {
                    ui.add_sized([ui.available_width(), 26.0], egui::Label::new("toolbar"));
                });
            egui::SidePanel::right("outline-panel")
                .resizable(false)
                .exact_width(344.0)
                .show_separator_line(false)
                .frame(
                    Frame::default()
                        .fill(pal.panel)
                        .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                        .inner_margin(Margin::symmetric(8, 6)),
                )
                .show(ctx, |ui| {
                    edge = ui.max_rect().min.x - 8.0;
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut String::from("Search outline...")),
                    );
                    for i in 0..40 {
                        probe_row(ui, 24.0, format!("heading {i} with a fairly long title here"));
                    }
                    // App handle mirror (invisible drag strip).
                    let htop = ui.max_rect().min.y;
                    let hfull = ui.available_height();
                    let hr = egui::Rect::from_min_max(
                        egui::pos2(edge + 2.0, htop),
                        egui::pos2(edge + 14.0, htop + hfull),
                    );
                    ui.allocate_new_ui(UiBuilder::new().max_rect(hr), |ui| {
                        let _ = ui.allocate_exact_size(hr.size(), Sense::drag());
                    });
                });
            CentralPanel::default()
                .frame(Frame::default().fill(pal.bg))
                .show(ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("preview-scroll")
                        .auto_shrink(false)
                        .drag_to_scroll(false)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .show(ui, |ui| {
                            for i in 0..60 {
                                ui.add(egui::Label::new(format!(
                                    "Paragraph {i} with enough wrapping text to fill lines."
                                )));
                            }
                            ui.add_space(800.0);
                        });
                });
            // Grip dots mirror.
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("splitter-grips"),
            ));
            for dy in [-7.0, 0.0, 7.0] {
                p.circle_filled(egui::pos2(edge, 500.0 + dy), 1.6, pal.accent);
            }
        });
        println!("edge={edge:.0}");
        let prims = ctx.tessellate(out.shapes, 1.0);
        let mut cols: BTreeMap<i32, (usize, u32, u32, u32)> = BTreeMap::new();
        for prim in &prims {
            let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            for v in &mesh.vertices {
                let c = v.color;
                if c.r() < 90 && c.g() < 90 && c.b() < 90 && c.a() > 100 {
                    let e = cols.entry(v.pos.x as i32).or_insert((0, 0, 0, 0));
                    e.0 += 1;
                    e.1 += c.r() as u32;
                    e.2 += c.g() as u32;
                    e.3 += c.b() as u32;
                }
            }
        }
        for (x, (n, r, g, b)) in &cols {
            if *n > 120 {
                println!("x={x} n={n} avg=({},{},{})", r / *n as u32, g / *n as u32, b / *n as u32);
            }
        }
    }

    /// Diagnostic TEMPORARY: render the user's real doc through the real
    /// preview + real outline rows, tessellate, hunt tall dark columns.
    /// DELETE after diagnosis (absolute path, not portable).
    #[test]
    fn paint_audit_real_doc() {
        use std::collections::BTreeMap;
        let ctx = egui::Context::default();
        crate::theme::init_fonts(&ctx);
        crate::theme::apply(&ctx);
        ctx.set_theme(egui::ThemePreference::Light);
        let pal = crate::theme::palette(false);
        let text = std::fs::read_to_string(
            "C:\\Users\\okky\\ZTextproject\\level 1\\roblox-gamejam-readiness.md",
        )
        .expect("user doc must exist");
        let headings = crate::markdown::outline(&text);
        println!("headings={} text_bytes={}", headings.len(), text.len());
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(1920.0, 1050.0),
        ));
        raw.time = Some(1.0);
        raw.events.push(Event::PointerMoved(Pos2::new(100.0, 500.0)));
        let mut pv = crate::preview::Preview::new();
        let out = ctx.run(raw, |ctx| {
            egui::TopBottomPanel::top("toolbar")
                .frame(Frame::default().inner_margin(Margin::symmetric(10, 5)))
                .show(ctx, |ui| {
                    ui.add_sized([ui.available_width(), 26.0], egui::Label::new("toolbar"));
                });
            egui::SidePanel::right("outline-panel")
                .resizable(false)
                .exact_width(420.0)
                .show_separator_line(false)
                .frame(
                    Frame::default()
                        .fill(pal.panel)
                        .stroke(egui::Stroke::new(1.0_f32, pal.stroke))
                        .inner_margin(Margin::symmetric(8, 6)),
                )
                .show(ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("outline-scroll")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            for h in headings.iter() {
                                let size = match h.level {
                                    1 => 13.5,
                                    2 => 13.0,
                                    _ => 12.0,
                                };
                                let (rect, _) =
                                    tree_row_frame(ui, &pal, OUTLINE_ROW_H, false, true);
                                paint_guides(ui, &pal, rect, h.level as usize - 1, OUTLINE_STEP);
                                let tr = egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x + 20.0, rect.min.y),
                                    egui::pos2(rect.max.x - 4.0, rect.max.y),
                                );
                                ui.allocate_new_ui(UiBuilder::new().max_rect(tr), |ui| {
                                    ui.with_layout(
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(h.title.clone()).size(size),
                                                )
                                                .truncate(),
                                            );
                                        },
                                    );
                                });
                            }
                        });
                });
            CentralPanel::default()
                .frame(Frame::default().fill(pal.bg))
                .show(ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("preview-scroll")
                        .auto_shrink(false)
                        .drag_to_scroll(false)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .show(ui, |ui| {
                            crate::preview::show(ui, &mut pv, &text, None, false, 1);
                        });
                });
        });
        let prims = ctx.tessellate(out.shapes, 1.0);
        let mut cols: BTreeMap<i32, (usize, f32, f32)> = BTreeMap::new();
        for prim in &prims {
            let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            for v in &mesh.vertices {
                let c = v.color;
                if c.r() < 90 && c.g() < 90 && c.b() < 90 && c.a() > 100 {
                    let e = cols.entry(v.pos.x as i32).or_insert((0, 1e9, -1e9));
                    e.0 += 1;
                    e.1 = e.1.min(v.pos.y);
                    e.2 = e.2.max(v.pos.y);
                }
            }
        }
        for (x, (n, y0, y1)) in &cols {
            // Bar-like: tall span but suspiciously few vertices (a painted
            // rect is just 2 triangles); glyph columns have high counts.
            if *y1 - *y0 > 400.0 {
                println!("x={x} n={n} y=[{y0:.0},{y1:.0}]");
            }
        }
    }

    /// The splitter must be the ONLY cursor-changing spot near a panel edge:
    /// content cursor, then one ResizeHorizontal band, then plain arrow.
    #[test]
    fn splitters_are_single_bands() {
        let ctx = egui::Context::default();
        crate::theme::init_fonts(&ctx);
        crate::theme::apply(&ctx);
        let mut t = 0.0;
        for _ in 0..10 {
            t += 1.0 / 60.0;
            run_frame(&ctx, 700.0, 500.0, t);
        }
        // Outline edge (~x1141): editor text, calm scrollbar strip, one
        // explicit handle right of the edge, plain rows. No second spot.
        for (x, c) in sweep(&ctx, 500.0, &mut t, 1050, 1250) {
            if (1060..=1105).contains(&x) {
                assert_eq!(c, CursorIcon::Text, "editor should keep text cursor at x={x}");
            } else if (1125..=1138).contains(&x) {
                assert_eq!(c, CursorIcon::Default, "no hotspot left of handle at x={x}");
            } else if (1142..=1158).contains(&x) {
                assert_eq!(c, CursorIcon::ResizeHorizontal, "single handle at x={x}");
            } else if (1165..=1240).contains(&x) {
                assert_eq!(c, CursorIcon::Default, "rows keep arrow cursor at x={x}");
            }
        }
        // Files edge (~x259): plain rows, resize band, editor text.
        for (x, c) in sweep(&ctx, 500.0, &mut t, 200, 320) {
            if (205..=240).contains(&x) {
                assert_eq!(c, CursorIcon::Default, "rows keep arrow cursor at x={x}");
            } else if (252..=256).contains(&x) {
                assert_eq!(c, CursorIcon::ResizeHorizontal, "splitter band at x={x}");
            } else if (290..=315).contains(&x) {
                assert_eq!(c, CursorIcon::Text, "editor should keep text cursor at x={x}");
            }
        }
    }
}
