//! Page-lifetime language selection. The first authorized page fixes process-owned native labels.
//! Stored UI language is read by each page before paint; no review state is reloaded to apply it.
use std::sync::atomic::{AtomicU8, Ordering};
use tauri::{Manager, WebviewWindow};
#[derive(Default)]
pub struct Localization(AtomicU8);
impl Localization {
    fn select(&self, locale: &str) -> Result<(), String> {
        let value = match locale {
            "en" | "qps-ploc" => 1,
            "fr" => 2,
            _ => return Err("Unsupported interface language".into()),
        };
        let _ = self
            .0
            .compare_exchange(0, value, Ordering::SeqCst, Ordering::SeqCst);
        Ok(())
    }
    pub fn text<'a>(&self, english: &'a str, french: &'a str) -> &'a str {
        if self.0.load(Ordering::SeqCst) == 2 {
            french
        } else {
            english
        }
    }
}
#[tauri::command]
pub fn desktop_locale(window: WebviewWindow, locale: String) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    window.state::<Localization>().select(&locale)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_is_allowlisted_and_cannot_change_under_live_windows() {
        let locale = Localization::default();
        assert!(locale.select("../fr").is_err());
        assert_eq!(locale.text("en", "fr"), "en");
        locale.select("fr").unwrap();
        locale.select("en").unwrap();
        assert_eq!(locale.text("en", "fr"), "fr");
    }
}
