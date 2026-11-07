#!/usr/bin/env python3
"""Generate review-only SVG families and their comparison sheet (PyGObject/Cairo)."""

from pathlib import Path
import math
import xml.etree.ElementTree as ET

import cairo
import gi

gi.require_version('Rsvg', '2.0')
from gi.repository import Rsvg

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[1]
WHITE = '#ffffff'
INACTIVE = '#808080'
GREEN = '#5cdb87'
BOLT = 'M17.5 6.8 11.5 17.2H16L14.5 25.2 21 14.6H16.5Z'
FAMILIES = [
    ('01-soft-fill', 'Soft Fill', 2.2, 'Rounded and substantial.', 'Closest to the chosen battery and arrow.'),
    ('02-soft-outline', 'Soft Outline', 1.5, 'Light, open silhouettes.', 'One line weight across the family.'),
    ('03-compact', 'Compact 1x', 2.0, 'Simpler shapes; 2 px strokes.', 'Drawn around a 20 px working grid.'),
]


def svg(body, size=20):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">\n{body}\n</svg>\n'


def path(d, stroke, width, fill='none'):
    return f'<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>'


def wifi(family, width, level):
    compact = family == '03-compact'
    outer = 'M3 7Q10 1 17 7' if compact else 'M2 6.5Q10 -0.5 18 6.5'
    inner = 'M6 10Q10 6.5 14 10' if compact else 'M5 10Q10 5.5 15 10'
    if level == 'off':
        mark = 'M13 12.5 17 16.5M17 12.5 13 16.5'
        body = path(outer, INACTIVE, width) + path(inner, INACTIVE, width)
        body += f'<circle cx="10" cy="14.5" r="1.5" fill="{INACTIVE}"/>'
        body += '<defs><mask id="cross-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="20" height="20"><rect width="20" height="20" fill="white"/>'
        body += path(mark, 'black', width + 2) + '</mask></defs>'
        # The gap is transparent, so the off marker also works over wallpaper.
        shapes = body[:body.index('<defs>')]
        definitions = body[body.index('<defs>'):]
        return svg(definitions + f'<g mask="url(#cross-gap)">{shapes}</g>' + path(mark, WHITE, width))
    return svg(path(outer, WHITE if level == 100 else INACTIVE, width)
               + path(inner, WHITE if level >= 66 else INACTIVE, width)
               + f'<circle cx="10" cy="14.5" r="1.5" fill="{WHITE}"/>')


def volume(family, width, level):
    outline = family == '02-soft-outline'
    compact = family == '03-compact'
    if compact:
        speaker = 'M3 7H6L10 4V16L6 13H3Q2 13 2 12V8Q2 7 3 7Z'
    else:
        speaker = 'M3 7H5.5L8.6 4.3Q9.5 3.5 9.5 4.7V15.3Q9.5 16.5 8.6 15.7L5.5 13H3Q2 13 2 12V8Q2 7 3 7Z'
    body = path(speaker, WHITE if outline else 'none', width if outline else 0, 'none' if outline else WHITE)
    if level == 'mute':
        body += path('M13.5 8 17.5 12M17.5 8 13.5 12', WHITE, width)
    else:
        body += path('M12.5 7Q15.5 10 12.5 13', WHITE if level >= 50 else INACTIVE, width)
        body += path('M15.5 4Q21 10 15.5 16', WHITE if level == 100 else INACTIVE, width)
    return svg(body)


def bluetooth(width):
    return svg(path('M10 2V18L15 14 5 6M10 2 15 6 5 14', WHITE, width))


def brightness(family, width):
    filled = family != '02-soft-outline'
    body = f'<circle cx="10" cy="10" r="3" fill="{WHITE if filled else "none"}" stroke="{WHITE if not filled else "none"}" stroke-width="{width}"/>'
    body += path('M10 2V4M10 16V18M2 10H4M16 10H18M4.5 4.5 6 6M14 14 15.5 15.5M4.5 15.5 6 14M14 6 15.5 4.5', WHITE, width)
    return svg(body)


def battery(family, charging=False):
    compact = family == '03-compact'
    outline = family == '02-soft-outline'
    size = 26 if compact else 32
    transform = ' transform="scale(0.8125)"' if compact else ''
    colour = GREEN if charging else WHITE
    if compact:
        body = f'<rect x="1" y="6" width="21" height="14" rx="3" fill="none" stroke="{WHITE}" stroke-width="2"/>'
        body += f'<rect x="24" y="10" width="2" height="6" rx="1" fill="{WHITE}"/>'
        body += f'<rect x="4" y="9" width="10.5" height="8" rx="1.5" fill="{colour}"/>'
    else:
        weight = 1.8 if outline else 2.0
        body = f'<rect x="2" y="8.25" width="25" height="15.5" rx="4" fill="none" stroke="{WHITE}" stroke-width="{weight}"/>'
        body += f'<rect x="29" y="13" width="1.8" height="6" rx="0.9" fill="{WHITE}"/>'
        body += f'<rect x="4.8" y="{12.5 if outline else 11.05}" width="13.58" height="{7 if outline else 9.9}" rx="1.6" fill="{colour}"/>'
    if charging:
        definitions = f'<defs><mask id="bolt-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="{size}" height="{size}"><rect width="{size}" height="{size}" fill="white"/><path d="{BOLT}"{transform} fill="black" stroke="black" stroke-width="1.9" stroke-linejoin="round"/></mask></defs>'
        body = definitions + f'<g mask="url(#bolt-gap)">{body}</g><path d="{BOLT}"{transform} fill="{WHITE}"/>'
    return svg(body, size)


def generate_assets():
    for family, _, width, _, _ in FAMILIES:
        folder = OUT / family
        folder.mkdir(exist_ok=True)
        assets = {f'wifi-{level}': wifi(family, width, level) for level in [33, 66, 100, 'off']}
        assets.update({f'volume-{level}': volume(family, width, level) for level in [20, 50, 100, 'mute']})
        assets.update({'bluetooth': bluetooth(width), 'brightness': brightness(family, width),
                       'battery-70': battery(family), 'battery-charging-70': battery(family, True),
                       'control': svg(path('M6 8 10 12 14 8' if family == '03-compact' else 'M5.5 8 10 12.5 14.5 8', WHITE, width))})
        for name, data in assets.items():
            ET.fromstring(data)
            (folder / f'{name}.svg').write_text(data)


def preview():
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, 1500, 1210)
    ctx = cairo.Context(surface)
    ctx.set_source_rgb(.055, .065, .085)
    ctx.paint()

    def text(s, x, y, size=16, colour=(.94, .95, .97), bold=False):
        ctx.select_font_face('sans-serif', 0, cairo.FONT_WEIGHT_BOLD if bold else 0)
        ctx.set_font_size(size)
        ctx.set_source_rgb(*colour)
        ctx.move_to(x, y)
        ctx.show_text(s)

    def panel(x, y, w, h, r=16, colour=(.10, .12, .15)):
        ctx.new_sub_path()
        for cx, cy, angle in [(x+w-r,y+r,-90),(x+w-r,y+h-r,0),(x+r,y+h-r,90),(x+r,y+r,180)]:
            ctx.arc(cx, cy, r, math.radians(angle), math.radians(angle+90))
        ctx.close_path()
        ctx.set_source_rgb(*colour)
        ctx.fill()

    def icon(file, x, y, size):
        handle = Rsvg.Handle.new_from_file(str(file))
        rect = Rsvg.Rectangle()
        rect.x, rect.y, rect.width, rect.height = x, y, size, size
        assert handle.render_document(ctx, rect), file

    def bar(folder, x, y, current=False):
        panel(x, y, 635, 44, 11, (.055, .065, .085))
        text('10:24', x+17, y+28, 14)
        text('EN', x+115, y+28, 13)
        names = ['wifi-100', 'bluetooth', 'volume-100', 'brightness', 'battery-70', 'control']
        offsets = [192, 258, 324, 390, 456, 533]
        for name, dx in zip(names, offsets):
            size = 26 if name.startswith('battery') else 20
            if current and name == 'brightness':
                # Brightness currently comes from the system theme, so leave it out of the reference.
                continue
            file = folder / (('nshell-' if current else '') + name + '.svg')
            icon(file, x+dx, y+(44-size)/2, size)

    text('NSHELL / A COHERENT STATUS FAMILY', 50, 43, 13, (.5, .6, .7), True)
    text('Same language. Different weight.', 50, 96, 36, bold=True)
    text('Wi-Fi, sound, Bluetooth, brightness, battery and the Soft arrow. One family at a time.', 51, 130, 16, (.6,.67,.75))
    panel(38, 160, 1424, 120)
    text('CURRENT', 65, 193, 13, (.5,.63,.74), True)
    text('The installed set', 65, 227, 22, bold=True)
    bar(ROOT/'icons/scalable/status', 448, 198, current=True)
    text('Actual panel sizes', 1130, 226, 14, (.6,.67,.75))
    labels = [('wifi-100','Wi-Fi'), ('volume-100','Sound'), ('bluetooth','Bluetooth'),
              ('brightness','Brightness'), ('battery-70','Battery'), ('control','Control')]
    for i, (family, title, width, detail, extra) in enumerate(FAMILIES):
        folder = OUT/family
        y = 306+i*276
        panel(38, y, 1424, 250)
        text('0'+str(i+1), 65, y+33, 13, (.5,.63,.74), True)
        text(title, 65, y+74, 27, bold=True)
        text(detail, 65, y+108, 14, (.62,.69,.76))
        text(extra, 65, y+134, 13, (.52,.60,.69))
        text('20 px icons / 26 px battery', 65, y+211, 13, (.52,.60,.69))
        for j, (name, label) in enumerate(labels):
            cx = 494+j*160
            text(label, cx-25, y+30, 13, (.6,.68,.76))
            size = 72.8 if name.startswith('battery') else 56
            icon(folder/(name+'.svg'), cx-size/2, y+74-size/2, size)
        bar(folder, 448, y+166)
        text('WEAK / MUTED / CHARGING', 1120, y+155, 11, (.5,.63,.74), True)
        for name, dx, size in [('wifi-33',1128,20), ('volume-mute',1204,20), ('battery-charging-70',1280,26)]:
            icon(folder/(name+'.svg'), dx, y+181-(size-20)/2, size)
    text('Original SVG geometry / transparent backgrounds / concepts only; the installed shell is unchanged.', 50, 1178, 13, (.52,.61,.70))
    surface.write_to_png(str(OUT/'topbar-icon-families.png'))


if __name__ == '__main__':
    generate_assets()
    preview()
    print('Generated 39 standalone SVGs and topbar-icon-families.png')
