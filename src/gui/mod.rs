//! Desktop GUI for hdpassw — dark, prototype-aligned interface.
//!
//! Unlocks the encrypted seed vault, lists known sites, generates + copies
//! passwords, walks through per-site password changes, checks Have I Been
//! Pwned, exports metadata and recovers a lost rotation counter.
//! Pure Rust, no webview — this binary links directly against the same
//! crypto/session/store/manager code as the CLI via the `hdpassw` library.
//!
//! Layout follows the OpenDesign prototype: a top bar (brand + actions), a
//! rotation banner, a collapsible add/edit panel, the account list with chips
//! and per-row actions, and modal dialogs for export / recover / delete.
//! Colors come from `theme`, which mirrors the prototype's CSS variables.
//!
//! The whole interface lives in the library (not the binary) so integration
//! tests can render it headlessly via `egui_kittest`; `src/bin/gui.rs` is a
//! thin launcher around [`run`].

pub mod theme;

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use zeroize::Zeroizing;

use crate::crypto::{self, encode::Charset};
use crate::i18n::Lang;
use crate::session::Session;
use crate::store::{self, SiteRecord, SiteStore};
use crate::{clipboard, manager};

thread_local! {
    /// Measured content height per wizard step, from the previous frame.
    ///
    /// egui has no vertical centering, so the block is offset by
    /// `(panel - content) / 2`. Heights differ a lot between steps (the
    /// 24-word grid is much taller than the two choice cards); caching per
    /// step means switching steps uses that step's own height instead of the
    /// previous one, which would make the content jump for a frame.
    static WIZARD_HEIGHTS: std::cell::RefCell<[f32; 5]> =
        const { std::cell::RefCell::new([0.0; 5]) };
}

const CHARSETS: [Charset; 5] = [
    Charset::Alpha,
    Charset::Alphanumeric,
    Charset::Full,
    Charset::Pin,
    Charset::Hex,
];

/// Which screen of the "no vault yet" setup wizard is showing.
enum SetupStage {
    /// Choose between generating a new seed or restoring an existing one.
    Choice,
    /// Freshly generated seed phrase, shown once for the user to write down.
    ShowNew {
        phrase: Zeroizing<String>,
        written_down: bool,
    },
    /// Quiz the user on a handful of random words from a freshly generated
    /// phrase, to catch transcription mistakes before they're locked in.
    Verify {
        phrase: Zeroizing<String>,
        /// 0-indexed positions into the word list, ascending.
        positions: Vec<usize>,
        answers: Vec<String>,
        error: String,
    },
    /// Free-text entry for restoring an existing seed phrase.
    Restore { input: String, error: String },
    /// Set the vault password that will encrypt `phrase` on disk, plus the
    /// optional BIP39 passphrase used for password derivation.
    SetPassword {
        phrase: Zeroizing<String>,
        pw1: Zeroizing<String>,
        pw2: Zeroizing<String>,
        bip39: Zeroizing<String>,
        /// The screen to return to on "Back" — whichever led here.
        previous: Box<SetupStage>,
    },
}

struct SetupState {
    stage: SetupStage,
}

/// BIP39 word suggestions for a partially typed prefix.
fn word_suggestions(prefix: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return Vec::new();
    }
    bip39::Language::English
        .word_list()
        .iter()
        .filter(|w| w.starts_with(prefix))
        .take(6)
        .copied()
        .collect()
}

/// Render clickable completion chips for `prefix`. Returns the picked word.
fn suggestion_chips(ui: &mut egui::Ui, prefix: &str) -> Option<&'static str> {
    let candidates = word_suggestions(prefix);
    if candidates.is_empty() {
        return None;
    }
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for w in candidates {
            if ui.small_button(w).clicked() {
                picked = Some(w);
            }
        }
    });
    picked
}

/// Load a CJK-capable font so Chinese labels don't render as tofu.
///
/// egui's built-in fonts are Latin-only, so a system CJK face is borrowed at
/// startup. Windows paths are resolved through `%WINDIR%` rather than being
/// hardcoded to `C:` — Windows can be installed on any drive.
pub fn install_cjk_font(ctx: &egui::Context) {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
        let fonts = std::path::Path::new(&windir).join("Fonts");
        // Ordered by coverage and how likely the face is present:
        // Microsoft YaHei (Win7+), DengXian (Win10+), SimHei, SimSun.
        for name in [
            "msyh.ttc",
            "msyh.ttf",
            "Deng.ttf",
            "simhei.ttf",
            "simsun.ttc",
            "SimSunExtG.ttf",
        ] {
            candidates.push(fonts.join(name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ] {
            candidates.push(std::path::PathBuf::from(path));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for path in [
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
        ] {
            candidates.push(std::path::PathBuf::from(path));
        }
    }

    let Some(data) = candidates.iter().find_map(|p| fs::read(p).ok()) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(data).into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Path of the saved window geometry, next to the metadata file.
fn window_state_path() -> PathBuf {
    store::default_db_path()
        .parent()
        .map(|p| p.join("window"))
        .unwrap_or_else(|| PathBuf::from("window"))
}

/// Restore the last window size, if one was saved.
fn load_window_size() -> Option<egui::Vec2> {
    let text = fs::read_to_string(window_state_path()).ok()?;
    let mut parts = text.split_whitespace();
    let w: f32 = parts.next()?.parse().ok()?;
    let h: f32 = parts.next()?.parse().ok()?;
    // Ignore nonsense from a corrupt file.
    (w >= 480.0 && h >= 360.0 && w <= 10000.0 && h <= 10000.0).then(|| egui::vec2(w, h))
}

/// Persist the window size. Best-effort: a read-only config dir must not
/// break the app.
fn save_window_size(size: egui::Vec2) {
    let path = window_state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, format!("{:.0} {:.0}\n", size.x, size.y));
}

/// Load the CJK font on demand, at most once per process.
///
/// The font file (Microsoft YaHei and friends) is ~19 MB and egui keeps the
/// whole thing in memory plus a glyph atlas. English-only users never need
/// it, so it is loaded when the UI language is Chinese and again if the user
/// switches to Chinese at runtime.
pub fn ensure_cjk_font(ctx: &egui::Context, lang: Lang) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOADED: AtomicBool = AtomicBool::new(false);
    if lang != Lang::Zh || LOADED.load(Ordering::Relaxed) {
        return;
    }
    install_cjk_font(ctx);
    LOADED.store(true, Ordering::Relaxed);
}

/// Launch the desktop GUI. Called by the `hdpassw-gui` binary.
pub fn run() -> eframe::Result {
    let lang = Lang::current();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(load_window_size().unwrap_or(egui::vec2(940.0, 720.0)))
            .with_min_inner_size([720.0, 560.0])
            .with_title(lang.window_title()),
        ..Default::default()
    };

    eframe::run_native(
        "hdpassw",
        options,
        Box::new(|cc| {
            ensure_cjk_font(&cc.egui_ctx, lang);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::with_lang(lang)))
        }),
    )
}

/// A long-running operation running off the UI thread.
///
/// The UI stays responsive while these run and shows a busy indicator; the
/// result is collected by [`App::poll_task`] on a later frame.
enum Task {
    /// Decrypt the vault and derive the master key (scrypt, ~1s).
    Unlock(std::sync::mpsc::Receiver<Result<Session, String>>),
    /// Create the vault: encrypt the phrase (Argon2id) and unlock (scrypt).
    SaveVault(std::sync::mpsc::Receiver<Result<Session, String>>),
    /// Check every account against Have I Been Pwned (one HTTPS request each).
    Pwned(std::sync::mpsc::Receiver<Result<Vec<(String, u64)>, String>>),
}

/// One in-flight per-site password change.
#[derive(Clone)]
struct Bump {
    site: String,
    old: u32,
    new: u32,
    /// 0 = old password not yet copied, 1 = old copied, new one pending.
    step: u8,
}

/// Modal dialogs. Only one can be open at a time.
#[derive(Default)]
enum Dialog {
    #[default]
    None,
    /// Confirm deleting a site record.
    Delete(String),
    /// Export the metadata file as TOML.
    Export { text: String, copied: bool },
    /// Recover a lost rotation counter by scanning verifiers.
    Recover(RecoverState),
}

#[derive(Default)]
struct RecoverState {
    site: String,
    user: String,
    verifier: String,
    result: Option<u32>,
    error: String,
    scanning: bool,
}

pub struct App {
    /// UI language.
    lang: Lang,
    /// Vault password entered by the user. Zeroed on drop and after use.
    password: Zeroizing<String>,
    /// Optional BIP39 passphrase entered on the unlock screen.
    bip39: Zeroizing<String>,
    /// Live session holding the derived master key, once unlocked.
    session: Option<Session>,
    /// Site metadata, loaded after unlock.
    sites: SiteStore,
    /// Where `sites` is persisted.
    db_path: PathBuf,
    /// Status line shown at the bottom of the window.
    status: String,
    /// Whether `status` represents an error (shown in red) or success (green).
    status_is_error: bool,

    /// First-run wizard state, active when no vault exists yet.
    setup: Option<SetupState>,
    /// Set when the user explicitly asks to create or restore a vault from the
    /// unlock screen, even though a vault file already exists. Lets them
    /// replace a forgotten vault without deleting the file by hand.
    force_setup: bool,
    /// Inline error under the vault password field on the unlock screen.
    /// `None` means no error; set when decryption fails.
    unlock_error: Option<String>,
    /// A long-running operation in flight (scrypt unlock, vault creation,
    /// breach check). Each one blocks for hundreds of milliseconds to
    /// seconds — scrypt needs ~1s and the breach check is one HTTPS request
    /// per account — so they all run on a worker thread and report back
    /// through a channel that [`App::poll_task`] drains each frame.
    task: Option<Task>,

    /// Last window size written to disk, so we only persist on real changes.
    saved_window: Option<egui::Vec2>,


    /// In-progress password change for one site.
    bump: Option<Bump>,

    /// "Add / edit site" form state.
    show_add_form: bool,
    /// Site being edited, or `None` when adding a new one.
    editing: Option<String>,
    new_site: String,
    new_user: String,
    new_counter: String,
    new_length: String,
    new_charset: Charset,
    new_notes: String,

    /// Live search filter for the account list.
    query: String,
    /// Sites whose password was found in a breach by the last check.
    leaked: std::collections::BTreeSet<String>,
    /// The open dialog, if any.
    dialog: Dialog,
}

impl Default for App {
    fn default() -> Self {
        Self::with_lang(Lang::current())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.persist_window_size(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| self.draw(ui));
    }
}

impl App {
    pub fn with_lang(lang: Lang) -> Self {
        Self {
            lang,
            password: Zeroizing::default(),
            bip39: Zeroizing::default(),
            session: None,
            sites: SiteStore::default(),
            db_path: store::default_db_path(),
            status: String::new(),
            status_is_error: false,
            setup: None,
            force_setup: false,
            unlock_error: None,
            task: None,
            saved_window: None,
            bump: None,
            show_add_form: false,
            editing: None,
            new_site: String::new(),
            new_user: String::new(),
            new_counter: "1".to_string(),
            new_length: "32".to_string(),
            new_charset: Charset::default(),
            new_notes: String::new(),
            query: String::new(),
            leaked: Default::default(),
            dialog: Dialog::None,
        }
    }

    /// Whether a record still needs its password changed: behind the global
    /// rotation level, or found in a breach.
    fn needs_rotation(&self, site: &str, record: &SiteRecord) -> bool {
        record.counter < self.sites.rotation || self.leaked.contains(site)
    }

    // ── Preview helpers (headless render checks) ─────────────────────────────
    //
    // Only compiled with the `gui-preview` feature so they never ship in the
    // release binary.
    //
    // These exist so the interface can be rendered to an image without a
    // display server or real vault, for visual verification against the
    // design spec in OpenDesign. They never run in the real app.

    /// Show the unlock screen in its error state.
    #[cfg(feature = "gui-preview")]
    pub fn preview_unlock_error(&mut self) {
        self.unlock_error = Some(self.lang.unlock_wrong_pw().to_string());
    }

    /// Show the wizard's 24-word step with a fixed sample phrase.
    #[cfg(feature = "gui-preview")]
    pub fn preview_wizard_words(&mut self) {
        const PHRASE: &str = "inflict general excuse memory idle virus visit page pair \
approve raven cigar silent employ evoke skill solve accident shiver library acoustic \
hope faith genuine";
        self.setup = Some(SetupState {
            stage: SetupStage::ShowNew {
                phrase: Zeroizing::new(PHRASE.to_string()),
                written_down: true,
            },
        });
        self.force_setup = true;
    }

    /// Populate the main screen with demo accounts, for render checks.
    #[cfg(feature = "gui-preview")]
    pub fn preview_main_screen(&mut self) {
        use chrono::NaiveDate;
        let today = chrono::Local::now().date_naive();
        let mut store = SiteStore {
            rotation: 7,
            ..Default::default()
        };
        let rows = [
            ("github.com", "jakob", 7u32, "a3f9", Some("主账号，已开启两步验证")),
            ("aws.amazon.com", "jakob@ops", 6, "e05d", None),
            ("linkedin.com", "jakob.h", 5, "7c1e", None),
            ("figma.com", "jakob", 7, "9a47", None),
            ("proton.me", "jakob", 7, "4b82", Some("加密邮箱")),
            ("adobe.com", "jakob", 7, "2d6b", Some("仅字母，旧账号")),
            ("mail.google.com", "jakob", 7, "c8f3", None),
        ];
        for (site, user, counter, verifier, notes) in rows {
            store.sites.insert(
                site.to_string(),
                SiteRecord {
                    user: user.to_string(),
                    counter,
                    length: 32,
                    charset: "full".to_string(),
                    verifier: verifier.to_string(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    modified: today,
                    notes: notes.map(str::to_string),
                },
            );
        }
        self.sites = store;
        self.leaked.insert("linkedin.com".to_string());
        self.unlock_for_preview();
    }

    /// Derive a throwaway session so previews can show unlocked screens.
    #[cfg(feature = "gui-preview")]
    fn unlock_for_preview(&mut self) {
        const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon \
                                     abandon abandon abandon abandon abandon about";
        if let Ok(session) = Session::unlock(TEST_MNEMONIC, Some(String::new())) {
            self.session = Some(session);
        }
    }

    /// Put a site into the two-step rotate hand-off, for render checks.
    #[cfg(feature = "gui-preview")]
    pub fn preview_bump(&mut self, site: &str) {
        if let Some(record) = self.sites.sites.get(site) {
            let old = record.counter;
            let new = if old < self.sites.rotation {
                self.sites.rotation
            } else {
                old + 1
            };
            self.bump = Some(Bump {
                site: site.to_string(),
                old,
                new,
                step: 0,
            });
        }
    }

    /// Show the wizard's verification step.
    #[cfg(feature = "gui-preview")]
    pub fn preview_wizard_verify(&mut self) {
        const PHRASE: &str = "inflict general excuse memory idle virus visit page pair \
approve raven cigar silent employ evoke skill solve accident shiver library acoustic \
hope faith genuine";
        self.setup = Some(SetupState {
            stage: SetupStage::Verify {
                phrase: Zeroizing::new(PHRASE.to_string()),
                positions: vec![3, 6, 11, 18, 21, 14],
                answers: vec![String::new(); 6],
                error: String::new(),
            },
        });
        self.force_setup = true;
    }

    /// Show the wizard's vault-password step.
    #[cfg(feature = "gui-preview")]
    pub fn preview_wizard_password(&mut self) {
        const PHRASE: &str = "abandon abandon abandon abandon abandon abandon \
abandon abandon abandon abandon abandon about";
        self.setup = Some(SetupState {
            stage: SetupStage::SetPassword {
                phrase: Zeroizing::new(PHRASE.to_string()),
                pw1: Zeroizing::default(),
                pw2: Zeroizing::default(),
                bip39: Zeroizing::default(),
                previous: Box::new(SetupStage::Choice),
            },
        });
        self.force_setup = true;
    }

    /// Show the wizard's restore-words step.
    #[cfg(feature = "gui-preview")]
    pub fn preview_wizard_restore(&mut self) {
        self.setup = Some(SetupState {
            stage: SetupStage::Restore {
                input: String::new(),
                error: String::new(),
            },
        });
        self.force_setup = true;
    }

    /// Show the unlock screen in its "unlocking…" state.
    #[cfg(feature = "gui-preview")]
    pub fn preview_unlock_busy(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<Result<Session, String>>();
        // Leak the sender so the channel stays open and `try_recv` keeps
        // reporting `Empty` — exactly what a real in-flight unlock looks like.
        std::mem::forget(tx);
        self.task = Some(Task::Unlock(rx));
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = text.into();
        self.status_is_error = is_error;
    }

    /// Draw one frame of the interface into `ui`.
    ///
    /// Kept separate from [`eframe::App::update`] so headless render tests can
    /// drive the exact same code without an eframe context.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        self.poll_task(ui.ctx());
        if self.session.is_none() {
            self.unlock_screen(ui);
        } else {
            self.main_screen(ui);
        }
        self.show_dialogs(ui.ctx());
        // The main screen draws its own status strip (it must reserve space
        // below the scrolling list); the unlock screen just appends one.
        if self.session.is_none() && !self.status.is_empty() {
            self.status_bar(ui);
        }
    }

    // ── Unlock screen ────────────────────────────────────────────────────────

    fn unlock_screen(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
        let vault = store::vault_path();

        // A missing vault means first run: show the wizard instead of a
        // password prompt the user could never satisfy. The same screen is
        // reachable on demand via "create or restore" when a vault does exist.
        if self.force_setup || !vault.exists() {
            self.setup_wizard(ui, &vault);
            return;
        }

        ui.vertical_centered(|ui| {
            // Center the unlock card vertically. A fixed leading space looks
            // off on tall windows, where the card would float near the top.
            let content_height = 400.0;
            let top_space = ((ui.available_height() - content_height) / 2.0).max(40.0);
            ui.add_space(top_space);

            // Brand mark, mirroring the design spec's lock card. Centered by
            // measuring the row's real width rather than guessing a constant —
            // `ui.horizontal` fills the available width, so a fixed offset
            // drifts as soon as the font or zoom changes.
            const MARK: f32 = 26.0;
            const GAP: f32 = 9.0;
            let brand_text_width = ui
                .painter()
                .layout_no_wrap(
                    "hdpassw".to_owned(),
                    egui::FontId::proportional(17.0),
                    theme::TEXT,
                )
                .size()
                .x;
            let brand_width = MARK + GAP + brand_text_width;
            ui.allocate_ui_with_layout(
                egui::vec2(brand_width, MARK),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    theme::brand_mark(ui, MARK);
                    ui.label(
                        egui::RichText::new("hdpassw")
                            .size(17.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                },
            );
            ui.add_space(22.0);

            ui.allocate_ui_with_layout(
                egui::vec2(380.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let busy = self.busy();
                    ui.label(
                        egui::RichText::new(l.unlock_title())
                            .size(22.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(l.unlock_sub2())
                            .size(12.5)
                            .color(theme::TEXT_2),
                    );
                    if busy {
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(l.unlocking_hint())
                                .size(11.5)
                                .color(theme::TEXT_3),
                        );
                    }
                    ui.add_space(20.0);

                    let mut pw_had_focus = false;
                    theme::card().show(ui, |ui| {
                        ui.add_enabled_ui(!busy, |ui| {
                            ui.label(
                                egui::RichText::new(l.vault_password())
                                    .size(12.0)
                                    .color(theme::TEXT_2),
                            );
                            let pw_response = ui.add(
                                egui::TextEdit::singleline(&mut *self.password)
                                    .password(true)
                                    .desired_width(f32::INFINITY),
                            );
                            // Failed unlock: outline the field itself, as the
                            // design spec does with aria-invalid.
                            if self.unlock_error.is_some() && !busy {
                                ui.painter().rect_stroke(
                                    pw_response.rect,
                                    egui::CornerRadius::same(6),
                                    egui::Stroke::new(1.0_f32, theme::BAD),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            pw_had_focus = pw_response.has_focus();
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(l.vault_pw_hint())
                                    .size(11.0)
                                    .color(theme::TEXT_3),
                            );
                        });

                        // Error sits directly under the password field, as in
                        // the design spec, and stays readable while a retry is
                        // in flight — so it lives outside the disabled block.
                        if let Some(err) = &self.unlock_error {
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new(err).size(11.5).color(theme::BAD));
                        }

                        ui.add_enabled_ui(!busy, |ui| {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(l.bip39_passphrase())
                                    .size(12.0)
                                    .color(theme::TEXT_2),
                            );
                            let pass_response = ui.add(
                                egui::TextEdit::singleline(&mut *self.bip39)
                                    .password(true)
                                    .desired_width(f32::INFINITY),
                            );
                            pw_had_focus = pw_had_focus || pass_response.has_focus();
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(l.bip39_hint_short())
                                    .size(11.0)
                                    .color(theme::TEXT_3),
                            );

                            ui.add_space(18.0);
                            let unlock = ui
                                .add_enabled_ui(!busy, |ui| {
                                    ui.add_sized(
                                        [ui.available_width(), 42.0],
                                        egui::Button::new(
                                            egui::RichText::new(if busy {
                                                l.unlocking()
                                            } else {
                                                l.unlock()
                                            })
                                            .color(theme::PRIMARY_BTN_TEXT)
                                            .strong(),
                                        )
                                        .fill(theme::PRIMARY_BTN),
                                    )
                                })
                                .inner
                                .clicked();

                            // Enter submits from either field, and nowhere else.
                            let enter = !busy
                                && ui.input(|i| {
                                    i.key_pressed(egui::Key::Enter) && i.modifiers.is_none()
                                })
                                && pw_had_focus;
                            if unlock || enter {
                                self.try_unlock(&vault);
                            }
                        }); // end add_enabled_ui(!busy)

                        ui.add_space(10.0);
                        // Both links on one row, pushed to the edges, as in
                        // the design spec's `.lock-links`.
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(l.recover_link())
                                                    .size(11.5)
                                                    .underline()
                                                    .color(theme::TEXT_3),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.dialog =
                                            Dialog::Recover(RecoverState::default());
                                    }
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(l.create_or_recover())
                                                    .size(11.5)
                                                    .underline()
                                                    .color(theme::TEXT_3),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        // Explicit request to create or
                                        // restore, even though a vault exists.
                                        self.force_setup = true;
                                    }
                                },
                            );
                        });
                    });
                },
            );
        });
    }

    // ── Setup wizard ─────────────────────────────────────────────────────────

    /// First-run wizard: create a new seed phrase or restore an existing
    /// one, then encrypt it into a vault file — all without touching the CLI.
    fn setup_wizard(&mut self, ui: &mut egui::Ui, vault: &std::path::Path) {
        let l = self.lang;
        let setup = self.setup.get_or_insert(SetupState {
            stage: SetupStage::Choice,
        });

        let mut next_stage: Option<SetupStage> = None;
        let mut finish: Option<(Zeroizing<String>, Zeroizing<String>, Zeroizing<String>)> = None;
        let mut back_to_unlock = false;
        // Copy feedback is buffered here and applied after the borrowed
        // `setup` state is released.
        let mut copy_status: Option<String> = None;
        // Read once, outside the closures: `self` is borrowed by `setup`.
        let self_busy = self.task.is_some();

        // `allocate_ui_with_layout` reserves a fixed-width block and lets the
        // parent's centered layout position it — unlike `set_max_width`,
        // which keeps the left edge fixed and just shrinks the right side.
        // The outer `vertical_centered` is what actually centers the block
        // horizontally; `allocate_ui_with_layout` alone anchors it left.
        // Two-pass vertical centering: the wizard's height varies a lot
        // between steps (the 24-word grid is much taller than the two choice
        // cards), so a hardcoded estimate misplaces it. Measuring the real
        // content height first and centering on that keeps every step
        // balanced without guessing.
        let stage_key = match &setup.stage {
            SetupStage::Choice => 0,
            SetupStage::ShowNew { .. } => 1,
            SetupStage::Verify { .. } => 2,
            SetupStage::Restore { .. } => 3,
            SetupStage::SetPassword { .. } => 4,
        };
        let content_height = WIZARD_HEIGHTS.with(|h| h.borrow()[stage_key]).max(240.0);
        // Capture the full panel height *before* entering the centered block:
        // inside it, `available_height` already reflects the space consumed by
        // the previous frame's leading gap, which would shrink the gap each
        // frame instead of settling.
        let panel_height = ui.available_height();
        // Scroll, because the 24-word step is taller than a short window —
        // without this the footer buttons fall outside the viewport.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
        ui.vertical_centered(|ui| {
            let top_space = ((panel_height - content_height) / 2.0).max(24.0);
            ui.add_space(top_space);
            let block_top = ui.cursor().top();
            ui.allocate_ui_with_layout(
                egui::vec2(620.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // Step indicator, mirroring the prototype's `.wiz-steps`.
                    // Restoring skips the "write down" step, so the labels
                    // differ between the two paths.
                    let (steps, current): (&[&str], usize) = match &setup.stage {
                        SetupStage::Choice => (
                            &[
                                l.step_choose(),
                                l.step_write(),
                                l.step_verify(),
                                l.step_password(),
                            ],
                            0,
                        ),
                        SetupStage::ShowNew { .. } => (
                            &[
                                l.step_choose(),
                                l.step_write(),
                                l.step_verify(),
                                l.step_password(),
                            ],
                            1,
                        ),
                        SetupStage::Verify { .. } => (
                            &[
                                l.step_choose(),
                                l.step_write(),
                                l.step_verify(),
                                l.step_password(),
                            ],
                            2,
                        ),
                        SetupStage::SetPassword { previous, .. } => {
                            if matches!(**previous, SetupStage::Restore { .. }) {
                                (&[l.step_choose(), l.step_words(), l.step_password()], 2)
                            } else {
                                (
                                    &[
                                        l.step_choose(),
                                        l.step_write(),
                                        l.step_verify(),
                                        l.step_password(),
                                    ],
                                    3,
                                )
                            }
                        }
                        SetupStage::Restore { .. } => {
                            (&[l.step_choose(), l.step_words(), l.step_password()], 1)
                        }
                    };
                    theme::step_bar(ui, steps, current);
                    ui.add_space(20.0);

                    match &mut setup.stage {
                        SetupStage::Choice => {
                            ui.label(
                                egui::RichText::new(l.wizard_create_title())
                                    .size(21.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(l.wizard_create_sub())
                                    .size(12.5)
                                    .color(theme::TEXT_2),
                            );
                            ui.add_space(20.0);

                            // Two large choice cards, as in the prototype.
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                let w = (ui.available_width() - 12.0) / 2.0;
                                if theme::choice_card(
                                    ui,
                                    w,
                                    l.choice_new_title(),
                                    l.choice_new_sub(),
                                ) {
                                    let phrase = Zeroizing::new(crypto::generate().to_string());
                                    next_stage = Some(SetupStage::ShowNew {
                                        phrase,
                                        written_down: false,
                                    });
                                }
                                if theme::choice_card(
                                    ui,
                                    w,
                                    l.choice_restore_title(),
                                    l.choice_restore_sub(),
                                ) {
                                    next_stage = Some(SetupStage::Restore {
                                        input: String::new(),
                                        error: String::new(),
                                    });
                                }
                            });

                            // Only offer a way back when a vault already
                            // exists — on true first run there is nothing to
                            // return to.
                            if vault.exists() {
                                ui.add_space(14.0);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(l.wiz_back_to_unlock())
                                                .size(11.5)
                                                .underline()
                                                .color(theme::TEXT_3),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    back_to_unlock = true;
                                }
                            }
                        }
                        SetupStage::ShowNew {
                            phrase,
                            written_down,
                        } => {
                            let words: Vec<&str> = phrase.split_whitespace().collect();
                            ui.label(
                                egui::RichText::new(l.seed_step_title())
                                    .size(21.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(l.write_it_down())
                                    .size(12.5)
                                    .color(theme::TEXT_2),
                            );
                            ui.add_space(18.0);

                            // Three-column word grid inside a bordered box,
                            // matching the design spec's `.seed-box`. The whole
                            // phrase can be copied at once, and clicking any
                            // word copies just that word — useful when a site
                            // asks for "the 7th word".
                            egui::Frame::new()
                                .fill(theme::SURFACE_2)
                                .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM))
                                .inner_margin(egui::Margin::symmetric(14, 12))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    egui::Grid::new("seed_words_grid")
                                        .num_columns(3)
                                        .min_col_width(150.0)
                                        .spacing([12.0, 4.0])
                                        .show(ui, |ui| {
                                            for (i, w) in words.iter().enumerate() {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 7.0;
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "{:02}",
                                                            i + 1
                                                        ))
                                                        .family(egui::FontFamily::Monospace)
                                                        .size(9.5)
                                                        .color(theme::TEXT_3),
                                                    );
                                                    let resp = ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(*w)
                                                                .family(
                                                                    egui::FontFamily::Monospace,
                                                                )
                                                                .size(12.5)
                                                                .color(theme::TEXT),
                                                        )
                                                        .sense(egui::Sense::click()),
                                                    );
                                                    if resp.hovered() {
                                                        ui.ctx().set_cursor_icon(
                                                            egui::CursorIcon::PointingHand,
                                                        );
                                                    }
                                                    if resp.clicked() {
                                                        let _ = clipboard::copy_and_clear(w, 30);
                                                        copy_status = Some(format!(
                                                            "✓ {} {w}",
                                                            l.copied_word()
                                                        ));
                                                    }
                                                });
                                                if (i + 1) % 3 == 0 {
                                                    ui.end_row();
                                                }
                                            }
                                        });

                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        if ui.button(l.copy_all_words()).clicked() {
                                            let joined = words.join(" ");
                                            let _ = clipboard::copy_and_clear(&joined, 60);
                                            copy_status = Some(l.copied_words().to_string());
                                        }
                                        ui.label(
                                            egui::RichText::new(l.copy_words_hint())
                                                .size(10.5)
                                                .color(theme::TEXT_3),
                                        );
                                    });
                                    ui.add_space(6.0);
                                    // Inline note rather than a second
                                    // warning strip: the backup warning below
                                    // already carries the coloured banner.
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(l.copy_words_warn())
                                                .size(10.5)
                                                .color(theme::TEXT_3),
                                        )
                                        .wrap(),
                                    );
                                });

                            ui.add_space(16.0);
                            // Warning strip from the design spec: warn-soft
                            // background, warn text.
                            egui::Frame::new()
                                .fill(theme::WARN_SOFT)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM))
                                .inner_margin(egui::Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(l.backup_warn())
                                            .size(11.5)
                                            .color(theme::WARN),
                                    );
                                });
                            ui.add_space(14.0);
                            ui.checkbox(written_down, l.i_wrote_it_down());
                            ui.add_space(16.0);

                            // Footer row: border-top, buttons right-aligned.
                            ui.separator();
                            ui.add_space(10.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_enabled_ui(*written_down, |ui| {
                                        if theme::primary_button(ui, l.wiz_next()) {
                                            let word_count = words.len();
                                            let quiz_size = 6.min(word_count);
                                            let mut positions = rand::seq::index::sample(
                                                &mut rand::thread_rng(),
                                                word_count,
                                                quiz_size,
                                            )
                                            .into_vec();
                                            positions.sort_unstable();

                                            next_stage = Some(SetupStage::Verify {
                                                phrase: phrase.clone(),
                                                positions,
                                                answers: vec![String::new(); quiz_size],
                                                error: String::new(),
                                            });
                                        }
                                    });
                                    if ui.button(l.back()).clicked() {
                                        next_stage = Some(SetupStage::Choice);
                                    }
                                },
                            );
                        }
                        SetupStage::Verify {
                            phrase,
                            positions,
                            answers,
                            error,
                        } => {
                            let words: Vec<&str> = phrase.split_whitespace().collect();
                            ui.label(
                                egui::RichText::new(l.quiz_title())
                                    .size(21.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(l.quick_check())
                                    .size(12.5)
                                    .color(theme::TEXT_2),
                            );
                            ui.add_space(18.0);

                            const LABEL_WIDTH: f32 = 90.0;
                            const INPUT_WIDTH: f32 = 240.0;
                            const SPACING: f32 = 12.0;
                            let grid_width = LABEL_WIDTH + SPACING + INPUT_WIDTH;

                            ui.allocate_ui_with_layout(
                                egui::vec2(grid_width, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    theme::card().show(ui, |ui| {
                                        egui::Grid::new("verify_grid")
                                            .num_columns(2)
                                            .min_col_width(LABEL_WIDTH)
                                            .spacing([SPACING, 6.0])
                                            .show(ui, |ui| {
                                                for (slot, &word_idx) in
                                                    positions.iter().enumerate()
                                                {
                                                    ui.label(format!(
                                                        "{}{}{}",
                                                        l.word_no(),
                                                        word_idx + 1,
                                                        l.word_suffix()
                                                    ));
                                                    ui.vertical(|ui| {
                                                        ui.add(
                                                            egui::TextEdit::singleline(
                                                                &mut answers[slot],
                                                            )
                                                            .desired_width(INPUT_WIDTH),
                                                        );
                                                        let lower =
                                                            answers[slot].trim().to_lowercase();
                                                        if let Some(picked) =
                                                            suggestion_chips(ui, &lower)
                                                        {
                                                            answers[slot] = picked.to_string();
                                                        }
                                                    });
                                                    ui.end_row();
                                                }
                                            });
                                    });
                                },
                            );

                            ui.add_space(8.0);
                            if !error.is_empty() {
                                ui.label(egui::RichText::new(error.as_str()).color(theme::BAD));
                                ui.add_space(6.0);
                            }

                            ui.separator();
                            ui.add_space(10.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::primary_button(ui, l.wiz_next()) {
                                        let all_correct = positions.iter().zip(answers.iter()).all(
                                            |(&idx, ans)| ans.trim().to_lowercase() == words[idx],
                                        );

                                        if all_correct {
                                            next_stage = Some(SetupStage::SetPassword {
                                                phrase: phrase.clone(),
                                                pw1: Zeroizing::default(),
                                                pw2: Zeroizing::default(),
                                                bip39: Zeroizing::default(),
                                                previous: Box::new(SetupStage::Verify {
                                                    phrase: phrase.clone(),
                                                    positions: positions.clone(),
                                                    answers: answers.clone(),
                                                    error: String::new(),
                                                }),
                                            });
                                        } else {
                                            *error = l.quiz_mismatch().to_string();
                                        }
                                    }
                                    if ui.button(l.back()).clicked() {
                                        next_stage = Some(SetupStage::ShowNew {
                                            phrase: phrase.clone(),
                                            written_down: true,
                                        });
                                    }
                                },
                            );
                        }
                        SetupStage::Restore { input, error } => {
                            ui.label(
                                egui::RichText::new(l.choice_restore_title())
                                    .size(21.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(l.enter_all_words())
                                    .size(12.5)
                                    .color(theme::TEXT_2),
                            );
                            ui.add_space(14.0);
                            ui.add(
                                egui::TextEdit::multiline(input)
                                    .desired_rows(4)
                                    .desired_width(ui.available_width())
                                    .font(egui::TextStyle::Monospace),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(l.restore_hint())
                                    .size(11.0)
                                    .color(theme::TEXT_3),
                            );

                            // Suggest completions for whichever word is still
                            // being typed (the last one, as long as the user
                            // hasn't already finished it with a trailing space).
                            let ends_with_space =
                                input.chars().last().is_some_and(char::is_whitespace);
                            if !ends_with_space {
                                let last_word = input.split_whitespace().last().unwrap_or("");
                                let lower = last_word.to_lowercase();
                                if let Some(picked) = suggestion_chips(ui, &lower) {
                                    let mut words: Vec<String> =
                                        input.split_whitespace().map(str::to_lowercase).collect();
                                    match words.last_mut() {
                                        Some(last) => *last = picked.to_string(),
                                        None => words.push(picked.to_string()),
                                    }
                                    *input = words.join(" ");
                                    input.push(' ');
                                }
                            }
                            ui.add_space(8.0);

                            if !error.is_empty() {
                                ui.label(egui::RichText::new(error.as_str()).color(theme::BAD));
                                ui.add_space(6.0);
                            }

                            ui.separator();
                            ui.add_space(10.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::primary_button(ui, l.wiz_next()) {
                                        let normalized = input
                                            .split_whitespace()
                                            .map(|w| w.to_lowercase())
                                            .collect::<Vec<_>>()
                                            .join(" ");

                                        match crypto::parse(&normalized) {
                                            Ok(_) => {
                                                next_stage = Some(SetupStage::SetPassword {
                                                    phrase: Zeroizing::new(normalized),
                                                    pw1: Zeroizing::default(),
                                                    pw2: Zeroizing::default(),
                                                    bip39: Zeroizing::default(),
                                                    previous: Box::new(SetupStage::Restore {
                                                        input: input.clone(),
                                                        error: String::new(),
                                                    }),
                                                });
                                            }
                                            Err(e) => *error = format!("✗ {e}"),
                                        }
                                    }
                                    if ui.button(l.back()).clicked() {
                                        next_stage = Some(SetupStage::Choice);
                                    }
                                },
                            );
                        }
                        SetupStage::SetPassword {
                            phrase,
                            pw1,
                            pw2,
                            bip39,
                            previous,
                        } => {
                            ui.label(
                                egui::RichText::new(l.choose_vault_pw())
                                    .size(21.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(l.vault_pw_sub())
                                    .size(12.5)
                                    .color(theme::TEXT_2),
                            );
                            ui.add_space(16.0);

                            ui.allocate_ui_with_layout(
                                egui::vec2(380.0, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    theme::card().show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(l.password())
                                                .size(12.0)
                                                .color(theme::TEXT_2),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut **pw1)
                                                .password(true)
                                                .desired_width(f32::INFINITY),
                                        );
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new(l.confirm_password())
                                                .size(12.0)
                                                .color(theme::TEXT_2),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut **pw2)
                                                .password(true)
                                                .desired_width(f32::INFINITY),
                                        );

                                        ui.add_space(14.0);
                                        ui.label(
                                            egui::RichText::new(l.bip39_passphrase())
                                                .size(12.0)
                                                .color(theme::TEXT_2),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut **bip39)
                                                .password(true)
                                                .desired_width(f32::INFINITY),
                                        );
                                        ui.add_space(3.0);
                                        ui.label(
                                            egui::RichText::new(l.bip39_hint())
                                                .size(10.5)
                                                .color(theme::TEXT_3),
                                        );
                                    });
                                },
                            );

                            ui.separator();
                            ui.add_space(10.0);
                            let mut save_clicked = false;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_enabled_ui(!self_busy, |ui| {
                                        if theme::primary_button(
                                            ui,
                                            if self_busy {
                                                l.saving()
                                            } else if matches!(**previous, SetupStage::Restore { .. }) {
                                                l.save_vault_unlock()
                                            } else {
                                                l.create_vault_unlock()
                                            },
                                        ) {
                                            save_clicked = true;
                                        }
                                    });
                                    ui.add_enabled_ui(!self_busy, |ui| {
                                        if ui.button(l.back()).clicked() {
                                            next_stage = Some(std::mem::replace(
                                                previous.as_mut(),
                                                SetupStage::Choice,
                                            ));
                                        }
                                    });
                                },
                            );

                            if save_clicked {
                                if pw1.is_empty() {
                                    self.status = l.pw_empty().to_string();
                                    self.status_is_error = true;
                                } else if pw1 != pw2 {
                                    self.status = l.pw_mismatch().to_string();
                                    self.status_is_error = true;
                                } else {
                                    finish = Some((phrase.clone(), pw1.clone(), bip39.clone()));
                                }
                            }
                        }
                    }
                },
            );
            // Remember the real height so the next frame centers on it.
            let used = ui.cursor().top() - block_top;
            if used > 1.0 {
                WIZARD_HEIGHTS.with(|h| h.borrow_mut()[stage_key] = used);
            }
        });
            });

        if let Some(stage) = next_stage {
            setup.stage = stage;
        }

        if let Some(msg) = copy_status {
            self.set_status(msg, false);
        }

        if back_to_unlock {
            self.force_setup = false;
            self.setup = None;
            return;
        }

        if let Some((phrase, password, bip39)) = finish {
            self.complete_setup(vault, &phrase, &password, &bip39);
        }
    }

    /// Encrypt `phrase` into the vault file at `vault`, then unlock a
    /// session directly so the user lands on the main screen.
    fn complete_setup(
        &mut self,
        vault: &std::path::Path,
        phrase: &str,
        password: &str,
        bip39: &str,
    ) {
        if self.task.is_some() {
            return;
        }
        let vault = vault.to_path_buf();
        let phrase = Zeroizing::new(phrase.to_string());
        let password = Zeroizing::new(password.to_string());
        let bip39 = bip39.to_string();
        let (tx, rx) = std::sync::mpsc::channel();

        // Argon2id (64 MiB) plus scrypt (128 MiB) take a couple of seconds
        // together — far too long to block the frame loop, so the whole
        // create-and-unlock runs on a worker.
        std::thread::spawn(move || {
            let result = crypto::vault_seal(&phrase, &password)
                .map_err(|e| e.to_string())
                .and_then(|data| {
                    if let Some(parent) = vault.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    fs::write(&vault, data).map_err(|e| e.to_string())?;

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&vault, fs::Permissions::from_mode(0o600))
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                })
                // The BIP39 passphrase feeds the derivation, not the vault.
                .and_then(|()| {
                    Session::unlock(&phrase, Some(bip39)).map_err(|e| e.to_string())
                });
            let _ = tx.send(result);
        });

        self.task = Some(Task::SaveVault(rx));
    }

    /// Start unlocking on a background thread.
    ///
    /// scrypt (128 MiB, N=2^17) takes roughly a second, which would freeze the
    /// window if run inline. The worker sends the session — or an error
    /// message — back through a channel that [`App::poll_task`] drains each
    /// frame, so the unlock screen can show its "unlocking…" state.
    fn try_unlock(&mut self, vault: &std::path::Path) {
        if self.task.is_some() {
            return;
        }
        self.unlock_error = None;

        let password = std::mem::take(&mut *self.password);
        let bip39 = std::mem::take(&mut *self.bip39);
        let vault = vault.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = fs::read(&vault)
                .map_err(|e| e.to_string())
                .and_then(|data| crypto::vault_open(&data, &password).map_err(|e| e.to_string()))
                .and_then(|phrase| {
                    Session::unlock(&phrase, Some(bip39.to_string())).map_err(|e| e.to_string())
                });
            let _ = tx.send(result);
        });

        self.task = Some(Task::Unlock(rx));
    }

    /// Drain a finished background task, if any. Called once per frame.
    ///
    /// Every task shares the same shape — a channel that yields exactly one
    /// result — so the bookkeeping (busy state, repaint requests, error
    /// reporting) lives here instead of being repeated per operation.
    fn poll_task(&mut self, ctx: &egui::Context) {
        let Some(task) = self.task.take() else {
            return;
        };

        // Putting the task back keeps the busy state alive while it runs.
        let put_back = |this: &mut Self, task: Task| {
            this.task = Some(task);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        };

        match task {
            Task::Unlock(rx) => match rx.try_recv() {
                Ok(Ok(session)) => self.finish_unlock(session),
                Ok(Err(msg)) => {
                    // Wrong password is by far the common case, so map it to
                    // the design spec's copy; anything else keeps the reason.
                    self.unlock_error = Some(if msg.contains("incorrect password") {
                        self.lang.unlock_wrong_pw().to_string()
                    } else {
                        msg
                    });
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    put_back(self, Task::Unlock(rx))
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.unlock_error = Some("unlock worker stopped unexpectedly".to_string());
                }
            },
            Task::SaveVault(rx) => match rx.try_recv() {
                Ok(Ok(session)) => {
                    self.setup = None;
                    self.finish_unlock(session);
                    self.set_status(self.lang.vault_created(), false);
                }
                Ok(Err(msg)) => self.set_status(format!("✗ {msg}"), true),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    put_back(self, Task::SaveVault(rx))
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.set_status("✗ vault worker stopped unexpectedly", true);
                }
            },
            Task::Pwned(rx) => match rx.try_recv() {
                Ok(Ok(breached)) => {
                    self.leaked.clear();
                    for (site, _) in &breached {
                        self.leaked.insert(site.clone());
                    }
                    self.report_pwned(&breached);
                }
                Ok(Err(msg)) => self.set_status(format!("✗ {msg}"), true),
                Err(std::sync::mpsc::TryRecvError::Empty) => put_back(self, Task::Pwned(rx)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.set_status("✗ breach check stopped unexpectedly", true);
                }
            },
        }
    }

    /// Adopt a freshly derived session and load the metadata.
    fn finish_unlock(&mut self, session: Session) {
        self.unlock_error = None;
        self.session = Some(session);
        self.sites = store::load(&self.db_path).unwrap_or_default();
        self.status.clear();
    }

    /// Whether a long-running task is in flight.
    fn busy(&self) -> bool {
        self.task.is_some()
    }

    /// Write the window size to disk when the user resizes, so the app
    /// reopens at the size they left it. Skips writes while the pointer is
    /// down (mid-drag) and when nothing actually changed.
    fn persist_window_size(&mut self, ctx: &egui::Context) {
        let size = ctx.screen_rect().size();
        if size.x < 1.0 || size.y < 1.0 {
            return;
        }
        let changed = match self.saved_window {
            Some(prev) => (prev - size).length() > 1.0,
            None => true,
        };
        if !changed || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        if self.saved_window.is_none() {
            // First frame: adopt the size we were launched with rather than
            // rewriting the file on every start.
            self.saved_window = Some(size);
            return;
        }
        self.saved_window = Some(size);
        save_window_size(size);
    }

    // ── Main screen ──────────────────────────────────────────────────────────

    fn main_screen(&mut self, ui: &mut egui::Ui) {
        self.top_bar(ui);

        // Reserve room for the status strip so the list never covers it.
        let status_height = if self.status.is_empty() { 0.0 } else { 39.0 };
        let list_height = (ui.available_height() - status_height).max(80.0);

        // Content scrolls between the top bar and the status strip, with the
        // 22px side padding the design spec uses.
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 22,
                        right: 22,
                        top: 12,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        // The panel (add/edit) is the focus while it is
                        // open: dim and block the content behind it.
                        let dimmed = self.show_add_form;
                        ui.scope(|ui| {
                            if dimmed {
                                ui.set_opacity(0.5);
                                ui.disable();
                            }
                            self.rotation_banner(ui);
                        });
                        ui.add_space(12.0);
                        if self.show_add_form {
                            self.add_form(ui);
                            ui.add_space(8.0);
                        }
                        ui.scope(|ui| {
                            if dimmed {
                                ui.set_opacity(0.5);
                                ui.disable();
                            }
                            self.account_list(ui);
                        });
                    });
            });

        if !self.status.is_empty() {
            self.status_bar(ui);
        }
    }

    /// Brand on the left, actions on the right.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
        let busy = self.task.is_some();
        let narrow = ui.available_width() < 760.0;
        let mut run_pwned = false;
        let mut do_rotate = false;
        let mut more_button: Option<egui::Response> = None;

        // 50px bar with a hairline underneath, matching the design spec.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(22, 9))
            .show(ui, |ui| {
                ui.set_min_height(32.0);
                ui.horizontal(|ui| {
                    // Brand: mark, name, divider, rotation level.
                    theme::brand_mark(ui, 20.0);
                    ui.add_space(9.0);
                    ui.label(
                        egui::RichText::new("hdpassw")
                            .size(17.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    ui.add_space(9.0);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
                    ui.painter().vline(
                        r.center().x,
                        r.y_range(),
                        egui::Stroke::new(1.0_f32, theme::LINE_STRONG),
                    );
                    ui.add_space(9.0);
                    ui.label(
                        egui::RichText::new(l.brand_local())
                            .size(11.5)
                            .color(theme::TEXT_3),
                    );

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;

                            // Language toggle: a small outlined box. In a
                            // narrow window it lives in the "更多" menu.
                            let lang_label = if self.lang == Lang::Zh { "EN" } else { "中" };
                            if !narrow
                                && ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(lang_label)
                                            .size(11.0)
                                            .strong()
                                            .color(theme::TEXT_2),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(5))
                                    .min_size(egui::vec2(32.0, 28.0)),
                                )
                                .on_hover_text(l.language())
                                .clicked()
                            {
                                self.lang = if self.lang == Lang::Zh {
                                    Lang::En
                                } else {
                                    Lang::Zh
                                };
                                ensure_cjk_font(ui.ctx(), self.lang);
                                self.lang.save_preference();
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(
                                    self.lang.window_title(),
                                ));
                            }

                            if theme::btn_ghost(ui, l.lock()) {
                                self.session = None;
                                self.sites = SiteStore::default();
                                self.show_add_form = false;
                                self.leaked.clear();
                                self.query.clear();
                                self.status.clear();
                            }

                            let pwned_label = if busy {
                                l.checking()
                            } else {
                                l.test_if_pwned()
                            };
                            let enabled = !self.sites.sites.is_empty() && !busy;
                            if ui
                                .add_enabled(
                                    enabled,
                                    egui::Button::new(
                                        egui::RichText::new(pwned_label)
                                            .size(12.0)
                                            .color(theme::TEXT),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                run_pwned = true;
                            }

                            // Narrow windows collapse these into "更多".
                            if !narrow {
                                if theme::btn_quiet(ui, l.export_()) {
                                    let text = self.export_toml();
                                    self.dialog = Dialog::Export {
                                        text,
                                        copied: false,
                                    };
                                }
                                if theme::btn_quiet(ui, l.recover_link()) {
                                    self.dialog = Dialog::Recover(RecoverState::default());
                                }
                                if theme::btn_quiet(ui, l.bump_all()) {
                                    do_rotate = true;
                                }
                            } else {
                                more_button = Some(ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(l.more()).size(12.0).color(theme::TEXT),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .min_size(egui::vec2(0.0, 32.0)),
                                ));
                            }
                        },
                    );
                });
            });

        // Narrow-window menu: the wide-only actions live here behind "更多".
        if let Some(btn) = &more_button {
            let mut picked: Option<u8> = None;
            egui::Popup::menu(btn)
                .id(egui::Id::new("more_menu"))
                .show(|ui| {
                    ui.set_min_width(170.0);
                    if ui.button(l.recover_link()).clicked() {
                        picked = Some(0);
                    }
                    if ui.button(l.bump_all()).clicked() {
                        picked = Some(1);
                    }
                    if ui.button(l.export_()).clicked() {
                        picked = Some(2);
                    }
                    if ui.button(l.language()).clicked() {
                        picked = Some(3);
                    }
                });
            match picked {
                Some(0) => self.dialog = Dialog::Recover(RecoverState::default()),
                Some(1) => do_rotate = true,
                Some(2) => {
                    let text = self.export_toml();
                    self.dialog = Dialog::Export { text, copied: false };
                }
                Some(3) => {
                    self.lang = if self.lang == Lang::Zh { Lang::En } else { Lang::Zh };
                    ensure_cjk_font(ui.ctx(), self.lang);
                    self.lang.save_preference();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(
                        self.lang.window_title(),
                    ));
                }
                _ => {}
            }
        }

        // Hairline below the bar.
        let full = ui.available_rect_before_wrap();
        let y = ui.min_rect().bottom();
        ui.painter().hline(
            full.x_range(),
            y,
            egui::Stroke::new(1.0_f32, theme::LINE),
        );

        if do_rotate {
            self.rotate_global();
        }
        if run_pwned {
            self.check_all_pwned();
        }
    }

    /// Global rotation level plus how many accounts are behind.
    fn rotation_banner(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
        let rotation = self.sites.rotation;
        let behind = self
            .sites
            .sites
            .iter()
            .filter(|(site, r)| r.counter < rotation || self.leaked.contains(*site))
            .count();

        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(16, 11))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(l.rotation_banner_title())
                                .size(13.0)
                                .strong()
                                .color(theme::TEXT),
                        );
                        ui.add_space(3.0);
                        // Sub line with a status dot, per the design spec.
                        let (msg, color) = if self.sites.sites.is_empty() {
                            (l.no_accounts_yet().to_string(), theme::TEXT_2)
                        } else if behind == 0 {
                            (l.rotation_all_caught_up().to_string(), theme::OK)
                        } else {
                            (format!("{behind} {}", l.rotation_behind_n()), theme::WARN)
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (dot, _) = ui.allocate_exact_size(
                                egui::vec2(7.0, 7.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(dot.center(), 3.5, color);
                            ui.label(egui::RichText::new(msg).size(11.5).color(color));
                        });
                    });

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add_enabled(
                                    !self.sites.sites.is_empty(),
                                    egui::Button::new(
                                        egui::RichText::new(l.bump_all())
                                            .size(12.0)
                                            .color(theme::PRIMARY_BTN_TEXT),
                                    )
                                    .fill(theme::PRIMARY_BTN)
                                    .stroke(egui::Stroke::NONE)
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .on_hover_text(l.rotate_hint())
                                .clicked()
                            {
                                self.rotate_global();
                            }
                        },
                    );
                });
            });
    }

    /// Searchable list of accounts, each with copy / edit / bump / delete.
    ///
    /// Layout follows the design spec's `.acct` rows: compact two-line cards
    /// (7px vertical padding), one ellipsised meta line, and small inline
    /// `.act` buttons rather than full-size buttons, so a long list stays
    /// scannable. Below ~760px the row stacks and the actions move under the
    /// text.
    fn account_list(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
        let mut to_copy: Option<(String, String, u32, usize, String)> = None;
        let mut start_bump: Option<(String, u32, u32)> = None;
        let mut open_edit: Option<String> = None;
        let mut open_delete: Option<String> = None;
        let mut advance_bump = false;
        let mut cancel_bump = false;
        let mut copy_old_now = false;

        let rotation = self.sites.rotation;
        let q = self.query.trim().to_lowercase();
        let narrow = ui.available_width() < 760.0;

        let mut visible: Vec<(String, SiteRecord)> = self
            .sites
            .sites
            .iter()
            .filter(|(site, r)| {
                if q.is_empty() {
                    return true;
                }
                let hay = format!("{site} {} {}", r.user, r.notes.clone().unwrap_or_default())
                    .to_lowercase();
                hay.contains(&q)
            })
            .map(|(s, r)| (s.clone(), r.clone()))
            .collect();
        visible.sort_by(|a, b| a.0.cmp(&b.0));

        // Header row: title + count on the left, search + add on the right.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            ui.label(
                egui::RichText::new(l.accounts())
                    .size(14.0)
                    .strong()
                    .color(theme::TEXT),
            );
            let count = if q.is_empty() {
                format!("{} {}", self.sites.sites.len(), l.accounts_count_suffix())
            } else {
                format!(
                    "{} / {} {}",
                    visible.len(),
                    self.sites.sites.len(),
                    l.accounts_count_suffix()
                )
            };
            ui.label(
                egui::RichText::new(count)
                    .size(11.5)
                    .color(theme::TEXT_3),
            );

            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let add_label = if self.show_add_form {
                        l.cancel()
                    } else {
                        l.add_account()
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(add_label)
                                    .size(12.0)
                                    .color(theme::TEXT),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0_f32, theme::LINE_STRONG))
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(0.0, 30.0)),
                        )
                        .clicked()
                    {
                        self.show_add_form = !self.show_add_form;
                        if !self.show_add_form {
                            self.reset_add_form();
                        }
                    }
                    // Search field with the magnifier inside it. The text
                    // edit gets a left margin so the hint never runs under
                    // the icon.
                    let width = if narrow { 150.0 } else { 210.0 };
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(l.search_placeholder())
                            .desired_width(width)
                            .margin(egui::Margin {
                                left: 26,
                                right: 8,
                                top: 5,
                                bottom: 5,
                            }),
                    );
                    let c = resp.rect.left_center() + egui::vec2(14.0, 0.0);
                    ui.painter()
                        .circle_stroke(c, 4.5, egui::Stroke::new(1.3_f32, theme::TEXT_3));
                    ui.painter().line_segment(
                        [c + egui::vec2(3.2, 3.2), c + egui::vec2(6.5, 6.5)],
                        egui::Stroke::new(1.3_f32, theme::TEXT_3),
                    );
                },
            );
        });
        ui.add_space(8.0);

        if self.sites.sites.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    egui::RichText::new(l.no_sites())
                        .size(12.0)
                        .color(theme::TEXT_3),
                );
            });
            return;
        }

        if visible.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    egui::RichText::new(l.search_none())
                        .size(12.0)
                        .color(theme::TEXT_3),
                );
            });
            return;
        }

        for (site, record) in &visible {
            let needs = self.needs_rotation(site, record);
            let leaked = self.leaked.contains(site);
            let bumping = self.bump.as_ref().filter(|b| &b.site == site).map(|b| b.step);

            let row = egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let has_bump_block = bumping.is_some();

                    if narrow {
                        // Stacked: identity on top, actions right-aligned
                        // underneath. The row is height-constrained because
                        // `with_layout` in a vertical Ui would otherwise
                        // claim all remaining height.
                        account_identity(ui, l, site, record, needs, leaked);
                        ui.add_space(4.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 26.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                account_actions(
                                    ui,
                                    l,
                                    site,
                                    record,
                                    needs,
                                    bumping,
                                    &mut to_copy,
                                    &mut start_bump,
                                    &mut open_edit,
                                    &mut open_delete,
                                    &mut advance_bump,
                                    &mut cancel_bump,
                                    &mut copy_old_now,
                                    rotation,
                                );
                            },
                        );
                    } else {
                        ui.horizontal(|ui| {
                            account_identity(ui, l, site, record, needs, leaked);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    account_actions(
                                        ui,
                                        l,
                                        site,
                                        record,
                                        needs,
                                        bumping,
                                        &mut to_copy,
                                        &mut start_bump,
                                        &mut open_edit,
                                        &mut open_delete,
                                        &mut advance_bump,
                                        &mut cancel_bump,
                                        &mut copy_old_now,
                                        rotation,
                                    );
                                },
                            );
                        });
                    }

                    if has_bump_block {
                        let step = bumping.unwrap_or(0);
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                let (title, sub) = if step == 0 {
                                    (l.bump_step1_title(), l.bump_step1_sub())
                                } else {
                                    (l.bump_step2_title(), l.bump_step2_sub())
                                };
                                ui.label(
                                    egui::RichText::new(title)
                                        .size(12.0)
                                        .strong()
                                        .color(theme::TEXT),
                                );
                                ui.label(
                                    egui::RichText::new(sub).size(11.0).color(theme::TEXT_2),
                                );
                            });
                    }
                });

            // Hover tint, as in the spec.
            let resp = row.response.interact(egui::Sense::hover());
            if resp.hovered() && bumping.is_none() {
                ui.painter().rect_stroke(
                    resp.rect,
                    egui::CornerRadius::same(theme::RADIUS),
                    egui::Stroke::new(1.0_f32, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );
            }

            ui.add_space(6.0);
        }

        if let Some((site, user, counter, length, charset_str)) = to_copy {
            let session = self.session.as_ref().expect("unlocked");
            match generate_and_copy(session, &site, &user, counter, length, &charset_str) {
                Ok(()) => self.set_status(format!("✓ {site}: {}", l.copied_pw()), false),
                Err(e) => self.set_status(format!("✗ {e}"), true),
            }
        }

        if let Some((site, old, new)) = start_bump {
            self.bump = Some(Bump {
                site,
                old,
                new,
                step: 0,
            });
        }

        if copy_old_now {
            self.copy_old_password();
        }

        if advance_bump {
            self.advance_bump();
        }

        if cancel_bump {
            self.bump = None;
            self.set_status(l.bump_cancelled(), false);
        }

        if let Some(site) = open_edit {
            self.start_edit(&site);
        }

        if let Some(site) = open_delete {
            self.dialog = Dialog::Delete(site);
        }
    }

    /// Bottom status line: green for success, red for errors.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let color = if self.status_is_error {
            theme::BAD
        } else {
            theme::OK
        };
        // 38px strip with a hairline above it, per the design spec.
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0_f32, theme::LINE),
        );
        egui::Frame::new()
            .fill(theme::SURFACE)
            .inner_margin(egui::Margin::symmetric(22, 8))
            .show(ui, |ui| {
                ui.set_min_height(22.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    // The message already carries a ✓/⚠/✗ prefix from the
                    // action that produced it; keep it and colour the line.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.status).size(12.0).color(color),
                        )
                        .truncate(),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if theme::act(ui, l_dismiss(self.lang), theme::Act::Plain) {
                                self.status.clear();
                            }
                        },
                    );
                });
            });
    }

    // ── Add / edit form ──────────────────────────────────────────────────────

    fn add_form(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
        let editing = self.editing.is_some();
        let title = if editing {
            l.edit_account()
        } else {
            l.add_account()
        };

        theme::card().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(8.0);

            egui::Grid::new("add_site_grid")
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label(label(l.field_site()));
                    ui.add_enabled(
                        !editing,
                        egui::TextEdit::singleline(&mut self.new_site)
                            .hint_text("example.com")
                            .desired_width(320.0),
                    );
                    ui.end_row();

                    ui.label(label(l.field_user()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_user)
                            .hint_text("name@example.com")
                            .desired_width(320.0),
                    );
                    ui.end_row();

                    ui.label(label(l.field_counter()));
                    ui.add(egui::TextEdit::singleline(&mut self.new_counter).desired_width(80.0));
                    ui.end_row();

                    ui.label(label(l.field_length()));
                    ui.add(egui::TextEdit::singleline(&mut self.new_length).desired_width(80.0));
                    ui.end_row();

                    ui.label(label(l.field_charset()));
                    egui::ComboBox::from_id_salt("charset")
                        .selected_text(charset_label(l, self.new_charset))
                        .show_ui(ui, |ui| {
                            for charset in CHARSETS {
                                ui.selectable_value(
                                    &mut self.new_charset,
                                    charset,
                                    charset_label(l, charset),
                                );
                            }
                        });
                    ui.end_row();

                    ui.label(label(l.field_notes()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_notes)
                            .hint_text(l.optional())
                            .desired_width(320.0),
                    );
                    ui.end_row();
                });

            if self.new_length.trim() != "32" || self.new_charset != Charset::default() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(l.settings_warn())
                        .size(10.5)
                        .color(theme::WARN),
                );
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(l.save()).clicked() {
                    self.save_site();
                }
                if ui.button(l.cancel()).clicked() {
                    self.show_add_form = false;
                    self.reset_add_form();
                }
            });
        });
    }

    fn reset_add_form(&mut self) {
        self.editing = None;
        self.new_site.clear();
        self.new_user.clear();
        self.new_counter = "1".to_string();
        self.new_length = "32".to_string();
        self.new_charset = Charset::default();
        self.new_notes.clear();
    }

    /// Prefill the form with an existing record so it can be edited.
    fn start_edit(&mut self, site: &str) {
        let Some(record) = self.sites.sites.get(site) else {
            return;
        };
        self.editing = Some(site.to_string());
        self.new_site = site.to_string();
        self.new_user = record.user.clone();
        self.new_counter = record.counter.to_string();
        self.new_length = record.length.to_string();
        self.new_charset = Charset::from_str(&record.charset).unwrap_or_default();
        self.new_notes = record.notes.clone().unwrap_or_default();
        self.show_add_form = true;
    }

    fn save_site(&mut self) {
        let l = self.lang;
        let site = self.new_site.trim().to_lowercase();
        let user = self.new_user.trim().to_string();

        if site.is_empty() || !site.contains('.') {
            self.set_status(l.site_invalid(), true);
            return;
        }
        if user.is_empty() {
            self.set_status(l.user_empty(), true);
            return;
        }

        let counter: u32 = match self.new_counter.trim().parse() {
            Ok(n) => n,
            Err(_) => return self.set_status(l.counter_invalid(), true),
        };
        let length: usize = match self.new_length.trim().parse() {
            Ok(n) => n,
            Err(_) => return self.set_status(l.length_invalid(), true),
        };
        let notes = {
            let n = self.new_notes.trim();
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        };

        let session = self.session.as_ref().expect("unlocked");
        let result = manager::upsert_site(
            &mut self.sites,
            session,
            &site,
            &user,
            counter,
            length,
            self.new_charset,
            notes,
        )
        .and_then(|()| store::save(&self.db_path, &self.sites));

        let verb = if self.editing.is_some() {
            l.updated()
        } else {
            l.added()
        };
        match result {
            Ok(()) => {
                self.set_status(format!("✓ {} {site}", verb), false);
                self.reset_add_form();
                self.show_add_form = false;
            }
            Err(e) => self.set_status(format!("✗ {e}"), true),
        }
    }

    // ── Rotation, bump, pwned ────────────────────────────────────────────────

    /// Copy the current (old) password for the site being rotated, without
    /// advancing the hand-off. Lets the user log in first.
    fn copy_old_password(&mut self) {
        let l = self.lang;
        let Some(bump) = self.bump.clone() else {
            return;
        };
        let Some(record) = self.sites.sites.get(&bump.site).cloned() else {
            return;
        };
        let charset = match Charset::from_str(&record.charset) {
            Ok(c) => c,
            Err(e) => return self.set_status(format!("✗ {e}"), true),
        };
        let password = {
            let session = self.session.as_ref().expect("unlocked");
            manager::generate_password(
                session,
                &bump.site,
                &record.user,
                bump.old,
                record.length,
                charset,
            )
        };
        match password {
            Ok(p) => {
                let p = Zeroizing::new(p);
                if let Err(e) = clipboard::copy_and_clear(&p, 30) {
                    return self.set_status(format!("✗ {e}"), true);
                }
                self.set_status(
                    format!("✓ {}: {}", bump.site, l.copied_old_pw()),
                    false,
                );
            }
            Err(e) => self.set_status(format!("✗ {e}"), true),
        }
    }

    /// Advances an in-progress password change one step.
    ///
    /// Step 0 → 1: copies the OLD password (current counter) so the user
    /// can log in and start the change on the site.
    /// Step 1 → done: copies the NEW password (target counter) and only
    /// then persists the bumped counter and recomputed verifier to disk.
    /// If either step fails, `bump` is left as-is so the user can retry
    /// without losing their place.
    fn advance_bump(&mut self) {
        let l = self.lang;
        let Some(bump) = self.bump.clone() else {
            return;
        };
        let Some(record) = self.sites.sites.get(&bump.site).cloned() else {
            self.bump = None;
            return;
        };
        let charset = match Charset::from_str(&record.charset) {
            Ok(c) => c,
            Err(e) => return self.set_status(format!("✗ {e}"), true),
        };

        if bump.step == 0 {
            // Copying the old password is a separate action; "next" only
            // moves the hand-off forward.
            let site = bump.site.clone();
            self.bump = Some(Bump { step: 1, ..bump });
            self.set_status(format!("✓ {site}: {}", l.bump_step2_hint()), false);
            return;
        }

        let new_password_result = {
            let session = self.session.as_ref().expect("unlocked");
            manager::generate_password(
                session,
                &bump.site,
                &record.user,
                bump.new,
                record.length,
                charset,
            )
        };
        let new_password = match new_password_result {
            Ok(p) => Zeroizing::new(p),
            Err(e) => return self.set_status(format!("✗ {e}"), true),
        };
        if let Err(e) = clipboard::copy_and_clear(&new_password, 30) {
            return self.set_status(format!("✗ {e}"), true);
        }

        let upsert_result = {
            let session = self.session.as_ref().expect("unlocked");
            manager::upsert_site(
                &mut self.sites,
                session,
                &bump.site,
                &record.user,
                bump.new,
                record.length,
                charset,
                record.notes.clone(),
            )
        };
        if let Err(e) = upsert_result {
            return self.set_status(format!("✗ {e}"), true);
        }
        if let Err(e) = store::save(&self.db_path, &self.sites) {
            return self.set_status(format!("✗ {e}"), true);
        }

        self.leaked.remove(&bump.site);
        self.bump = None;
        self.set_status(
            format!("✓ {} → /{}: {}", bump.site, bump.new, l.saved_now_at()),
            false,
        );
    }

    /// Bump the global rotation counter only — no site's own counter or
    /// password is touched. Sites whose counter is now behind are flagged
    /// in the list; use the per-site "Bump" button to actually change one.
    fn rotate_global(&mut self) {
        let old = self.sites.rotation;
        let new = old + 1;
        self.sites.rotation = new;
        self.sites.rotation_since = chrono::Local::now().date_naive();

        if let Err(e) = store::save(&self.db_path, &self.sites) {
            self.sites.rotation = old;
            return self.set_status(format!("✗ {e}"), true);
        }

        let stale = self
            .sites
            .sites
            .values()
            .filter(|r| r.counter < new)
            .count();
        let l = self.lang;
        self.set_status(
            if stale == 0 {
                format!(
                    "✓ {} /{} → /{} — {}",
                    l.rotation(),
                    old,
                    new,
                    l.rotation_done_all_caught_up()
                )
            } else {
                format!(
                    "✓ {} /{} → /{} — {} {} {}",
                    l.rotation(),
                    old,
                    new,
                    stale,
                    l.rotation_done_behind(),
                    l.bump()
                )
            },
            false,
        );
    }

    /// Derive every stored site's password and check it against Have I
    /// Been Pwned, then summarize results in the status line. Passwords
    /// are held only long enough to hash and check, never displayed.
    fn check_all_pwned(&mut self) {
        if self.task.is_some() {
            return;
        }
        let session = self.session.as_ref().expect("unlocked");

        // Derive every password up front — BLAKE3 is fast, so this stays on
        // the UI thread — then hand the already-derived strings to a worker.
        // That keeps the slow part (SHA-1 + one HTTPS request per account)
        // off the frame loop and avoids cloning the master key.
        let mut passwords: Vec<(String, Zeroizing<String>)> = Vec::new();
        for (site, record) in &self.sites.sites {
            let charset = match Charset::from_str(&record.charset) {
                Ok(c) => c,
                Err(e) => return self.set_status(format!("✗ {e}"), true),
            };
            match manager::generate_password(
                session,
                site,
                &record.user,
                record.counter,
                record.length,
                charset,
            ) {
                Ok(p) => passwords.push((site.clone(), Zeroizing::new(p))),
                Err(e) => return self.set_status(format!("✗ {e}"), true),
            }
        }

        let total = passwords.len();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut breached = Vec::new();
            for (site, password) in &passwords {
                match crate::pwned::check(password) {
                    Ok(crate::pwned::PwnedStatus::Safe) => {}
                    Ok(crate::pwned::PwnedStatus::Pwned(count)) => {
                        breached.push((site.clone(), count));
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                }
            }
            let _ = tx.send(Ok(breached));
        });

        self.set_status(
            format!("{} {}…", self.lang.pwned_checking(), total),
            false,
        );
        self.task = Some(Task::Pwned(rx));
    }

    /// Summarize a finished breach check in the status line.
    fn report_pwned(&mut self, breached: &[(String, u64)]) {
        let l = self.lang;
        let total = self.sites.sites.len();
        if breached.is_empty() {
            self.set_status(
                format!("✓ {}（{}）— {}", l.pwned_none(), total, l.accounts()),
                false,
            );
        } else {
            let list = breached
                .iter()
                .map(|(site, n)| format!("{site} ({n})"))
                .collect::<Vec<_>>()
                .join(", ");
            self.set_status(
                format!(
                    "⚠ {}/{} {}: {}",
                    breached.len(),
                    total,
                    l.pwned_found_prefix(),
                    list
                ),
                true,
            );
        }
    }

    // ── Export ───────────────────────────────────────────────────────────────

    /// Render the metadata file as TOML, matching `sites.toml` on disk.
    fn export_toml(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("version = {}\n", self.sites.version));
        out.push_str(&format!("rotation = {}\n", self.sites.rotation));
        out.push_str(&format!("rotation_since = {}\n", self.sites.rotation_since));

        for (site, r) in &self.sites.sites {
            out.push_str(&format!("\n[sites.\"{site}\"]\n"));
            out.push_str(&format!("user = \"{}\"\n", escape_toml(&r.user)));
            out.push_str(&format!("counter = {}\n", r.counter));
            out.push_str(&format!("length = {}\n", r.length));
            out.push_str(&format!("charset = \"{}\"\n", escape_toml(&r.charset)));
            out.push_str(&format!("verifier = \"{}\"\n", escape_toml(&r.verifier)));
            out.push_str(&format!("created = {}\n", r.created));
            out.push_str(&format!("modified = {}\n", r.modified));
            if let Some(notes) = &r.notes {
                out.push_str(&format!("notes = \"{}\"\n", escape_toml(notes)));
            }
        }
        out
    }

    // ── Dialogs ──────────────────────────────────────────────────────────────

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let l = self.lang;
        let mut close = false;

        match &mut self.dialog {
            Dialog::None => return,
            Dialog::Delete(site) => {
                let site = site.clone();
                egui::Window::new(l.delete_title())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(
                            egui::RichText::new(site.clone())
                                .strong()
                                .color(theme::TEXT),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(l.delete_sub())
                                .size(11.0)
                                .color(theme::TEXT_2),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button(l.delete_()).clicked() {
                                self.sites.sites.remove(&site);
                                self.leaked.remove(&site);
                                if let Err(e) = store::save(&self.db_path, &self.sites) {
                                    self.set_status(format!("✗ {e}"), true);
                                } else {
                                    self.set_status(format!("✓ {} {site}", l.delete_()), false);
                                }
                                close = true;
                            }
                            if ui.button(l.cancel()).clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::Export { text, copied } => {
                let mut copy_now = false;
                egui::Window::new(l.export_title())
                    .collapsible(false)
                    .resizable(true)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(
                            egui::RichText::new(l.export_sub())
                                .size(11.0)
                                .color(theme::TEXT_2),
                        );
                        ui.add_space(8.0);
                        let mut preview = text.clone();
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut preview)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .interactive(false),
                                );
                            });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button(l.export_copy()).clicked() {
                                copy_now = true;
                            }
                            if *copied {
                                ui.label(
                                    egui::RichText::new(format!("✓ {}", l.export_copied()))
                                        .size(11.0)
                                        .color(theme::OK),
                                );
                            }
                            if ui.button(l.close()).clicked() {
                                close = true;
                            }
                        });
                    });
                if copy_now {
                    if let Err(e) = clipboard::copy_and_clear(text, 30) {
                        self.set_status(format!("✗ {e}"), true);
                    } else {
                        *copied = true;
                        self.set_status(l.export_copied(), false);
                    }
                }
            }
            Dialog::Recover(state) => {
                let mut find = false;
                egui::Window::new(l.recover_title())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_max_width(400.0);
                        ui.label(
                            egui::RichText::new(l.recover_sub())
                                .size(11.0)
                                .color(theme::TEXT_2),
                        );
                        ui.add_space(10.0);

                        ui.label(label(l.field_site()));
                        ui.add(
                            egui::TextEdit::singleline(&mut state.site)
                                .hint_text("example.com")
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(6.0);
                        ui.label(label(l.field_user()));
                        ui.add(
                            egui::TextEdit::singleline(&mut state.user)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(6.0);
                        ui.label(label(l.recover_verifier()));
                        ui.add(
                            egui::TextEdit::singleline(&mut state.verifier)
                                .hint_text("a3f9")
                                .desired_width(f32::INFINITY),
                        );

                        ui.add_space(10.0);
                        if !state.error.is_empty() {
                            ui.label(
                                egui::RichText::new(&state.error)
                                    .size(11.0)
                                    .color(theme::BAD),
                            );
                            ui.add_space(6.0);
                        }
                        if let Some(n) = state.result {
                            egui::Frame::new()
                                .fill(theme::OK_SOFT)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM))
                                .inner_margin(egui::Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(l.recover_found())
                                                .size(11.5)
                                                .color(theme::TEXT),
                                        );
                                        ui.label(
                                            egui::RichText::new(n.to_string())
                                                .size(15.0)
                                                .strong()
                                                .family(egui::FontFamily::Monospace)
                                                .color(theme::OK),
                                        );
                                    });
                                });
                            ui.add_space(8.0);
                        }

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!state.scanning, egui::Button::new(l.recover_find()))
                                .clicked()
                            {
                                find = true;
                            }
                            if ui.button(l.close()).clicked() {
                                close = true;
                            }
                        });
                    });

                if find {
                    // Scan rounds 1..=20 for a matching verifier. Cheap enough
                    // to do synchronously (BLAKE3 per round), so no threads.
                    state.error.clear();
                    state.result = None;
                    let site = state.site.trim().to_lowercase();
                    let user = state.user.trim().to_string();
                    let code = state.verifier.trim().to_lowercase();

                    if site.is_empty() || !site.contains('.') {
                        state.error = l.recover_bad_site().to_string();
                    } else if user.is_empty() {
                        state.error = l.recover_bad_user().to_string();
                    } else if code.len() != 4 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
                        state.error = l.recover_bad_code().to_string();
                    } else {
                        let session = self.session.as_ref().expect("unlocked");
                        let mut found = None;
                        for n in 1..=20u32 {
                            match session.verifier(&site, &user, n) {
                                Ok(v) if v == code => {
                                    found = Some(n);
                                    break;
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    state.error = format!("✗ {e}");
                                    break;
                                }
                            }
                        }
                        match found {
                            Some(n) => state.result = Some(n),
                            None if state.error.is_empty() => {
                                state.error = l.recover_not_found().to_string()
                            }
                            None => {}
                        }
                    }
                }
            }
        }

        if close {
            self.dialog = Dialog::None;
        }
    }
}

/// One account row's identity block: name, status chips, and a single
/// ellipsised meta line (`user · /N · verifier · notes`).
fn account_identity(
    ui: &mut egui::Ui,
    l: Lang,
    site: &str,
    record: &SiteRecord,
    needs: bool,
    leaked: bool,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(site)
                    .size(14.0)
                    .strong()
                    .color(theme::TEXT),
            );
            // One chip only: a leaked account is the more urgent fact, so it
            // replaces the "needs new password" chip rather than stacking.
            if leaked {
                theme::chip(ui, l.chip_leaked(), theme::BAD, theme::BAD_SOFT);
            } else if needs {
                theme::chip(ui, l.chip_needs_rotation(), theme::WARN, theme::WARN_SOFT);
            }
        });
        ui.add_space(2.0);

        // One line, clipped with an ellipsis so a long note cannot stretch
        // the card. Built as a single string rather than several labels so
        // truncation happens at the end of the whole line.
                let mut meta = format!(
            "{} · {} {} · {} {}",
            record.user,
            l.field_counter(),
            record.counter,
            l.verifier_prefix(),
            record.verifier
        );
        if let Some(notes) = record.notes.as_ref().filter(|n| !n.is_empty()) {
            meta.push_str(" · ");
            meta.push_str(notes);
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(meta)
                    .size(11.5)
                    .color(theme::TEXT_3),
            )
            .truncate(),
        );
    });
}

/// The per-row action cluster. Order (left to right) matches the spec:
/// delete, edit, rotate, copy.
#[allow(clippy::too_many_arguments)]
fn account_actions(
    ui: &mut egui::Ui,
    l: Lang,
    site: &str,
    record: &SiteRecord,
    needs: bool,
    bumping: Option<u8>,
    to_copy: &mut Option<(String, String, u32, usize, String)>,
    start_bump: &mut Option<(String, u32, u32)>,
    open_edit: &mut Option<String>,
    open_delete: &mut Option<String>,
    advance_bump: &mut bool,
    cancel_bump: &mut bool,
    copy_old_now: &mut bool,
    rotation: u32,
) {
    match bumping {
        Some(0) => {
            // Copy-old and "next" are separate, as in the spec: the user can
            // copy the old password more than once before moving on.
            if theme::act(ui, l.copy_old_pw(), theme::Act::Copy) {
                *copy_old_now = true;
            }
            if theme::act(ui, l.next_step(), theme::Act::Warn) {
                *advance_bump = true;
            }
            if theme::act(ui, l.cancel(), theme::Act::Plain) {
                *cancel_bump = true;
            }
        }
        Some(_) => {
            if theme::act(ui, l.copy_new_pw_save(), theme::Act::Copy) {
                *advance_bump = true;
            }
            if theme::act(ui, l.cancel(), theme::Act::Plain) {
                *cancel_bump = true;
            }
        }
        None => {
            if theme::act(ui, l.copy_password(), theme::Act::Copy) {
                *to_copy = Some((
                    site.to_string(),
                    record.user.clone(),
                    record.counter,
                    record.length,
                    record.charset.clone(),
                ));
            }
            if needs && theme::act(ui, l.bump(), theme::Act::Warn) {
                let new = if record.counter < rotation {
                    rotation
                } else {
                    record.counter + 1
                };
                *start_bump = Some((site.to_string(), record.counter, new));
            }
            if theme::act(ui, l.edit_(), theme::Act::Plain) {
                *open_edit = Some(site.to_string());
            }
            if theme::act(ui, l.delete_(), theme::Act::Danger) {
                *open_delete = Some(site.to_string());
            }
        }
    }
}

/// Small helper so the grid labels share one style.
fn label(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(11.5).color(theme::TEXT_2)
}

fn l_dismiss(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "dismiss",
        Lang::Zh => "关闭",
    }
}

fn charset_label(lang: Lang, charset: Charset) -> &'static str {
    match charset {
        Charset::Alpha => lang.charset_alpha(),
        Charset::Alphanumeric => lang.charset_alphanumeric(),
        Charset::Full => lang.charset_full(),
        Charset::Pin => lang.charset_pin(),
        Charset::Hex => lang.charset_hex(),
    }
}

/// Escape a string for a TOML basic string.
fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn generate_and_copy(
    session: &Session,
    site: &str,
    user: &str,
    counter: u32,
    length: usize,
    charset_str: &str,
) -> crate::error::Result<()> {
    let charset = Charset::from_str(charset_str)?;
    let password = manager::generate_password(session, site, user, counter, length, charset)?;
    clipboard::copy_and_clear(&password, 30)
}


/// Re-exported for headless render tests, which must install the same fonts
/// the real app does or CJK text renders as tofu.
#[cfg(feature = "gui")]
pub fn install_cjk_font_for_tests(ctx: &egui::Context) {
    install_cjk_font(ctx);
}
