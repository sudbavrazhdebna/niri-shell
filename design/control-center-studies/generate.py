#!/usr/bin/env python3
"""Render the control-center proposal; this does not change the running shell."""

from pathlib import Path
import math

import cairo
import gi

gi.require_version("Rsvg", "2.0")
gi.require_version("Pango", "1.0")
gi.require_version("PangoCairo", "1.0")
from gi.repository import Pango, PangoCairo, Rsvg


HERE = Path(__file__).resolve().parent
ICON_DIR = Path("/usr/share/icons/Adwaita/symbolic")
ICONS = {path.stem: path for path in ICON_DIR.rglob("*.svg")}
SCALE = 2
surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, 440 * SCALE, 670 * SCALE)
c = cairo.Context(surface)
c.scale(SCALE, SCALE)
WHITE, MUTED, BLUE = "f4f5f7", "a5a9b2", "62a6ff"


def color(value, alpha=1):
    c.set_source_rgba(*(int(value[i:i+2], 16) / 255 for i in (0, 2, 4)), alpha)


def box(x, y, w, h, radius, fill, stroke=None):
    c.new_sub_path()
    for cx, cy, start in [(x+w-radius, y+radius, -90),
                           (x+w-radius, y+h-radius, 0),
                           (x+radius, y+h-radius, 90),
                           (x+radius, y+radius, 180)]:
        c.arc(cx, cy, radius, math.radians(start), math.radians(start+90))
    c.close_path()
    color(fill)
    c.fill_preserve()
    if stroke:
        color(stroke)
        c.set_line_width(1)
        c.stroke()
    else:
        c.new_path()


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


def toggle(x, y, enabled=True):
    box(x, y, 34, 20, 10, BLUE if enabled else "4a4e57")
    box(x + (16 if enabled else 3), y+3, 14, 14, 7, WHITE)


def connection(y, glyph, title, state, enabled=True):
    icon(glyph, 58, y+11, 20)
    text(title, 92, y+2, weight="Medium")
    text(state, 92, y+21, 11, MUTED, width=240)
    toggle(350, y+10, enabled)


def slider(y, glyph, label, value):
    icon(glyph, 58, y, 17)
    text(label, 84, y-1, 12, weight="Medium")
    text(f"{value}%", 336, y-1, 12, MUTED, width=48, align=Pango.Alignment.RIGHT)
    box(58, y+29, 326, 4, 2, "444851")
    box(58, y+29, 326*value/100, 4, 2, WHITE)
    box(58 + 326*value/100 - 6, y+25, 12, 12, 6, WHITE)


gradient = cairo.LinearGradient(0, 0, 440, 670)
gradient.add_color_stop_rgb(0, .09, .14, .18)
gradient.add_color_stop_rgb(1, .13, .11, .18)
c.set_source(gradient)
c.paint()
text("LAYOUT STUDY / 02", 40, 22, 11, MUTED, "Medium")
# A static sample of the topbar anchor, showing the open state.
color(WHITE)
c.set_line_width(2.4)
c.set_line_cap(cairo.LINE_CAP_ROUND)
c.set_line_join(cairo.LINE_JOIN_ROUND)
c.move_to(374.5, 55)
c.line_to(380, 49.5)
c.line_to(385.5, 55)
c.stroke()

box(40, 72, 360, 558, 18, "23262c", "464951")

connection(88, "network-wireless", "Wi-Fi", "Home network")
connection(138, "bluetooth", "Bluetooth", "Headphones")

divider(196)
slider(212, "audio-volume-high", "Sound", 67)
slider(264, "display-brightness", "Brightness", 42)

box(56, 314, 158, 40, 10, "30343b")
box(226, 314, 158, 40, 10, "30343b")
icon("weather-clear-night", 68, 326, 16)
text("Stay awake", 94, 318, 11, weight="Medium")
text("Off", 94, 333, 10, MUTED)
icon("keyboard-brightness", 238, 326, 16)
text("Keyboard light", 264, 318, 11, weight="Medium")
text("50%", 264, 333, 10, MUTED)

for x, label, value, percent in [(58, "CPU", "12%", .12), (230, "Memory", "9.3 / 27 GB", .34)]:
    text(label, x, 370, 11, MUTED)
    text(value, x+54, 370, 11, width=100, align=Pango.Alignment.RIGHT)
    box(x, 393, 154, 3, 1.5, "444851")
    box(x, 393, 154*percent, 3, 1.5, "b7bfcc")

divider(412)
box(58, 428, 36, 36, 8, "3d4350")
text("♪", 67, 431, 23)
text("Evening playlist", 106, 428, 12, weight="Medium", width=156)
text("Music player", 106, 447, 11, MUTED, width=156)
icon("media-skip-backward", 278, 439, 16)
icon("media-playback-start", 318, 437, 20)
icon("media-skip-forward", 362, 439, 16)

text("Power mode", 58, 482, 11, MUTED)
box(56, 504, 328, 32, 9, "30343b")
box(165, 507, 106, 26, 7, "4a505b")
for x, label in [(58, "Saver"), (166, "Balanced"), (274, "Performance")]:
    text(label, x, 511, 11, WHITE if label == "Balanced" else MUTED,
         width=108, align=Pango.Alignment.CENTER)

divider(552)
for x, label, glyph in [(58, "Lock", "system-lock-screen"),
                         (142, "Sleep", "weather-clear-night"),
                         (226, "Restart", "system-reboot"),
                         (310, "Shut down", "system-shutdown")]:
    icon(glyph, x+28, 568, 18)
    text(label, x, 596, 10, MUTED, width=72, align=Pango.Alignment.CENTER)

text("Illustrative states · optional media section shown", 40, 647, 10, MUTED)
surface.write_to_png(str(HERE / "unified-panel.png"))
print(HERE / "unified-panel.png")
