//! Original vector-drawn chrome surfaces; no reference artwork is bundled.
use eframe::egui::{
    self, Color32, FontId, Painter, Rect, RichText, Shape, Stroke, StrokeKind, Vec2,
};

const INK: Color32 = Color32::from_rgb(20, 38, 58);
const SILVER: Color32 = Color32::from_rgb(194, 214, 232);
const SHADOW: Color32 = Color32::from_rgb(19, 30, 44);
const PANEL: Color32 = Color32::from_rgb(39, 53, 70);
const SCREEN: Color32 = Color32::from_rgb(21, 35, 51);
const RUBY: Color32 = Color32::from_rgb(175, 12, 38);

fn mix(a: Color32, b: Color32, fraction: f32) -> Color32 {
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * fraction).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

fn gradient(rect: Rect, stops: &[(f32, Color32)], radius: f32) -> Shape {
    let mut mesh = egui::epaint::Mesh::default();
    let radius = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    let inset = |y: f32| {
        let edge = (y - rect.top()).min(rect.bottom() - y).max(0.0);
        if edge < radius {
            radius - (radius * radius - (radius - edge).powi(2)).max(0.0).sqrt()
        } else {
            0.0
        }
    };
    let sample = |fraction: f32| {
        for pair in stops.windows(2) {
            if fraction <= pair[1].0 {
                return mix(
                    pair[0].1,
                    pair[1].1,
                    ((fraction - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0.0, 1.0),
                );
            }
        }
        stops.last().unwrap().1
    };
    // Fine strips keep rounded corners smooth without a raster texture.
    for i in 0..64 {
        let a = i as f32 / 64.0;
        let b = (i + 1) as f32 / 64.0;
        let top = egui::lerp(rect.y_range(), a);
        let bottom = egui::lerp(rect.y_range(), b);
        let index = mesh.vertices.len() as u32;
        for (pos, color) in [
            (egui::pos2(rect.left() + inset(top), top), sample(a)),
            (egui::pos2(rect.right() - inset(top), top), sample(a)),
            (egui::pos2(rect.left() + inset(bottom), bottom), sample(b)),
            (egui::pos2(rect.right() - inset(bottom), bottom), sample(b)),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices.extend_from_slice(&[
            index,
            index + 1,
            index + 2,
            index + 2,
            index + 1,
            index + 3,
        ]);
    }
    Shape::mesh(mesh)
}

fn metal(rect: Rect, radius: f32) -> Shape {
    gradient(
        rect,
        &[
            (0.0, Color32::from_rgb(229, 240, 249)),
            (0.12, Color32::from_rgb(148, 173, 194)),
            (0.21, Color32::from_rgb(65, 88, 114)),
            (0.25, Color32::from_rgb(166, 188, 207)),
            (0.30, Color32::from_rgb(53, 74, 99)),
            (1.0, Color32::from_rgb(8, 18, 32)),
        ],
        radius,
    )
}

pub fn chassis(rect: Rect) -> Shape {
    let inner = rect.shrink(1.5);
    Shape::Vec(vec![
        Shape::rect_filled(rect.translate(egui::vec2(0.0, 1.0)), 0, SHADOW),
        metal(rect, 0.0),
        Shape::rect_stroke(rect, 0, Stroke::new(0.75, SILVER), StrokeKind::Inside),
        Shape::rect_filled(inner, 0, PANEL),
        Shape::rect_stroke(inner, 0, Stroke::new(0.75, SHADOW), StrokeKind::Inside),
    ])
}

pub fn inset_chassis(rect: Rect) -> Shape {
    Shape::Vec(vec![
        metal(rect, 0.0),
        Shape::rect_stroke(rect, 0, Stroke::new(0.75, SILVER), StrokeKind::Inside),
        gradient(
            rect.shrink(1.5),
            &[
                (0.0, Color32::BLACK),
                (0.2, SCREEN),
                (1.0, Color32::from_rgb(2, 8, 18)),
            ],
            0.0,
        ),
        Shape::rect_stroke(
            rect.shrink(1.5),
            0,
            Stroke::new(0.75, SHADOW),
            StrokeKind::Inside,
        ),
    ])
}

pub fn button(ui: &mut egui::Ui, text: &str, enabled: bool, color: Color32) -> egui::Response {
    let background = ui.painter().add(Shape::Noop);
    // Keep phone touch areas generous while the visible control remains compact.
    let touch_layout = cfg!(target_os = "android") || ui.ctx().screen_rect().width() < 500.0;
    let silver = color == Color32::from_rgb(43, 55, 71);
    let foreground = if enabled {
        Color32::from_rgb(241, 249, 255)
    } else {
        Color32::from_rgb(183, 197, 211)
    };
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(foreground))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(0.0, if touch_layout { 48.0 } else { 34.0 }))
            .corner_radius(0),
    );
    let rect = Rect::from_center_size(
        response.rect.center(),
        Vec2::new(response.rect.width(), 34.0),
    );
    let pressed = response.is_pointer_button_down_on();
    let base = if enabled {
        color
    } else {
        Color32::from_rgb(47, 63, 80)
    };
    let face = if silver && enabled {
        metal(rect.shrink(1.0), 0.0)
    } else {
        gradient(
            rect.shrink(1.0),
            &[
                (
                    0.0,
                    mix(
                        base,
                        Color32::from_rgb(185, 223, 255),
                        if pressed { 0.08 } else { 0.50 },
                    ),
                ),
                (
                    0.18,
                    mix(base, Color32::WHITE, if pressed { 0.0 } else { 0.18 }),
                ),
                (0.23, mix(base, Color32::BLACK, 0.18)),
                (0.70, mix(base, Color32::BLACK, 0.55)),
                (1.0, mix(base, Color32::BLACK, 0.90)),
            ],
            0.0,
        )
    };
    let mut shapes = vec![
        metal(rect, 0.0),
        face,
        Shape::rect_stroke(
            rect,
            0,
            Stroke::new(
                0.75,
                if enabled {
                    SILVER
                } else {
                    Color32::from_rgb(88, 108, 130)
                },
            ),
            StrokeKind::Inside,
        ),
    ];
    if enabled {
        let shadow_color = Color32::from_rgb(1, 5, 12);
        let shadow =
            ui.painter()
                .layout_no_wrap(text.to_owned(), FontId::monospace(14.0), shadow_color);
        let position = rect.center() - shadow.size() / 2.0 + egui::vec2(0.0, 1.0);
        shapes.push(Shape::galley(position, shadow, shadow_color));
    }
    if pressed {
        shapes.push(Shape::rect_stroke(
            rect.shrink(1.0),
            0,
            Stroke::new(1.0, SHADOW),
            StrokeKind::Inside,
        ));
    }
    if response.hovered() && enabled {
        shapes.push(Shape::rect_stroke(
            rect,
            0,
            Stroke::new(1.0, Color32::from_rgb(158, 220, 255)),
            StrokeKind::Inside,
        ));
    }
    if response.has_focus() {
        shapes.push(Shape::rect_stroke(
            rect.expand(1.0),
            0,
            Stroke::new(1.0, Color32::from_rgb(158, 220, 255)),
            StrokeKind::Inside,
        ));
    }
    ui.painter().set(background, Shape::Vec(shapes));
    response
}

pub fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let background = ui.painter().add(Shape::Noop);
    let frame = egui::Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (title_rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), egui::Sense::hover());
            ui.painter().add(metal(title_rect, 0.0));
            ui.painter().add(gradient(
                Rect::from_min_size(
                    title_rect.min + egui::vec2(2.0, 2.0),
                    Vec2::new(2.0, title_rect.height() - 4.0),
                ),
                &[
                    (0.0, Color32::from_rgb(252, 146, 166)),
                    (0.18, RUBY),
                    (1.0, Color32::from_rgb(38, 0, 11)),
                ],
                0.0,
            ));
            ui.painter().text(
                title_rect.left_center() + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                title,
                FontId::monospace(13.0),
                Color32::from_rgb(230, 244, 255),
            );
            // Etched grooves describe the physical panel without suggesting device status.
            for i in 0..3 {
                let x = title_rect.right() - 10.0 - i as f32 * 5.0;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, title_rect.top() + 8.0),
                        egui::pos2(x, title_rect.bottom() - 8.0),
                    ],
                    Stroke::new(1.0, Color32::from_rgb(84, 110, 135)),
                );
            }
            ui.add_space(6.0);
            body(ui);
        });
    ui.painter().set(background, chassis(frame.response.rect));
}

pub fn header(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), egui::Sense::hover());
    ui.painter().add(metal(rect, 0.0));
    ui.painter().add(gradient(
        Rect::from_min_size(rect.min + egui::vec2(4.0, 7.0), Vec2::new(2.0, 34.0)),
        &[
            (0.0, Color32::from_rgb(250, 136, 159)),
            (0.18, RUBY),
            (1.0, Color32::from_rgb(17, 0, 6)),
        ],
        0.0,
    ));
    ui.painter().add(gradient(
        Rect::from_min_size(rect.min + egui::vec2(12.0, 29.0), Vec2::new(138.0, 2.0)),
        &[
            (0.0, Color32::from_rgb(245, 114, 142)),
            (0.4, RUBY),
            (1.0, Color32::from_rgb(35, 0, 10)),
        ],
        0.0,
    ));
    ui.painter()
        .rect_stroke(rect, 0, Stroke::new(0.75, SILVER), StrokeKind::Inside);
    let name = rect.min + egui::vec2(12.0, 4.0);
    ui.painter().text(
        name + egui::vec2(1.0, 1.0),
        egui::Align2::LEFT_TOP,
        "ANE-200W",
        FontId::monospace(24.0),
        Color32::from_rgb(1, 5, 12),
    );
    ui.painter().text(
        name,
        egui::Align2::LEFT_TOP,
        "ANE-200W",
        FontId::monospace(24.0),
        Color32::from_rgb(230, 244, 255),
    );
    ui.painter().text(
        rect.min + egui::vec2(12.0, 34.0),
        egui::Align2::LEFT_TOP,
        "AIR VELOCITY / FIELD CONSOLE",
        FontId::monospace(9.0),
        SILVER,
    );
    let vent_left = (rect.right() - 80.0).max(rect.left() + 230.0);
    if vent_left + 40.0 < rect.right() {
        ui.painter().add(gradient(
            Rect::from_min_max(
                egui::pos2(vent_left - 6.0, rect.top() + 10.0),
                egui::pos2(rect.right() - 8.0, rect.top() + 36.0),
            ),
            &[
                (0.0, Color32::from_rgb(253, 160, 183)),
                (0.14, mix(RUBY, Color32::WHITE, 0.10)),
                (0.20, RUBY),
                (1.0, Color32::from_rgb(20, 0, 8)),
            ],
            0.0,
        ));
        for i in 0..5 {
            let y = rect.top() + 14.0 + i as f32 * 4.0;
            ui.painter().line_segment(
                [egui::pos2(vent_left, y), egui::pos2(rect.right() - 14.0, y)],
                Stroke::new(1.0, INK),
            );
        }
    }
}

pub fn status_surface(painter: &Painter, rect: Rect) {
    painter.add(gradient(
        rect,
        &[
            (0.0, Color32::from_rgb(57, 78, 101)),
            (0.12, Color32::from_rgb(37, 54, 73)),
            (1.0, Color32::from_rgb(28, 42, 57)),
        ],
        0.0,
    ));
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, SILVER),
    );
}
