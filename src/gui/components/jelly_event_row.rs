use gpui::{IntoElement, ParentElement, SharedString, Styled as _, div, px, relative};
use gpui_component::h_flex;

use crate::gui::rendering::jelly_image_cache::JellySurfaceImage;
use crate::gui::state::events::EventLine;
use crate::gui::theme::Palette;
use crate::gui::views::primitives::{event_color, status_dot};

/// 事件行：中性卡片底 + 左侧类型色点 + 文字。
/// 不再按 tone 整行横向渐变（旧版渐变方向与内容无关、视觉偏花），类型区分完全交给
/// 左侧 status_dot 的颜色。`image` 暂保留以兼容调用方签名，但不再贴 surface bitmap。
pub(crate) fn jelly_event_row(
    line: &EventLine,
    palette: &Palette,
    _image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let color = event_color(line.kind, palette);

    h_flex()
        .w_full()
        .min_h(px(38.))
        .gap(px(10.))
        .items_start()
        .px(px(12.))
        .py(px(9.))
        .rounded(px(9.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_soft)
        .child(status_dot(color))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(12.5))
                .line_height(relative(1.35))
                .text_color(palette.text)
                .child(SharedString::from(line.text.clone())),
        )
}
