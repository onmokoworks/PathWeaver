use super::*;
use crate::export::tests::{curve, square};
use crate::model::{Artboard, Placed, Stroke};

/// Check the cross-reference table and return the objects by number.
fn objects(pdf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf).to_string();
    assert!(text.starts_with("%PDF-1.4\n"));
    assert!(text.ends_with("%%EOF\n"));
    let start: usize = text[text.rfind("startxref\n").unwrap() + 10..].lines().next().unwrap().parse().unwrap();
    let table = &pdf[start..];
    assert!(table.starts_with(b"xref\n0 "));
    let lines: Vec<&str> = std::str::from_utf8(&table[..table.windows(7).position(|w| w == b"trailer").unwrap()]).unwrap().lines().collect();
    let count: usize = lines[1].split(' ').nth(1).unwrap().parse().unwrap();
    let mut out = vec![String::new()];
    for (i, line) in lines[3..3 + count - 1].iter().enumerate() {
        let offset: usize = line[..10].parse().unwrap();
        let head = format!("{} 0 obj\n", i + 1);
        assert!(pdf[offset..].starts_with(head.as_bytes()), "object {} at {offset}", i + 1);
        let end = offset + pdf[offset..].windows(6).position(|w| w == b"endobj").unwrap();
        out.push(String::from_utf8_lossy(&pdf[offset..end]).to_string());
    }
    out
}
/// The decompressed content streams of the pages, in order.
fn contents(pdf: &[u8], objs: &[String]) -> Vec<String> {
    objs.iter()
        .filter(|o| o.contains("/Type /Page ") || o.contains("/Type /Page>"))
        .map(|page| {
            let at = page.find("/Contents ").unwrap() + 10;
            let id: usize = page[at..].split(' ').next().unwrap().parse().unwrap();
            let obj = &objs[id];
            let len: usize = obj[obj.find("/Length ").unwrap() + 8..].split(' ').next().unwrap().parse().unwrap();
            let head = format!("{id} 0 obj\n");
            let offset = pdf.windows(head.len()).position(|w| w == head.as_bytes()).unwrap();
            let data = offset + pdf[offset..].windows(7).position(|w| w == b"stream\n").unwrap() + 7;
            String::from_utf8(miniz_oxide::inflate::decompress_to_vec_zlib(&pdf[data..data + len]).unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn one_page_per_frame_with_curves_lines_and_fills() {
    let mut d = Document::default();
    let mut c = curve(&mut d);
    c.start_cap = Cap::Round;
    c.end_cap = Cap::Round;
    let s = square(&mut d, 300., 0., 50.);
    d.strokes = vec![c, s];
    d.artboards = vec![Artboard { id: d.id(), name: "A".into(), x: -20., y: -100., w: 240., h: 200. }];
    let pdf = pdf(&d, &[[-20., -100., 240., 200.], [290., -10., 70., 70.]], Some([255, 255, 255]), Options::default()).unwrap();
    let objs = objects(&pdf);
    assert!(objs.iter().any(|o| o.contains("/Type /Pages /Kids [") && o.contains("/Count 2")));
    assert!(objs.iter().any(|o| o.contains("/MediaBox [0 0 240 200]")));
    assert!(objs.iter().any(|o| o.contains("/MediaBox [0 0 70 70]")));
    let pages = contents(&pdf, &objs);
    assert_eq!(pages.len(), 2);
    // y flips at the frame's top
    assert!(pages[0].starts_with("1.000000 0.000000 0.000000 -1.000000 20 100 cm\n"), "{}", pages[0]);
    assert!(pages[0].contains("1 1 1 rg\n-20 -100 240 200 re\nf\n"));
    assert!(pages[0].contains("0 0 m\n60 -80 140 80 200 0 c\n"), "{}", pages[0]);
    assert!(pages[0].contains("RG 10 w 1 J 0 j 10 M\n"));
    assert!(pages[0].contains("\nS\n"));
    assert!(pages[0].contains("0.784 0.118 0.118 rg\n300 0 m\n350 0 l\n"));
    assert!(pages[0].contains("h\nf\n"));
    // 輪郭にする: no lines left
    let pdf = super::pdf(&d, &[[-20., -100., 240., 200.]], None, Options { outline: true }).unwrap();
    let page = &contents(&pdf, &objects(&pdf))[0];
    assert!(!page.contains(" w ") && page.contains(" c\n"));
}

#[test]
fn pictures_with_alpha_opacity_and_jpeg() {
    let mut d = Document::default();
    // 2×1 PNG: an opaque red and a half-clear pixel (premultiplied)
    let png = crate::raster::Image::new(0., 0., 1., 2, 1, vec![255, 0, 0, 255, 0, 0, 64, 128]).data_url().to_string();
    // a minimal JPEG header: SOI, APP0, SOF0 (16×8, 3 components)
    let jpeg_bytes: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 17, 8, 0, 8, 0, 16, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xff, 0xd9];
    let jpeg = format!("data:image/jpeg;base64,{}", crate::raster::base64(&jpeg_bytes));
    d.assets.insert("p".into(), png);
    d.assets.insert("j".into(), jpeg);
    let mut a = Stroke::new(d.id(), "png".into());
    a.image = Some(Placed { asset: "p".into(), width: 2., height: 1., matrix: [10., 0., 0., 10., 0., 0.], opacity: 0.5, reference: false, knockout: None });
    let mut b = Stroke::new(d.id(), "jpeg".into());
    b.image = Some(Placed { asset: "j".into(), width: 16., height: 8., matrix: [1., 0., 0., 1., 0., 20.], opacity: 1., reference: false, knockout: None });
    let mut c = a.clone();
    c.id = d.id();
    d.strokes = vec![a, b, c];
    let pdf = pdf(&d, &[[0., 0., 40., 40.]], None, Options::default()).unwrap();
    let objs = objects(&pdf);
    let images: Vec<_> = objs.iter().filter(|o| o.contains("/Subtype /Image")).collect();
    assert_eq!(images.len(), 3, "png + its mask + jpeg; the second use shares the first");
    assert!(images.iter().any(|o| o.contains("/Width 2 /Height 1 /ColorSpace /DeviceGray")));
    assert!(images.iter().any(|o| o.contains("/ColorSpace /DeviceRGB") && o.contains("/SMask ")));
    assert!(images.iter().any(|o| o.contains("/Width 16 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode")));
    assert!(objs.iter().any(|o| o.contains("/GS0 << /Type /ExtGState /ca 0.5 /CA 0.5 >>")));
    let page = &contents(&pdf, &objs)[0];
    // pixel space → world, row 0 on top
    assert!(page.contains("20.000000 0.000000 0.000000 -10.000000 0 10 cm\n/Im0 Do"), "{page}");
    assert_eq!(page.matches("/Im0 Do").count(), 2);
    assert!(page.contains("/GS0 gs"));
}

#[test]
fn huge_pages_use_a_user_unit_and_bad_frames_are_refused() {
    let mut d = Document::default();
    let s = square(&mut d, 0., 0., 10.);
    d.strokes.push(s);
    let pdf = pdf(&d, &[[0., 0., 28800., 100.]], None, Options::default()).unwrap();
    assert!(objects(&pdf).iter().any(|o| o.contains("/MediaBox [0 0 14400 50] /UserUnit 2")));
    assert!(super::pdf(&d, &[], None, Options::default()).is_err());
    assert!(super::pdf(&d, &[[0., 0., 0., 10.]], None, Options::default()).is_err());
}

#[test]
fn gradient_fills_are_shadings() {
    use crate::paint::{Gradient, GradientStop};
    let mut d = Document::default();
    let mut s = square(&mut d, 0., 0., 100.);
    let stops = vec![
        GradientStop { offset: 0.2, color: [255, 0, 0], opacity: 1. },
        GradientStop { offset: 0.5, color: [0, 255, 0], opacity: 1. },
        GradientStop { offset: 1., color: [0, 0, 255], opacity: 1. },
    ];
    s.fill_gradient = Some(Gradient::radial(kurbo::Point::new(50., 50.), 50., stops));
    d.strokes = vec![s];
    let pdf = pdf(&d, &[[0., 0., 100., 100.]], None, Options::default()).unwrap();
    let objs = objects(&pdf);
    assert!(objs.iter().any(|o| o.contains("/Shading << /Sh0 << /ShadingType 3") && o.contains("/Bounds [0.2 0.5]")), "{objs:?}");
    let pages = contents(&pdf, &objs);
    assert!(pages[0].contains("W n\n") && pages[0].contains("/Sh0 sh\n"));
}
