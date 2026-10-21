//! Language choice for the strings the backend itself shows (tray menu,
//! tooltip, notifications, window titles). The webviews localise themselves.

use crate::models::AppSettings;

pub fn use_chinese(settings: &AppSettings) -> bool {
    match settings.language.as_str() {
        "zh" => true,
        "en" => false,
        _ => sys_locale::get_locale()
            .map(|l| l.to_lowercase().starts_with("zh"))
            .unwrap_or(false),
    }
}

pub fn tr(zh: bool, en: &'static str, cn: &'static str) -> &'static str {
    if zh {
        cn
    } else {
        en
    }
}
