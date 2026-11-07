#!/usr/bin/env python3
"""Render the control-center proposal; this does not change the running shell."""

from pathlib import Path
import math

import cairo
import gi
from PIL import Image, ImageDraw, ImageFilter, ImageFont

gi.require_version("Rsvg", "2.0")
gi.require_version("Pango", "1.0")
gi.require_version("PangoCairo", "1.0")
from gi.repository import Pango, PangoCairo, Rsvg


HERE = Path(__file__).resolve().parent
ICON_DIR = Path("/usr/share/icons/Adwaita/symbolic")
ICONS = {path.stem: path for path in ICON_DIR.rglob("*.svg")}
SCALE = 2
c: cairo.Context
DEPTH = False
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


def raised_box(x, y, w, h, radius, fill, stroke="51555f"):
    if DEPTH:
        shadow(x, y, w, h, radius)
    box(x, y, w, h, radius, fill, stroke if DEPTH else None)
    if DEPTH:
        rounded_path(c, x+.5, y+.5, w-1, h-1, radius-.5)
        sheen = cairo.LinearGradient(0, y, 0, y+h)
        sheen.add_color_stop_rgba(0, 1, 1, 1, .045)
        sheen.add_color_stop_rgba(1, 0, 0, 0, .035)
        c.set_source(sheen)
        c.fill()


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


def divider(y):
    box(56, y, 328, 1, .5, "363940")


def quick_button(x, glyph, title, state, enabled):
    fill = ("355b89" if DEPTH else "0d73dd") if enabled else "30343b"
    raised_box(x, 88, 104, 78, 10, fill, "6081a8" if enabled else "51555f")
    icon(glyph, x+42, 98, 20)
    text(title, x+8, 125, 12, weight="Medium", width=88,
         align=Pango.Alignment.CENTER)
    text(state, x+8, 144, 10, "e4efff" if enabled else MUTED, width=88,
         align=Pango.Alignment.CENTER)


def slider(y, glyph, label, value):
    icon(glyph, 58, y, 17)
    text(label, 84, y-1, 12, weight="Medium")
    text(f"{value}%", 336, y-1, 12, MUTED, width=48, align=Pango.Alignment.RIGHT)
    box(58, y+29, 326, 4, 2, "444851")
    box(58, y+29, 326*value/100, 4, 2, WHITE)
    thumb_x = 58 + 326*value/100 - 6
    if DEPTH:
        shadow(thumb_x, y+25, 12, 12, 6, blur=2, offset=1, opacity=.5)
    box(thumb_x, y+25, 12, 12, 6, WHITE)


def render(depth):
    global c, DEPTH
    DEPTH = depth
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, 440*SCALE, 652*SCALE)
    c = cairo.Context(surface)
    c.scale(SCALE, SCALE)
    gradient = cairo.LinearGradient(0, 0, 440, 652)
    gradient.add_color_stop_rgb(0, .09, .14, .18)
    gradient.add_color_stop_rgb(1, .13, .11, .18)
    c.set_source(gradient)
    c.paint()
    text("LAYOUT STUDY / 05", 40, 22, 11, MUTED, "Medium")
    # A static sample of the topbar anchor, showing the open state.
    color(WHITE)
    c.set_line_width(2.4)
    c.set_line_cap(cairo.LINE_CAP_ROUND)
    c.set_line_join(cairo.LINE_JOIN_ROUND)
    c.move_to(374.5, 55)
    c.line_to(380, 49.5)
    c.line_to(385.5, 55)
    c.stroke()

    if DEPTH:
        shadow(40, 72, 360, 540, 18, blur=12, offset=8, opacity=.5)
    box(40, 72, 360, 540, 18, "23262c", "464951")

    quick_button(56, "network-wireless", "Wi-Fi", "Home network", True)
    quick_button(168, "bluetooth", "Bluetooth", "Headphones", True)
    quick_button(280, "weather-clear-night", "Stay awake", "Off", False)

    divider(182)
    slider(198, "audio-volume-high", "Sound", 67)
    slider(250, "display-brightness", "Brightness", 42)

    raised_box(56, 300, 328, 36, 10, "30343b")
    icon("keyboard-brightness", 68, 310, 16)
    text("Keyboard light", 94, 309, 12, weight="Medium")
    text("50%", 324, 309, 12, BLUE, width=48, align=Pango.Alignment.RIGHT)

    box(58, 352, 36, 36, 8, "3d4350")
    text("♪", 67, 355, 23)
    text("Evening playlist", 106, 352, 12, weight="Medium", width=156)
    text("Music player", 106, 371, 11, MUTED, width=156)
    icon("media-skip-backward", 278, 363, 16)
    if DEPTH:
        raised_box(312, 355, 32, 32, 16, "353941")
    icon("media-playback-start", 318, 361, 20)
    icon("media-skip-forward", 362, 363, 16)

    divider(404)
    for x, label, value, percent in [(58, "CPU", "12%", .12), (230, "Memory", "9.3 / 27 GB", .34)]:
        text(label, x, 420, 11, MUTED)
        text(value, x+54, 420, 11, width=100, align=Pango.Alignment.RIGHT)
        box(x, 443, 154, 3, 1.5, "444851")
        box(x, 443, 154*percent, 3, 1.5, "b7bfcc")

    text("Power mode", 58, 464, 11, MUTED)
    box(56, 486, 328, 32, 9, "30343b")
    raised_box(165, 489, 106, 26, 7, "4a505b")
    for x, label in [(58, "Saver"), (166, "Balanced"), (274, "Performance")]:
        text(label, x, 493, 11, WHITE if label == "Balanced" else MUTED,
             width=108, align=Pango.Alignment.CENTER)

    divider(534)
    for x, label, glyph in [(58, "Lock", "system-lock-screen"),
                             (142, "Sleep", "weather-clear-night"),
                             (226, "Restart", "system-reboot"),
                             (310, "Shut down", "system-shutdown")]:
        if DEPTH:
            raised_box(x+21, 544, 32, 32, 11, "30343b")
        icon(glyph, x+28, 551 if DEPTH else 550, 18)
        text(label, x, 584 if DEPTH else 578, 10, MUTED, width=72, align=Pango.Alignment.CENTER)

    text("Illustrative states · optional media section shown", 40, 629, 10, MUTED)
    path = HERE / ("unified-panel.png" if depth else "flat-panel.png")
    surface.write_to_png(str(path))
    return path


flat, depth = render(False), render(True)
comparison = Image.new("RGB", (848, 652), "#191e27")
labels = ImageDraw.Draw(comparison)
font = ImageFont.truetype("DejaVuSans.ttf", 16)
for path, x, label in [(flat, 0, "01 / Прежний вариант"),
                       (depth, 424, "02 / Мягкий объём")]:
    labels.text((x+32, 24), label, font=font, fill="#e2e5eb")
    panel = Image.open(path).convert("RGB").crop((20*SCALE, 60*SCALE, 420*SCALE, 622*SCALE))
    comparison.paste(panel.resize((400, 562), Image.Resampling.LANCZOS), (x+12, 64))
comparison.save(HERE / "depth-comparison.png")
print(HERE / "depth-comparison.png")
