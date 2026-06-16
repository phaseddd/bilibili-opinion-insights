use gpui::{
    Bounds, Corners, FontWeight, IntoElement, ParentElement, SharedString, Styled as _, Window,
    canvas, div, linear_color_stop, linear_gradient, point, px, relative, size,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::materials::JellyMaterialToken;
use crate::gui::rendering::jelly_image_cache::JellySurfaceImage;
use crate::gui::state::events::EventKind;
use crate::gui::state::results::{FailureItem, ResultItem, ResultKind};
use crate::gui::theme::Palette;
use crate::gui::views::primitives::{status_badge, status_dot};

pub(crate) fn result_row(
    item: &ResultItem,
    palette: &Palette,
    image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let kind_label = match item.kind {
        ResultKind::Comments => "评论",
        ResultKind::Danmaku => "弹幕",
    };
    let kind = match item.kind {
        ResultKind::Comments => EventKind::Comments,
        ResultKind::Danmaku => EventKind::Danmaku,
    };
    let material = JellyMaterialToken::for_event(kind, palette);
    let has_image = image.is_some();
    let backing_alpha = if has_image { 0.025 } else { 0.06 };
    let border_alpha = if has_image { 0.12 } else { 0.16 };
    let shadow_alpha = if has_image { 0.05 } else { 0.08 };

    let row = h_flex()
        .relative()
        .w_full()
        .gap(px(10.))
        .items_start()
        .p(px(10.))
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(material.rim.opacity(border_alpha))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(backing_alpha), 0.0),
            linear_color_stop(material.shell_end.opacity(backing_alpha + 0.02), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(shadow_alpha),
            offset: gpui::point(px(0.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(-8.),
        }]);

    let row = if let Some(image) = image {
        row.child(surface_background(image, 10.))
    } else {
        row
    };

    row.child(status_badge(kind_label, kind, palette)).child(
        v_flex()
            .relative()
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

pub(crate) fn failure_row(
    failure: &FailureItem,
    palette: &Palette,
    image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let material = JellyMaterialToken::for_tone(crate::gui::materials::JellyTone::Error, palette);
    let has_image = image.is_some();
    let backing_alpha = if has_image { 0.025 } else { 0.06 };
    let border_alpha = if has_image { 0.12 } else { 0.18 };
    let shadow_alpha = if has_image { 0.05 } else { 0.08 };

    let row = h_flex()
        .relative()
        .w_full()
        .gap(px(8.))
        .p(px(10.))
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(material.rim.opacity(border_alpha))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(backing_alpha), 0.0),
            linear_color_stop(material.shell_end.opacity(backing_alpha + 0.02), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(shadow_alpha),
            offset: gpui::point(px(0.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(-8.),
        }]);

    let row = if let Some(image) = image {
        row.child(surface_background(image, 10.))
    } else {
        row
    };

    row.child(status_dot(material.state_aura)).child(
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .text_size(px(12.))
            .line_height(relative(1.3))
            .text_color(palette.error)
            .child(format!(
                "{} · {} · {}",
                failure.kind, failure.bvid, failure.error
            )),
    )
}

fn surface_background(image: JellySurfaceImage, radius: f32) -> impl IntoElement {
    canvas(
        move |_, _window: &mut Window, _cx| (),
        move |bounds, _, window: &mut Window, _cx| {
            let origin_x = f32::from(bounds.origin.x);
            let origin_y = f32::from(bounds.origin.y);
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let image_bounds = Bounds::new(
                point(px(origin_x), px(origin_y)),
                size(px(width), px(height)),
            );
            let _ = window.paint_image(
                image_bounds,
                Corners::from(px(radius)),
                image.image.clone(),
                0,
                false,
            );
        },
    )
    .absolute()
    .inset_0()
}

pub(crate) fn empty_result_state(
    palette: &Palette,
    image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let has_image = image.is_some();
    let container = v_flex()
        .relative()
        .gap(px(6.))
        .p(px(12.))
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(palette.border)
        .bg(if has_image {
            palette.surface_soft.opacity(0.62)
        } else {
            palette.surface_soft
        });

    let container = if let Some(image) = image {
        container.child(surface_background(image, 10.))
    } else {
        container
    };

    container
        .child(
            div()
                .relative()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("暂无结果"),
        )
        .child(
            div()
                .relative()
                .text_size(px(11.))
                .text_color(palette.muted)
                .line_height(relative(1.3))
                .child("完成后这里会显示评论、弹幕、失败项和输出文件。"),
        )
}
