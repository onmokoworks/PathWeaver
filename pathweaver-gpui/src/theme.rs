//! Shared UI roles. Artwork colors are never passed through this palette.
//! Control dimensions follow Tweeq: 12 px type, 24 px inputs, 4 px radius.
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
static DARK: AtomicBool = AtomicBool::new(false);
#[derive(Clone, Copy)]
pub enum Role {
    Background,
    Surface,
    Toolbar,
    Input,
    Button,
    Hover,
    Border,
    Text,
    Muted,
    Accent,
    Selected,
    OnAccent,
    Soft,
    Canvas,
    Grid,
    NodeCanvas,
    Node,
    NodeMuted,
    NodeIo,
    NodeNumber,
    NodePaint,
    NodeCombine,
    NodeGeometry,
    ShapeSocket,
    NumberSocket,
    Guide,
    Error,
    Warning,
}
use Role::*;
pub fn color(role: Role) -> u32 {
    palette(role, is_dark())
}
fn palette(role: Role, dark: bool) -> u32 {
    match (role, dark) {
        (Background, false) => 0xffffff,
        (Background, true) => 0x111111,
        (Surface, false) => 0xfcfcfd,
        (Surface, true) => 0x19191b,
        (Toolbar, false) => 0xf9f9fb,
        (Toolbar, true) => 0x1c1c1f,
        (Input, false) => 0xf0f0f3,
        (Input, true) => 0x232326,
        (Button, false) => 0xe8e8ec,
        (Button, true) => 0x2e2e32,
        (Hover, false) => 0xe0e1e6,
        (Hover, true) => 0x39393f,
        (Border, false) => 0xd9d9e0,
        (Border, true) => 0x36363d,
        (Text, false) => 0x202024,
        (Text, true) => 0xeeeef0,
        (Muted, false) => 0x606069,
        (Muted, true) => 0xb0b0b9,
        (Accent, false) => 0x0000ff,
        (Accent, true) => 0xa2a2ff,
        (Selected, _) => 0x3e3ee8,
        (OnAccent, _) => 0xffffff,
        (Soft, false) => 0xe4e4ff,
        (Soft, true) => 0x34345d,
        (Canvas, false) => 0xffffff,
        (Canvas, true) => 0x161618,
        (Grid, false) => 0xf0f0f3,
        (Grid, true) => 0x242429,
        (NodeCanvas, false) => 0xf5f5f7,
        (NodeCanvas, true) => 0x111113,
        (Node, false) => 0xffffff,
        (Node, true) => 0x242427,
        (NodeMuted, false) => 0xe8e8ec,
        (NodeMuted, true) => 0x1b1b1e,
        (NodeIo, false) => 0xf3dce0,
        (NodeIo, true) => 0x4f2f38,
        (NodeNumber, false) => 0xeadcf5,
        (NodeNumber, true) => 0x403052,
        (NodePaint, false) => 0xf6e6c7,
        (NodePaint, true) => 0x493d23,
        (NodeCombine, false) => 0xd5ecdf,
        (NodeCombine, true) => 0x294535,
        (NodeGeometry, false) => 0xdae7f9,
        (NodeGeometry, true) => 0x2b3e56,
        (ShapeSocket, false) => 0x287a50,
        (ShapeSocket, true) => 0x88c7a4,
        (NumberSocket, false) => 0x8650ad,
        (NumberSocket, true) => 0xc2a1e1,
        (Guide, false) => 0x008b91,
        (Guide, true) => 0x55cbd1,
        (Error, false) => 0xce2c31,
        (Error, true) => 0xff9592,
        (Warning, false) => 0x9c5200,
        (Warning, true) => 0xffc080,
    }
}
pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}
fn preferences() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|p| PathBuf::from(p).join("Library/Application Support/PathWeaver/appearance.txt"))
}
pub fn initialize() {
    let dark = preferences()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "dark");
    DARK.store(dark, Ordering::Relaxed);
}
pub fn toggle() -> Result<(), String> {
    let dark = !is_dark();
    DARK.store(dark, Ordering::Relaxed);
    if let Some(p) = preferences() {
        std::fs::create_dir_all(p.parent().unwrap())
            .and_then(|_| std::fs::write(p, if dark { "dark" } else { "light" }))
            .map_err(|e| format!("配色は変更しました。設定の保存に失敗しました：{e}"))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: u32) -> f64 {
        let channel = |s: u32| {
            let v = ((c >> s) & 255) as f64 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
    #[test]
    fn text_and_focus_have_readable_contrast_in_both_modes() {
        for dark in [false, true] {
            for (text, bg) in [
                (Text, Surface),
                (Text, Input),
                (Muted, Surface),
                (OnAccent, Selected),
                (Accent, Input),
            ] {
                let a = luminance(palette(text, dark));
                let b = luminance(palette(bg, dark));
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }
}
