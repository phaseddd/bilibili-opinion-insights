use gpui::{
    FontWeight, IntoElement, ParentElement, Styled as _, div, hsla, linear_color_stop,
    linear_gradient, px, relative, rgb,
};

use crate::gui::motion::wave_between;
use crate::gui::theme::Palette;

#[derive(Clone, Copy)]
pub enum JellyProgressPhase {
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

pub fn jelly_progress(
    percent: f32,
    phase: JellyProgressPhase,
    motion_tick: u64,
    palette: &Palette,
) -> impl IntoElement {
    let percent = percent.clamp(0., 100.);
    let fill = (percent / 100.).clamp(0.02, 1.);
    let active = matches!(
        phase,
        JellyProgressPhase::Validating
            | JellyProgressPhase::Running
            | JellyProgressPhase::Cancelling
    );
    let pulse = if active {
        wave_between(motion_tick, 0.18, 0.08, 0.22)
    } else {
        0.
    };
    let cap_wobble = match phase {
        JellyProgressPhase::Running => ((motion_tick as f32 * 0.52).sin()) * 3.2,
        JellyProgressPhase::Cancelling => -((motion_tick as f32 * 0.35).sin().abs()) * 3.8,
        JellyProgressPhase::Failed => -1.8,
        _ => 0.,
    };
    let aura = match phase {
        JellyProgressPhase::Completed => palette.success,
        JellyProgressPhase::Failed => palette.error,
        JellyProgressPhase::Cancelling => palette.warning,
        _ => palette.accent,
    };
    let bubble_opacity = if percent > 1. { 0.9 } else { 0.0 };

    gpui_component::v_flex()
        .gap(px(8.))
        .child(
            div()
                .relative()
                .h(px(34.))
                .w_full()
                .rounded(px(999.))
                .overflow_hidden()
                .border_1()
                .border_color(aura.opacity(0.22 + pulse * 0.4))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(rgb(0xeafcff), 0.0),
                    linear_color_stop(rgb(0xffeff5), 1.0),
                ))
                .shadow(vec![gpui::BoxShadow {
                    color: aura.opacity(0.12 + pulse * 0.12),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(28.),
                    spread_radius: px(-18.),
                }])
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .right(px(6.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .rounded(px(999.))
                        .bg(hsla(210., 0.32, 0.18, 0.08)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(5.))
                        .left(px(5.))
                        .bottom(px(5.))
                        .w(relative(fill))
                        .rounded(px(999.))
                        .overflow_hidden()
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(palette.accent, 0.0),
                            linear_color_stop(palette.accent_2, 1.0),
                        ))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: aura.opacity(0.26 + pulse * 0.3),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(18.),
                                spread_radius: px(-10.),
                            },
                            gpui::BoxShadow {
                                color: hsla(0., 0., 1., 0.48),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.),
                            },
                        ])
                        .child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .left(px(16.))
                                .right(px(22.))
                                .h(px(7.))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., 0.36 + pulse * 0.5)),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(-13. + cap_wobble))
                                .top(px(-3.))
                                .w(px(34. + pulse * 12.))
                                .h(px(30. - pulse * 4.))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., bubble_opacity))
                                .border_1()
                                .border_color(hsla(0., 0., 1., 0.72)),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(12. + cap_wobble * 0.4))
                                .top(px(6.))
                                .size(px(9. + pulse * 8.))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., (0.26 + pulse) * bubble_opacity)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(14.))
                        .top(px(8.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(format!("{percent:.0}%")),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(progress_microcopy(phase)),
        )
}

fn progress_microcopy(phase: JellyProgressPhase) -> &'static str {
    match phase {
        JellyProgressPhase::Idle => "等待开始采集。",
        JellyProgressPhase::Validating => "正在校验输入与登录态。",
        JellyProgressPhase::Running => "采集中：进度由真实事件推进。",
        JellyProgressPhase::Cancelling => "正在请求取消，等待 worker 返回。",
        JellyProgressPhase::Completed => "采集完成，结果可复核。",
        JellyProgressPhase::Failed => "采集失败，保留当前进度与错误信息。",
    }
}
