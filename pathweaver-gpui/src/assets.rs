//! Replace the SVG sources and rebuild to customize the provisional toolbar.
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;
pub const UI_FONT: &str = "Hiragino Sans";
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/select.svg" => include_bytes!("../assets/icons/select.svg"),
            "icons/direct.svg" => include_bytes!("../assets/icons/direct.svg"),
            "icons/anchor.svg" => include_bytes!("../assets/icons/anchor.svg"),
            "icons/pen.svg" => include_bytes!("../assets/icons/pen.svg"),
            "icons/width.svg" => include_bytes!("../assets/icons/width.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok([
            "icons/select.svg",
            "icons/direct.svg",
            "icons/anchor.svg",
            "icons/pen.svg",
            "icons/width.svg",
        ]
        .into_iter()
        .filter(|p| p.starts_with(path))
        .map(Into::into)
        .collect())
    }
}
