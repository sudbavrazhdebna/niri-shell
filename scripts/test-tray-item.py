#!/usr/bin/env python3
"""A fake tray app for testing nshell's tray in the sandbox.

    scripts/dev-run.sh python3 scripts/test-tray-item.py [--pixmap] [--passive] [--qt]

Like Qt apps, it registers once and never again. With --qt it also owns the name
org.kde.StatusNotifierItem-<pid>-1 and registers by that name, as Qt does.

Prints every menu click and activation. Send SIGUSR1 to switch its icon.
"""
import os
import signal
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

ITEM_XML = """
<node><interface name="org.kde.StatusNotifierItem">
  <property name="Category" type="s" access="read"/>
  <property name="Id" type="s" access="read"/>
  <property name="Title" type="s" access="read"/>
  <property name="Status" type="s" access="read"/>
  <property name="IconName" type="s" access="read"/>
  <property name="IconPixmap" type="a(iiay)" access="read"/>
  <property name="IconThemePath" type="s" access="read"/>
  <property name="ItemIsMenu" type="b" access="read"/>
  <property name="Menu" type="o" access="read"/>
  <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
  <signal name="NewIcon"/>
</interface></node>"""

MENU_XML = """
<node><interface name="com.canonical.dbusmenu">
  <method name="GetLayout">
    <arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
    <arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/>
  </method>
  <method name="Event">
    <arg type="i" direction="in"/><arg type="s" direction="in"/>
    <arg type="v" direction="in"/><arg type="u" direction="in"/>
  </method>
  <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
  <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
</interface></node>"""

ICONS = ["mail-unread", "weather-clear", "face-smile"]
state = {"icon": 0, "checked": True}
pixmap = "--pixmap" in sys.argv
status = "Passive" if "--passive" in sys.argv else "Active"


def red_square(size=22):
    argb = bytes([255, 220, 40, 40]) * (size * size)
    return [(size, size, argb)]


def node(id_, props, children=()):
    return GLib.Variant("(ia{sv}av)", (id_, props, list(children)))


def layout():
    s = lambda v: GLib.Variant("s", v)  # noqa: E731
    deep = node(21, {"label": s("Deep item")})
    return node(0, {}, [
        node(1, {"label": s("_Hello")}),
        node(2, {"type": s("separator")}),
        node(3, {"label": s("Checkbox"), "toggle-type": s("checkmark"),
                 "toggle-state": GLib.Variant("i", 1 if state["checked"] else 0)}),
        node(4, {"label": s("Disabled"), "enabled": GLib.Variant("b", False)}),
        node(5, {"label": s("Hidden"), "visible": GLib.Variant("b", False)}),
        node(20, {"label": s("More"), "children-display": s("submenu")}, [deep]),
    ])


def get_item_prop(conn, sender, path, iface, name):
    values = {
        "Category": GLib.Variant("s", "ApplicationStatus"),
        "Id": GLib.Variant("s", "nshell-test"),
        "Title": GLib.Variant("s", "nshell test item"),
        "Status": GLib.Variant("s", status),
        "IconName": GLib.Variant("s", "" if pixmap else ICONS[state["icon"]]),
        "IconPixmap": GLib.Variant("a(iiay)", red_square() if pixmap else []),
        "IconThemePath": GLib.Variant("s", ""),
        "ItemIsMenu": GLib.Variant("b", False),
        "Menu": GLib.Variant("o", "/MenuBar"),
    }
    return values.get(name)


def item_call(conn, sender, path, iface, method, params, inv):
    print(f"item: {method}{tuple(params.unpack())}", flush=True)
    inv.return_value(None)


def menu_call(conn, sender, path, iface, method, params, inv):
    if method == "GetLayout":
        inv.return_value(GLib.Variant.new_tuple(GLib.Variant("u", 1), layout()))
    elif method == "AboutToShow":
        inv.return_value(GLib.Variant("(b)", (False,)))
    elif method == "Event":
        id_, kind, _, _ = params.unpack()
        print(f"menu: {kind} {id_}", flush=True)
        if id_ == 3:
            state["checked"] = not state["checked"]
        inv.return_value(None)


def main():
    conn = Gio.bus_get_sync(Gio.BusType.SESSION)
    item_info = Gio.DBusNodeInfo.new_for_xml(ITEM_XML).interfaces[0]
    menu_info = Gio.DBusNodeInfo.new_for_xml(MENU_XML).interfaces[0]
    conn.register_object("/StatusNotifierItem", item_info, item_call, get_item_prop)
    conn.register_object("/MenuBar", menu_info, menu_call)

    def next_icon():
        state["icon"] = (state["icon"] + 1) % len(ICONS)
        conn.emit_signal(None, "/StatusNotifierItem", "org.kde.StatusNotifierItem", "NewIcon", None)
        print(f"icon: {ICONS[state['icon']]}", flush=True)
        return GLib.SOURCE_CONTINUE

    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, next_icon)

    service = conn.get_unique_name()
    if "--qt" in sys.argv:
        service = f"org.kde.StatusNotifierItem-{os.getpid()}-1"
        conn.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                       "RequestName", GLib.Variant("(su)", (service, 0)), None,
                       Gio.DBusCallFlags.NONE, -1, None)

    def register():
        conn.call_sync("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
                       "org.kde.StatusNotifierWatcher", "RegisterStatusNotifierItem",
                       GLib.Variant("(s)", (service,)), None,
                       Gio.DBusCallFlags.NONE, -1, None)
        print("registered", flush=True)
        return GLib.SOURCE_REMOVE

    GLib.idle_add(register)
    GLib.MainLoop().run()


main()
