use gpui::{
    FontWeight, IntoElement, ParentElement, SharedString, Styled as _, div, linear_color_stop,
    linear_gradient, px, relative,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::materials::JellyMaterialToken;
use crate::gui::state::events::{EventKind, EventLine};
use crate::gui::state::results::{FailureItem, ResultItem, ResultKind};
use crate::gui::theme::Palette;
use crate::gui::views::primitives::{event_color, status_badge, status_dot};

pub(crate) fn event_row(line: &EventLine, palette: &Palette) -> impl IntoElement {
    let material = JellyMaterialToken::for_event(line.kind, palette);
    h_flex()
        .w_full()
        .gap(px(8.))
        .items_start()
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(material.rim.opacity(0.16))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.05), 0.0),
            linear_color_stop(material.shell_end.opacity(0.08), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(0.08),
            offset: gpui::point(px(0.), px(4.)),
            blur_radius: px(10.),
            spread_radius: px(-6.),
        }])
        .child(status_dot(event_color(line.kind, palette)))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.text)
                .child(SharedString::from(line.text.clone())),
        )
}

pub(crate) fn result_row(item: &ResultItem, palette: &Palette) -> impl IntoElement {
    let kind_label = match item.kind {
        ResultKind::Comments => "评论",
        ResultKind::Danmaku => "弹幕",
    };
    let kind = match item.kind {
        ResultKind::Comments => EventKind::Comments,
        ResultKind::Danmaku => EventKind::Danmaku,
    };
    let material = JellyMaterialToken::for_event(kind, palette);
    h_flex()
        .w_full()
        .gap(px(10.))
        .items_start()
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(material.rim.opacity(0.16))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.06), 0.0),
            linear_color_stop(material.shell_end.opacity(0.08), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(0.08),
            offset: gpui::point(px(0.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(-8.),
        }])
        .child(status_badge(kind_label, kind, palette))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(5.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "{} · 扫描 {} · 新增 {}",
                            item.bvid, item.scanned, item.appended
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(SharedString::from(item.extra.clone())),
                )
                .children(item.outputs.iter().map(|path| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(format!("文件：{}", path.display()))
                })),
        )
}

pub(crate) fn failure_row(failure: &FailureItem, palette: &Palette) -> impl IntoElement {
    let material = JellyMaterialToken::for_tone(crate::gui::materials::JellyTone::Error, palette);
    h_flex()
        .gap(px(8.))
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(material.rim.opacity(0.18))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.06), 0.0),
            linear_color_stop(material.shell_end.opacity(0.08), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(0.08),
            offset: gpui::point(px(0.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(-8.),
        }])
        .child(status_dot(material.state_aura))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.error)
                .child(format!(
                    "{} · {} · {}",
                    failure.kind, failure.bvid, failure.error
                )),
        )
}

pub(crate) fn empty_result_state(palette: &Palette) -> impl IntoElement {
    v_flex()
        .gap(px(6.))
        .p(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_soft)
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("暂无结果"),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .line_height(relative(1.3))
                .child("完成后这里会显示评论、弹幕、失败项和输出文件。"),
        )
}
