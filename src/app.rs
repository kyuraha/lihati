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

pub struct App {
    state: PersistState,
    doc: Document,
    root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    outline_collapsed: HashSet<usize>,
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
        let mut app = App {
            state,
            doc: Document::new(),
            root: None,
            expanded: HashSet::new(),
            outline_collapsed: HashSet::new(),
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
            ThemePref::System => ThemePref::Light,
            ThemePref::Light => ThemePref::Dark,
            ThemePref::Dark => ThemePref::System,
        };
        theme::set_pref(ctx, self.state.theme);
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        TopBottomPanel::top("toolbar")
            .frame(Frame::default().fill(pal.panel).inner_margin(Margin::symmetric(8, 4)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
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
                    ui.separator();

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if toggle_btn(ui, "Outline", self.state.show_outline, &pal).clicked() {
                            self.state.show_outline = !self.state.show_outline;
                        }
                        ui.separator();
                        let theme_label = match self.state.theme {
                            ThemePref::System => "Theme: System",
                            ThemePref::Light => "Theme: Light",
                            ThemePref::Dark => "Theme: Dark",
                        };
                        if small_btn(ui, theme_label).on_hover_text("Cycle theme").clicked() {
                            self.cycle_theme(ui.ctx());
                        }
                        if let Some(m) = segmented_view(ui, self.state.view, self.cur_dark, &pal) {
                            self.state.view = m;
                        }
                        let name = self.doc.file_name();
                        let dot = if self.doc.dirty { "\u{25cf} " } else { "" };
                        ui.add_sized(
                            [ui.available_width().min(380.0), 18.0],
                            Label::new(
                                RichText::new(format!("{}{}", dot, name))
                                    .size(13.0)
                                    .color(if self.doc.dirty { pal.warn } else { pal.weak }),
                            )
                            .truncate(),
                        );
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
            .default_width(220.0)
            .width_range(150.0..=400.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("FILES").size(11.0).color(pal.weak));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(Button::new(RichText::new("\u{00ab}").size(12.0)))
                            .on_hover_text("Hide files panel")
                            .clicked()
                        {
                            self.state.show_dir = false;
                        }
                        if ui
                            .add(Button::new(RichText::new("\u{2026}").size(12.0)))
                            .on_hover_text("Choose folder\u{2026}")
                            .clicked()
                        {
                            self.choose_root();
                        }
                    });
                });
                ui.separator();

                match self.root.clone() {
                    Some(root) if root.is_dir() => {
                        ScrollArea::vertical()
                            .id_salt("files-scroll")
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                self.draw_dir_entries(ui, &root, 0);
                            });
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
                let mut toggle = false;
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 11.0 + 2.0);
                    if draw_caret(ui, expanded).clicked() {
                        toggle = true;
                    }
                    let name_resp =
                        ui.selectable_label(false, RichText::new(e.name.clone()).size(13.0));
                    if name_resp.clicked() {
                        toggle = true;
                    }
                });
                if toggle {
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
                let path = e.path.clone();
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 11.0 + 16.0);
                    let mut label = RichText::new(e.name.clone()).size(13.0);
                    if selected {
                        label = label.color(theme::palette(ui.visuals().dark_mode).accent);
                    }
                    if ui.selectable_label(selected, label).clicked() {
                        self.open_path(path);
                    }
                });
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

                match self.state.view {
                    ViewMode::Source => {
                        let Self { doc, jump, editor_pitch, .. } = self;
                        editor_pane(ui, doc, jump, editor_pitch);
                    }
                    ViewMode::Split => {
                        ui.columns(2, |cols| {
                            {
                                let Self { doc, jump, editor_pitch, .. } = self;
                                editor_pane(&mut cols[0], doc, jump, editor_pitch);
                            }
                            {
                                let Self { doc, preview, cur_dark, .. } = self;
                                preview_pane(&mut cols[1], doc, preview, *cur_dark);
                            }
                        });
                    }
                    ViewMode::Preview => {
                        let Self { doc, preview, cur_dark, .. } = self;
                        preview_pane(ui, doc, preview, *cur_dark);
                    }
                }
            });
    }

    fn draw_outline_panel(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        egui::SidePanel::right("outline-panel")
            .resizable(true)
            .default_width(230.0)
            .width_range(160.0..=420.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("OUTLINE").size(11.0).color(pal.weak));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(Button::new(RichText::new("\u{00bb}").size(12.0)))
                            .on_hover_text("Hide outline")
                            .clicked()
                        {
                            self.state.show_outline = false;
                        }
                    });
                });
                ui.add_space(2.0);
                ui.separator();

                let headings = self.headings.clone();
                ScrollArea::vertical()
                    .id_salt("outline-scroll")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if headings.is_empty() {
                            ui.add_space(16.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("No headings yet").weak().size(12.5));
                            });
                            return;
                        }

                        let mut open_ancestors: Vec<(u8, usize)> = Vec::new();
                        for (idx, h) in headings.iter().enumerate() {
                            while let Some(&(lvl, _)) = open_ancestors.last() {
                                if lvl >= h.level {
                                    open_ancestors.pop();
                                } else {
                                    break;
                                }
                            }
                            let visible = open_ancestors
                                .iter()
                                .all(|(_, line)| !self.outline_collapsed.contains(line));
                            let has_children = headings
                                .get(idx + 1)
                                .map(|n| n.level > h.level)
                                .unwrap_or(false);

                            if visible {
                                let indent = ((h.level as i32 - 1).max(0)) as f32 * 12.0 + 4.0;
                                let size = match h.level {
                                    1 => 13.2,
                                    2 => 12.8,
                                    3 => 12.4,
                                    _ => 12.0,
                                };
                                let text = if h.title.trim().is_empty() {
                                    "(untitled)".to_string()
                                } else {
                                    h.title.clone()
                                };
                                let mut rt = RichText::new(text).size(size);
                                rt = if h.level <= 2 {
                                    rt.color(pal.text)
                                } else {
                                    rt.color(pal.weak)
                                };
                                if h.level == 1 {
                                    rt = rt.font(eframe::egui::FontId::new(
                                        size,
                                        theme::family_bold(),
                                    ));
                                }
                                let line = h.line;
                                ui.horizontal(|ui| {
                                    ui.add_space(indent);
                                    if has_children {
                                        let is_open =
                                            !self.outline_collapsed.contains(&line);
                                        if draw_caret_small(ui, is_open).clicked() {
                                            if is_open {
                                                self.outline_collapsed.insert(line);
                                            } else {
                                                self.outline_collapsed.remove(&line);
                                            }
                                        }
                                    } else {
                                        ui.add_space(14.0);
                                    }
                                    if ui
                                        .add(Button::new(rt).truncate())
                                        .on_hover_text(format!("Go to line {}", line + 1))
                                        .clicked()
                                    {
                                        self.jump = Some(line);
                                        if self.state.view == ViewMode::Preview {
                                            self.state.view = ViewMode::Source;
                                        }
                                    }
                                });
                            }

                            if has_children && !self.outline_collapsed.contains(&h.line) {
                                open_ancestors.push((h.level, h.line));
                            }
                        }
                    });
            });
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
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for m in [ViewMode::Source, ViewMode::Split, ViewMode::Preview] {
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

fn draw_caret(ui: &mut egui::Ui, expanded: bool) -> egui::Response {
    let pal = theme::palette(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(16.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.expand(2.0), 3, pal.hover);
    }
    let c = rect.center();
    let s = 3.4f32;
    let pts: Vec<egui::Pos2> = if expanded {
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
    ui.painter().add(egui::Shape::convex_polygon(pts, pal.weak, egui::Stroke::NONE));
    resp
}

fn draw_caret_small(ui: &mut egui::Ui, expanded: bool) -> egui::Response {
    let pal = theme::palette(ui.visuals().dark_mode);
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(13.0, 16.0), Sense::click());
    let color = if resp.hovered() { pal.text } else { pal.weak };
    let c = rect.center();
    let s = 2.8f32;
    let pts: Vec<egui::Pos2> = if expanded {
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
    ui.painter().add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
    resp
}

fn editor_pane(
    ui: &mut egui::Ui,
    doc: &mut Document,
    jump: &mut Option<usize>,
    pitch: &mut f32,
) {
    ScrollArea::both()
        .id_salt("editor-scroll")
        .auto_shrink(false)
        .drag_to_scroll(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
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
        });
}

fn preview_pane(ui: &mut egui::Ui, doc: &mut Document, pv: &mut preview::Preview, dark: bool) {
    ScrollArea::vertical()
        .id_salt("preview-scroll")
        .auto_shrink(false)
        .drag_to_scroll(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            let base = doc.path.as_ref().and_then(|p| parent_of(p));
            let version = doc.version;
            preview::show(ui, pv, &doc.text, base.as_deref(), dark, version);
        });
}
