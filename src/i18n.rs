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
        .args([
            "query",
            "HKCU\\Control Panel\\International",
            "/v",
            "LocaleName",
        ])
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
        brand_local => "This device", "本机";
        more => "More", "更多";
        vault_pw_sub => "This password only unlocks the vault on this machine and cannot be recovered. A long sentence you will remember works well.",
            "这个密码只用于解锁本机密码库，忘记后无法找回。建议用一句只有你记得住的长密码。";
        create_vault_unlock => "Create vault & unlock", "创建密码库并解锁";
        restore_hint => "Pick a suggestion below to fill it in, or just type.", "可点选下面的候选词快速填入，或直接输入。";
        word_no => "Word", "第";
        word_suffix => "", " 个词";
        vault_password => "Unlock password", "解锁密码";
        unlock => "Unlock", "解锁";
        unlocking => "Unlocking…", "正在解锁…";
        unlocking_hint => "Deriving the key (about a second)…", "正在派生密钥（约需一秒）…";
        unlock_wrong_pw => "Unlock password is incorrect, please try again.", "解锁密码不正确，请重试。";
        // Setup wizard
        no_vault => "No vault found yet.", "尚未创建密码库。";
        no_accounts_yet => "No accounts yet, add your first one.", "还没有账号，先添加一个";
        words_suffix => "words", "个单词";
        write_it_down => "Whoever holds these 24 words can generate all your passwords. They will not be shown again after setup.",
            "谁拿到这 24 个词，就能生成你所有的密码。完成创建后，不会再显示。";
        i_wrote_it_down => "I have written this down", "我已抄写完毕";
        copy_all_words => "Copy all 24 words", "复制全部 24 个词";
        copy_words_hint => "Click any word to copy it on its own.", "点击单个词可单独复制。";
        copy_words_warn => "Copying puts the phrase on the clipboard, where other apps can read it. Prefer paper, and clear the clipboard afterwards.",
            "复制会把恢复词放入剪贴板，其他程序可以读取。优先用纸笔，用完请清空剪贴板。";
        copied_word => "Copied word", "已复制单词";
        copied_words => "✓ Copied all words — clipboard clears in 60s", "✓ 已复制全部恢复词 — 60 秒后清除";
        seed_step_title => "Write down these 24 recovery words", "记下这 24 个恢复词";
        backup_warn => "Write the words down with pen and paper and store them offline — this is the only backup.",
            "请用纸笔抄写并离线保存——这是唯一的备份方式。";
        back => "Back", "上一步";
        continue_ => "Continue", "继续";
        next_step => "Next step", "下一步";
        bump_step2_hint => "Now copy the new password and save it", "现在复制新密码并保存";
        quick_check => "Check the words below and fix any spelling mistakes:",
            "请核对下面几个词，改正拼写错误：";
        quiz_mismatch => "One or more recovery words are wrong. Check them and try again.",
            "有恢复词填错了，请核对后重试。";
        enter_all_words => "Enter your existing 24 words, separated by spaces:", "输入已有的 24 个词，以空格分隔：";
        choose_vault_pw => "Set the unlock password", "设置解锁密码";
        password => "Password", "密码";
        confirm_password => "Confirm password", "确认密码";
        save_vault_unlock => "Save & unlock", "保存并解锁";
        saving => "Saving…", "正在保存…";
        pw_empty => "✗ Password cannot be empty", "✗ 密码不能为空";
        pw_mismatch => "✗ Passwords do not match", "✗ 两次输入的密码不一致";
        vault_created => "✓ Vault created and unlocked", "✓ 密码库已创建并解锁";
        // Main screen
        rotation => "rotation", "轮换";
        lock => "Lock", "锁定";
        cancel => "Cancel", "取消";
        test_if_pwned => "Check breaches", "检查泄露";
        rotate => "Rotate", "全局轮换";
        rotate_hint => "Bump the global rotation counter only — sites are flagged behind, not changed. Use \"Bump\" on a site to actually change its password.",
            "仅提升全局轮换计数器——各账号会被标记为落后，密码不变。要对单个账号真正改密，请用该账号的「更新」按钮。";
        no_sites => "No sites saved yet. Click \"+ Add site\" to create one.", "还没有账号。点击「+ 添加账号」创建一个。";
        generate_copy => "Generate & copy", "生成并复制";
        copy_old_pw => "Copy old password", "复制旧密码";
        copy_new_pw_save => "Copy new password & save", "复制新密码并保存";
        bump => "New password", "换新密码";
        copied_pw => "Copied password — clears in 30s", "已复制密码 — 30 秒后自动清除";
        copied_old_pw => "Copied OLD password — log in, then click \"Copy new password & save\"",
            "已复制旧密码 — 登录后点击「复制新密码并保存」";
        saved_now_at => "Saved: new password copied, paste it on the site",
            "已保存：新密码已复制，请到网站上粘贴";
        bump_cancelled => "Bump cancelled — nothing saved", "已取消——未做任何修改";
        rotation_done_all_caught_up => "every site is already caught up", "所有账号均已是最新";
        rotation_done_behind => "site(s) now behind, use \"Bump\" to change them",
            "个账号已落后，请用「更新」逐个改密";
        pwned_checking => "Checking", "正在检查";
        checking => "Checking…", "检查中…";
        pwned_none => "Checked — none found in known breaches", "检测完成 — 未发现已知泄露";
        pwned_found_prefix => "password(s) found in known breaches:", "个密码出现在已知泄露库中：";
        // Add-site form
        user => "User", "用户名";
        length => "Length", "长度";
        save => "Save", "保存";
        counter_invalid => "✗ Counter must be a positive integer", "✗ 计数器必须是正整数";
        length_invalid => "✗ Length must be a positive integer", "✗ 长度必须是正整数";
        added => "Added", "已添加";
        updated => "Updated", "已更新";
        length_charset_warn => "⚠ Length and charset feed into the derived password, but aren't part of your seed phrase. If you change them from the defaults, you'll need to remember these exact settings (or keep this sites file) to regenerate this password later.",
            "⚠ 长度和字符集参与密码派生，但不属于恢复词。如果改过默认值，之后必须记住这些设置（或保留本清单文件）才能重新生成相同密码。";
        // Language
        language => "Language", "语言";

        // ── New interface (prototype-aligned) ────────────────────────────────
        accounts => "Accounts", "账号";
        export_ => "Export", "导出";
        search_placeholder => "Search site, user or notes", "搜索网站、用户名或备注";
        search_none => "No matching accounts", "没有匹配的账号";
        accounts_count_suffix => "", "个";
        // Rotation banner
        rotation_banner_title => "New passwords", "换新密码";
        rotation_behind_n => "account(s) waiting for a new password", "个账号待换新密码";
        rotation_all_caught_up => "every account uses a new password", "全部账号都已使用新密码";
        bump_all => "Rotate all passwords", "统一换新密码";
        // Chips
        chip_needs_rotation => "Needs new password", "待换新密码";
        chip_leaked => "Leaked", "曾泄露";
        verifier_prefix => "verifier", "校验码";
        // Bump flow
        bump_step1_title => "Step 1: copy the OLD password to log in", "第 1 步：先复制旧密码登录";
        bump_step1_sub => "After logging in, come back to copy the new password and save it. Cancelling changes nothing.",
            "登录成功后回来，再复制新密码并保存。取消不会修改任何记录。";
        bump_step2_title => "Step 2: copy the NEW password and save", "第 2 步：复制新密码并保存";
        bump_step2_sub => "Paste the new password into the site's change-password page, then come back.",
            "把新密码粘贴到网站的修改密码页，保存后回来完成。";
        // Add / edit form
        add_account => "Add account", "添加账号";
        edit_account => "Edit account", "编辑账号";
        edit_ => "Edit", "编辑";
        field_site => "Site", "网站";
        field_user => "Username", "登录用户名";
        field_counter => "Password round", "密码轮次";
        field_length => "Password length", "密码长度";
        field_charset => "Character set", "字符范围";
        field_notes => "Notes", "备注";
        optional => "optional", "可选";
        charset_alpha => "Letters only", "仅字母";
        charset_alphanumeric => "Letters and digits", "字母和数字";
        charset_full => "Letters, digits and symbols", "字母、数字和符号";
        charset_pin => "Digits only", "仅数字";
        charset_hex => "0-9 and a-f", "仅 0–9 和 a–f";
        settings_warn => "Changing length or character set changes the derived password, and they are not part of your seed phrase.",
            "修改长度或字符范围会改变派生出的密码，且它们不属于恢复词。";
        // BIP39 passphrase
        bip39_passphrase => "Extra passphrase (optional)", "附加口令（可选）";
        bip39_hint => "Optional. When restoring on another device you must enter exactly the same text, otherwise every password will differ.",
            "可选。换设备或重装恢复时，必须输入完全相同的内容，否则所有密码都会不同。";
        bip39_hint_short => "Only needed if you set one when creating the vault; leave empty otherwise.", "创建密码库时设置过才需要填；没有设置就留空。";
        // Recover
        recover_link => "Recover password round", "找回密码轮次";
        recover_title => "Recover password round", "找回密码轮次";
        recover_sub => "Scans rounds 1–20 until the 4-character verifier from your metadata backup matches. Nothing is revealed.",
            "从轮次 1 到 20 逐一比对元数据备份里的 4 位校验码。不会泄露任何密码。";
        recover_verifier => "Verifier (4 hex chars)", "校验码（4 位十六进制）";
        recover_find => "Start searching", "开始查找";
        recover_scanning => "Scanning…", "扫描中…";
        recover_found => "Found! Round =", "找到！轮次 =";
        recover_not_found => "Not found — check the site, username and verifier.", "未找到，请检查网站、用户名或校验码。";
        recover_bad_site => "That doesn't look like a full site name, e.g. example.com.", "网站地址看起来不完整，例如 example.com。";
        recover_bad_user => "Enter the username for this account.", "请输入这个账号的登录用户名。";
        recover_bad_code => "The verifier is 4 hexadecimal characters, e.g. a3f1.", "校验码是 4 位十六进制字符，例如 a3f1。";
        // Export
        export_title => "Export metadata", "导出元数据";
        export_sub => "This file contains no passwords — safe to keep in git or a cloud drive.", "此文件不含密码，可放心放进 git 或云盘备份。";
        export_copy => "Copy", "复制";
        export_copied => "Metadata copied", "已复制元数据";
        // Unlock screen
        unlock_sub => "Your passwords are derived, never stored.", "密码是即时派生的，从不保存。";
        unlock_title => "Unlock vault", "解锁密码库";
        unlock_sub2 => "The unlock password is only used on this machine and is never uploaded.",
            "解锁密码只在本机使用，不会上传。";
        create_or_recover => "Create or restore vault", "创建或恢复密码库";
        vault_pw_hint => "Used only on this machine; it cannot be recovered if forgotten.", "只在本机使用，忘记后无法找回。";
        // Wizard
        wizard_create_title => "Create or restore vault", "创建或恢复密码库";
        wizard_create_sub => "The vault only stores your recovery words and account details — never passwords. Each password is generated from them on the spot, and nothing is uploaded.",
            "密码库只保存恢复词和账号资料，不保存密码；每次用恢复词和账号资料现场生成，也不会上传。";
        choice_new_title => "Generate new recovery words", "生成新的恢复词";
        choice_new_sub => "First time here — generate 24 words and verify them.", "第一次使用，生成 24 个词并核对。";
        choice_restore_title => "I have recovery words", "我有恢复词";
        choice_restore_sub => "Enter your existing 24 words to restore your vault.", "输入已有的 24 个词，恢复原来的密码库。";
        step_choose => "Choose", "选择";
        step_write => "Write down", "记下";
        step_verify => "Verify", "核对";
        quiz_title => "Verify recovery words", "核对恢复词";
        step_words => "Recovery words", "恢复词";
        step_password => "Vault password", "解锁密码";
        wiz_next => "Continue", "继续";
        wiz_back_to_unlock => "Back to unlock", "返回解锁";
        // Misc
        delete_ => "Delete", "删除";
        copy_password => "Copy password", "复制密码";
        close => "Close", "关闭";
        site_invalid => "✗ Site must look like a domain, e.g. example.com", "✗ 网站需形如 example.com";
        user_empty => "✗ Username cannot be empty", "✗ 用户名不能为空";
        delete_title => "Delete this account?", "删除这个账号？";
        delete_sub => "The record is removed from the metadata file. You can add it back later — the password is derived from your seed, so it is not lost.",
            "该记录会从元数据文件中移除。之后可以重新添加——密码由恢复词派生，不会丢失。";
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
            Lang::En => format!("Your recovery words ({words} words)"),
            Lang::Zh => format!("你的恢复词（{words} 个单词）"),
        }
    }
}
