//! Look and feel: four palettes (Catppuccin Mocha, Light, Sepia, Night), reading fonts from Windows, icon font.

use crate::settings::{ReadingFont, ThemeId};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke, Ui, vec2,
};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Window chrome.
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub warn: Color32,
    /// The reading surface.
    pub paper: Color32,
    pub ink: Color32,
    pub ink_dim: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub hit: Color32,
    pub hit_current: Color32,
    pub selection: Color32,
    pub quote_bar: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn palette(id: ThemeId) -> Palette {
    match id {
        ThemeId::Mocha => Palette {
            dark: true,
            bg: rgb(24, 24, 37),
            panel: rgb(30, 30, 46),
            card: rgb(49, 50, 68),
            card_hover: rgb(59, 60, 80),
            border: rgb(69, 71, 90),
            text: rgb(205, 214, 244),
            dim: rgb(147, 153, 178),
            accent: rgb(203, 166, 247),
            on_accent: rgb(30, 30, 46),
            warn: rgb(249, 226, 175),
            paper: rgb(30, 30, 46),
            ink: rgb(205, 214, 244),
            ink_dim: rgb(147, 153, 178),
            code_bg: rgb(24, 24, 37),
            link: rgb(137, 180, 250),
            hit: rgb(249, 226, 175).gamma_multiply(0.38),
            hit_current: rgb(250, 179, 135).gamma_multiply(0.75),
            selection: rgb(203, 166, 247).gamma_multiply(0.35),
            quote_bar: rgb(180, 190, 254),
        },
        ThemeId::Light => Palette {
            dark: false,
            bg: rgb(234, 236, 242),
            panel: rgb(244, 245, 249),
            card: rgb(255, 255, 255),
            card_hover: rgb(238, 240, 250),
            border: rgb(209, 213, 224),
            text: rgb(31, 35, 48),
            dim: rgb(104, 110, 128),
            accent: rgb(88, 86, 214),
            on_accent: rgb(255, 255, 255),
            warn: rgb(190, 120, 20),
            paper: rgb(253, 253, 254),
            ink: rgb(31, 35, 48),
            ink_dim: rgb(104, 110, 128),
            code_bg: rgb(240, 242, 247),
            link: rgb(40, 90, 200),
            hit: rgb(255, 224, 80).gamma_multiply(0.7),
            hit_current: rgb(255, 150, 60).gamma_multiply(0.85),
            selection: rgb(88, 86, 214).gamma_multiply(0.28),
            quote_bar: rgb(88, 86, 214),
        },
        ThemeId::Sepia => Palette {
            dark: false,
            bg: rgb(228, 216, 190),
            panel: rgb(238, 227, 202),
            card: rgb(246, 238, 218),
            card_hover: rgb(240, 229, 203),
            border: rgb(205, 190, 158),
            text: rgb(67, 52, 34),
            dim: rgb(125, 106, 80),
            accent: rgb(160, 82, 45),
            on_accent: rgb(255, 248, 232),
            warn: rgb(170, 100, 20),
            paper: rgb(244, 236, 216),
            ink: rgb(58, 44, 28),
            ink_dim: rgb(125, 106, 80),
            code_bg: rgb(233, 223, 198),
            link: rgb(130, 60, 30),
            hit: rgb(240, 190, 60).gamma_multiply(0.6),
            hit_current: rgb(220, 110, 50).gamma_multiply(0.75),
            selection: rgb(160, 82, 45).gamma_multiply(0.28),
            quote_bar: rgb(160, 82, 45),
        },
        ThemeId::Night => Palette {
            dark: true,
            bg: rgb(0, 0, 0),
            panel: rgb(10, 10, 12),
            card: rgb(24, 24, 28),
            card_hover: rgb(34, 34, 40),
            border: rgb(44, 44, 52),
            text: rgb(205, 205, 210),
            dim: rgb(130, 130, 140),
            accent: rgb(138, 180, 248),
            on_accent: rgb(0, 0, 0),
            warn: rgb(240, 200, 100),
            paper: rgb(0, 0, 0),
            ink: rgb(184, 184, 190),
            ink_dim: rgb(120, 120, 130),
            code_bg: rgb(18, 18, 22),
            link: rgb(138, 180, 248),
            hit: rgb(240, 200, 100).gamma_multiply(0.35),
            hit_current: rgb(255, 150, 80).gamma_multiply(0.7),
            selection: rgb(138, 180, 248).gamma_multiply(0.32),
            quote_bar: rgb(138, 180, 248),
        },
    }
}

pub fn theme_name(id: ThemeId) -> &'static str {
    use crate::i18n::tr;
    match id {
        ThemeId::Mocha => "Mocha",
        ThemeId::Light => tr("Светлая", "Light"),
        ThemeId::Sepia => tr("Сепия", "Sepia"),
        ThemeId::Night => tr("Ночь", "Night"),
    }
}

pub fn install(ctx: &egui::Context, p: &Palette) {
    let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    let p = *p;
    ctx.style_mut_of(theme, move |st| {
        st.spacing.item_spacing = vec2(8.0, 6.0);
        st.spacing.button_padding = vec2(10.0, 5.0);
        st.spacing.interact_size.y = 28.0;
        st.spacing.scroll.bar_width = 9.0;
        st.spacing.scroll.floating = false;
        let v = &mut st.visuals;
        v.dark_mode = p.dark;
        v.panel_fill = p.panel;
        v.window_fill = p.card;
        v.extreme_bg_color = p.bg;
        v.faint_bg_color = p.card.gamma_multiply(0.5);
        v.code_bg_color = p.code_bg;
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, p.border);
        v.override_text_color = Some(p.text);
        v.selection.bg_fill = p.selection;
        v.selection.stroke = Stroke::new(1.0, p.accent);
        v.hyperlink_color = p.link;
        v.window_shadow = egui::Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 120 } else { 50 }),
        };
        v.popup_shadow = v.window_shadow;
        let r = CornerRadius::same(7);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.inactive.bg_fill = p.border;
        v.widgets.inactive.weak_bg_fill = p.card_hover;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.hovered.bg_fill = p.card_hover;
        v.widgets.hovered.weak_bg_fill = p.card_hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.active.bg_fill = p.accent.gamma_multiply(0.35);
        v.widgets.active.weak_bg_fill = p.accent.gamma_multiply(0.35);
        v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.open.bg_fill = p.card_hover;
        v.widgets.open.weak_bg_fill = p.card_hover;
    });
}

fn windows_font(file: &str) -> Option<Vec<u8>> {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::fs::read(format!(r"{windir}\Fonts\{file}")).ok()
}

pub const SANS: &str = "sans";
pub const SERIF: &str = "serif";
pub const MONO: &str = "mono";
pub const ICONS: &str = "icons";

/// `(family name, [regular, bold, italic, bold italic] files)`.
const FACES: [(&str, [&str; 4]); 3] = [
    (SANS, ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"]),
    (SERIF, ["georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"]),
    (MONO, ["consola.ttf", "consolab.ttf", "consolai.ttf", "consolaz.ttf"]),
];

pub fn family(base: &str, bold: bool, italic: bool) -> FontFamily {
    let suffix = match (bold, italic) {
        (false, false) => "",
        (true, false) => "-b",
        (false, true) => "-i",
        (true, true) => "-bi",
    };
    FontFamily::Name(format!("{base}{suffix}").into())
}

pub fn reading_base(font: ReadingFont) -> &'static str {
    match font {
        ReadingFont::Serif => SERIF,
        ReadingFont::Sans => SANS,
    }
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// Windows fonts in front of egui's built-in ones: Segoe UI (interface), Georgia (reading), Consolas (code) with
/// bold and italic faces, Segoe MDL2 for the toolbar icons, Segoe UI Symbol as a glyph fallback.
pub fn install_fonts(ctx: &egui::Context, with_cjk: bool) {
    let mut fonts = FontDefinitions::default();
    let defaults: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let add_data = |fonts: &mut FontDefinitions, name: &str, bytes: Vec<u8>| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
    };
    if let Some(b) = windows_font("seguisym.ttf") {
        add_data(&mut fonts, "segoe-symbols", b);
    }
    if with_cjk {
        for (name, file) in [
            ("cjk-sc", "msyh.ttc"),
            ("cjk-jp", "YuGothM.ttc"),
            ("cjk-kr", "malgun.ttf"),
        ] {
            if let Some(b) = windows_font(file) {
                add_data(&mut fonts, name, b);
            }
        }
    }
    let tail = |fonts: &FontDefinitions| -> Vec<String> {
        let mut t: Vec<String> = Vec::new();
        for n in ["segoe-symbols", "cjk-sc", "cjk-jp", "cjk-kr"] {
            if fonts.font_data.contains_key(n) {
                t.push(n.to_owned());
            }
        }
        t.extend(defaults.iter().cloned());
        t
    };
    for (base, files) in FACES {
        let mut loaded: [Option<String>; 4] = [None, None, None, None];
        for (i, file) in files.iter().enumerate() {
            if let Some(bytes) = windows_font(file) {
                let name = format!("{base}-face{i}");
                add_data(&mut fonts, &name, bytes);
                loaded[i] = Some(name);
            }
        }
        let regular = loaded[0].clone();
        for (i, suffix) in ["", "-b", "-i", "-bi"].iter().enumerate() {
            let mut list: Vec<String> = Vec::new();
            if let Some(face) = loaded[i].clone().or_else(|| regular.clone()) {
                list.push(face);
            }
            list.extend(tail(&fonts));
            fonts
                .families
                .insert(FontFamily::Name(format!("{base}{suffix}").into()), list);
        }
    }
    if let Some(b) = windows_font("segmdl2.ttf") {
        add_data(&mut fonts, "icons-font", b);
        fonts
            .families
            .insert(FontFamily::Name(ICONS.into()), vec!["icons-font".into()]);
    } else {
        fonts.families.insert(FontFamily::Name(ICONS.into()), tail(&fonts));
    }
    // the interface itself: Segoe UI in front of the proportional family
    if fonts.font_data.contains_key("sans-face0") {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "sans-face0".into());
    }
    if fonts.font_data.contains_key("mono-face0") {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono-face0".into());
    }
    ctx.set_fonts(fonts);
}

pub fn card(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(12)
        .inner_margin(Margin::same(14))
}

/// Primary (accent-filled) button.
pub fn primary(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).strong().color(p.on_accent)).fill(p.accent))
}

/// A square icon button from the icon font; `on` paints it as pressed.
pub fn icon_button(ui: &mut Ui, p: &Palette, glyph: &str, tip: &str, on: bool) -> egui::Response {
    let size = vec2(32.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if on {
            p.accent.gamma_multiply(0.3)
        } else if resp.hovered() {
            p.card_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 7, fill);
        let color = if on { p.accent } else { p.text };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            icon_font(16.0),
            color,
        );
    }
    resp.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_has_readable_ink() {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        for id in ThemeId::ALL {
            let p = palette(id);
            let (a, b) = (lum(p.ink).max(lum(p.paper)), lum(p.ink).min(lum(p.paper)));
            let ratio = (a + 0.05) / (b + 0.05);
            assert!(ratio >= 7.0, "{id:?}: contrast {ratio:.1}");
            let (a, b) = (lum(p.on_accent).max(lum(p.accent)), lum(p.on_accent).min(lum(p.accent)));
            assert!((a + 0.05) / (b + 0.05) >= 3.0, "{id:?}: accent button contrast");
        }
    }

    #[test]
    fn font_families_follow_the_naming_scheme() {
        assert_eq!(family(SERIF, true, false), FontFamily::Name("serif-b".into()));
        assert_eq!(family(MONO, true, true), FontFamily::Name("mono-bi".into()));
        assert_eq!(family(SANS, false, false), FontFamily::Name("sans".into()));
    }
}
