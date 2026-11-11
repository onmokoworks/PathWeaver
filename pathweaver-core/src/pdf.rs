//! A minimal PDF writer for export: the scene of `export.rs` as vector paths
//! (fills, lines with their caps / joins, opacity) and embedded bitmaps, one
//! page per frame, 1 unit = 1 pt. Streams are Flate-compressed (miniz_oxide).
use std::collections::HashMap;
use std::fmt::Write as _;

use kurbo::{Affine, BezPath, PathEl, Point};

use super::export::{num, scene, Item, Options, Picture, Shape, Source};
use super::model::{Cap, Document, Join};

/// Largest page side most PDF readers accept (200 in); bigger pages use UserUnit.
const MAX_PAGE: f64 = 14400.;
/// Largest embedded picture.
const MAX_IMAGE_PIXELS: usize = 64_000_000;

struct Writer {
    out: Vec<u8>,
    offsets: Vec<Option<usize>>,
}
impl Writer {
    fn reserve(&mut self) -> usize {
        self.offsets.push(None);
        self.offsets.len()
    }
    fn object(&mut self, id: usize, body: &[u8]) {
        self.offsets[id - 1] = Some(self.out.len());
        self.out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
    }
    /// A stream object; `dict` is the inside of its dictionary minus /Length.
    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.object(id, &body);
    }
}
fn flate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// Shared page resources: opacity states and pictures, named once.
#[derive(Default)]
struct Resources {
    states: HashMap<(u32, u32), String>,
    images: HashMap<String, (String, usize)>,
}

fn path_ops(p: &BezPath, c: &mut String) {
    let mut at = Point::ZERO;
    for e in p.elements() {
        match *e {
            PathEl::MoveTo(a) => {
                let _ = writeln!(c, "{} {} m", num(a.x), num(a.y));
                at = a;
            }
            PathEl::LineTo(a) => {
                let _ = writeln!(c, "{} {} l", num(a.x), num(a.y));
                at = a;
            }
            PathEl::QuadTo(a, b) => {
                let (c1, c2) = (at + (a - at) * (2. / 3.), b + (a - b) * (2. / 3.));
                let _ = writeln!(c, "{} {} {} {} {} {} c", num(c1.x), num(c1.y), num(c2.x), num(c2.y), num(b.x), num(b.y));
                at = b;
            }
            PathEl::CurveTo(a, b, d) => {
                let _ = writeln!(c, "{} {} {} {} {} {} c", num(a.x), num(a.y), num(b.x), num(b.y), num(d.x), num(d.y));
                at = d;
            }
            PathEl::ClosePath => c.push_str("h\n"),
        }
    }
}
fn rgb(c: [u8; 3]) -> String {
    format!("{} {} {}", num(c[0] as f64 / 255.), num(c[1] as f64 / 255.), num(c[2] as f64 / 255.))
}
fn matrix(m: Affine) -> String {
    let c = m.as_coeffs();
    format!("{:.6} {:.6} {:.6} {:.6} {} {}", c[0], c[1], c[2], c[3], num(c[4]), num(c[5]))
}

struct Pdf {
    w: Writer,
    res: Resources,
}
impl Pdf {
    /// The graphics state name for fill / line opacity (none when opaque).
    fn state(&mut self, fill: f64, line: f64) -> Option<String> {
        let key = ((fill.clamp(0., 1.) * 1000.).round() as u32, (line.clamp(0., 1.) * 1000.).round() as u32);
        if key == (1000, 1000) {
            return None;
        }
        let n = self.res.states.len();
        Some(self.res.states.entry(key).or_insert_with(|| format!("GS{n}")).clone())
    }
    fn shape(&mut self, s: &Shape, c: &mut String) {
        if s.path.elements().is_empty() || (s.fill.is_none() && s.line.is_none()) {
            return;
        }
        c.push_str("q\n");
        if let Some(gs) = self.state(s.fill.map_or(1., |p| p.opacity), s.line.map_or(1., |l| l.paint.opacity)) {
            let _ = writeln!(c, "/{gs} gs");
        }
        if let Some(f) = s.fill {
            let _ = writeln!(c, "{} rg", rgb(f.color));
        }
        if let Some(l) = s.line {
            let cap = match l.cap {
                Cap::Butt => 0,
                Cap::Round => 1,
                Cap::Square => 2,
            };
            let join = match l.join {
                Join::Miter => 0,
                Join::Round => 1,
                Join::Bevel => 2,
            };
            let _ = writeln!(c, "{} RG {} w {cap} J {join} j {} M", rgb(l.paint.color), num(l.width), num(l.miter_limit));
        }
        path_ops(&s.path, c);
        c.push_str(match (s.fill.is_some(), s.line.is_some()) {
            (true, true) => "B\n",
            (true, false) => "f\n",
            _ => "S\n",
        });
        c.push_str("Q\n");
    }
    fn picture(&mut self, p: &Picture, c: &mut String) -> Result<(), String> {
        let key = match &p.source {
            Source::Data(s) => s.clone(),
            Source::Bitmap(img) => format!("bitmap:{:p}", std::sync::Arc::as_ptr(img)),
        };
        let name = match self.res.images.get(&key) {
            Some((n, _)) => n.clone(),
            None => {
                let id = self.image(&p.source)?;
                let n = format!("Im{}", self.res.images.len());
                self.res.images.insert(key, (n.clone(), id));
                n
            }
        };
        c.push_str("q\n");
        if let Some(gs) = self.state(p.opacity, p.opacity) {
            let _ = writeln!(c, "/{gs} gs");
        }
        // The image's unit square (row 0 on top) → its pixels → the world.
        let m = p.matrix * Affine::new([p.width, 0., 0., -p.height, 0., p.height]);
        let _ = writeln!(c, "{} cm\n/{name} Do\nQ", matrix(m));
        Ok(())
    }
    /// Write one picture (and its alpha mask) as image objects.
    fn image(&mut self, source: &Source) -> Result<usize, String> {
        let (w, h, rgb, alpha) = match source {
            Source::Bitmap(img) => {
                let mut rgb = Vec::with_capacity(img.width * img.height * 3);
                let mut alpha = Vec::with_capacity(img.width * img.height);
                for p in img.rgba.chunks_exact(4) {
                    let a = p[3];
                    let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
                    rgb.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2])]);
                    alpha.push(a);
                }
                (img.width, img.height, rgb, alpha)
            }
            Source::Data(url) => {
                let (mime, data) = url
                    .strip_prefix("data:")
                    .and_then(|r| r.split_once(";base64,"))
                    .ok_or("画像データが壊れています")?;
                let bytes = super::raster::unbase64(data).ok_or("画像データが壊れています")?;
                if mime == "image/jpeg" || mime == "image/jpg" {
                    return self.jpeg(&bytes);
                }
                if mime != "image/png" {
                    return Err(format!("PDF に入れられない画像形式です（{mime}）"));
                }
                let (w, h, channels, px) = super::raster::decode_png_pixels(&bytes, MAX_IMAGE_PIXELS)?;
                let mut rgb = Vec::with_capacity(w * h * 3);
                let mut alpha = Vec::with_capacity(w * h);
                for p in px.chunks_exact(channels) {
                    rgb.extend_from_slice(&p[..3]);
                    alpha.push(if channels == 4 { p[3] } else { 255 });
                }
                (w, h, rgb, alpha)
            }
        };
        let mask = if alpha.iter().any(|&a| a != 255) {
            let id = self.w.reserve();
            self.w.stream(id, &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"), &flate(&alpha));
            format!(" /SMask {id} 0 R")
        } else {
            String::new()
        };
        let id = self.w.reserve();
        self.w.stream(id, &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode{mask}"), &flate(&rgb));
        Ok(id)
    }
    /// A JPEG goes in as it is (DCTDecode).
    fn jpeg(&mut self, b: &[u8]) -> Result<usize, String> {
        let (w, h, comps) = jpeg_info(b).ok_or("JPEG を読めません")?;
        let space = match comps {
            1 => "/DeviceGray",
            3 => "/DeviceRGB",
            // Adobe CMYK JPEGs are stored inverted
            4 => "/DeviceCMYK /Decode [1 0 1 0 1 0 1 0]",
            _ => return Err("JPEG を読めません".into()),
        };
        let id = self.w.reserve();
        self.w.stream(id, &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {space} /BitsPerComponent 8 /Filter /DCTDecode"), b);
        Ok(id)
    }
    fn items(&mut self, items: &[Item], c: &mut String) -> Result<(), String> {
        for item in items {
            match item {
                Item::Group(_, children) => self.items(children, c)?,
                Item::Shape(_, s) => self.shape(s, c),
                Item::Picture(_, p) => self.picture(p, c)?,
            }
        }
        Ok(())
    }
}

/// Width, height and colour components from a JPEG's frame header.
fn jpeg_info(b: &[u8]) -> Option<(usize, usize, u8)> {
    if b.len() < 4 || b[0] != 0xff || b[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            let f = b.get(i + 4..i + 10)?;
            let h = u16::from_be_bytes([f[1], f[2]]) as usize;
            let w = u16::from_be_bytes([f[3], f[4]]) as usize;
            return (w > 0 && h > 0).then_some((w, h, f[5]));
        }
        i += 2 + len;
    }
    None
}

/// Bytes as base64 (how the PDF crosses to the web side).
pub fn base64(bytes: &[u8]) -> String {
    super::raster::base64(bytes)
}

/// A PDF with one page per frame (x, y, w, h in units = points), optionally
/// over a solid `background`.
pub fn pdf(doc: &Document, frames: &[[f64; 4]], background: Option<[u8; 3]>, options: Options) -> Result<Vec<u8>, String> {
    if frames.is_empty() {
        return Err("書き出す範囲がありません".into());
    }
    for f in frames {
        if !(f.iter().all(|v| v.is_finite()) && f[2] > 0. && f[3] > 0.) {
            return Err("書き出す範囲が空です".into());
        }
    }
    let items = scene(doc, options)?;
    let mut pdf = Pdf { w: Writer { out: b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec(), offsets: vec![] }, res: Resources::default() };
    let catalog = pdf.w.reserve();
    let pages = pdf.w.reserve();
    let resources = pdf.w.reserve();
    let info = pdf.w.reserve();
    let mut kids = vec![];
    for &[x, y, w, h] in frames {
        // Pages past the readers' limit are scaled down and given a UserUnit.
        let k = (w.max(h) / MAX_PAGE).max(1.);
        let mut c = String::new();
        // world (y down, frame origin) → page (y up)
        let _ = writeln!(c, "{} cm", matrix(Affine::scale(1. / k) * Affine::new([1., 0., 0., -1., -x, y + h])));
        if let Some(bg) = background {
            let _ = writeln!(c, "{} rg\n{} {} {} {} re\nf", rgb(bg), num(x), num(y), num(w), num(h));
        }
        pdf.items(&items, &mut c)?;
        let content = pdf.w.reserve();
        pdf.w.stream(content, "/Filter /FlateDecode", &flate(c.as_bytes()));
        let page = pdf.w.reserve();
        let unit = if k > 1. { format!(" /UserUnit {}", num(k)) } else { String::new() };
        pdf.w.object(
            page,
            format!(
                "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {} {}]{unit} /Resources {resources} 0 R /Contents {content} 0 R >>",
                num(w / k),
                num(h / k)
            )
            .as_bytes(),
        );
        kids.push(page);
    }
    let mut states: Vec<_> = pdf.res.states.iter().collect();
    states.sort_by(|a, b| a.1.cmp(b.1));
    let states: String = states.iter().map(|((f, l), n)| format!(" /{n} << /Type /ExtGState /ca {} /CA {} >>", num(*f as f64 / 1000.), num(*l as f64 / 1000.))).collect();
    let mut images: Vec<_> = pdf.res.images.values().collect();
    images.sort();
    let images: String = images.iter().map(|(n, id)| format!(" /{n} {id} 0 R")).collect();
    pdf.w.object(resources, format!("<< /ProcSet [/PDF /ImageB /ImageC] /ExtGState <<{states} >> /XObject <<{images} >> >>").as_bytes());
    let kids_ref: String = kids.iter().map(|k| format!("{k} 0 R ")).collect();
    pdf.w.object(pages, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids_ref.trim_end(), kids.len()).as_bytes());
    pdf.w.object(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>").as_bytes());
    pdf.w.object(info, b"<< /Producer (PathWeaver) /Creator (PathWeaver) >>");
    let xref = pdf.w.out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", pdf.w.offsets.len() + 1);
    for o in &pdf.w.offsets {
        let _ = writeln!(tail, "{:010} 00000 n ", o.ok_or("PDF の組み立てに失敗しました")?);
    }
    let _ = write!(tail, "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", pdf.w.offsets.len() + 1);
    pdf.w.out.extend_from_slice(tail.as_bytes());
    Ok(pdf.w.out)
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
