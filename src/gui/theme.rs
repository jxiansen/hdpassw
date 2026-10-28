//! Dark theme matching the OpenDesign prototype (`index.html`).
//!
//! The prototype's palette is the source of truth here; these constants are
//! copied verbatim from its CSS custom properties so the desktop app and the
//! design reference stay in sync. Everything the GUI draws pulls colors from
//! this module rather than hardcoding `Color32::from_rgb(...)` at call sites.
use egui::{Color32, CornerRadius, Stroke, Visuals};

// ── Palette (from the prototype's :root variables) ───────────────────────────

/// App background — `--bg`.
pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x0F);
/// Card / panel surface — `--surface`.
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x18, 0x1B);
/// Hairline borders — `--line`.
pub const LINE: Color32 = Color32::from_rgb(0x23, 0x2A, 0x2F);
/// Slightly recessed surface for inset boxes — `--surface-2`.
pub const SURFACE_2: Color32 = Color32::from_rgb(0x1A, 0x20, 0x24);
/// Raised surface for hovered rows and inset chips — `--surface-3`.
pub const SURFACE_3: Color32 = Color32::from_rgb(0x20, 0x27, 0x2C);
/// Emphasised borders — `--line-strong`.
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x32, 0x3C, 0x43);
/// Primary text — `--text`.
pub const TEXT: Color32 = Color32::from_rgb(0xE7, 0xEC, 0xF0);
/// Secondary text — `--text-2`.
pub const TEXT_2: Color32 = Color32::from_rgb(0xA3, 0xAF, 0xB8);
/// Tertiary text / placeholders — `--text-3`.
pub const TEXT_3: Color32 = Color32::from_rgb(0x83, 0x90, 0x99);
/// Success / "all good" — `--ok`.
pub const OK: Color32 = Color32::from_rgb(0x54, 0xC4, 0x95);
/// Warning / "needs rotation" — `--warn`.
pub const WARN: Color32 = Color32::from_rgb(0xE7, 0xB7, 0x5C);
/// Error / "leaked" — `--bad`.
pub const BAD: Color32 = Color32::from_rgb(0xEC, 0x84, 0x79);
/// Focus ring — `--focus`.
pub const FOCUS: Color32 = Color32::from_rgb(0x8A, 0xB4, 0xFF);
/// Alias for the focus accent, used for selection and focus strokes.
pub const ACCENT: Color32 = FOCUS;
/// Primary button fill — `.btn.primary` in the design spec: a light,
/// near-white button on the dark surface (inverted contrast, not an accent).
pub const PRIMARY_BTN: Color32 = Color32::from_rgb(0xE9, 0xEE, 0xF2);
/// Primary button text color — the dark background color.
pub const PRIMARY_BTN_TEXT: Color32 = BG;

/// Translucent success tint for chips and banners — `--ok-soft`.
pub const OK_SOFT: Color32 = Color32::from_rgba_premultiplied(21, 49, 38, 255);
/// Translucent warning tint — `--warn-soft`.
pub const WARN_SOFT: Color32 = Color32::from_rgba_premultiplied(58, 46, 23, 255);
/// Translucent error tint — `--bad-soft`.
pub const BAD_SOFT: Color32 = Color32::from_rgba_premultiplied(59, 33, 30, 255);

// ── Metrics ──────────────────────────────────────────────────────────────────

/// Corner radius for cards — `--radius` in the prototype.
pub const RADIUS: u8 = 10;
/// Corner radius for small controls (buttons, inputs).
pub const RADIUS_SM: u8 = 8;

/// Apply the dark theme to an egui context.
///
/// Called once at startup. Widget visuals are derived from the palette above
/// so `Button`, `TextEdit` and friends pick up the right colors automatically
/// instead of every call site styling itself.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut v = Visuals::dark();

    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = SURFACE;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_stroke = Stroke::new(1.0_f32, LINE);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS_SM);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SURFACE;
    w.noninteractive.weak_bg_fill = SURFACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_2);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    w.inactive.bg_fill = SURFACE;
    w.inactive.weak_bg_fill = Color32::from_rgb(0x1B, 0x21, 0x25);
    w.inactive.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.inactive.corner_radius = CornerRadius::same(RADIUS_SM);

    w.hovered.bg_fill = Color32::from_rgb(0x22, 0x2A, 0x30);
    w.hovered.weak_bg_fill = Color32::from_rgb(0x24, 0x2C, 0x33);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.hovered.corner_radius = CornerRadius::same(RADIUS_SM);

    w.active.bg_fill = Color32::from_rgb(0x2A, 0x33, 0x3B);
    w.active.weak_bg_fill = Color32::from_rgb(0x2A, 0x33, 0x3B);
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.active.corner_radius = CornerRadius::same(RADIUS_SM);

    w.open.bg_fill = SURFACE;
    w.open.weak_bg_fill = Color32::from_rgb(0x1B, 0x21, 0x25);
    w.open.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    w.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.open.corner_radius = CornerRadius::same(RADIUS_SM);

    style.visuals = v;

    // Slightly roomier than egui's default, matching the prototype's spacing.
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;

    ctx.set_style(style);
}

/// A card surface: `SURFACE` fill, hairline border, rounded corners.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(egui::Margin::symmetric(16, 14))
}

/// A small pill used for status chips ("leaked", "needs rotation").
pub fn chip(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(fg));
        });
}

/// A subdued text button (`.btn.quiet`): no border, dim label, tinted on
/// hover. Used for secondary top-bar actions like "rotate" and "export".
pub fn btn_quiet(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(12.0).color(TEXT_2))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6))
            .min_size(egui::vec2(0.0, 32.0)),
    )
    .clicked()
}

/// An outlined button (`.btn.ghost`): hairline border, full-strength label.
/// Used for "check breaches" and "lock".
pub fn btn_ghost(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(12.0).color(TEXT))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0_f32, LINE_STRONG))
            .corner_radius(CornerRadius::same(6))
            .min_size(egui::vec2(0.0, 32.0)),
    )
    .clicked()
}

/// How an inline row action should read (`.act` variants).
#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    /// Default: dim label, brightens on hover.
    Plain,
    /// Destructive: only turns red on hover, so the list stays calm.
    Danger,
    /// "Needs attention": warn tint, for the per-row rotate action.
    Warn,
    /// Emphasised (filled surface): the row's primary action, "copy".
    Copy,
}

/// A compact inline row action, matching `.act`: 26px tall, transparent,
/// 11px label, tinted background only on hover.
pub fn act(ui: &mut egui::Ui, text: &str, kind: Act) -> bool {
    let (fg, bg) = match kind {
        Act::Plain => (TEXT_2, Color32::TRANSPARENT),
        Act::Danger => (TEXT_2, Color32::TRANSPARENT),
        Act::Warn => (WARN, Color32::TRANSPARENT),
        Act::Copy => (TEXT, SURFACE_3),
    };
    let resp = ui.add(
        egui::Button::new(egui::RichText::new(text).size(11.0).color(fg))
            .fill(bg)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(5))
            .min_size(egui::vec2(0.0, 26.0)),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The primary action button (`.btn.primary`): light fill, dark text.
/// Used for "Continue" / "Save & unlock" — the one main action per screen.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> bool {
    let btn = egui::Button::new(egui::RichText::new(text).color(PRIMARY_BTN_TEXT).strong())
        .fill(PRIMARY_BTN)
        .min_size(egui::vec2(0.0, 32.0));
    ui.add(btn).clicked()
}

/// A large clickable card used for the wizard's two setup choices, matching
/// the prototype's `.choice`: bold title over a dim explanation, whole card
/// clickable. Returns `true` when clicked.
pub fn choice_card(ui: &mut egui::Ui, width: f32, title: &str, subtitle: &str) -> bool {
    let mut clicked = false;
    let response = egui::Frame::new()
        .fill(SURFACE_2)
        .stroke(egui::Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(RADIUS_SM))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.label(egui::RichText::new(title).size(13.5).strong().color(TEXT));
            ui.add_space(3.0);
            ui.label(egui::RichText::new(subtitle).size(11.5).color(TEXT_3));
        })
        .response;

    // Make the whole card interactive, with a pointer cursor and hover border.
    let response = response.interact(egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(RADIUS_SM),
            egui::Stroke::new(1.0_f32, LINE_STRONG),
            egui::StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        clicked = true;
    }
    clicked
}

/// The app's brand mark: a rounded square with three dots joined by lines,
/// matching the prototype's inline SVG logo.
pub fn brand_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.4_f32, TEXT_2);
    p.rect_stroke(
        rect,
        CornerRadius::same((size * 0.18) as u8),
        stroke,
        egui::StrokeKind::Inside,
    );
    let dot = (size * 0.07).max(1.4);
    let at = |fx: f32, fy: f32| rect.min + egui::vec2(size * fx, size * fy);
    for (fx, fy) in [(0.35, 0.35), (0.65, 0.5), (0.35, 0.65)] {
        p.circle_filled(at(fx, fy), dot, TEXT_2);
    }
    let line = egui::Stroke::new(1.2_f32, TEXT_2);
    p.line_segment([at(0.41, 0.39), at(0.58, 0.47)], line);
    p.line_segment([at(0.41, 0.61), at(0.58, 0.53)], line);
}

/// Numbered step indicator for the setup wizard, matching the design spec's
/// `.steps`: dots joined by lines; the current step is a solid white dot with
/// dark number, done steps keep a check, upcoming ones are dim outlines.
pub fn step_bar(ui: &mut egui::Ui, steps: &[&str], current: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, name) in steps.iter().enumerate() {
            if i > 0 {
                let (line, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
                let line_color = if i <= current { TEXT_3 } else { LINE };
                ui.painter().hline(
                    line.x_range(),
                    line.center().y,
                    egui::Stroke::new(1.0_f32, line_color),
                );
            }
            let (fg, fill, stroke) = if i < current {
                (TEXT_2, SURFACE_3, Stroke::new(1.0_f32, LINE_STRONG))
            } else if i == current {
                // Current step: transparent with a text-coloured ring, per
                // the updated design spec.
                (TEXT, Color32::TRANSPARENT, Stroke::new(1.0_f32, TEXT))
            } else {
                (
                    TEXT_3,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0_f32, LINE_STRONG),
                )
            };
            let (dot, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            // Radius 9 inside a 20px box leaves a 1px margin so the filled
            // "current" dot does not visually touch its label.
            ui.painter().circle_filled(dot.center(), 9.0, fill);
            if stroke != Stroke::NONE {
                ui.painter().circle_stroke(dot.center(), 9.0, stroke);
            }
            ui.painter().text(
                dot.center(),
                egui::Align2::CENTER_CENTER,
                if i < current {
                    "✓".to_string()
                } else {
                    (i + 1).to_string()
                },
                egui::FontId::monospace(10.0),
                fg,
            );
            ui.add_space(7.0);
            ui.label(
                egui::RichText::new(*name)
                    .size(11.5)
                    .color(if i == current { TEXT } else { TEXT_3 }),
            );
            ui.add_space(14.0);
        }
    });
}
