# Control center proposal

This is a visual proposal kept for reference. Its live-shell trial was reverted;
`src/modules/control.rs` and `src/style.css` use the original control center. Values, device names and media
metadata are illustrative. Generate with `python3 generate.py` (PyGObject, Cairo,
Pango, Pillow and the installed Adwaita symbolic icon set).

## Grouped popup

`grouped-panel.png` is the selected proposal. The material/grouping
reference is the Sequoia Control Center, illustrated in this
[Apple Community screenshot](https://discussions.apple.com/thread/255860450).

One continuous popup contains a regular grid of individual surfaces. The outer
padding is 16 px, gaps are 8 px, and inner surfaces share a 12 px corner radius,
quiet edge and short ambient shadow. Sound, brightness, media, each system gauge
and power profiles have their own surface. Lock, Sleep, Restart and Shut down are
four separate actions inside one shared bottom surface.
No divider lines are needed between those surfaces. The outer popup carries the
broadest shadow; its gradient is a static material/blur approximation, not a sample
of a live desktop background.

The current controls and menu icons remain. Wi-Fi, Bluetooth, keyboard light and
Stay awake are four equal buttons in the top row. The keyboard light button sits
next to Bluetooth and cycles Off / 50% / 100%; its compact caption is "Key light".
The other buttons toggle their function. Blue icon circles show the enabled state,
while each tile keeps a neutral background.
No new navigation or actions are implied.

The panel has no heading or branding. Sound and brightness sit directly below the
top buttons. Keyboard light no longer occupies a separate row; the sample shows
50%. The player sits above CPU/memory, grouping passive system
information with the power profiles below the everyday controls.

Brightness and keyboard light remain conditional on hardware support; media remains
conditional on an available player. Those sections are shown together here to
review their spacing. The original control center retains its restart/shutdown confirmation and
fits the available screen height.
