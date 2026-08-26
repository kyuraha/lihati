use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Id, Label, Margin, Sense, Stroke, TextFormat,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use eframe::egui::text::LayoutJob;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Theme};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::{self};

type Spans = Vec<(Color32, String)>;

static SYNTAX: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();

pub struct Preview {
    parsed: Option<(u64, Arc<Vec<Event<'static>>>)>,
    images: HashMap<PathBuf, ((SystemTime, u64), TextureHandle)>,
    code: HashMap<(u64, bool), Arc<Vec<Spans>>>,
    pub content_height: f32,
}

impl Preview {
    pub fn new() -> Self {
        Preview {
            parsed: None,
            images: HashMap::new(),
            code: HashMap::new(),
            content_height: 0.0,
        }
    }

    fn events_for(&mut self, version: u64, text: &str) -> Arc<Vec<Event<'static>>> {
        if self.parsed.as_ref().is_none_or(|(v, _)| *v != version) {
            let mut opts = Options::empty();
            opts.insert(Options::ENABLE_TABLES);
            opts.insert(Options::ENABLE_TASKLISTS);
            opts.insert(Options::ENABLE_STRIKETHROUGH);
            let events: Vec<Event<'static>> =
                Parser::new_ext(text, opts).map(|e| own_event(e)).collect();
            self.parsed = Some((version, Arc::new(events)));
        }
        self.parsed.as_ref().map(|(_, e)| e.clone()).unwrap()
    }
}

pub fn warmup() {
    std::thread::spawn(|| {
        let _ = SYNTAX.get_or_init(SyntaxSet::load_defaults_newlines);
        let _ = THEMES.get_or_init(ThemeSet::load_defaults);
    });
}

fn own_cow(c: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'static> {
    match c {
        pulldown_cmark::CowStr::Boxed(s) => pulldown_cmark::CowStr::Boxed(s),
        other => pulldown_cmark::CowStr::from(other.into_string()),
    }
}

fn own_tag(t: Tag<'_>) -> Tag<'static> {
    match t {
        Tag::Paragraph => Tag::Paragraph,
        Tag::Heading { level, id, classes, attrs } => Tag::Heading {
            level,
            id: id.map(own_cow),
            classes: classes.into_iter().map(own_cow).collect(),
            attrs: attrs
                .into_iter()
                .map(|(k, v)| (own_cow(k), v.map(own_cow)))
                .collect(),
        },
        Tag::BlockQuote(kind) => Tag::BlockQuote(kind),
        Tag::CodeBlock(kind) => Tag::CodeBlock(match kind {
            CodeBlockKind::Indented => CodeBlockKind::Indented,
            CodeBlockKind::Fenced(info) => CodeBlockKind::Fenced(own_cow(info)),
        }),
        Tag::HtmlBlock => Tag::HtmlBlock,
        Tag::MetadataBlock(kind) => Tag::MetadataBlock(kind),
        Tag::List(start) => Tag::List(start),
        Tag::Item => Tag::Item,
        Tag::FootnoteDefinition(name) => Tag::FootnoteDefinition(own_cow(name)),
        Tag::DefinitionList => Tag::DefinitionList,
        Tag::DefinitionListTitle => Tag::DefinitionListTitle,
        Tag::DefinitionListDefinition => Tag::DefinitionListDefinition,
        Tag::Table(aligns) => Tag::Table(aligns),
        Tag::TableHead => Tag::TableHead,
        Tag::TableRow => Tag::TableRow,
        Tag::TableCell => Tag::TableCell,
        Tag::Emphasis => Tag::Emphasis,
        Tag::Strong => Tag::Strong,
        Tag::Strikethrough => Tag::Strikethrough,
        Tag::Link { link_type, dest_url, title, id } => Tag::Link {
            link_type,
            dest_url: own_cow(dest_url),
            title: own_cow(title),
            id: own_cow(id),
        },
        Tag::Image { link_type, dest_url, title, id } => Tag::Image {
            link_type,
            dest_url: own_cow(dest_url),
            title: own_cow(title),
            id: own_cow(id),
        },
        Tag::Superscript => Tag::Superscript,
        Tag::Subscript => Tag::Subscript,
    }
}

fn own_event(e: Event<'_>) -> Event<'static> {
    match e {
        Event::Start(t) => Event::Start(own_tag(t)),
        Event::End(t) => Event::End(t),
        Event::Text(c) => Event::Text(own_cow(c)),
        Event::Code(c) => Event::Code(own_cow(c)),
        Event::Html(c) => Event::Html(own_cow(c)),
        Event::InlineHtml(c) => Event::InlineHtml(own_cow(c)),
        Event::FootnoteReference(n) => Event::FootnoteReference(own_cow(n)),
        Event::SoftBreak => Event::SoftBreak,
        Event::HardBreak => Event::HardBreak,
        Event::Rule => Event::Rule,
        Event::InlineMath(c) => Event::InlineMath(own_cow(c)),
        Event::DisplayMath(c) => Event::DisplayMath(own_cow(c)),
        Event::TaskListMarker(b) => Event::TaskListMarker(b),
    }
}

struct Cfg<'a> {
    base: Option<&'a Path>,
    dark: bool,
    pv: &'a mut Preview,
    depth: usize,
    block_no: usize,
}

const MAX_NESTING: usize = 48;

const BODY: f32 = 15.5;

fn closes(start: &Tag, end: &pulldown_cmark::TagEnd) -> bool {
    use pulldown_cmark::TagEnd as E;
    matches!(
        (start, end),
        (Tag::Paragraph, E::Paragraph)
            | (Tag::Heading { .. }, E::Heading(_))
            | (Tag::BlockQuote(_), E::BlockQuote(_))
            | (Tag::CodeBlock(_), E::CodeBlock)
            | (Tag::HtmlBlock, E::HtmlBlock)
            | (Tag::List(_), E::List(_))
            | (Tag::FootnoteDefinition(_), E::FootnoteDefinition)
            | (Tag::Table(_), E::Table)
            | (Tag::TableHead, E::TableHead)
            | (Tag::TableRow, E::TableRow)
            | (Tag::TableCell, E::TableCell)
            | (Tag::Item, E::Item)
            | (Tag::Emphasis, E::Emphasis)
            | (Tag::Strong, E::Strong)
            | (Tag::Strikethrough, E::Strikethrough)
            | (Tag::Link { .. }, E::Link)
            | (Tag::Image { .. }, E::Image)
            | (Tag::Superscript, E::Superscript)
            | (Tag::Subscript, E::Subscript)
    )
}

fn heading_num(level: &HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn show(ui: &mut Ui, pv: &mut Preview, text: &str, base: Option<&Path>, dark: bool, version: u64) {
    let evs = pv.events_for(version, text);
    let mut cfg = Cfg { base, dark, pv, depth: 0, block_no: 0 };
    let top = ui.cursor().top();

    ui.add_space(12.0);
    let outer_avail = ui.available_width();
    const GUTTER: f32 = 28.0;
    const MAX_CONTENT_W: f32 = 760.0;
    let content_w = (outer_avail - GUTTER * 2.0).clamp(140.0, MAX_CONTENT_W);
    let side = ((outer_avail - content_w) * 0.5).max(GUTTER);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_max_width(content_w - ui.style().spacing.item_spacing.x);
                let mut i = 0usize;
                let mut first = true;
                while i < evs.len() {
                    blocks_until(ui, &mut cfg, &evs, &mut i, None, &mut first);
                }
            });
        });
    });

    cfg.pv.content_height = ui.cursor().bottom() - top + 40.0;
}

fn gap(ui: &mut Ui, first: bool, amount: f32) {
    if !first {
        ui.add_space(amount);
    }
}

fn blocks_until(
    ui: &mut Ui,
    cfg: &mut Cfg,
    evs: &[Event],
    i: &mut usize,
    stop: Option<&Tag>,
    first: &mut bool,
) {
    while *i < evs.len() {
        if let Event::End(e) = &evs[*i] {
            if let Some(s) = stop {
                if closes(s, e) {
                    *i += 1;
                    return;
                }
            }
        }
        render_block(ui, cfg, evs, i, first);
    }
}

fn render_block(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize, first: &mut bool) {
    match &evs[*i] {
        Event::Start(Tag::Paragraph) => {
            *i += 1;
            gap(ui, *first, 9.0);
            paragraph(ui, cfg, evs, i);
            *first = false;
        }
        Event::Start(Tag::Heading { level, .. }) => {
            let lvl = heading_num(level);
            *i += 1;
            gap(ui, *first, 16.0);
            heading(ui, cfg, evs, i, lvl);
            *first = false;
        }
        Event::Start(Tag::BlockQuote(_)) => {
            *i += 1;
            gap(ui, *first, 10.0);
            quote(ui, cfg, evs, i);
            *first = false;
        }
        Event::Start(Tag::CodeBlock(kind)) => {
            let lang = match kind {
                CodeBlockKind::Fenced(info) => info
                    .split([' ', ','])
                    .next()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                _ => None,
            };
            *i += 1;
            gap(ui, *first, 10.0);
            code_block(ui, cfg, evs, i, lang.as_deref());
            *first = false;
        }
        Event::Start(Tag::List(start)) => {
            let ordered_start = *start;
            *i += 1;
            gap(ui, *first, 6.0);
            list(ui, cfg, evs, i, ordered_start, 0);
            *first = false;
        }
        Event::Start(Tag::Table(aligns)) => {
            let aligns = aligns.clone();
            *i += 1;
            gap(ui, *first, 10.0);
            table(ui, cfg, evs, i, &aligns);
            *first = false;
        }
        Event::Rule => {
            *i += 1;
            gap(ui, *first, 10.0);
            hr(ui);
            *first = false;
        }
        Event::Start(Tag::FootnoteDefinition(name)) => {
            *i += 1;
            gap(ui, *first, 6.0);
            footnote_def(ui, cfg, evs, i, name);
            *first = false;
        }
        Event::Start(Tag::HtmlBlock) => {
            *i += 1;
            let mut raw = String::new();
            loop {
                match evs.get(*i) {
                    Some(Event::Html(h)) => {
                        raw.push_str(h);
                        *i += 1;
                    }
                    Some(Event::End(e)) if closes(&Tag::HtmlBlock, e) => {
                        *i += 1;
                        break;
                    }
                    None => break,
                    _ => *i += 1,
                }
            }
            if !raw.trim().is_empty() {
                gap(ui, *first, 8.0);
                html_block(ui, &raw, theme::palette(cfg.dark));
                *first = false;
            }
        }
        Event::Text(t) => {
            let t = t.to_string();
            gap(ui, *first, 9.0);
            let mut st = InlineSt::new(ui.available_width(), theme::palette(cfg.dark));
            st.push(&t);
            add_inline(ui, st);
            *first = false;
            *i += 1;
        }
        Event::Code(c) => {
            let c = c.to_string();
            gap(ui, *first, 9.0);
            let mut st = InlineSt::new(ui.available_width(), theme::palette(cfg.dark));
            st.push_code(&c);
            add_inline(ui, st);
            *first = false;
            *i += 1;
        }
        _ => {
            *i += 1;
        }
    }
}

fn footnote_def(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize, name: &pulldown_cmark::CowStr) {
    let pal = theme::palette(cfg.dark);
    let mut body = String::new();
    while *i < evs.len() {
        match &evs[*i] {
            Event::Text(t) => body.push_str(t),
            Event::Code(c) => body.push_str(c),
            Event::SoftBreak => body.push(' '),
            Event::End(e) if closes(&Tag::FootnoteDefinition(name.clone()), e) => {
                *i += 1;
                break;
            }
            _ => {}
        }
        *i += 1;
    }
    ui.horizontal_top(|ui| {
        ui.label(
            eframe::egui::RichText::new(format!("^{name}"))
                .font(FontId::monospace(12.5))
                .color(pal.weak),
        );
        ui.vertical(|ui| {
            let mut st = InlineSt::new(ui.available_width(), pal);
            st.weak_text = true;
            st.head_size = Some(13.0);
            st.push(body.trim());
            add_inline(ui, st);
        });
    });
}

fn html_block(ui: &mut Ui, raw: &str, pal: theme::Palette) {
    egui::Frame::default()
        .fill(pal.faint_fill)
        .stroke(Stroke::new(1.0_f32, pal.stroke))
        .corner_radius(3)
        .inner_margin(Margin::same(9))
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.add(
                Label::new(
                    eframe::egui::RichText::new(raw.trim())
                        .font(FontId::monospace(12.5))
                        .color(pal.weak),
                )
                .wrap(),
            );
        });
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    Para,
    Head,
    Item,
}

fn inline_stop(stop: Stop, e: &pulldown_cmark::TagEnd) -> bool {
    use pulldown_cmark::TagEnd as E;
    match stop {
        Stop::Para => matches!(e, E::Paragraph),
        Stop::Head => matches!(e, E::Heading(_)),
        Stop::Item => matches!(e, E::Item),
    }
}

fn is_block_start(t: &Tag) -> bool {
    matches!(
        t,
        Tag::List(_)
            | Tag::Item
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::Table(_)
            | Tag::BlockQuote(_)
            | Tag::FootnoteDefinition(_)
            | Tag::Heading { .. }
    )
}

struct InlineSt<'a> {
    job: LayoutJob,
    italics: u32,
    strong: u32,
    strike: u32,
    link: Option<String>,
    links: Vec<(std::ops::Range<usize>, String)>,
    head_size: Option<f32>,
    weak_text: bool,
    pal: theme::Palette,
    _p: std::marker::PhantomData<&'a ()>,
}

impl<'a> InlineSt<'a> {
    fn new(width: f32, pal: theme::Palette) -> Self {
        let mut job = LayoutJob::default();
        job.wrap.max_width = if width.is_finite() && width > 40.0 { width } else { 480.0 };
        job.wrap.break_anywhere = true;
        InlineSt {
            job,
            italics: 0,
            strong: 0,
            strike: 0,
            link: None,
            links: Vec::new(),
            head_size: None,
            weak_text: false,
            pal,
            _p: std::marker::PhantomData,
        }
    }

    fn link_at(&self, byte: usize) -> Option<&str> {
        self.links
            .iter()
            .find(|(r, _)| r.contains(&byte))
            .map(|(_, u)| u.as_str())
    }

    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let size = self.head_size.unwrap_or(BODY);
        let family = if self.head_size.is_some() || self.strong > 0 {
            theme::family_bold()
        } else if self.italics > 0 {
            theme::family_italic()
        } else {
            FontFamily::Proportional
        };
        let color = if self.link.is_some() {
            self.pal.accent
        } else if self.weak_text {
            self.pal.weak
        } else {
            self.pal.text
        };
        let start = self.job.text.len();
        self.job.append(
            text, 0.0,
            TextFormat {
                font_id: FontId::new(size, family),
                color,
                background: Color32::TRANSPARENT,
                italics: self.italics > 0 && self.strong == 0,
                underline: if self.link.is_some() {
                    Stroke::new(1.0_f32, color.gamma_multiply(0.7))
                } else {
                    Stroke::NONE
                },
                strikethrough: if self.strike > 0 {
                    Stroke::new(1.0_f32, color)
                } else {
                    Stroke::NONE
                },
                ..Default::default()
            },
        );
        if let Some(url) = self.link.clone() {
            self.links.push((start..self.job.text.len(), url));
        }
    }

    fn push_code(&mut self, text: &str) {
        let p = &self.pal;
        self.job.append(
            text, 0.0,
            TextFormat {
                font_id: FontId::new(self.head_size.unwrap_or(14.0), FontFamily::Monospace),
                color: p.text,
                background: p.faint_fill,
                ..Default::default()
            },
        );
    }
}

fn inline_until(cfg: &Cfg, evs: &[Event], i: &mut usize, stop: Stop, st: &mut InlineSt) {
    use pulldown_cmark::TagEnd as E;
    while *i < evs.len() {
        match &evs[*i] {
            Event::End(e) if stop == Stop::Item && matches!(e, E::Paragraph | E::Item) => {
                return;
            }
            Event::Start(t) if stop == Stop::Item && is_block_start(t) => {
                return;
            }
            Event::End(e) if inline_stop(stop, e) => {
                *i += 1;
                return;
            }
            Event::Text(t) => {
                st.push(t);
                *i += 1;
            }
            Event::Code(c) => {
                st.push_code(c);
                *i += 1;
            }
            Event::SoftBreak => {
                st.push(" ");
                *i += 1;
            }
            Event::HardBreak => {
                st.push("\n");
                *i += 1;
            }
            Event::FootnoteReference(n) => {
                let save_weak = st.weak_text;
                st.weak_text = true;
                st.push(format!("[{}]", n).as_str());
                st.weak_text = save_weak;
                *i += 1;
            }
            Event::InlineMath(c) | Event::DisplayMath(c) => {
                let c = c.to_string();
                st.push_code(&c);
                *i += 1;
            }
            Event::Start(Tag::Emphasis) => {
                st.italics += 1;
                *i += 1;
            }
            Event::Start(Tag::Strong) => {
                st.strong += 1;
                *i += 1;
            }
            Event::Start(Tag::Strikethrough) => {
                st.strike += 1;
                *i += 1;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if st.link.is_none() {
                    st.link = Some(dest_url.to_string());
                }
                *i += 1;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let tag = evs[*i].clone_event_tag();
                let alt = collect_alt(evs, *i + 1, &tag);
                let dest = dest_url.to_string();
                render_inline_image(st, &dest, &alt, cfg);
                skip_through(evs, i, &tag);
            }
            Event::InlineHtml(_) | Event::Html(_) => {
                *i += 1;
            }
            _ => {
                *i += 1;
            }
        }
    }
}

trait CloneTag {
    fn clone_event_tag(&self) -> Tag<'_>;
}

impl CloneTag for Event<'_> {
    fn clone_event_tag(&self) -> Tag<'_> {
        match self {
            Event::Start(t) => t.clone(),
            other => panic!("not a start tag: {other:?}"),
        }
    }
}

fn collect_alt(evs: &[Event], mut j: usize, img_tag: &Tag) -> String {
    let mut alt = String::new();
    while j < evs.len() {
        match &evs[j] {
            Event::Text(t) => alt.push_str(t),
            Event::Code(c) => alt.push_str(c),
            Event::End(e) if closes(img_tag, e) => break,
            _ => {}
        }
        j += 1;
    }
    alt
}

fn skip_through(evs: &[Event], i: &mut usize, tag: &Tag) {
    *i += 1;
    while *i < evs.len() {
        if let Event::End(e) = &evs[*i] {
            if closes(tag, e) {
                *i += 1;
                return;
            }
        }
        *i += 1;
    }
}

fn render_inline_image(st: &mut InlineSt, dest: &str, alt: &str, cfg: &Cfg) {
    let label = if alt.trim().is_empty() {
        "image".to_string()
    } else {
        format!("image: {alt}")
    };
    let hint = if resolve_local(cfg.base, dest).is_some() {
        label
    } else if let Some(rest) = dest.strip_prefix("https://") {
        format!("{label} ({})", rest.split('/').next().unwrap_or(dest))
    } else {
        format!("{label} ({dest})")
    };
    let save = st.link.clone();
    st.link = Some(String::new());
    st.push(&format!("[{hint}]"));
    st.link = save;
}

fn add_inline(ui: &mut Ui, mut st: InlineSt) {
    if st.links.is_empty() {
        ui.add(Label::new(st.job).selectable(true));
        return;
    }
    let job = std::mem::take(&mut st.job);
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(rect.min, galley.clone(), Color32::TRANSPARENT);
    let mut clicked_url: Option<String> = None;
    if let Some(pos) = resp.hover_pos() {
        if rect.contains(pos) {
            let byte = galley.cursor_from_pos(pos - rect.min).ccursor.index;
            if st.link_at(byte).is_some() {
                resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        }
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if rect.contains(pos) {
                let byte = galley.cursor_from_pos(pos - rect.min).ccursor.index;
                if let Some(url) = st.link_at(byte) {
                    clicked_url = Some(url.to_string());
                }
            }
        }
    }
    if let Some(url) = clicked_url {
        open_link(ui, &url);
    }
}

fn open_link(ui: &Ui, url: &str) {
    let url = url.trim();
    if url.is_empty() {
        return;
    }
    if url.starts_with('#') {
        return;
    }
    ui.ctx().output_mut(|o| {
        o.commands
            .push(eframe::egui::OutputCommand::OpenUrl(eframe::egui::OpenUrl::new_tab(url.to_string())))
    });
}

fn resolve_local(base: Option<&Path>, src: &str) -> Option<PathBuf> {
    let s = src.trim();
    if s.is_empty()
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("data:")
        || s.starts_with('#')
        || s.contains("://")
    {
        return None;
    }
    let p = Path::new(s);
    let full = if p.is_absolute() { p.to_path_buf() } else { base?.join(p) };
    Some(full)
}

fn paragraph(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize) {
    let pal = theme::palette(cfg.dark);
    let width = ui.available_width();

    if let Some(Event::Start(tag @ Tag::Image { dest_url, .. })) = evs.get(*i) {
        let tag = tag.clone();
        let dest = dest_url.to_string();
        let alt = collect_alt(evs, *i + 1, &tag);
        skip_through(evs, i, &tag);
        if matches!(evs.get(*i), Some(Event::End(e)) if closes(&Tag::Paragraph, e)) {
            *i += 1;
            render_image_block(ui, cfg, &dest, &alt, pal);
            return;
        }
        let mut st = InlineSt::new(width, pal);
        render_inline_image(&mut st, &dest, &alt, cfg);
        inline_until(cfg, evs, i, Stop::Para, &mut st);
        add_inline(ui, st);
        return;
    }

    let mut st = InlineSt::new(width, pal);
    inline_until(cfg, evs, i, Stop::Para, &mut st);
    add_inline(ui, st);
}

fn heading(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize, lvl: u8) {
    let pal = theme::palette(cfg.dark);
    let sizes = [26.0, 21.5, 18.0, 16.0, 14.8, 13.8];
    let size = sizes[(lvl as usize).min(6) - 1];
    let mut st = InlineSt::new(ui.available_width(), pal);
    st.head_size = Some(size);
    inline_until(cfg, evs, i, Stop::Head, &mut st);
    add_inline(ui, st);
    if lvl <= 2 {
        ui.add_space(4.0);
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
        ui.painter().line_segment(
            [egui::pos2(rect.left(), rect.top()), egui::pos2(rect.right(), rect.top())],
            Stroke::new(1.0_f32, pal.stroke),
        );
    }
    ui.add_space(4.0);
}

fn quote(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize) {
    let pal = theme::palette(cfg.dark);
    if cfg.depth >= MAX_NESTING {
        let tag = evs[*i].clone_event_tag();
        skip_through(evs, i, &tag);
        return;
    }
    cfg.depth += 1;
    let frame = egui::Frame::default()
        .fill(pal.faint_fill)
        .corner_radius(3)
        .inner_margin(Margin::symmetric(12, 7));
    let rr = frame.show(ui, |ui| {
        let mut first = true;
        blocks_until(ui, cfg, evs, i, Some(&Tag::BlockQuote(None)), &mut first);
    });
    cfg.depth -= 1;
    let r = rr.response.rect;
    ui.painter().line_segment(
        [egui::pos2(r.left() + 1.0, r.top() + 2.0), egui::pos2(r.left() + 1.0, r.bottom() - 2.0)],
        Stroke::new(2.5_f32, pal.accent.gamma_multiply(0.55)),
    );
}

fn code_theme(dark: bool) -> (&'static Theme, Color32) {
    let ts = THEMES.get_or_init(ThemeSet::load_defaults);
    let (name, fallback) = if dark {
        ("base16-ocean.dark", Color32::from_rgb(0x1b, 0x1e, 0x24))
    } else {
        ("InspiredGitHub", Color32::from_rgb(0xf6, 0xf8, 0xfa))
    };
    match ts.themes.get(name) {
        Some(t) => {
            let bg = t.settings.background.unwrap_or(syntect::highlighting::Color {
                r: fallback.r(),
                g: fallback.g(),
                b: fallback.b(),
                a: 255,
            });
            (t, Color32::from_rgba_unmultiplied(bg.r, bg.g, bg.b, 255))
        }
        None => (ts.themes.values().next().unwrap(), fallback),
    }
}

fn highlight_code(code: &str, lang: Option<&str>, dark: bool) -> Vec<Spans> {
    let ss = SYNTAX.get_or_init(SyntaxSet::load_defaults_newlines);
    let (theme_ref, _) = code_theme(dark);
    let syn = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut hl = HighlightLines::new(syn, theme_ref);
    let mut out: Vec<Spans> = Vec::new();
    for line in LinesWithEndings::from(code) {
        let spans = hl.highlight_line(line, ss).unwrap_or_default();
        let mut row: Spans = Vec::new();
        for (style, txt) in spans {
            let trimmed_end = txt.trim_end_matches(['\n', '\r']);
            if trimmed_end.is_empty() {
                continue;
            }
            let c = style.foreground;
            row.push((
                Color32::from_rgba_unmultiplied(c.r, c.g, c.b, 255),
                trimmed_end.to_string(),
            ));
        }
        out.push(row);
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

fn code_block(ui: &mut Ui, cfg: &mut Cfg, evs: &[Event], i: &mut usize, lang: Option<&str>) {
    let mut raw = String::new();
    loop {
        match evs.get(*i) {
            Some(Event::Text(t)) => {
                raw.push_str(t);
                *i += 1;
            }
            Some(Event::Html(h)) => {
                raw.push_str(h);
                *i += 1;
            }
            Some(Event::End(e)) if closes(&Tag::CodeBlock(CodeBlockKind::Indented), e) => {
                *i += 1;
                break;
            }
            None => break,
            _ => {
                *i += 1;
            }
        }
    }

    let hash_key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut h);
        lang.unwrap_or("").hash(&mut h);
        (h.finish(), cfg.dark)
    };

    let lines = match cfg.pv.code.get(&hash_key) {
        Some(v) => v.clone(),
        None => {
            let hl = Arc::new(highlight_code(&raw, lang, cfg.dark));
            if cfg.pv.code.len() > 96 {
                cfg.pv.code.clear();
            }
            cfg.pv.code.insert(hash_key, hl.clone());
            hl
        }
    };

    let (_, bg) = code_theme(cfg.dark);
    let pal = theme::palette(cfg.dark);

    cfg.block_no += 1;
    let id = Id::new(("lihati-code-copy", cfg.block_no));
    let frame = egui::Frame::default()
        .fill(bg)
        .corner_radius(4)
        .inner_margin(egui::Margin::same(9))
        .stroke(Stroke::new(1.0_f32, pal.stroke.gamma_multiply(0.6)));
    let rr = frame.show(ui, |ui| {
        let mut job = LayoutJob::default();
        let w = ui.available_width();
        job.wrap.max_width = if w.is_finite() && w > 40.0 { w } else { 480.0 };
        job.wrap.break_anywhere = true;
        let mono = FontId::monospace(13.0);
        let n = lines.len();
        for (idx, row) in lines.iter().enumerate() {
            for (color, txt) in row {
                job.append(txt, 0.0, TextFormat::simple(mono.clone(), *color));
            }
            if idx + 1 < n {
                job.append("\n", 0.0, TextFormat::simple(mono.clone(), pal.weak));
            }
        }
        ui.add(Label::new(job).selectable(true));
    });

    let resp = ui.interact(rr.response.rect, id, Sense::click());
    if resp.hovered() && !raw.is_empty() {
        let chip = egui::Rect::from_min_size(
            resp.rect.right_top() - Vec2::new(52.0, -6.0),
            Vec2::new(44.0, 18.0),
        );
        let p = ui.painter();
        p.rect_filled(chip, 3.0, pal.active);
        p.text(chip.center(), Align2::CENTER_CENTER, "Copy", FontId::proportional(11.0), pal.text);
    }
    if resp.clicked() && !raw.is_empty() {
        ui.output_mut(|o| o.commands.push(eframe::egui::OutputCommand::CopyText(raw.clone())));
        ui.ctx().request_repaint();
    }
}

fn draw_checkbox(ui: &mut Ui, checked: bool, pal: &theme::Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.shrink(0.5), 3.0, if checked { pal.accent } else { pal.extreme });
    p.rect_stroke(
        rect.shrink(0.5),
        3,
        Stroke::new(1.2_f32, if checked { pal.accent } else { pal.weak }),
        egui::StrokeKind::Middle,
    );
    if checked {
        let a = egui::pos2(rect.left() + 3.5, rect.center().y + 0.5);
        let b = egui::pos2(rect.center().x - 0.5, rect.bottom() - 4.0);
        let c = egui::pos2(rect.right() - 3.5, rect.top() + 4.0);
        p.line_segment([a, b], Stroke::new(1.8_f32, Color32::WHITE));
        p.line_segment([b, c], Stroke::new(1.8_f32, Color32::WHITE));
    }
}

fn plain_cell(evs: &[Event], i: &mut usize) -> String {
    let mut s = String::new();
    while *i < evs.len() {
        match &evs[*i] {
            Event::Text(t) => {
                s.push_str(t);
                *i += 1;
            }
            Event::Code(c) => {
                s.push_str(c);
                *i += 1;
            }
            Event::SoftBreak => {
                s.push(' ');
                *i += 1;
            }
            Event::End(e) if closes(&Tag::TableCell, e) => {
                *i += 1;
                break;
            }
            _ => *i += 1,
        }
    }
    s
}

fn table(
    ui: &mut Ui,
    cfg: &mut Cfg,
    evs: &[Event],
    i: &mut usize,
    aligns: &[pulldown_cmark::Alignment],
) {
    let mut header: Vec<String> = Vec::new();
    if let Some(Event::Start(Tag::TableHead)) = evs.get(*i) {
        *i += 1;
        while *i < evs.len() {
            match &evs[*i] {
                Event::Start(Tag::TableCell) => {
                    *i += 1;
                    header.push(plain_cell(evs, i));
                }
                Event::End(e) if closes(&Tag::TableHead, e) => {
                    *i += 1;
                    break;
                }
                _ => *i += 1,
            }
        }
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    while *i < evs.len() {
        match &evs[*i] {
            Event::Start(Tag::TableRow) => {
                *i += 1;
                let mut cells = Vec::new();
                while *i < evs.len() {
                    match &evs[*i] {
                        Event::Start(Tag::TableCell) => {
                            *i += 1;
                            cells.push(plain_cell(evs, i));
                        }
                        Event::End(e) if closes(&Tag::TableRow, e) => {
                            *i += 1;
                            break;
                        }
                        _ => *i += 1,
                    }
                }
                rows.push(cells);
            }
            Event::End(e) if closes(&Tag::Table(Vec::new()), e) => {
                *i += 1;
                break;
            }
            _ => *i += 1,
        }
    }

    let ncols = aligns.len().max(header.len()).max(rows.iter().map(|r| r.len()).max().unwrap_or(0)).max(1);
    let pal = theme::palette(cfg.dark);
    let header_font = FontId::new(14.2, theme::family_bold());
    let body_font = FontId::proportional(BODY - 0.5);
    const PAD_X: f32 = 16.0;
    const PAD_Y: f32 = 5.0;

    let layout_cell_galley = |ui: &Ui, text: &str, w: f32, font: &FontId, align: Option<&pulldown_cmark::Alignment>| -> (Arc<egui::Galley>, f32) {
        use pulldown_cmark::Alignment;
        let wrap_w = (w - PAD_X).max(28.0);
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_w;
        job.wrap.break_anywhere = true;
        job.append(text, 0.0, TextFormat::simple(font.clone(), pal.text));
        let g = ui.painter().layout_job(job);
        let x_off = match align {
            Some(Alignment::Right) => (wrap_w - g.size().x).max(0.0),
            Some(Alignment::Center) => ((wrap_w - g.size().x) * 0.5).max(0.0),
            _ => 0.0,
        };
        (g, x_off)
    };

    let natural: Vec<f32> = (0..ncols)
        .map(|col| {
            let mut w = ui
                .ctx()
                .fonts(|f| {
                    f.layout_no_wrap(
                        header.get(col).cloned().unwrap_or_default(),
                        header_font.clone(),
                        pal.text,
                    )
                })
                .size()
                .x;
            for row in &rows {
                w = w.max(
                    ui.ctx()
                        .fonts(|f| {
                            f.layout_no_wrap(
                                row.get(col).cloned().unwrap_or_default(),
                                body_font.clone(),
                                pal.text,
                            )
                        })
                        .size()
                        .x,
                );
            }
            w + PAD_X
        })
        .collect();

    let avail = ui.available_width().max(160.0);
    let total: f32 = natural.iter().sum();
    let widths: Vec<f32> = if total > avail {
        let scale = (avail / total).max(0.08);
        natural.iter().map(|w| (*w * scale).max(48.0)).collect()
    } else {
        natural
    };

    let row_w: f32 = widths.iter().sum::<f32>().min(avail);

    let paint_row = |ui: &mut Ui, cells: &[String], font: &FontId, header_row: bool| -> f32 {
        let galleys: Vec<(Arc<egui::Galley>, f32)> = (0..ncols)
            .map(|col| {
                layout_cell_galley(
                    ui,
                    cells.get(col).map(String::as_str).unwrap_or(""),
                    widths[col],
                    font,
                    aligns.get(col),
                )
            })
            .collect();
        let row_h = galleys
            .iter()
            .map(|(g, _)| g.size().y)
            .fold(0.0_f32, f32::max)
            + PAD_Y * 2.0;

        let y_top = ui.cursor().top();
        let x_left = ui.cursor().left();
        for (col, (g, x_off)) in galleys.iter().enumerate() {
            let cell_x = x_left + widths[..col].iter().sum::<f32>();
            let y_text = y_top + PAD_Y + if header_row { 1.0 } else { 0.0 };
            ui.painter().galley(
                egui::pos2(cell_x + PAD_X * 0.5 + x_off, y_text),
                g.clone(),
                Color32::TRANSPARENT,
            );
        }
        ui.allocate_exact_size(Vec2::new(row_w, row_h), Sense::hover());
        row_h
    };

    ui.vertical(|ui| {
        paint_row(ui, &header, &header_font, true);
        rule_line(ui, row_w, pal.stroke);

        for (ri, row) in rows.iter().enumerate() {
            paint_row(ui, row, &body_font, false);
            if ri + 1 < rows.len() {
                rule_line(ui, row_w, pal.stroke.gamma_multiply(0.4));
            }
        }
    });
    ui.add_space(6.0);
}

fn rule_line(ui: &mut Ui, w: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w.min(ui.available_width()), 1.0), Sense::hover());
    ui.painter().line_segment(
        [egui::pos2(rect.left(), rect.center().y), egui::pos2(rect.left() + rect.width(), rect.center().y)],
        Stroke::new(1.0_f32, color),
    );
}

fn flush_inline(ui: &mut Ui, st: &mut Option<InlineSt>, first: &mut bool) {
    if let Some(s) = st.take() {
        if !s.job.text.is_empty() {
            gap(ui, *first, 4.0);
            add_inline(ui, s);
            *first = false;
        }
    }
}

fn list(
    ui: &mut Ui,
    cfg: &mut Cfg,
    evs: &[Event],
    i: &mut usize,
    ordered_start: Option<u64>,
    depth: usize,
) {
    let pal = theme::palette(cfg.dark);
    if cfg.depth >= MAX_NESTING {
        let tag = evs[*i].clone_event_tag();
        skip_through(evs, i, &tag);
        return;
    }
    cfg.depth += 1;
    let mut counter = ordered_start.unwrap_or(1);
    while *i < evs.len() {
        match &evs[*i] {
            Event::Start(Tag::Item) => {
                *i += 1;
                let mut task: Option<bool> = None;
                if let Some(Event::TaskListMarker(b)) = evs.get(*i) {
                    task = Some(*b);
                    *i += 1;
                }
                let marker = if ordered_start.is_some() {
                    let m = format!("{counter}.");
                    counter += 1;
                    m
                } else {
                    "\u{2022}".to_string()
                };
                ui.horizontal_top(|ui| {
                    ui.add_space(depth as f32 * 18.0);
                    if let Some(done) = task {
                        draw_checkbox(ui, done, &pal);
                        ui.add_space(2.0);
                    } else {
                        ui.with_layout(
                            eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                            |ui| {
                                let min_w = if ordered_start.is_some() { 26.0 } else { 14.0 };
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::new(min_w, 20.0), Sense::hover());
                                ui.painter().text(
                                    egui::pos2(rect.left(), rect.center().y),
                                    Align2::LEFT_CENTER,
                                    marker,
                                    FontId::proportional(BODY - 0.5),
                                    pal.weak,
                                );
                            },
                        );
                    }
                    ui.vertical(|ui| {
                        let mut first = true;
                        let mut st: Option<InlineSt> = None;
                        while *i < evs.len() {
                            match &evs[*i] {
                                Event::End(e) if closes(&Tag::Item, e) => {
                                    *i += 1;
                                    break;
                                }
                                Event::End(e) if closes(&Tag::Paragraph, e) => {
                                    flush_inline(ui, &mut st, &mut first);
                                    *i += 1;
                                }
                                Event::Start(t) if is_block_start(t) => {
                                    flush_inline(ui, &mut st, &mut first);
                                    render_block(ui, cfg, evs, i, &mut first);
                                }
                                _ => {
                                    if st.is_none() {
                                        st = Some(InlineSt::new(
                                            ui.available_width(),
                                            theme::palette(cfg.dark),
                                        ));
                                    }
                                    inline_until(
                                        cfg,
                                        evs,
                                        i,
                                        Stop::Item,
                                        st.as_mut().unwrap(),
                                    );
                                }
                            }
                        }
                        flush_inline(ui, &mut st, &mut first);
                    });
                });
                ui.add_space(2.0);
            }
            Event::End(e) if closes(&Tag::List(if ordered_start.is_some() { Some(1) } else { None }), e) => {
                *i += 1;
                break;
            }
            _ => {
                *i += 1;
            }
        }
    }
    cfg.depth -= 1;
}

fn hr(ui: &mut Ui) {
    ui.add_space(4.0);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 2.0), Sense::hover());
    let pal = theme::palette(ui.visuals().dark_mode);
    ui.painter().rect_filled(rect, 1.0, pal.stroke);
    ui.add_space(6.0);
}

fn load_texture(ui: &Ui, cfg: &mut Cfg, path: PathBuf) -> Option<TextureHandle> {
    let meta = std::fs::metadata(&path).ok()?;
    let key_meta = (meta.modified().ok()?, meta.len());
    if let Some((m, tex)) = cfg.pv.images.get(&path) {
        if *m == key_meta {
            return Some(tex.clone());
        }
    }
    let img = image::open(&path).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ui.ctx().load_texture(
        format!("md-img:{}", path.display()),
        eframe::egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::default(),
    );
    if cfg.pv.images.len() > 48 {
        cfg.pv.images.clear();
    }
    cfg.pv.images.insert(path, (key_meta, tex.clone()));
    Some(tex)
}

fn render_image_block(ui: &mut Ui, cfg: &mut Cfg, dest: &str, alt: &str, pal: theme::Palette) {
    ui.add_space(2.0);
    if let Some(path) = resolve_local(cfg.base, dest) {
        if let Some(tex) = load_texture(ui, cfg, path) {
            let nat = tex.size_vec2();
            let avail_w = ui.available_width();
            let max_h = 520.0f32;
            let scale = (avail_w / nat.x).min(max_h / nat.y).min(1.0);
            let size = Vec2::new(nat.x * scale, nat.y * scale);
            ui.add(
                eframe::egui::Image::new(&tex)
                    .fit_to_exact_size(size)
                    .corner_radius(4),
            );
            if !alt.trim().is_empty() {
                ui.add_space(3.0);
                ui.label(
                    eframe::egui::RichText::new(alt)
                        .size(12.5)
                        .weak()
                        .italics(),
                );
            }
            ui.add_space(4.0);
            return;
        }
    }
    let host = if let Some(rest) = dest.strip_prefix("https://") {
        rest.split('/').next().unwrap_or(dest)
    } else if let Some(rest) = dest.strip_prefix("http://") {
        rest.split('/').next().unwrap_or(dest)
    } else {
        dest
    };
    let frame = egui::Frame::default()
        .fill(pal.faint_fill)
        .stroke(Stroke::new(1.0_f32, pal.stroke))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(10, 7));
    frame.show(ui, |ui| {
        let label = if alt.trim().is_empty() {
            format!("image unavailable \u{2014} {host}")
        } else {
            format!("{alt} \u{2014} {host}")
        };
        ui.label(eframe::egui::RichText::new(label).weak().size(13.0));
    });
    ui.add_space(2.0);
}
