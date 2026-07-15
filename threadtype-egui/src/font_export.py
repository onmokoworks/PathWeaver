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


def glyph_from_svg(svg, advance):
    root = ET.fromstring(svg)
    view_box = [float(value) for value in root.attrib.get("viewBox", "0 0 100 100").split()]
    min_x, min_y, width, height = view_box
    paths = root.findall(".//{http://www.w3.org/2000/svg}path")
    if not paths or not any(path.attrib.get("d", "").strip() for path in paths):
        return empty_glyph()
    width = max(width, 1.0)
    height = max(height, 1.0)
    scale = min(max(advance - 80, 1) / width, 700.0 / height)
    tx = (advance - width * scale) * 0.5 - min_x * scale
    ty = 80.0 + (min_y + height) * scale
    target = TTGlyphPen(None)
    quadratic = Cu2QuPen(target, max_err=1.0, reverse_direction=True)
    transformed = TransformPen(quadratic, (scale, 0, 0, -scale, tx, ty))
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
        glyphs[name] = glyph_from_svg(item.get("svg", ""), advance)
        metrics[name] = (advance, 0)
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
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupOS2(
        sTypoAscender=800,
        sTypoDescender=-200,
        sTypoLineGap=0,
        usWinAscent=800,
        usWinDescent=200,
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
