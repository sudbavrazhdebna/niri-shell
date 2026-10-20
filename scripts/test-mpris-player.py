#!/usr/bin/env python3
"""A fake media player (MPRIS) for testing nshell in the sandbox.

    scripts/dev-run.sh python3 scripts/test-mpris-player.py

Prints every command it gets; PlayPause toggles the state. Tracks have a length, an album
and a cover (a PNG it draws into a temp file); Seek and SetPosition move the position and
send Seeked, as real players do.
"""
import os
import struct
import tempfile
import time
import zlib

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

XML = """
<node><interface name="org.mpris.MediaPlayer2.Player">
  <method name="PlayPause"/><method name="Next"/><method name="Previous"/>
  <method name="Seek"><arg type="x" direction="in"/></method>
  <method name="SetPosition"><arg type="o" direction="in"/><arg type="x" direction="in"/></method>
  <property name="PlaybackStatus" type="s" access="read"/>
  <property name="Metadata" type="a{sv}" access="read"/>
  <property name="Position" type="x" access="read"/>
  <property name="CanGoNext" type="b" access="read"/>
  <property name="CanGoPrevious" type="b" access="read"/>
  <property name="CanSeek" type="b" access="read"/>
  <signal name="Seeked"><arg type="x"/></signal>
</interface></node>"""

# (title, artists, album, length in seconds, cover colour)
TRACKS = [("Midnight City", ["M83"], "Hurry Up, We're Dreaming", 243, (180, 60, 140)),
          ("Intro", ["The xx"], "xx", 127, (40, 110, 200))]
state = {"playing": True, "track": 0, "at": 0.0, "since": time.monotonic()}
COVERS = tempfile.mkdtemp(prefix="nshell-test-covers-")


def cover(rgb):
    """A 256×256 gradient PNG in the track's colour, written once."""
    path = os.path.join(COVERS, "%02x%02x%02x.png" % rgb)
    if not os.path.exists(path):
        rows = b"".join(b"\0" + bytes(int(c * (0.4 + 0.6 * y / 255)) for x in range(256) for c in rgb)
                        for y in range(256))
        chunk = lambda kind, data: (struct.pack(">I", len(data)) + kind + data
                                    + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))
        with open(path, "wb") as f:
            f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 256, 256, 8, 2, 0, 0, 0))
                    + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
    return "file://" + path


def position():
    """Microseconds into the track now."""
    gone = time.monotonic() - state["since"] if state["playing"] else 0
    length = TRACKS[state["track"]][3]
    return int(min(state["at"] + gone, length) * 1e6)


def jump(seconds):
    state["at"] = max(0.0, min(seconds, TRACKS[state["track"]][3]))
    state["since"] = time.monotonic()


def props():
    title, artist, album, length, rgb = TRACKS[state["track"]]
    return {
        "PlaybackStatus": GLib.Variant("s", "Playing" if state["playing"] else "Paused"),
        "Metadata": GLib.Variant("a{sv}", {
            "mpris:trackid": GLib.Variant("o", f"/dev/nshell/test/track{state['track']}"),
            "mpris:length": GLib.Variant("x", length * 1_000_000),
            "mpris:artUrl": GLib.Variant("s", cover(rgb)),
            "xesam:title": GLib.Variant("s", title),
            "xesam:artist": GLib.Variant("as", artist),
            "xesam:album": GLib.Variant("s", album),
        }),
        "Position": GLib.Variant("x", position()),
        "CanGoNext": GLib.Variant("b", True),
        "CanGoPrevious": GLib.Variant("b", True),
        "CanSeek": GLib.Variant("b", True),
    }


def changed(conn):
    conn.emit_signal(None, "/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties",
                     "PropertiesChanged",
                     GLib.Variant("(sa{sv}as)", ("org.mpris.MediaPlayer2.Player", props(), [])))


def call(conn, sender, path, iface, method, params, inv):
    print(f"player: {method} {params.unpack()}", flush=True)
    seeked = False
    if method == "PlayPause":
        jump(position() / 1e6)
        state["playing"] = not state["playing"]
    elif method in ("Next", "Previous"):
        state["track"] = (state["track"] + 1) % len(TRACKS)
        jump(0)
    elif method == "Seek":
        jump(position() / 1e6 + params.unpack()[0] / 1e6)
        seeked = True
    elif method == "SetPosition":
        jump(params.unpack()[1] / 1e6)
        seeked = True
    inv.return_value(None)
    if seeked:
        conn.emit_signal(None, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player",
                         "Seeked", GLib.Variant("(x)", (position(),)))
    else:
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
