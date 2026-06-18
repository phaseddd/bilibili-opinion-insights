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
            app_bg: rgb(0xe7ecf3).into(),
            surface: rgb(0xffffff).into(),
            surface_soft: rgb(0xeef2f8).into(),
            event_bg: rgb(0xf4f7fb).into(),
            border: rgb(0xd0d9e6).into(),
            text: rgb(0x131a2b).into(),
            muted: rgb(0x586374).into(),
            accent: rgb(0x15c8d8).into(),
            accent_2: rgb(0xfb7299).into(),
            success: rgb(0x18a66a).into(),
            warning: rgb(0xd98410).into(),
            error: rgb(0xd92d20).into(),
        }
    }
}
