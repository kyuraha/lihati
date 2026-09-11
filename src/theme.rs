use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Margin, Style, Visuals};

use crate::state::ThemePref;

const INTER_400: &[u8] = include_bytes!("../assets/fonts/inter-400.ttf");
const INTER_700: &[u8] = include_bytes!("../assets/fonts/inter-700.ttf");
const INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/inter-italic.ttf");
const MONO_400: &[u8] = include_bytes!("../assets/fonts/jetbrains-mono-400.ttf");

pub const BOLD_FAMILY_NAME: &str = "lihati-bold";
pub const ITALIC_FAMILY_NAME: &str = "lihati-italic";

pub fn family_bold() -> FontFamily {
    FontFamily::Name(BOLD_FAMILY_NAME.into())
}

pub fn family_italic() -> FontFamily {
    FontFamily::Name(ITALIC_FAMILY_NAME.into())
}

pub fn init_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter-400".into(), FontData::from_static(INTER_400).into());
    fonts.font_data.insert("inter-700".into(), FontData::from_static(INTER_700).into());
    fonts.font_data.insert("inter-italic".into(), FontData::from_static(INTER_ITALIC).into());
    fonts.font_data.insert("jbmono-400".into(), FontData::from_static(MONO_400).into());

    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, "inter-400".into());
    }
    if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
        list.insert(0, "jbmono-400".into());
    }
    fonts.families.insert(FontFamily::Name(BOLD_FAMILY_NAME.into()), vec!["inter-700".into()]);
    fonts.families.insert(FontFamily::Name(ITALIC_FAMILY_NAME.into()), vec!["inter-italic".into()]);
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub extreme: Color32,
    pub stroke: Color32,
    pub faint_fill: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub warn: Color32,
    pub warn_soft: Color32,
    pub err: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: Color32::from_rgb(0x14, 0x16, 0x1a),
            panel: Color32::from_rgb(0x19, 0x1c, 0x21),
            extreme: Color32::from_rgb(0x10, 0x12, 0x16),
            stroke: Color32::from_rgb(0x2a, 0x2e, 0x36),
            faint_fill: Color32::from_rgb(0x20, 0x24, 0x2b),
            hover: Color32::from_rgb(0x25, 0x29, 0x31),
            active: Color32::from_rgb(0x2d, 0x33, 0x3d),
            text: Color32::from_rgb(0xd8, 0xdd, 0xe5),
            weak: Color32::from_rgb(0x8f, 0x97, 0xa4),
            accent: Color32::from_rgb(0x5b, 0x9d, 0xff),
            selection: Color32::from_rgba_premultiplied(0x5b, 0x9d, 0xff, 115),
            warn: Color32::from_rgb(0xe8, 0xb4, 0x5a),
            warn_soft: Color32::from_rgba_premultiplied(0xe8, 0xb4, 0x5a, 34),
            err: Color32::from_rgb(0xef, 0x7a, 0x70),
        }
    } else {
        Palette {
            bg: Color32::from_rgb(0xfb, 0xfb, 0xfa),
            panel: Color32::from_rgb(0xf2, 0xf2, 0xf0),
            extreme: Color32::WHITE,
            stroke: Color32::from_rgb(0xe2, 0xe2, 0xde),
            faint_fill: Color32::from_rgb(0xec, 0xec, 0xe9),
            hover: Color32::from_rgb(0xe7, 0xe7, 0xe4),
            active: Color32::from_rgb(0xdf, 0xdf, 0xdb),
            text: Color32::from_rgb(0x21, 0x26, 0x2d),
            weak: Color32::from_rgb(0x6b, 0x73, 0x82),
            accent: Color32::from_rgb(0x2f, 0x6f, 0xed),
            selection: Color32::from_rgba_premultiplied(0x2f, 0x6f, 0xed, 85),
            warn: Color32::from_rgb(0xb7, 0x79, 0x1f),
            warn_soft: Color32::from_rgba_premultiplied(0xe8, 0xb4, 0x5a, 60),
            err: Color32::from_rgb(0xd6, 0x45, 0x45),
        }
    }
}

fn styled(dark: bool) -> Style {
    let p = palette(dark);
    let mut style = Style::default();
    let v = &mut style.visuals;
    *v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.extreme;
    v.faint_bg_color = p.faint_fill;
    v.window_stroke = egui::Stroke::new(1.0_f32, p.stroke);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
    v.widgets.active.bg_fill = p.active;
    v.widgets.active.bg_stroke = egui::Stroke::NONE;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
    v.selection.bg_fill = p.selection;
    v.selection.stroke = egui::Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;
    v.override_text_color = Some(p.text);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.menu_margin = Margin::same(6);
    style.scroll_animation = egui::style::ScrollAnimation::none();
    // Grab zone for the panel splitters (Fitts's law). Keep it snug: the zone
    // starts exactly where the center scrollbar starts, so there is no stray
    // sliver on its left that would read as a second hotspot.
    style.interaction.resize_grab_radius_side = 12.0;
    // Freeze floating scrollbars at full width: the default thin<->wide pulse
    // on hover makes the hit-area next to a panel splitter breathe, which
    // reads as phantom hotspots and cursor flicker.
    style.spacing.scroll.floating_width = style.spacing.scroll.bar_width;
    style.text_styles = [
        (egui::TextStyle::Body, egui::FontId::proportional(15.0)),
        (egui::TextStyle::Button, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Small, egui::FontId::proportional(11.5)),
        (egui::TextStyle::Heading, egui::FontId::new(19.0, family_bold())),
        (
            egui::TextStyle::Monospace,
            egui::FontId::monospace(14.0),
        ),
        (
            egui::TextStyle::Name("PreviewBody".into()),
            egui::FontId::proportional(15.5),
        ),
    ]
    .into_iter()
    .collect();
    style
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_style_of(egui::Theme::Dark, styled(true));
    ctx.set_style_of(egui::Theme::Light, styled(false));
}

pub fn set_pref(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(match pref {
        ThemePref::Dark => egui::ThemePreference::Dark,
        ThemePref::Light => egui::ThemePreference::Light,
    });
}
