import json
import re
import sys
import xml.etree.ElementTree as ET

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path


def empty_glyph():
    return TTGlyphPen(None).glyph()


def notdef_glyph():
    pen = TTGlyphPen(None)
    pen.moveTo((40, 0))
    pen.lineTo((560, 0))
    pen.lineTo((560, 700))
    pen.lineTo((40, 700))
    pen.closePath()
    pen.moveTo((100, 60))
    pen.lineTo((100, 640))
    pen.lineTo((500, 640))
    pen.lineTo((500, 60))
    pen.closePath()
    return pen.glyph()


def glyph_from_svg(svg, font_metrics, left_side_bearing=0):
    root = ET.fromstring(svg)
    paths = root.findall(".//{http://www.w3.org/2000/svg}path")
    if not paths or not any(path.attrib.get("d", "").strip() for path in paths):
        return empty_glyph()
    ascender_y = float(font_metrics["ascender_y"])
    descender_y = float(font_metrics["descender_y"])
    baseline_y = float(font_metrics["baseline_y"])
    origin_x = float(font_metrics["origin_x"])
    y_scale = 1000.0 / max(descender_y - ascender_y, 40.0)
    # 作図時の縦横比を保持する。左右の箱幅は変形ではなく送り幅として扱う。
    x_scale = y_scale
    tx = -origin_x * x_scale + float(left_side_bearing)
    ty = baseline_y * y_scale
    target = TTGlyphPen(None)
    quadratic = Cu2QuPen(target, max_err=1.0, reverse_direction=True)
    transformed = TransformPen(quadratic, (x_scale, 0, 0, -y_scale, tx, ty))
    for path in paths:
        data = path.attrib.get("d", "").strip()
        if data:
            parse_path(data, transformed)
    return target.glyph()


def postscript_name(value):
    cleaned = re.sub(r"[^A-Za-z0-9-]", "", value.replace(" ", "-"))
    return cleaned or "ThreadType"


def main(payload_path, output_path):
    with open(payload_path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)

    font_metrics = payload.get(
        "metrics",
        {
            "origin_x": 70.0,
            "right_x": 550.0,
            "ascender_y": 70.0,
            "x_height_y": 310.0,
            "baseline_y": 710.0,
            "descender_y": 870.0,
        },
    )
    metric_scale = 1000.0 / max(
        float(font_metrics["descender_y"]) - float(font_metrics["ascender_y"]),
        40.0,
    )
    ascender = round(
        (float(font_metrics["baseline_y"]) - float(font_metrics["ascender_y"]))
        * metric_scale
    )
    descender = -round(
        (float(font_metrics["descender_y"]) - float(font_metrics["baseline_y"]))
        * metric_scale
    )
    x_height = round(
        (float(font_metrics["baseline_y"]) - float(font_metrics["x_height_y"]))
        * metric_scale
    )

    glyph_order = [".notdef"]
    glyphs = {".notdef": notdef_glyph()}
    metrics = {".notdef": (600, 0)}
    cmap = {}
    seen = {".notdef"}

    for item in payload["glyphs"]:
        name = item["name"]
        if name in seen:
            continue
        seen.add(name)
        advance = max(100, int(item.get("advance_width", 600)))
        glyph_order.append(name)
        left_side_bearing = int(item.get("left_side_bearing", 0))
        glyphs[name] = glyph_from_svg(
            item.get("svg", ""), font_metrics, left_side_bearing
        )
        metrics[name] = (advance, left_side_bearing)
        cmap[ord(item["character"])] = name

    family = payload.get("family_name", "ThreadType")
    style = payload.get("style_name", "Regular")
    full_name = f"{family} {style}".strip()
    ps_name = postscript_name(f"{family}-{style}")

    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder(glyph_order)
    builder.setupCharacterMap(cmap)
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics(metrics)
    builder.setupHorizontalHeader(ascent=ascender, descent=descender)
    builder.setupOS2(
        sTypoAscender=ascender,
        sTypoDescender=descender,
        sTypoLineGap=0,
        usWinAscent=max(0, ascender),
        usWinDescent=max(0, -descender),
        sxHeight=x_height,
    )
    builder.setupNameTable(
        {
            "familyName": family,
            "styleName": style,
            "uniqueFontIdentifier": f"ThreadType:{full_name}",
            "fullName": full_name,
            "psName": ps_name,
            "version": "Version 1.000",
        }
    )
    builder.setupPost()
    builder.setupMaxp()
    builder.save(output_path)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
