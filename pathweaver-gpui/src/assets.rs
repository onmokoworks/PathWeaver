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
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/fit.svg" => include_bytes!("../assets/icons/fit.svg"),
            "icons/nodes.svg" => include_bytes!("../assets/icons/nodes.svg"),
            "icons/sun.svg" => include_bytes!("../assets/icons/sun.svg"),
            "icons/moon.svg" => include_bytes!("../assets/icons/moon.svg"),
            "icons/eye.svg" => include_bytes!("../assets/icons/eye.svg"),
            "icons/eye-off.svg" => include_bytes!("../assets/icons/eye-off.svg"),
            "icons/library.svg" => include_bytes!("../assets/icons/library.svg"),
            "icons/sliders.svg" => include_bytes!("../assets/icons/sliders.svg"),
            "icons/chevron-down.svg" => include_bytes!("../assets/icons/chevron-down.svg"),
            "icons/power.svg" => include_bytes!("../assets/icons/power.svg"),
            "icons/trash.svg" => include_bytes!("../assets/icons/trash.svg"),
            "icons/copy.svg" => include_bytes!("../assets/icons/copy.svg"),
            "icons/check.svg" => include_bytes!("../assets/icons/check.svg"),
            "icons/close.svg" => include_bytes!("../assets/icons/close.svg"),
            "icons/folder-open.svg" => include_bytes!("../assets/icons/folder-open.svg"),
            "icons/save.svg" => include_bytes!("../assets/icons/save.svg"),
            "icons/undo.svg" => include_bytes!("../assets/icons/undo.svg"),
            "icons/redo.svg" => include_bytes!("../assets/icons/redo.svg"),
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
            "icons/plus.svg",
            "icons/fit.svg",
            "icons/nodes.svg",
            "icons/sun.svg",
            "icons/moon.svg",
            "icons/eye.svg",
            "icons/eye-off.svg",
            "icons/library.svg",
            "icons/sliders.svg",
            "icons/chevron-down.svg",
            "icons/power.svg",
            "icons/trash.svg",
            "icons/copy.svg",
            "icons/check.svg",
            "icons/close.svg",
            "icons/folder-open.svg",
            "icons/save.svg",
            "icons/undo.svg",
            "icons/redo.svg",
        ]
        .into_iter()
        .filter(|p| p.starts_with(path))
        .map(Into::into)
        .collect())
    }
}
