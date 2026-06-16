use gpui::{
    Bounds, Corners, IntoElement, ParentElement, SharedString, Styled as _, Window, canvas, div,
    linear_color_stop, linear_gradient, point, px, relative, size,
};
use gpui_component::h_flex;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::rendering::jelly_image_cache::JellySurfaceImage;
use crate::gui::state::events::EventLine;
use crate::gui::theme::Palette;
use crate::gui::views::primitives::{event_color, status_dot};

pub(crate) fn jelly_event_row(
    line: &EventLine,
    palette: &Palette,
    image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let material = JellyMaterialToken::for_event(line.kind, palette);
    let color = event_color(line.kind, palette);
    let has_image = image.is_some();
    let backing_alpha = if has_image { 0.025 } else { 0.05 };
    let border_alpha = if has_image { 0.12 } else { 0.16 };
    let shadow_alpha = if has_image { 0.05 } else { 0.08 };

    let row = h_flex()
        .relative()
        .w_full()
        .min_h(px(42.))
        .gap(px(8.))
        .items_start()
        .p(px(8.))
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
            offset: gpui::point(px(0.), px(4.)),
            blur_radius: px(10.),
            spread_radius: px(-6.),
        }]);

    let row = if let Some(image) = image {
        row.child(
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
                        Corners::from(px(10.)),
                        image.image.clone(),
                        0,
                        false,
                    );
                },
            )
            .absolute()
            .inset_0(),
        )
    } else {
        row
    };

    row.child(status_dot(color)).child(
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .text_size(px(12.))
            .line_height(relative(1.3))
            .text_color(palette.text)
            .child(SharedString::from(line.text.clone())),
    )
}
