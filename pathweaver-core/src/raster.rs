//! Raster stage of appearance graphs (After Effects style): shapes are turned
//! into a bitmap at a fixed resolution chosen on the node, processed per pixel,
//! and can be traced back into vector contours.
//!
//! Pixels are premultiplied RGBA8. Pixel (0, 0) covers the world square
//! `[x0, x0 + 1/scale) × [y0, y0 + 1/scale)`.
use kurbo::Point;
use std::sync::{Arc, OnceLock};

use super::appearance::Layer;

/// Upper bound for one bitmap (about 2000 × 2000).
pub const MAX_PIXELS: usize = 4_000_000;
/// Resolution used when a raster node receives shapes directly.
pub const DEFAULT_SCALE: f64 = 2.;
/// Most vertices a trace may produce.
const MAX_TRACE_VERTICES: usize = 150_000;

pub struct Image {
    pub x0: f64,
    pub y0: f64,
    /// Pixels per document unit.
    pub scale: f64,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// A soft effect (glow, blur, shadow): shape nodes after it leave it as is
    /// instead of tracing it back into stepped outlines.
    pub soft: bool,
    png: OnceLock<String>,
}
impl Clone for Image {
    fn clone(&self) -> Self {
        Self::new(self.x0, self.y0, self.scale, self.width, self.height, self.rgba.clone()).with_soft(self.soft)
    }
}
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}×{} @{} at {},{})", self.width, self.height, self.scale, self.x0, self.y0)
    }
}
impl PartialEq for Image {
    fn eq(&self, o: &Self) -> bool {
        self.soft == o.soft && self.x0 == o.x0 && self.y0 == o.y0 && self.scale == o.scale && self.width == o.width && self.height == o.height && self.rgba == o.rgba
    }
}
impl Image {
    pub fn new(x0: f64, y0: f64, scale: f64, width: usize, height: usize, rgba: Vec<u8>) -> Self {
        Self { x0, y0, scale, width, height, rgba, soft: false, png: OnceLock::new() }
    }
    pub fn with_soft(mut self, soft: bool) -> Self {
        self.soft = soft;
        self
    }
    pub fn x1(&self) -> f64 {
        self.x0 + self.width as f64 / self.scale
    }
    pub fn y1(&self) -> f64 {
        self.y0 + self.height as f64 / self.scale
    }
    /// The world rectangle covered by the bitmap (used for bounds and hit testing).
    pub fn rect(&self) -> Vec<Point> {
        vec![
            Point::new(self.x0, self.y0),
            Point::new(self.x1(), self.y0),
            Point::new(self.x1(), self.y1()),
            Point::new(self.x0, self.y1()),
        ]
    }
    fn alpha(&self, x: usize, y: usize) -> f32 {
        self.rgba[(y * self.width + x) * 4 + 3] as f32 / 255.
    }
    /// PNG data URL (straight alpha), encoded once per bitmap.
    pub fn data_url(&self) -> &str {
        self.png.get_or_init(|| format!("data:image/png;base64,{}", base64(&png(self))))
    }
    pub fn layer(self, opacity: f64) -> Layer {
        let contours = vec![self.rect()];
        Layer { color: [0; 3], opacity, contours, image: Some(Arc::new(self)) }
    }
}

fn check_size(w: usize, h: usize) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("ラスターの大きさが0です".into());
    }
    if w.saturating_mul(h) > MAX_PIXELS {
        return Err(format!(
            "ラスターが大きすぎます（{w}×{h}px）。ラスター化ノードの解像度を下げてください"
        ));
    }
    Ok(())
}

fn bounds(layers: &[Layer]) -> Option<(Point, Point)> {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in layers.iter().filter(|l| l.opacity > 0.).flat_map(|l| l.contours.iter().flatten()) {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    min.x.is_finite().then_some((min, max))
}

/// Anti-aliased coverage (0..1) of `rings` (non-zero winding) on a w×h grid.
/// `to_px` maps world points to pixel coordinates.
fn coverage(rings: &[Vec<Point>], w: usize, h: usize, to_px: impl Fn(Point) -> Point) -> Vec<f32> {
    const SUB: usize = 4;
    struct Edge {
        y0: f64,
        y1: f64,
        x: f64,
        dxdy: f64,
        dir: i32,
    }
    let mut edges = Vec::new();
    for ring in rings {
        if ring.len() < 3 {
            continue;
        }
        for i in 0..ring.len() {
            let a = to_px(ring[i]);
            let b = to_px(ring[(i + 1) % ring.len()]);
            if (a.y - b.y).abs() < 1e-12 {
                continue;
            }
            let (top, bot, dir) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };
            let dxdy = (bot.x - top.x) / (bot.y - top.y);
            edges.push(Edge { y0: top.y, y1: bot.y, x: top.x, dxdy, dir });
        }
    }
    edges.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut out = vec![0f32; w * h];
    let mut next = 0;
    let mut active: Vec<usize> = Vec::new();
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    let weight = 1. / SUB as f32;
    for row in 0..h {
        let line = &mut out[row * w..(row + 1) * w];
        for s in 0..SUB {
            let y = row as f64 + (s as f64 + 0.5) / SUB as f64;
            while next < edges.len() && edges[next].y0 <= y {
                active.push(next);
                next += 1;
            }
            active.retain(|&i| edges[i].y1 > y);
            crossings.clear();
            for &i in &active {
                let e = &edges[i];
                if e.y0 <= y {
                    crossings.push((e.x + (y - e.y0) * e.dxdy, e.dir));
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            let mut start = 0.;
            for &(x, dir) in &crossings {
                let was = winding != 0;
                winding += dir;
                if !was && winding != 0 {
                    start = x;
                } else if was && winding == 0 {
                    span(line, start, x, weight);
                }
            }
        }
    }
    out
}
fn span(line: &mut [f32], a: f64, b: f64, weight: f32) {
    let w = line.len() as f64;
    let (a, b) = (a.clamp(0., w), b.clamp(0., w));
    if b <= a {
        return;
    }
    let first = a.floor() as usize;
    let last = (b.ceil() as usize).min(line.len());
    for (i, v) in line.iter_mut().enumerate().take(last).skip(first) {
        let lo = a.max(i as f64);
        let hi = b.min(i as f64 + 1.);
        if hi > lo {
            *v += (hi - lo) as f32 * weight;
        }
    }
}

/// Composite `src` (premultiplied, alpha `a`) over the pixel at `i`.
fn over(dst: &mut [u8], i: usize, r: f32, g: f32, b: f32, a: f32) {
    let k = 1. - a;
    let px = &mut dst[i * 4..i * 4 + 4];
    px[0] = (r * 255. + px[0] as f32 * k).round().clamp(0., 255.) as u8;
    px[1] = (g * 255. + px[1] as f32 * k).round().clamp(0., 255.) as u8;
    px[2] = (b * 255. + px[2] as f32 * k).round().clamp(0., 255.) as u8;
    px[3] = (a * 255. + px[3] as f32 * k).round().clamp(0., 255.) as u8;
}

/// Bilinear sample of a premultiplied image at pixel-space point (x, y) (pixel centres at +0.5).
fn sample(img: &Image, x: f64, y: f64) -> [f32; 4] {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = (fx - x0) as f32;
    let ty = (fy - y0) as f32;
    let get = |xi: f64, yi: f64| -> [f32; 4] {
        if xi < 0. || yi < 0. || xi >= img.width as f64 || yi >= img.height as f64 {
            return [0.; 4];
        }
        let i = (yi as usize * img.width + xi as usize) * 4;
        [img.rgba[i] as f32, img.rgba[i + 1] as f32, img.rgba[i + 2] as f32, img.rgba[i + 3] as f32]
    };
    let (a, b, c, d) = (get(x0, y0), get(x0 + 1., y0), get(x0, y0 + 1.), get(x0 + 1., y0 + 1.));
    let mut out = [0f32; 4];
    for k in 0..4 {
        let top = a[k] * (1. - tx) + b[k] * tx;
        let bot = c[k] * (1. - tx) + d[k] * tx;
        out[k] = (top * (1. - ty) + bot * ty) / 255.;
    }
    out
}

/// Draw everything (shapes and bitmaps) into one bitmap at `scale` px/unit,
/// with `pad` document units of empty margin. `None` when nothing is visible.
pub fn rasterize(layers: &[Layer], scale: f64, pad: f64) -> Result<Option<Image>, String> {
    if !(scale.is_finite() && scale > 0.) {
        return Err("ラスターの解像度が不正です".into());
    }
    let Some((min, max)) = bounds(layers) else { return Ok(None) };
    let x0 = ((min.x - pad) * scale).floor() / scale;
    let y0 = ((min.y - pad) * scale).floor() / scale;
    let w = (((max.x + pad - x0) * scale).ceil() as usize).max(1);
    let h = (((max.y + pad - y0) * scale).ceil() as usize).max(1);
    check_size(w, h)?;
    let mut rgba = vec![0u8; w * h * 4];
    let to_px = |p: Point| Point::new((p.x - x0) * scale, (p.y - y0) * scale);
    for layer in layers.iter().filter(|l| l.opacity > 0.) {
        let o = layer.opacity.clamp(0., 1.) as f32;
        if let Some(img) = &layer.image {
            // Resample the bitmap onto this grid.
            for y in 0..h {
                for x in 0..w {
                    let wx = x0 + (x as f64 + 0.5) / scale;
                    let wy = y0 + (y as f64 + 0.5) / scale;
                    let s = sample(img, (wx - img.x0) * img.scale, (wy - img.y0) * img.scale);
                    if s[3] > 0. {
                        over(&mut rgba, y * w + x, s[0] * o, s[1] * o, s[2] * o, s[3] * o);
                    }
                }
            }
            continue;
        }
        let cov = coverage(&layer.contours, w, h, to_px);
        let [r, g, b] = layer.color.map(|c| c as f32 / 255.);
        for (i, c) in cov.iter().enumerate() {
            let a = c.min(1.) * o;
            if a > 0. {
                over(&mut rgba, i, r * a, g * a, b * a, a);
            }
        }
    }
    Ok(Some(Image::new(x0, y0, scale, w, h, rgba)))
}

/// The bitmap a raster node works on: an existing single bitmap is reused;
/// otherwise everything is drawn at the resolution of any bitmap present
/// (or the default), with `pad` units of margin.
pub fn as_image(layers: &[Layer], pad: f64) -> Result<Option<Image>, String> {
    let visible: Vec<&Layer> = layers.iter().filter(|l| l.opacity > 0.).collect();
    if pad <= 0. && visible.len() == 1 && visible[0].opacity >= 1. {
        if let Some(img) = &visible[0].image {
            return Ok(Some((**img).clone()));
        }
    }
    let scale = layers.iter().filter_map(|l| l.image.as_ref()).map(|i| i.scale).fold(f64::NAN, f64::max);
    rasterize(layers, if scale.is_finite() { scale } else { DEFAULT_SCALE }, pad)
}

/// Gaussian-like blur (three box passes) with radius ≈ 2σ in document units.
pub fn blur(img: &Image, radius: f64) -> Result<Image, String> {
    let sigma = radius * img.scale / 2.;
    if sigma < 0.05 {
        return Ok(img.clone());
    }
    // Room for the spread.
    let pad = (sigma * 4.).ceil() as usize;
    let (w, h) = (img.width + pad * 2, img.height + pad * 2);
    check_size(w, h)?;
    let boxes = box_sizes(sigma);
    let mut out = vec![0u8; w * h * 4];
    let mut plane = vec![0f32; w * h];
    let mut tmp = vec![0f32; w * h];
    for c in 0..4 {
        plane.iter_mut().for_each(|v| *v = 0.);
        for y in 0..img.height {
            for x in 0..img.width {
                plane[(y + pad) * w + x + pad] = img.rgba[(y * img.width + x) * 4 + c] as f32;
            }
        }
        for &r in &boxes {
            box_h(&plane, &mut tmp, w, h, r);
            box_v(&tmp, &mut plane, w, h, r);
        }
        for (i, v) in plane.iter().enumerate() {
            out[i * 4 + c] = v.round().clamp(0., 255.) as u8;
        }
    }
    // Premultiplied colour may not exceed alpha after rounding.
    for px in out.chunks_exact_mut(4) {
        px[0] = px[0].min(px[3]);
        px[1] = px[1].min(px[3]);
        px[2] = px[2].min(px[3]);
    }
    let s = img.scale;
    Ok(Image::new(img.x0 - pad as f64 / s, img.y0 - pad as f64 / s, s, w, h, out).with_soft(img.soft))
}
fn box_sizes(sigma: f64) -> [usize; 3] {
    // Box radii whose three passes approximate a Gaussian of `sigma`.
    let n = 3.;
    let w_ideal = (12. * sigma * sigma / n + 1.).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12. * sigma * sigma - n * (wl * wl) as f64 - 4. * n * wl as f64 - 3. * n) / (-4. * wl as f64 - 4.);
    let m = m_ideal.round() as i64;
    let mut out = [0usize; 3];
    for (i, v) in out.iter_mut().enumerate() {
        let size = if (i as i64) < m { wl } else { wu };
        *v = ((size.max(1) - 1) / 2) as usize;
    }
    out
}
fn box_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1. / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let out = &mut dst[y * w..(y + 1) * w];
        let mut acc = 0f32;
        for x in 0..=r.min(w - 1) {
            acc += row[x];
        }
        for x in 0..w {
            out[x] = acc * norm;
            let add = x + r + 1;
            if add < w {
                acc += row[add];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
}
fn box_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1. / (2 * r + 1) as f32;
    for x in 0..w {
        let mut acc = 0f32;
        for y in 0..=r.min(h - 1) {
            acc += src[y * w + x];
        }
        for y in 0..h {
            dst[y * w + x] = acc * norm;
            let add = y + r + 1;
            if add < h {
                acc += src[add * w + x];
            }
            if y >= r {
                acc -= src[(y - r) * w + x];
            }
        }
    }
}

/// Average straight colour of the visible pixels (black when empty).
fn average_color(img: &Image) -> [f32; 3] {
    let mut sum = [0f64; 4];
    for px in img.rgba.chunks_exact(4) {
        for k in 0..4 {
            sum[k] += px[k] as f64;
        }
    }
    if sum[3] <= 0. {
        return [0.; 3];
    }
    [(sum[0] / sum[3]) as f32, (sum[1] / sum[3]) as f32, (sum[2] / sum[3]) as f32]
}

/// Map alpha through `f` (straight colour kept; empty pixels take the average colour).
fn map_alpha(img: &Image, mut f: impl FnMut(usize, usize, f32) -> f32) -> Image {
    let avg = average_color(img);
    let mut out = img.rgba.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            let i = (y * img.width + x) * 4;
            let a = img.rgba[i + 3] as f32 / 255.;
            let color = if a > 0. {
                [img.rgba[i] as f32 / 255. / a, img.rgba[i + 1] as f32 / 255. / a, img.rgba[i + 2] as f32 / 255. / a]
            } else {
                avg
            };
            let na = f(x, y, a).clamp(0., 1.);
            for k in 0..3 {
                out[i + k] = (color[k].min(1.) * na * 255.).round() as u8;
            }
            out[i + 3] = (na * 255.).round() as u8;
        }
    }
    Image::new(img.x0, img.y0, img.scale, img.width, img.height, out)
}

/// Darkness on white paper as black ink: ink = 1 − luminance of the image
/// composited over white. White (or empty) is no ink, black is full ink, so a
/// white shape stacked over a dark one erases it.
pub fn ink(img: &Image) -> Image {
    let mut out = vec![0u8; img.rgba.len()];
    for (o, p) in out.chunks_exact_mut(4).zip(img.rgba.chunks_exact(4)) {
        let a = p[3] as f32 / 255.;
        // premultiplied colour over white
        let [r, g, b] = [0, 1, 2].map(|k| p[k] as f32 / 255. + (1. - a));
        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        o[3] = ((1. - lum).clamp(0., 1.) * 255.).round() as u8;
    }
    Image::new(img.x0, img.y0, img.scale, img.width, img.height, out).with_soft(img.soft)
}

/// Hard (or soft) edge: alpha below `level` disappears, above becomes opaque.
pub fn threshold(img: &Image, level: f64, softness: f64) -> Image {
    let lo = (level - softness * 0.5) as f32;
    let hi = (level + softness * 0.5) as f32;
    map_alpha(img, |_, _, a| {
        if hi - lo < 1e-4 {
            if a >= level as f32 { 1. } else { 0. }
        } else {
            let t = ((a - lo) / (hi - lo)).clamp(0., 1.);
            t * t * (3. - 2. * t)
        }
    })
}

/// Smooth value noise added to alpha (cell size in document units).
pub fn noise(img: &Image, amount: f64, size: f64, seed: u32) -> Image {
    let cell = (size * img.scale).max(0.5);
    let gx = (img.width as f64 / cell).ceil() as usize + 2;
    let hash = |x: usize, y: usize| -> f32 {
        let mut v = seed ^ (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
        v ^= v >> 13;
        v = v.wrapping_mul(0x5bd1_e995);
        v ^= v >> 15;
        (v as f32 / u32::MAX as f32) * 2. - 1.
    };
    let _ = gx;
    let amount = amount as f32;
    map_alpha(img, |x, y, a| {
        let fx = x as f64 / cell;
        let fy = y as f64 / cell;
        let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
        let s = |t: f32| t * t * (3. - 2. * t);
        let (tx, ty) = (s(tx), s(ty));
        let top = hash(ix, iy) * (1. - tx) + hash(ix + 1, iy) * tx;
        let bot = hash(ix, iy + 1) * (1. - tx) + hash(ix + 1, iy + 1) * tx;
        a + amount * (top * (1. - ty) + bot * ty)
    })
}

/// Replace the colour, keeping alpha (the 色 node on a bitmap).
pub fn tint(img: &Image, color: [u8; 3], opacity: f64) -> Image {
    let mut out = img.rgba.clone();
    let o = opacity.clamp(0., 1.) as f32;
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as f32 / 255. * o;
        for k in 0..3 {
            px[k] = (color[k] as f32 * a).round() as u8;
        }
        px[3] = (a * 255.).round() as u8;
    }
    Image::new(img.x0, img.y0, img.scale, img.width, img.height, out).with_soft(img.soft)
}

/// Apply a world affine map `p ↦ m(p)` by resampling (same resolution).
pub fn transform(img: &Image, m: impl Fn(Point) -> Point, inverse: impl Fn(Point) -> Point) -> Result<Image, String> {
    let corners = img.rect().into_iter().map(&m).collect::<Vec<_>>();
    let (mut min, mut max) = (corners[0], corners[0]);
    for p in &corners {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    let s = img.scale;
    let x0 = (min.x * s).floor() / s;
    let y0 = (min.y * s).floor() / s;
    let w = (((max.x - x0) * s).ceil() as usize).max(1);
    let h = (((max.y - y0) * s).ceil() as usize).max(1);
    check_size(w, h)?;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let p = inverse(Point::new(x0 + (x as f64 + 0.5) / s, y0 + (y as f64 + 0.5) / s));
            let v = sample(img, (p.x - img.x0) * s, (p.y - img.y0) * s);
            let i = (y * w + x) * 4;
            for k in 0..4 {
                out[i + k] = (v[k] * 255.).round().clamp(0., 255.) as u8;
            }
        }
    }
    Ok(Image::new(x0, y0, s, w, h, out).with_soft(img.soft))
}

/// Trace the alpha channel back into vector contours (marching squares at
/// `level`, simplified within `tolerance` pixels, `smooth` rounds of corner
/// cutting). The result is one layer in the bitmap's average colour.
/// Rings enclosing less than `min_area` (document units²) are dropped, so
/// specks and pinholes left by noise disappear.
pub fn trace(img: &Image, level: f64, tolerance: f64, smooth: usize, min_area: f64) -> Result<Layer, String> {
    let level = level.clamp(0.001, 0.999) as f32;
    // Grid of pixel centres with a one-pixel empty border, so every ring closes.
    let (gw, gh) = (img.width + 2, img.height + 2);
    let v = |i: usize, j: usize| -> f32 {
        if i == 0 || j == 0 || i > img.width || j > img.height {
            0.
        } else {
            img.alpha(i - 1, j - 1)
        }
    };
    let h_id = |i: usize, j: usize| 2 * (j * gw + i);
    let v_id = |i: usize, j: usize| 2 * (j * gw + i) + 1;
    let lerp = |a: f32, b: f32| ((level - a) / (b - a)).clamp(0., 1.) as f64;
    let mut points: std::collections::HashMap<usize, Point> = std::collections::HashMap::new();
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for j in 0..gh - 1 {
        for i in 0..gw - 1 {
            let (tl, tr, br, bl) = (v(i, j), v(i + 1, j), v(i + 1, j + 1), v(i, j + 1));
            let case = (tl >= level) as u8 | ((tr >= level) as u8) << 1 | ((br >= level) as u8) << 2 | ((bl >= level) as u8) << 3;
            if case == 0 || case == 15 {
                continue;
            }
            let (e0, e1, e2, e3) = (h_id(i, j), v_id(i + 1, j), h_id(i, j + 1), v_id(i, j));
            let mut point = |id: usize| {
                points.entry(id).or_insert_with(|| {
                    let (x, y) = if id == e0 {
                        (i as f64 + lerp(tl, tr), j as f64)
                    } else if id == e1 {
                        (i as f64 + 1., j as f64 + lerp(tr, br))
                    } else if id == e2 {
                        (i as f64 + lerp(bl, br), j as f64 + 1.)
                    } else {
                        (i as f64, j as f64 + lerp(tl, bl))
                    };
                    Point::new(x, y)
                });
            };
            let center = (tl + tr + br + bl) * 0.25 >= level;
            let segs: &[(usize, usize)] = match case {
                1 | 14 => &[(e3, e0)],
                2 | 13 => &[(e0, e1)],
                3 | 12 => &[(e3, e1)],
                4 | 11 => &[(e1, e2)],
                6 | 9 => &[(e0, e2)],
                7 | 8 => &[(e3, e2)],
                5 => if center { &[(e0, e1), (e2, e3)] } else { &[(e3, e0), (e1, e2)] },
                10 => if center { &[(e3, e0), (e1, e2)] } else { &[(e0, e1), (e2, e3)] },
                _ => &[],
            };
            for &(a, b) in segs {
                point(a);
                point(b);
                segments.push((a, b));
            }
        }
    }
    if segments.len() > MAX_TRACE_VERTICES * 4 {
        return Err("トレース結果が細かすぎます。しきい値や解像度を見直してください".into());
    }
    // Chain segments into rings: every edge point joins exactly two segments.
    let mut at: std::collections::HashMap<usize, [usize; 2]> = std::collections::HashMap::with_capacity(points.len());
    for (k, &(a, b)) in segments.iter().enumerate() {
        for e in [a, b] {
            at.entry(e).and_modify(|v| v[1] = k).or_insert([k, usize::MAX]);
        }
    }
    let mut used = vec![false; segments.len()];
    let mut rings: Vec<Vec<Point>> = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (first, mut edge) = segments[start];
        let mut ring = vec![points[&first]];
        let mut current = start;
        while edge != first {
            ring.push(points[&edge]);
            let pair = at[&edge];
            let next = if pair[0] == current { pair[1] } else { pair[0] };
            if next == usize::MAX || used[next] {
                break;
            }
            used[next] = true;
            let (a, b) = segments[next];
            edge = if a == edge { b } else { a };
            current = next;
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    let s = img.scale;
    let mut total = 0;
    let mut world = Vec::new();
    let min_px = min_area.max(0.) * s * s;
    for ring in rings {
        if min_px > 0. && ring_area(&ring).abs() < min_px {
            continue;
        }
        let mut r = simplify_closed(&ring, tolerance.max(0.05));
        for _ in 0..smooth.min(4) {
            r = chaikin(&r);
        }
        if r.len() < 3 {
            continue;
        }
        total += r.len();
        if total > MAX_TRACE_VERTICES {
            return Err("トレース結果が細かすぎます。許容誤差を大きくしてください".into());
        }
        // Grid point (i, j) is the centre of pixel (i-1, j-1).
        world.push(r.into_iter().map(|p| Point::new(img.x0 + (p.x - 0.5) / s, img.y0 + (p.y - 0.5) / s)).collect::<Vec<_>>());
    }
    let contours = even_odd(&world);
    let avg = average_color(img);
    Ok(Layer {
        color: avg.map(|c| (c * 255.).round().clamp(0., 255.) as u8),
        opacity: 1.,
        contours,
        image: None,
    })
}

fn ring_area(r: &[Point]) -> f64 {
    let mut a = 0.;
    for i in 0..r.len() {
        let (p, q) = (r[i], r[(i + 1) % r.len()]);
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

fn even_odd(rings: &[Vec<Point>]) -> Vec<Vec<Point>> {
    use i_overlay::{core::{fill_rule::FillRule, overlay_rule::OverlayRule}, float::single::SingleFloatOverlay};
    let a: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| r.iter().map(|p| [p.x, p.y]).collect()).collect();
    let empty: Vec<Vec<[f64; 2]>> = vec![];
    a.overlay(&empty, OverlayRule::Subject, FillRule::EvenOdd)
        .into_iter()
        .flatten()
        .map(|c| c.into_iter().map(|p| Point::new(p[0], p[1])).collect())
        .collect()
}

fn simplify_closed(ring: &[Point], tol: f64) -> Vec<Point> {
    if ring.len() < 4 {
        return ring.to_vec();
    }
    // Split at the point farthest from the first, simplify both halves.
    let far = (1..ring.len()).max_by(|&a, &b| ring[0].distance(ring[a]).total_cmp(&ring[0].distance(ring[b]))).unwrap();
    let mut a = rdp(&ring[..=far], tol);
    let mut tail: Vec<Point> = ring[far..].to_vec();
    tail.push(ring[0]);
    let b = rdp(&tail, tol);
    a.pop();
    a.extend_from_slice(&b[..b.len() - 1]);
    a
}
fn rdp(points: &[Point], tol: f64) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (points[a], points[b]);
        let d = pb - pa;
        let len = d.hypot();
        let mut best = (0., a);
        for (k, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let dist = if len < 1e-12 { p.distance(pa) } else { (d.cross(*p - pa) / len).abs() };
            if dist > best.0 {
                best = (dist, k);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}
fn chaikin(ring: &[Point]) -> Vec<Point> {
    let n = ring.len();
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        out.push(a.lerp(b, 0.25));
        out.push(a.lerp(b, 0.75));
    }
    out
}

// ---- embedded pictures: base64 PNG (8-bit RGB / RGBA, any filter) ----
/// Decode an embedded picture into a bitmap placed at (x, y), `width` units wide.
pub fn picture(base64_png: &str, x: f64, y: f64, width: f64) -> Result<Image, String> {
    let bytes = unbase64(base64_png).ok_or("画像データが壊れています")?;
    let (w, h, rgba) = decode_png(&bytes)?;
    check_size(w, h)?;
    let scale = w as f64 / width.max(1e-6);
    Ok(Image::new(x, y, scale, w, h, rgba))
}
fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
/// Premultiplied RGBA8 from a PNG (what browsers produce from a canvas).
fn decode_png(b: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    const BAD: &str = "この画像は読み込めません（8bitのPNGに変換してください）";
    if b.len() < 8 || b[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return Err(BAD.into());
    }
    let (mut w, mut h, mut channels) = (0usize, 0usize, 0usize);
    let mut data = Vec::new();
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let kind = &b[i + 4..i + 8];
        let body = b.get(i + 8..i + 8 + len).ok_or(BAD)?;
        match kind {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(BAD.into());
                }
                w = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                h = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                let (depth, color, interlace) = (body[8], body[9], body[12]);
                channels = match color {
                    6 => 4,
                    2 => 3,
                    _ => 0,
                };
                if depth != 8 || channels == 0 || interlace != 0 {
                    return Err(BAD.into());
                }
            }
            b"IDAT" => data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    check_size(w, h)?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&data).map_err(|_| BAD.to_string())?;
    let stride = w * channels;
    if raw.len() < (stride + 1) * h {
        return Err(BAD.into());
    }
    let mut px = vec![0u8; stride * h];
    for y in 0..h {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= channels { px[y * stride + x - channels] as i32 } else { 0 };
            let up = if y > 0 { px[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if x >= channels && y > 0 { px[(y - 1) * stride + x - channels] as i32 } else { 0 };
            let pred = match filter {
                0 => 0,
                1 => a,
                2 => up,
                3 => (a + up) / 2,
                4 => {
                    let p = a + up - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - up).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc { a } else if pb <= pc { up } else { c }
                }
                _ => return Err(BAD.into()),
            };
            px[y * stride + x] = (src[x] as i32 + pred) as u8;
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in px.chunks_exact(channels) {
        let a = if channels == 4 { p[3] } else { 255 };
        let pre = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
        rgba.extend_from_slice(&[pre(p[0]), pre(p[1]), pre(p[2]), a]);
    }
    Ok((w, h, rgba))
}

// ---- PNG (straight alpha, zlib via miniz_oxide) ----
fn png(img: &Image) -> Vec<u8> {
    let mut raw = Vec::with_capacity((img.width * 4 + 1) * img.height);
    for y in 0..img.height {
        raw.push(0);
        for px in img.rgba[y * img.width * 4..(y + 1) * img.width * 4].chunks_exact(4) {
            let a = px[3];
            if a == 0 {
                raw.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
                raw.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a]);
            }
        }
    }
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(img.width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img.height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &data);
    chunk(&mut out, b"IEND", &[]);
    out
}
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}
fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, v) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *v = c;
        }
        t
    });
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// Pixels per unit for a soft effect over `layers` grown by `pad` units: the
/// default resolution, lowered for big artwork so the bitmap stays in budget.
pub fn soft_scale(layers: &[Layer], pad: f64) -> f64 {
    let Some((min, max)) = bounds(layers) else { return DEFAULT_SCALE };
    let area = (max.x - min.x + 2. * pad).max(1.) * (max.y - min.y + 2. * pad).max(1.);
    ((MAX_PIXELS as f64 * 0.6) / area).sqrt().min(DEFAULT_SCALE).max(0.02)
}
fn blur_plane(plane: &mut [f32], w: usize, h: usize, sigma: f64) {
    if sigma < 0.05 {
        return;
    }
    let mut tmp = vec![0f32; w * h];
    for r in box_sizes(sigma) {
        box_h(plane, &mut tmp, w, h, r);
        box_v(&tmp, plane, w, h, r);
    }
}
fn from_alpha(grid: &Image, alpha: &[f32], color: [u8; 3], opacity: f64) -> Image {
    let o = opacity.clamp(0., 1.) as f32;
    let mut rgba = vec![0u8; alpha.len() * 4];
    for (i, a) in alpha.iter().enumerate() {
        let a = (a.clamp(0., 1.) * o).min(1.);
        for k in 0..3 {
            rgba[i * 4 + k] = (color[k] as f32 * a).round() as u8;
        }
        rgba[i * 4 + 3] = (a * 255.).round() as u8;
    }
    Image::new(grid.x0, grid.y0, grid.scale, grid.width, grid.height, rgba).with_soft(true)
}
fn alpha_plane(img: &Image) -> Vec<f32> {
    img.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.).collect()
}

/// A real (Gaussian) glow around and/or inside the artwork, as smooth bitmaps
/// in one colour: (outer, inner). `size` ≈ how far the light reaches.
/// `hole`: the outer glow is cut out where the artwork is (no source kept).
pub fn glow(layers: &[Layer], size: f64, color: [u8; 3], amount: f64, outer: bool, inner: bool, hole: bool) -> Result<(Option<Image>, Option<Image>), String> {
    let pad = size * 2.;  // 4σ: the tail ends below 1/255, no visible edge
    let scale = soft_scale(layers, pad);
    let Some(mask) = rasterize(layers, scale, pad + 2. / scale)? else { return Ok((None, None)) };
    // Coverage of the artwork (a translucent shape still glows fully).
    let mut a = alpha_plane(&mask);
    let peak = a.iter().cloned().fold(0f32, f32::max);
    if peak > 0. {
        a.iter_mut().for_each(|v| *v = (*v / peak).min(1.));
    }
    let (w, h) = (mask.width, mask.height);
    let sigma = size * 0.5 * scale;
    let mut out = (None, None);
    if outer {
        let mut p = a.clone();
        blur_plane(&mut p, w, h, sigma);
        for (v, m) in p.iter_mut().zip(&a) {
            *v = (*v * 1.6).min(1.);
            if hole {
                *v *= 1. - m;
            }
        }
        out.0 = Some(from_alpha(&mask, &p, color, amount));
    }
    if inner {
        let mut p: Vec<f32> = a.iter().map(|m| 1. - m).collect();
        blur_plane(&mut p, w, h, sigma);
        for (v, m) in p.iter_mut().zip(&a) {
            *v = (*v * 1.6).min(1.) * m;
        }
        out.1 = Some(from_alpha(&mask, &p, color, amount));
    }
    Ok(out)
}

/// The artwork's silhouette blurred into one colour (drop shadows).
pub fn soft_silhouette(layers: &[Layer], radius: f64, color: [u8; 3], amount: f64) -> Result<Option<Image>, String> {
    let pad = radius * 2.;
    let scale = soft_scale(layers, pad);
    let Some(mask) = rasterize(layers, scale, pad + 2. / scale)? else { return Ok(None) };
    let mut a = alpha_plane(&mask);
    blur_plane(&mut a, mask.width, mask.height, radius * 0.5 * scale);
    Ok(Some(from_alpha(&mask, &a, color, amount)))
}

/// Everything blurred as pixels (the ぼかし node), kept in its colours.
pub fn soft_blur(layers: &[Layer], radius: f64) -> Result<Option<Image>, String> {
    let scale = soft_scale(layers, radius * 2.);
    let Some(img) = rasterize(layers, scale, 0.)? else { return Ok(None) };
    Ok(Some(blur(&img, radius)?.with_soft(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square(x: f64, y: f64, s: f64) -> Layer {
        Layer {
            color: [200, 40, 20],
            opacity: 1.,
            contours: vec![vec![Point::new(x, y), Point::new(x + s, y), Point::new(x + s, y + s), Point::new(x, y + s)]],
            image: None,
        }
    }
    fn area(rings: &[Vec<Point>]) -> f64 {
        rings.iter().map(|r| {
            let mut a = 0.;
            for i in 0..r.len() {
                a += r[i].x * r[(i + 1) % r.len()].y - r[(i + 1) % r.len()].x * r[i].y;
            }
            a * 0.5
        }).sum::<f64>().abs()
    }

    #[test]
    fn ink_is_darkness_on_white_paper() {
        // white, black, mid grey, transparent (premultiplied RGBA)
        let px = [255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255, 0, 0, 0, 0];
        let img = Image::new(0., 0., 1., 4, 1, px.to_vec());
        let a: Vec<u8> = ink(&img).rgba.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(a[0], 0, "white is no ink");
        assert_eq!(a[1], 255, "black is full ink");
        assert!((120..=135).contains(&a[2]), "grey is half ink: {}", a[2]);
        assert_eq!(a[3], 0, "empty is no ink");
    }
    #[test]
    fn rasterize_covers_the_shape_with_antialiased_edges() {
        let img = rasterize(&[square(0.25, 0., 10.)], 2., 1.).unwrap().unwrap();
        assert_eq!(img.scale, 2.);
        assert!(img.width >= 22 && img.height >= 22);
        let inside = img.alpha(((5. - img.x0) * 2.) as usize, ((5. - img.y0) * 2.) as usize);
        assert!(inside > 0.99);
        // left edge at x = 0.25 → half-covered pixel column
        let col = ((0.25 - img.x0) * 2.).floor() as usize;
        let edge = img.alpha(col, ((5. - img.y0) * 2.) as usize);
        assert!(edge > 0.3 && edge < 0.7, "{edge}");
        assert_eq!(img.alpha(0, 0), 0.);
        // colour premultiplied
        let i = (((5. - img.y0) * 2.) as usize * img.width + ((5. - img.x0) * 2.) as usize) * 4;
        assert_eq!(&img.rgba[i..i + 4], &[200, 40, 20, 255]);
    }

    #[test]
    fn trace_round_trips_a_square_and_keeps_holes() {
        let mut ring = square(0., 0., 40.);
        ring.contours.push(vec![Point::new(10., 10.), Point::new(10., 30.), Point::new(30., 30.), Point::new(30., 10.)]);
        let img = rasterize(&[ring], 2., 2.).unwrap().unwrap();
        let traced = trace(&img, 0.5, 0.5, 0, 0.).unwrap();
        assert_eq!(traced.color, [200, 40, 20]);
        let a = area(&traced.contours);
        // 40² − 20² = 1200 within a pixel of tolerance around the perimeter
        assert!((a - 1200.).abs() < 30., "area {a}");
        assert!(traced.contours.len() >= 2, "hole lost");
    }

    #[test]
    fn blur_spreads_and_threshold_hardens() {
        let img = rasterize(&[square(0., 0., 20.)], 2., 0.).unwrap().unwrap();
        let b = blur(&img, 4.).unwrap();
        assert!(b.width > img.width);
        let a_out = b.alpha(1, b.height / 2);
        assert!(a_out < 0.05);
        let mid = b.alpha(b.width / 2, b.height / 2);
        assert!(mid > 0.95);
        let t = threshold(&b, 0.5, 0.);
        assert!(t.rgba.chunks_exact(4).all(|p| p[3] == 0 || p[3] == 255));
        // the hardened square is about as large as the original (blur is symmetric)
        let traced = trace(&t, 0.5, 0.5, 0, 0.).unwrap();
        let a = area(&traced.contours);
        assert!(a > 300. && a < 440., "{a}");
    }

    #[test]
    fn trace_drops_specks_and_pinholes_below_the_minimum_area() {
        let mut big = square(0., 0., 40.);
        big.contours.push(vec![Point::new(20., 20.), Point::new(20., 22.), Point::new(22., 22.), Point::new(22., 20.)]); // 2×2 hole
        let speck = square(60., 0., 2.);
        let img = rasterize(&[big, speck], 2., 2.).unwrap().unwrap();
        let all = trace(&img, 0.5, 0.3, 0, 0.).unwrap();
        assert_eq!(all.contours.len(), 3);
        let clean = trace(&img, 0.5, 0.3, 0, 10.).unwrap();
        assert_eq!(clean.contours.len(), 1);
        assert!((area(&clean.contours) - 1600.).abs() < 30.);
    }

    #[test]
    fn noise_is_deterministic_and_changes_alpha() {
        let img = rasterize(&[square(0., 0., 20.)], 2., 2.).unwrap().unwrap();
        let a = noise(&img, 0.5, 3., 7);
        let b = noise(&img, 0.5, 3., 7);
        let c = noise(&img, 0.5, 3., 8);
        assert_eq!(a.rgba, b.rgba);
        assert_ne!(a.rgba, c.rgba);
        assert_ne!(a.rgba, img.rgba);
    }

    #[test]
    fn oversized_rasters_are_refused_with_a_message() {
        let e = rasterize(&[square(0., 0., 5000.)], 2., 0.).unwrap_err();
        assert!(e.contains("解像度"));
    }

    #[test]
    fn png_is_valid_and_cached() {
        let img = rasterize(&[square(0., 0., 4.)], 1., 0.).unwrap().unwrap();
        let url = img.data_url().to_string();
        assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert_eq!(img.data_url(), url);
        let bytes = png(&img);
        // IEND chunk with its fixed CRC
        assert_eq!(&bytes[bytes.len() - 8..], &[0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82]);
    }

    #[test]
    fn embedded_pictures_round_trip_through_png() {
        let img = rasterize(&[square(0.5, 0.5, 6.)], 2., 1.).unwrap().unwrap();
        let url = img.data_url();
        let b64 = url.trim_start_matches("data:image/png;base64,");
        let pic = picture(b64, 10., 20., 4.).unwrap();
        assert_eq!((pic.width, pic.height), (img.width, img.height));
        assert_eq!(pic.x0, 10.);
        assert!((pic.x1() - 14.).abs() < 1e-9, "placed 4 units wide");
        // premultiplied pixels survive (±1 from un/premultiplying)
        assert!(pic.rgba.iter().zip(&img.rgba).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 1));
        assert!(picture("not base64!", 0., 0., 1.).is_err());
        assert!(picture("aGVsbG8=", 0., 0., 1.).is_err());
    }

    #[test]
    fn png_filters_are_decoded() {
        // 2×2 RGB image with Sub, Up, Average and Paeth rows handled by the decoder
        let raw: Vec<u8> = vec![1, 10, 20, 30, 5, 5, 5, 4, 1, 1, 1, 2, 2, 2];
        let rows = [&raw[0..7], &raw[7..14]];
        let mut filtered = Vec::new();
        for r in rows { filtered.extend_from_slice(r); }
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&2u32.to_be_bytes());
        ihdr.extend_from_slice(&2u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&filtered, 6));
        chunk(&mut png, b"IEND", &[]);
        let (w, h, rgba) = decode_png(&png).unwrap();
        assert_eq!((w, h), (2, 2));
        // row 0, Sub: (10,20,30), (15,25,35); row 1, Paeth on top of row 0
        assert_eq!(&rgba[0..8], &[10, 20, 30, 255, 15, 25, 35, 255]);
        assert_eq!(&rgba[8..11], &[11, 21, 31]);
        assert_eq!(&rgba[12..15], &[17, 27, 37]);
    }

    #[test]
    fn transform_moves_and_scales_a_bitmap() {
        let img = rasterize(&[square(0., 0., 10.)], 2., 0.).unwrap().unwrap();
        let moved = transform(&img, |p| Point::new(p.x * 2. + 100., p.y * 2.), |p| Point::new((p.x - 100.) / 2., p.y / 2.)).unwrap();
        assert!((moved.x0 - 100.).abs() < 1.);
        assert!((moved.x1() - 120.).abs() < 1.);
        assert_eq!(moved.scale, 2.);
    }
}
