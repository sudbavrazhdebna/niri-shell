#!/usr/bin/env python3
"""A fake media player (MPRIS) for testing nshell in the sandbox.

    scripts/dev-run.sh python3 scripts/test-mpris-player.py

Prints every command it gets; PlayPause toggles the state.
"""
import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

XML = """
<node><interface name="org.mpris.MediaPlayer2.Player">
  <method name="PlayPause"/><method name="Next"/><method name="Previous"/>
  <property name="PlaybackStatus" type="s" access="read"/>
  <property name="Metadata" type="a{sv}" access="read"/>
  <property name="CanGoNext" type="b" access="read"/>
  <property name="CanGoPrevious" type="b" access="read"/>
</interface></node>"""

state = {"playing": True, "track": 0}
TRACKS = [("Midnight City", ["M83"]), ("Intro", ["The xx"])]


def props():
    title, artist = TRACKS[state["track"]]
    return {
        "PlaybackStatus": GLib.Variant("s", "Playing" if state["playing"] else "Paused"),
        "Metadata": GLib.Variant("a{sv}", {
            "xesam:title": GLib.Variant("s", title),
            "xesam:artist": GLib.Variant("as", artist),
        }),
        "CanGoNext": GLib.Variant("b", True),
        "CanGoPrevious": GLib.Variant("b", True),
    }


def changed(conn):
    conn.emit_signal(None, "/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties",
                     "PropertiesChanged",
                     GLib.Variant("(sa{sv}as)", ("org.mpris.MediaPlayer2.Player", props(), [])))


def call(conn, sender, path, iface, method, params, inv):
    print(f"player: {method}", flush=True)
    if method == "PlayPause":
        state["playing"] = not state["playing"]
    elif method in ("Next", "Previous"):
        state["track"] = (state["track"] + 1) % len(TRACKS)
    inv.return_value(None)
    changed(conn)


def main():
    conn = Gio.bus_get_sync(Gio.BusType.SESSION)
    info = Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0]
    conn.register_object("/org/mpris/MediaPlayer2", info, call,
                         lambda c, s, p, i, name: props().get(name))
    Gio.bus_own_name_on_connection(conn, "org.mpris.MediaPlayer2.nshelltest",
                                   Gio.BusNameOwnerFlags.NONE, None, None)
    GLib.MainLoop().run()


main()
