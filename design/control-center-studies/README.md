# Control center proposal

`unified-panel.png` is a visual proposal, not an installed theme. `flat-panel.png`
is its flat reference; `depth-comparison.png` shows both at logical size. Values,
device names and media metadata are illustrative. Generate with `python3 generate.py`
(PyGObject, Cairo, Pango, Pillow and the installed Adwaita symbolic icon set).

The proposal retains the current controls and menu icons. Wi-Fi, Bluetooth and Stay
awake are three equal buttons in the top row, with the icon above the name and state.
The entire button toggles its function; active buttons use a muted blue fill in the
depth proposal and the current bright blue in the flat reference.
Sliders, keyboard light, optional media, power profiles and session actions share one surface with
16 px outer padding, consistent text alignment and thin section separators. Power
profiles use a quieter selected segment. No new navigation or actions are implied.

The panel has no heading or branding. Sound and brightness sit directly below the
top buttons, followed by a full-width keyboard light button showing its actual
percentage (50% or 100%, plus Off), retaining the existing click-to-cycle interaction;
the sample shows 50%. The player sits above CPU/memory, grouping passive system
information with the power profiles below the everyday controls.

Depth follows the controls' role: the outer panel has a broad shadow, raised button
faces have a shorter soft shadow and a subtle edge/sheen, and slider thumbs have a
small contact shadow. The primary play action, selected power segment and session
buttons also have raised faces. Secondary transport icons remain quiet, transparent
actions. Labels, tracks and read-only CPU/memory stay on the panel surface. This is
a hierarchy of surfaces rather than identical shadows on every widget. The layout
stays the same in both previews; color and button depth are the proposed changes.

Brightness and keyboard light remain conditional on hardware support; media remains
conditional on an available player. Those sections are shown together here to
review their spacing. A real implementation would retain the existing confirmation
for restart/shutdown and fit the available screen height.
