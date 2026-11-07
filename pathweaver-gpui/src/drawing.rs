use crate::app::{Drag, Tool, Workspace};
use gpui::*;
use kurbo::Point as KPoint;
use pathweaver_core::outline;

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pan: [f32; 2],
    pub zoom: f32,
}
impl Default for View {
    fn default() -> Self {
        Self {
            pan: [60., 50.],
            zoom: 1.,
        }
    }
}
impl View {
    pub fn screen(self, p: KPoint, b: Bounds<Pixels>) -> Point<Pixels> {
        point(
            b.left() + px(self.pan[0] + p.x as f32 * self.zoom),
            b.top() + px(self.pan[1] + p.y as f32 * self.zoom),
        )
    }
    pub fn world(self, p: Point<Pixels>, b: Bounds<Pixels>) -> KPoint {
        KPoint::new(
            ((f32::from(p.x - b.left()) - self.pan[0]) / self.zoom) as f64,
            ((f32::from(p.y - b.top()) - self.pan[1]) / self.zoom) as f64,
        )
    }
    pub fn zoom_at(&mut self, p: Point<Pixels>, b: Bounds<Pixels>, factor: f32) {
        let w = self.world(p, b);
        self.zoom = (self.zoom * factor).clamp(0.08, 8.);
        self.pan = [
            f32::from(p.x - b.left()) - w.x as f32 * self.zoom,
            f32::from(p.y - b.top()) - w.y as f32 * self.zoom,
        ];
    }
}
pub fn rect(
    window: &mut Window,
    b: Bounds<Pixels>,
    color: u32,
    border: u32,
    width: f32,
    radius: f32,
) {
    window.paint_quad(quad(
        b,
        px(radius),
        rgb(color),
        px(width),
        rgb(border),
        Default::default(),
    ));
}
pub fn line(window: &mut Window, points: &[Point<Pixels>], color: u32, width: f32) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(color));
    }
}
pub fn text(
    window: &mut Window,
    cx: &mut App,
    at: Point<Pixels>,
    value: impl Into<SharedString>,
    color: u32,
    font_size: f32,
) {
    let value = value.into();
    let run = TextRun {
        len: value.len(),
        font: font("Hiragino Sans"),
        color: rgb(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(value, px(font_size), &[run], None);
    let _ = line.paint(at, px(font_size + 4.), window, cx);
}
pub fn dot(window: &mut Window, p: Point<Pixels>, radius: f32, color: u32) {
    rect(
        window,
        Bounds::new(
            p - point(px(radius), px(radius)),
            size(px(radius * 2.), px(radius * 2.)),
        ),
        color,
        color,
        0.,
        radius,
    );
}
pub fn wire(window: &mut Window, a: Point<Pixels>, b: Point<Pixels>, color: u32, width: f32) {
    let dx = (f32::from(b.x - a.x).abs() * 0.5).max(40.);
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(a);
    path.cubic_bezier_to(b, a + point(px(dx), px(0.)), b - point(px(dx), px(0.)));
    if let Ok(p) = path.build() {
        window.paint_path(p, rgb(color));
    }
}
impl Workspace {
    pub fn paint_art(&self, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
            rect(window, b, 0x242424, 0, 0., 0.);
            let step = self.session.doc.grid_spacing as f32;
            let lo = self.art_view.world(b.origin, b);
            let hi = self.art_view.world(b.bottom_right(), b);
            if step * self.art_view.zoom >= 8. {
                for x in
                    ((lo.x as f32 / step).floor() as i32)..=((hi.x as f32 / step).ceil() as i32)
                {
                    let x = x as f64 * step as f64;
                    line(
                        window,
                        &[
                            self.art_view.screen(KPoint::new(x, lo.y), b),
                            self.art_view.screen(KPoint::new(x, hi.y), b),
                        ],
                        0x2d2d2d,
                        1.,
                    );
                }
                for y in
                    ((lo.y as f32 / step).floor() as i32)..=((hi.y as f32 / step).ceil() as i32)
                {
                    let y = y as f64 * step as f64;
                    line(
                        window,
                        &[
                            self.art_view.screen(KPoint::new(lo.x, y), b),
                            self.art_view.screen(KPoint::new(hi.x, y), b),
                        ],
                        0x2d2d2d,
                        1.,
                    );
                }
            }
            for guide in &self.session.doc.guides {
                let (a, z) = if guide.vertical {
                    (
                        KPoint::new(guide.position, lo.y),
                        KPoint::new(guide.position, hi.y),
                    )
                } else {
                    (
                        KPoint::new(lo.x, guide.position),
                        KPoint::new(hi.x, guide.position),
                    )
                };
                line(
                    window,
                    &[self.art_view.screen(a, b), self.art_view.screen(z, b)],
                    0x536f7b,
                    1.,
                );
            }
            for (id, out) in &self.outlines {
                for layer in &out.layers {
                    let mut builder = PathBuilder::fill();
                    // The geometry core supplies normalized, consistently wound contours.
                    for c in &layer.outline.contours {
                        if c.len() < 3 {
                            continue;
                        }
                        builder.move_to(self.art_view.screen(c[0], b));
                        for p in &c[1..] {
                            builder.line_to(self.art_view.screen(*p, b));
                        }
                        builder.close();
                    }
                    if let Ok(path) = builder.build() {
                        let [r, g, bl] = layer.color;
                        window.paint_path(
                            path,
                            rgba(
                                ((r as u32) << 24)
                                    | ((g as u32) << 16)
                                    | ((bl as u32) << 8)
                                    | ((layer.opacity.clamp(0., 1.) * 255.) as u32),
                            ),
                        );
                    }
                }
                if self.session.selected.contains(id) {
                    for c in &out.contours {
                        let mut points: Vec<_> =
                            c.iter().map(|p| self.art_view.screen(*p, b)).collect();
                        if let Some(p) = points.first().copied() {
                            points.push(p);
                        }
                        line(window, &points, 0xe69b53, 1.);
                    }
                }
            }
            for s in self
                .session
                .doc
                .strokes
                .iter()
                .filter(|s| s.visible && self.session.selected.contains(&s.id))
            {
                if matches!(self.tool, Tool::Direct | Tool::Pen) {
                    let points: Vec<_> = outline::samples(s)
                        .iter()
                        .map(|p| self.art_view.screen(p.p, b))
                        .collect();
                    line(window, &points, 0x78a8db, 1.);
                    for n in &s.nodes {
                        let p = self.art_view.screen(n.p, b);
                        for v in [n.incoming, n.outgoing] {
                            if v.hypot() > 0.1 {
                                let q = self.art_view.screen(n.p + v, b);
                                line(window, &[p, q], 0x859bb1, 1.);
                                dot(window, q, 3., 0x9fb9d5);
                            }
                        }
                        rect(
                            window,
                            Bounds::new(p - point(px(3.5), px(3.5)), size(px(7.), px(7.))),
                            0xe1e1e1,
                            0x242424,
                            1.,
                            1.,
                        );
                    }
                }
                if self.tool == Tool::Width {
                    for target in s.width_targets() {
                        if let Some((center, direction)) = outline::width_location(s, target) {
                            let w = s.width(target).unwrap();
                            let a = self.art_view.screen(center + direction * w.left, b);
                            let z = self.art_view.screen(center - direction * w.right, b);
                            line(window, &[a, z], 0x86b5d4, 1.);
                            dot(window, a, 4., 0x85b8e7);
                            dot(window, z, 4., 0x85b8e7);
                        }
                    }
                }
            }
            if let Some(bounds) = self.session.bounds_cached(&self.outlines) {
                let a = self.art_view.screen(KPoint::new(bounds.x0, bounds.y0), b);
                let z = self.art_view.screen(KPoint::new(bounds.x1, bounds.y1), b);
                if self.tool == Tool::Select {
                    window.paint_quad(quad(
                        Bounds::from_corners(a, z),
                        px(0.),
                        transparent_black(),
                        px(1.),
                        rgb(0x81705c),
                        Default::default(),
                    ));
                }
            }
            if let Drag::Marquee { start, current, .. } = &self.drag {
                let a = self.art_view.screen(*start, b);
                let z = self.art_view.screen(*current, b);
                window.paint_quad(quad(
                    normal_bounds(a, z),
                    px(0.),
                    rgba(0x78a8db18),
                    px(1.),
                    rgb(0x78a8db),
                    Default::default(),
                ));
            }
            // The ruler occupies its own gutter, including sideways y-axis numerals.
            rect(
                window,
                Bounds::new(b.origin, size(b.size.width, px(20.))),
                0x303030,
                0,
                0.,
                0.,
            );
            rect(
                window,
                Bounds::new(b.origin, size(px(20.), b.size.height)),
                0x303030,
                0,
                0.,
                0.,
            );
            let tick = if self.art_view.zoom < 0.5 { 200. } else { 100. };
            for i in (lo.x / tick).floor() as i32..=(hi.x / tick).ceil() as i32 {
                let x = i as f64 * tick;
                let p = self.art_view.screen(KPoint::new(x, 0.), b);
                text(
                    window,
                    cx,
                    point(p.x + px(3.), b.top() + px(1.)),
                    format!("{x:.0}"),
                    0xaaaaaa,
                    10.,
                );
            }
            for i in (lo.y / tick).floor() as i32..=(hi.y / tick).ceil() as i32 {
                let y = i as f64 * tick;
                let p = self.art_view.screen(KPoint::new(0., y), b);
                vertical_digits(
                    window,
                    point(b.left() + px(6.), p.y - px(3.)),
                    &format!("{y:.0}"),
                );
            }
            if self.session.doc.strokes.is_empty() {
                text(
                    window,
                    cx,
                    b.center() - point(px(170.), px(12.)),
                    "P：ペンで描く   /   サンプルを開く",
                    0x999999,
                    15.,
                );
            }
        });
    }
}
pub fn normal_bounds(a: Point<Pixels>, b: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(a.x.min(b.x), a.y.min(b.y)),
        point(a.x.max(b.x), a.y.max(b.y)),
    )
}
fn vertical_digits(window: &mut Window, at: Point<Pixels>, s: &str) {
    let segments = [
        [(0., 0.), (4., 0.)],
        [(4., 0.), (4., 3.)],
        [(4., 3.), (4., 6.)],
        [(0., 6.), (4., 6.)],
        [(0., 3.), (0., 6.)],
        [(0., 0.), (0., 3.)],
        [(0., 3.), (4., 3.)],
    ];
    for (i, c) in s.chars().enumerate() {
        let mask = match c {
            '0' => 0b0111111,
            '1' => 0b0000110,
            '2' => 0b1011011,
            '3' => 0b1001111,
            '4' => 0b1100110,
            '5' => 0b1101101,
            '6' => 0b1111101,
            '7' => 0b0000111,
            '8' => 0b1111111,
            '9' => 0b1101111,
            '-' => 0b1000000,
            _ => 0,
        };
        for (j, seg) in segments.iter().enumerate() {
            if mask & (1 << j) != 0 {
                let p = seg.map(|(x, y)| at + point(px(y), px(-x - i as f32 * 6.)));
                line(window, &p, 0xaaaaaa, 0.8);
            }
        }
    }
}
impl crate::session::Session {
    pub fn bounds_cached(&self, cache: &[(u64, outline::Outline)]) -> Option<kurbo::Rect> {
        cache
            .iter()
            .filter(|(id, _)| self.selected.contains(id))
            .flat_map(|(_, o)| o.contours.iter().flatten())
            .map(|p| kurbo::Rect::from_points(*p, *p))
            .reduce(|a, b| a.union(b))
    }
}
