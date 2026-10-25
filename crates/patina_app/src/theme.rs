//! Patina's dark theme: neutral grays with a verdigris (patina green) accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(63, 165, 143);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(38, 96, 84);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    let bg = Color32::from_gray(32);
    let panel = Color32::from_gray(38);
    v.panel_fill = panel;
    v.window_fill = Color32::from_gray(42);
    v.extreme_bg_color = Color32::from_gray(24);
    v.faint_bg_color = Color32::from_gray(44);
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(58));
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(52));
    w.inactive.bg_fill = Color32::from_gray(52);
    w.inactive.weak_bg_fill = Color32::from_gray(48);
    w.hovered.bg_fill = Color32::from_gray(64);
    w.hovered.weak_bg_fill = Color32::from_gray(60);
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.open.weak_bg_fill = Color32::from_gray(56);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = CornerRadius::same(3);
    }

    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(6.0, 3.0);
        style.spacing.slider_width = 150.0;
    });
}
