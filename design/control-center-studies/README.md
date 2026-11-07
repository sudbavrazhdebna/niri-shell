# Control center proposal

`unified-panel.png` is a visual proposal, not an installed theme. Values, device
names and media metadata are illustrative. Generate with `python3 generate.py`
(PyGObject, Cairo, Pango and the installed Adwaita symbolic icon set).

The proposal retains the current controls and menu icons. Wi-Fi, Bluetooth and Stay
awake are three equal buttons in the top row, with the icon above the name and state.
The entire button toggles its function; active buttons use the current blue fill.
Sliders, keyboard light, optional media, power profiles and session actions share one surface with
16 px outer padding, consistent text alignment and thin section separators. Power
profiles use a quieter selected segment. No new navigation or actions are implied.

The panel has no heading or branding. Sound and brightness sit directly below the
top buttons, followed by a full-width keyboard light button showing its actual
percentage (50% or 100%, plus Off), retaining the existing click-to-cycle interaction;
the sample shows 50%. The player sits above CPU/memory, grouping passive system
information with the power profiles below the everyday controls.

Brightness and keyboard light remain conditional on hardware support; media remains
conditional on an available player. Those sections are shown together here to
review their spacing. A real implementation would retain the existing confirmation
for restart/shutdown and fit the available screen height.
