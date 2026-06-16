use gpui::{Hsla, rgb};

#[derive(Clone, Copy)]
pub struct Palette {
    pub app_bg: Hsla,
    pub surface: Hsla,
    pub surface_soft: Hsla,
    pub event_bg: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_2: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            app_bg: rgb(0xf6f8fb).into(),
            surface: rgb(0xffffff).into(),
            surface_soft: rgb(0xf8fbff).into(),
            event_bg: rgb(0xfbfcfe).into(),
            border: rgb(0xd9e2ec).into(),
            text: rgb(0x172033).into(),
            muted: rgb(0x667085).into(),
            accent: rgb(0x15c8d8).into(),
            accent_2: rgb(0xfb7299).into(),
            success: rgb(0x18a66a).into(),
            warning: rgb(0xc47a10).into(),
            error: rgb(0xd92d20).into(),
        }
    }
}
