#!/usr/bin/env python3
"""Render the control-center proposal; this does not change the running shell."""

from pathlib import Path
import math

import cairo
import gi
from PIL import Image, ImageFilter

gi.require_version("Rsvg", "2.0")
gi.require_version("Pango", "1.0")
gi.require_version("PangoCairo", "1.0")
from gi.repository import Pango, PangoCairo, Rsvg


HERE = Path(__file__).resolve().parent
ICON_DIR = Path("/usr/share/icons/Adwaita/symbolic")
ICONS = {path.stem: path for path in ICON_DIR.rglob("*.svg")}
SCALE = 2
c: cairo.Context
WHITE, MUTED, BLUE = "f4f5f7", "a5a9b2", "62a6ff"


def color(value, alpha=1):
    c.set_source_rgba(*(int(value[i:i+2], 16) / 255 for i in (0, 2, 4)), alpha)


def rounded_path(ctx, x, y, w, h, radius):
    ctx.new_sub_path()
    for cx, cy, start in [(x+w-radius, y+radius, -90),
                           (x+w-radius, y+h-radius, 0),
                           (x+radius, y+h-radius, 90),
                           (x+radius, y+radius, 180)]:
        ctx.arc(cx, cy, radius, math.radians(start), math.radians(start+90))
    ctx.close_path()


def box(x, y, w, h, radius, fill, stroke=None):
    rounded_path(c, x, y, w, h, radius)
    color(fill)
    c.fill_preserve()
    if stroke:
        color(stroke)
        c.set_line_width(1)
        c.stroke()
    else:
        c.new_path()


def shadow(x, y, w, h, radius, blur=5, offset=3, opacity=.35):
    mask = cairo.ImageSurface(cairo.FORMAT_A8, 440*SCALE, 652*SCALE)
    ctx = cairo.Context(mask)
    ctx.scale(SCALE, SCALE)
    rounded_path(ctx, x, y+offset, w, h, radius)
    ctx.set_source_rgba(1, 1, 1, 1)
    ctx.fill()
    mask.flush()
    alpha = Image.frombytes("L", (mask.get_width(), mask.get_height()),
                           bytes(mask.get_data()), "raw", "L", mask.get_stride())
    pixels = bytearray(alpha.filter(ImageFilter.GaussianBlur(blur*SCALE)).tobytes())
    blurred = cairo.ImageSurface.create_for_data(
        pixels, cairo.FORMAT_A8, mask.get_width(), mask.get_height(), mask.get_stride())
    c.save()
    c.identity_matrix()
    c.set_source_rgba(0, 0, 0, opacity)
    c.mask_surface(blurred, 0, 0)
    c.restore()


def text(value, x, y, size=13, ink=WHITE, weight="Normal", width=None, align=None):
    layout = PangoCairo.create_layout(c)
    font = Pango.FontDescription(f"Sans {weight}")
    font.set_absolute_size(size * Pango.SCALE)
    layout.set_font_description(font)
    layout.set_text(value, -1)
    if width:
        layout.set_width(int(width * Pango.SCALE))
        layout.set_ellipsize(Pango.EllipsizeMode.END)
    if align:
        layout.set_alignment(align)
    color(ink)
    c.move_to(x, y)
    PangoCairo.show_layout(c, layout)


def icon(name, x, y, size=18):
    handle = Rsvg.Handle.new_from_file(str(ICONS[name + "-symbolic"]))
    # Keep the same Adwaita symbols as the current menu; only the layout changes.
    c.save()
    c.push_group()
    rect = Rsvg.Rectangle()
    rect.x, rect.y, rect.width, rect.height = x, y, size, size
    handle.render_document(c, rect)
    mask = c.pop_group()
    color(WHITE)
    c.mask(mask)
    c.restore()


def plate(x, y, w, h, radius=12):
    shadow(x, y, w, h, radius, blur=2, offset=1, opacity=.16)
    box(x, y, w, h, radius, "424750", "535963")


def grouped_slider(y, glyph, label, value):
    plate(56, y, 328, 60)
    icon(glyph, 68, y+11, 17)
    text(label, 94, y+10, 12, weight="Medium")
    text(f"{value}%", 328, y+10, 12, MUTED, width=44, align=Pango.Alignment.RIGHT)
    box(70, y+40, 300, 5, 2.5, "626874")
    box(70, y+40, 300*value/100, 5, 2.5, WHITE)
    thumb_x = 70 + 300*value/100 - 6
    shadow(thumb_x, y+36.5, 12, 12, 6, blur=1.5, offset=1, opacity=.4)
    box(thumb_x, y+36.5, 12, 12, 6, WHITE)


def render_grouped():
    global c
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, 440*SCALE, 608*SCALE)
    c = cairo.Context(surface)
    c.scale(SCALE, SCALE)
    background = cairo.LinearGradient(0, 0, 440, 608)
    background.add_color_stop_rgb(0, .09, .14, .18)
    background.add_color_stop_rgb(1, .13, .11, .18)
    c.set_source(background)
    c.paint()
    text("LAYOUT STUDY / 07", 40, 22, 11, MUTED, "Medium")
    color(WHITE)
    c.set_line_width(2.4)
    c.set_line_cap(cairo.LINE_CAP_ROUND)
    c.set_line_join(cairo.LINE_JOIN_ROUND)
    c.move_to(374.5, 55)
    c.line_to(380, 49.5)
    c.line_to(385.5, 55)
    c.stroke()

    shadow(40, 72, 360, 492, 20, blur=12, offset=8, opacity=.5)
    rounded_path(c, 40, 72, 360, 492, 20)
    # Static material approximation; a live popup would sample its blurred backdrop.
    material = cairo.LinearGradient(40, 72, 400, 564)
    material.add_color_stop_rgb(0, .22, .24, .28)
    material.add_color_stop_rgb(1, .16, .18, .22)
    c.set_source(material)
    c.fill_preserve()
    color("59616b")
    c.set_line_width(1)
    c.stroke()

    for x, glyph, title, state, on in [
            (56, "network-wireless", "Wi-Fi", "Home", True),
            (140, "bluetooth", "Bluetooth", "Headphones", True),
            (224, "keyboard-brightness", "Key light", "50%", True),
            (308, "weather-clear-night", "Stay awake", "Off", False)]:
        plate(x, 88, 76, 78)
        box(x+24, 97, 28, 28, 14, "2785ed" if on else "626873")
        icon(glyph, x+28, 101, 20)
        text(title, x+6, 130, 11, weight="Medium", width=64,
             align=Pango.Alignment.CENTER)
        text(state, x+6, 149, 10, "c5cbd5", width=64,
             align=Pango.Alignment.CENTER)

    grouped_slider(174, "audio-volume-high", "Sound", 67)
    grouped_slider(242, "display-brightness", "Brightness", 42)

    plate(56, 310, 328, 60)
    box(68, 322, 36, 36, 8, "626b7b")
    text("♪", 77, 325, 23)
    text("Evening playlist", 116, 322, 12, weight="Medium", width=140)
    text("Music player", 116, 341, 11, "c5cbd5", width=140)
    icon("media-skip-backward", 268, 333, 16)
    icon("media-playback-start", 310, 331, 20)
    icon("media-skip-forward", 354, 333, 16)

    for x, label, value, percent in [(56, "CPU", "12%", .12),
                                     (224, "Memory", "9.3 / 27 GB", .34)]:
        plate(x, 378, 160, 44)
        text(label, x+12, 388, 11, "c5cbd5")
        text(value, x+58, 388, 11, width=90, align=Pango.Alignment.RIGHT)
        box(x+12, 411, 136, 3, 1.5, "626874")
        box(x+12, 411, 136*percent, 3, 1.5, "d1d9e5")

    plate(56, 430, 328, 56)
    text("Power mode", 68, 440, 11, "c5cbd5")
    box(166, 460, 108, 20, 6, "626a77")
    for x, label in [(60, "Saver"), (166, "Balanced"), (274, "Performance")]:
        text(label, x, 463, 11, width=106, align=Pango.Alignment.CENTER)

    plate(56, 494, 328, 54)
    for x, label, glyph in [(56, "Lock", "system-lock-screen"),
                             (140, "Sleep", "weather-clear-night"),
                             (224, "Restart", "system-reboot"),
                             (308, "Shut down", "system-shutdown")]:
        icon(glyph, x+28, 504, 20)
        text(label, x+6, 531, 10, "c5cbd5", width=64,
             align=Pango.Alignment.CENTER)

    text("Illustrative states · material/blur approximation", 40, 585, 10, MUTED)
    path = HERE / "grouped-panel.png"
    surface.write_to_png(str(path))
    return path


if __name__ == "__main__":
    print(render_grouped())
