#!/usr/bin/env python3
"""A window full of coloured stripes, to see blur behind menus and popups in the sandbox.

    scripts/dev-run.sh python3 scripts/test-backdrop.py [title]
"""
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402

COLORS = ["#e74c3c", "#f1c40f", "#2ecc71", "#3498db", "#9b59b6", "#ecf0f1"]


def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title=sys.argv[1] if len(sys.argv) > 1 else "backdrop")
    box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, homogeneous=True)
    css = Gtk.CssProvider()
    css.load_from_string("".join(f".c{i} {{ background: {c}; }}" for i, c in enumerate(COLORS)))
    Gtk.StyleContext.add_provider_for_display(win.get_display(), css, 800)
    for i in range(len(COLORS) * 3):
        stripe = Gtk.Box()
        stripe.add_css_class(f"c{i % len(COLORS)}")
        box.append(stripe)
    win.set_child(box)
    win.maximize()
    win.present()


app = Gtk.Application(application_id="dev.nshell.Backdrop")
app.connect("activate", on_activate)
app.run([])
