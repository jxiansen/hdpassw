//! Headless render check for the unlock screen's three states.
//!
//! Renders the real [`hdpassw::gui::App`] through `egui_kittest` and writes
//! PNGs to `/tmp/opencode/gui/`, so the states can be compared against the
//! design spec without a display server or a real vault.
//!
//! Run with:
//!
//! ```sh
//! cargo test --features gui,gui-preview --test render_states -- --nocapture
//! ```
//!
//! Development aid only: it produces images and never asserts.

#![cfg(all(feature = "gui", feature = "gui-preview"))]

use egui_kittest::Harness;
use hdpassw::gui::App;
use hdpassw::i18n::Lang;

fn shoot(app: &mut App, name: &str) {
    shoot_at(app, name, egui::vec2(940.0, 720.0));
}

fn shoot_at(app: &mut App, name: &str, size: egui::Vec2) {
    let mut harness = Harness::builder()
        .with_size(size)
        .build_ui(|ui| {
            hdpassw::gui::install_cjk_font_for_tests(ui.ctx());
            hdpassw::gui::theme::apply(ui.ctx());
            // Fill the surface and apply the same margins the real
            // CentralPanel uses, so the preview matches what users see.
            let full = ui.available_size();
            ui.painter().rect_filled(
                egui::Rect::from_min_size(ui.min_rect().min, full),
                0.0,
                hdpassw::gui::theme::BG,
            );
            ui.set_min_size(full);
            egui::Frame::new()
                .fill(hdpassw::gui::theme::BG)
                .inner_margin(egui::Margin::symmetric(20, 14))
                .show(ui, |ui| {
                    ui.set_min_width(full.x - 40.0);
                    app.draw(ui);
                });
        });
    // A busy screen keeps requesting repaints (that is the spinner working),
    // so step a fixed number of frames instead of waiting for quiescence.
    // Several frames: the wizard centers on its measured height, which needs
    // one frame to be recorded before the next can use it.
    harness.run_steps(5);
    match harness.render() {
        Ok(image) => {
            std::fs::create_dir_all("/tmp/opencode/gui").ok();
            let path = format!("/tmp/opencode/gui/{name}.png");
            image.save(&path).expect("write png");
            println!("wrote {path} ({}x{})", image.width(), image.height());
        }
        Err(e) => println!("render failed for {name}: {e}"),
    }
}

/// Point the app at a temp config dir containing a placeholder vault, so the
/// unlock screen is shown (its presence is what selects that screen).
fn unlock_app(lang: Lang) -> App {
    let tmp = std::env::temp_dir().join("hdpassw-render-states");
    let dir = tmp.join("hdpassw");
    std::fs::create_dir_all(&dir).ok();
    // Contents are irrelevant: only existence decides which screen appears.
    std::fs::write(dir.join("seed.vault"), b"version = 1\n").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
    }
    App::with_lang(lang)
}

#[test]
fn render_unlock_default() {
    let mut app = unlock_app(Lang::Zh);
    shoot(&mut app, "10-unlock-default");
}

#[test]
fn render_unlock_error() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_unlock_error();
    shoot(&mut app, "11-unlock-error");
}

#[test]
fn render_unlock_busy() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_unlock_busy();
    shoot(&mut app, "12-unlock-busy");
}

#[test]
fn render_wizard_words() {
    // The 24-word step, to check centering and the copy affordances.
    let mut app = unlock_app(Lang::Zh);
    app.preview_wizard_words();
    shoot(&mut app, "13-wizard-words");
}

#[test]
fn render_wizard_verify() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_wizard_verify();
    shoot(&mut app, "14-wizard-verify");
}

#[test]
fn render_wizard_password() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_wizard_password();
    shoot(&mut app, "15-wizard-password");
}

#[test]
fn render_wizard_restore() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_wizard_restore();
    shoot(&mut app, "16-wizard-restore");
}

#[test]
fn render_main_default() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_main_screen();
    shoot(&mut app, "20-main-default");
}

#[test]
fn render_main_narrow() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_main_screen();
    shoot_at(&mut app, "21-main-narrow", egui::vec2(720.0, 720.0));
}

#[test]
fn render_main_bump() {
    let mut app = unlock_app(Lang::Zh);
    app.preview_main_screen();
    app.preview_bump("aws.amazon.com");
    shoot(&mut app, "22-main-bump");
}
