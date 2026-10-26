//! Bilingual UI strings (English / Simplified Chinese) and language selection.
//!
//! `Lang::current()` resolves once at startup: explicit `HDPASSW_LANG`
//! overrides everything; otherwise the saved preference file
//! (`zh` marker file next to the sites.toml) decides; the OS locale is the
//! last fallback. The GUI writes the marker file whenever the user picks a
//! language, so the choice persists across launches.
use std::fs;
use std::path::{Path, PathBuf};

use crate::store::default_db_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    pub fn current() -> Self {
        if let Ok(l) = std::env::var("HDPASSW_LANG") {
            return parse(&l).unwrap_or(Lang::En);
        }
        if pref_marker().exists() {
            return Lang::Zh;
        }
        let locale = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .unwrap_or_default()
            .to_lowercase();
        if locale.starts_with("zh") {
            return Lang::Zh;
        }
        #[cfg(windows)]
        if windows_user_locale().starts_with("zh") {
            return Lang::Zh;
        }
        Lang::En
    }

    /// Persist the user's explicit choice (GUI language switcher).
    pub fn save_preference(self) {
        let marker = pref_marker();
        if self == Lang::Zh {
            if let Some(parent) = marker.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&marker, b"zh");
        } else {
            let _ = fs::remove_file(&marker);
        }
    }
}

fn parse(s: &str) -> Option<Lang> {
    let l = s.trim().to_lowercase();
    if l.starts_with("zh") {
        Some(Lang::Zh)
    } else if l.starts_with("en") {
        Some(Lang::En)
    } else {
        None
    }
}

/// Language preference marker, next to the sites.toml metadata file.
fn pref_marker() -> PathBuf {
    default_db_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lang")
}

/// Read the user's UI language from the Windows registry via `reg query`.
/// Better than nothing; harmless if it fails (falls back to English).
/// CREATE_NO_WINDOW keeps the console-window from flashing for a GUI app.
#[cfg(windows)]
fn windows_user_locale() -> String {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("reg")
        .args(["query", "HKCU\\Control Panel\\International", "/v", "LocaleName"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .find(|l| l.contains("LocaleName"))
                .and_then(|l| l.split_whitespace().last().map(str::to_string))
        })
        .unwrap_or_default()
}

macro_rules! strings {
    (
        $($name:ident => $en:expr, $zh:expr;)*
    ) => {
        $(
            pub fn $name(self) -> &'static str {
                match self { Lang::En => $en, Lang::Zh => $zh }
            }
        )*
    };
}

impl Lang {
    strings! {
        app_tagline => "Deterministic password manager", "确定性密码管理器";
        vault_password => "Vault password", "金库密码";
        unlock => "Unlock", "解锁";
        // Setup wizard
        no_vault => "No seed vault found yet.", "尚未找到种子金库。";
        create_new_seed => "Create new seed", "生成新助记词";
        restore_seed => "Restore existing seed", "恢复现有助记词";
        your_seed_phrase => "Your seed phrase", "你的助记词";
        words_suffix => "words", "个单词";
        write_it_down => "Write this down and store it offline. Never share it.",
            "请抄写下来并离线保存。切勿透露给任何人。";
        i_wrote_it_down => "I have written this down", "我已抄写完毕";
        back => "Back", "上一步";
        continue_ => "Continue", "继续";
        quick_check => "Quick check — enter these words from your seed phrase:",
            "小测验 — 请输入助记词中的这几个单词：";
        word_no => "Word", "第";
        quiz_mismatch => "✗ One or more words don't match. Check your seed phrase and try again.",
            "✗ 有单词不匹配。请核对助记词后重试。";
        enter_all_words => "Enter all words separated by spaces:", "请输入全部单词，以空格分隔：";
        choose_vault_pw => "Choose a vault password:", "设置金库密码：";
        password => "Password", "密码";
        confirm_password => "Confirm password", "确认密码";
        save_vault_unlock => "Save vault & unlock", "保存金库并解锁";
        pw_empty => "✗ Password cannot be empty", "✗ 密码不能为空";
        pw_mismatch => "✗ Passwords do not match", "✗ 两次输入的密码不一致";
        vault_created => "✓ Vault created and unlocked", "✓ 金库已创建并解锁";
        // Main screen
        sites => "Sites", "站点";
        rotation => "rotation", "轮换";
        lock => "Lock", "锁定";
        add_site => "+ Add site", "+ 添加站点";
        cancel => "Cancel", "取消";
        test_if_pwned => "Test if pwned", "检测泄露";
        rotate => "Rotate", "全局轮换";
        rotate_hint => "Bump the global rotation counter only — sites are flagged behind, not changed. Use \"Bump\" on a site to actually change its password.",
            "仅提升全局轮换计数器——各站点会被标记为落后，密码不变。要对单个站点真正改密，请用该站点的「更新」按钮。";
        no_sites => "No sites saved yet. Click \"+ Add site\" to create one.", "还没有站点。点击「+ 添加站点」创建一个。";
        generate_copy => "Generate & copy", "生成并复制";
        copy_old_pw => "Copy old password", "复制旧密码";
        copy_new_pw_save => "Copy new password & save", "复制新密码并保存";
        bump => "Bump", "更新";
        copied_pw => "Copied password — clears in 30s", "已复制密码 — 30 秒后自动清除";
        copied_old_pw => "Copied OLD password — log in, then click \"Copy new password & save\"",
            "已复制旧密码 — 登录后点击「复制新密码并保存」";
        saved_now_at => "Saved: new password copied, paste it on the site",
            "已保存：新密码已复制，请到网站上粘贴";
        bump_cancelled => "Bump cancelled — nothing saved", "已取消——未做任何修改";
        rotation_done_all_caught_up => "every site is already caught up", "所有站点均已是最新";
        rotation_done_behind => "site(s) now behind, use \"Bump\" to change them",
            "个站点已落后，请用「更新」逐个改密";
        pwned_none => "Checked — none found in known breaches", "检测完成 — 未发现已知泄露";
        pwned_found_prefix => "password(s) found in known breaches:", "个密码出现在已知泄露库中：";
        // Add-site form
        new_site => "New site", "新站点";
        site => "Site", "站点";
        user => "User", "用户名";
        counter => "Counter", "计数器";
        length => "Length", "长度";
        charset => "Charset", "字符集";
        notes => "Notes", "备注";
        save => "Save", "保存";
        site_empty => "✗ Site name cannot be empty", "✗ 站点名不能为空";
        counter_invalid => "✗ Counter must be a positive integer", "✗ 计数器必须是正整数";
        length_invalid => "✗ Length must be a positive integer", "✗ 长度必须是正整数";
        added => "Added", "已添加";
        length_charset_warn => "⚠ Length and charset feed into the derived password, but aren't part of your seed phrase. If you change them from the defaults, you'll need to remember these exact settings (or keep this sites file) to regenerate this password later.",
            "⚠ 长度和字符集参与密码派生，但不属于助记词。如果改过默认值，之后必须记住这些设置（或保留本站点文件）才能重新生成相同密码。";
        // Language
        language => "Language", "语言";
    }

    /// Window title for the GUI, in the selected language.
    pub fn window_title(self) -> String {
        match self {
            Lang::En => "hdpassw".to_string(),
            Lang::Zh => "hdpassw 密码管理器".to_string(),
        }
    }

    /// "Your seed phrase (24 words):" with language-appropriate punctuation.
    pub fn seed_phrase_label(self, words: usize) -> String {
        match self {
            Lang::En => format!("Your seed phrase ({words} words):"),
            Lang::Zh => format!("你的助记词（{words} 个单词）："),
        }
    }
}
