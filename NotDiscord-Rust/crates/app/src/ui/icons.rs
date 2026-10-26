use eframe::egui::{Color32, Painter, Rect, Shape, Stroke, pos2};

/// The call outline used by both the conversation action and call history.
pub(super) fn paint_call(painter: &Painter, rect: Rect, color: Color32) {
    let p = |x: f32, y: f32| {
        pos2(
            rect.left() + rect.width() * x / 24.,
            rect.top() + rect.height() * y / 24.,
        )
    };
    let points = [
        (5., 2.),
        (9., 7.),
        (6., 10.),
        (9., 15.),
        (14., 18.),
        (17., 15.),
        (22., 19.),
        (20., 22.),
        (16., 22.),
        (10., 18.),
        (5., 12.),
        (2., 6.),
        (2., 4.),
        (5., 2.),
    ]
    .map(|(x, y)| p(x, y));
    painter.add(Shape::line(points.to_vec(), Stroke::new(1.7_f32, color)));
}

/// One pin outline shared by the message toolbar, pinned-message header and navigation.
pub(super) fn paint_pin(painter: &Painter, rect: Rect, color: Color32) {
    let p = |x: f32, y: f32| {
        pos2(
            rect.left() + rect.width() * x / 24.,
            rect.top() + rect.height() * y / 24.,
        )
    };
    let stroke = Stroke::new((rect.width() * 0.077).clamp(1.1, 1.8), color);
    painter.add(Shape::closed_line(
        vec![
            p(9., 2.),
            p(19., 9.),
            p(16., 11.),
            p(16., 17.),
            p(7., 10.),
            p(12., 8.),
        ],
        stroke,
    ));
    painter.line_segment([p(9., 14.), p(3., 22.)], stroke);
}
