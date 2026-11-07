# Control center proposal

`unified-panel.png` is a visual proposal, not an installed theme. Values, device
names and media metadata are illustrative. Generate with `python3 generate.py`
(PyGObject, Cairo, Pango and the installed Adwaita symbolic icon set).

The proposal retains the current controls and menu icons. Connection rows replace
the large blue tiles; small switches carry their on/off state. Utility buttons,
sliders, optional media, power profiles and session actions share one surface with
16 px outer padding, consistent text alignment and thin section separators. Power
profiles use a quieter selected segment. No new navigation or actions are implied.

Brightness and keyboard light remain conditional on hardware support; media remains
conditional on an available player. Those sections are shown together here to
review their spacing. A real implementation would retain the existing confirmation
for restart/shutdown and fit the available screen height.
