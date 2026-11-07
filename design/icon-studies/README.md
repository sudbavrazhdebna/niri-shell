# Topbar icon families

Review concepts only. Installed assets remain in `icons/scalable/status/`.

- **Soft Fill:** rounded, substantial silhouettes; the closest continuation of the selected Classic battery and Soft chevron.
- **Soft Outline:** lighter, open silhouettes with a consistent 1.5 px line weight on the 20 px grid.
- **Compact 1x:** simpler forms and 2 px strokes on a 20 px grid; the battery has a separate 26 px grid.

All families include connected Wi-Fi levels and an explicit offline state, volume levels and mute, Bluetooth, brightness, a 70% battery with and without charging, and the downward chevron. Inactive signal bands are grey; charging is green. Battery outlines stay white.

The comparison includes the installed set, enlarged candidates, native 20 px icons and 26 px batteries. Curves still use antialiasing; a pixel grid alone does not guarantee perceived sharpness. Application tray icons are owned by their applications and are outside this set.

Regenerate the SVGs and comparison with `python3 design/icon-studies/generate.py` (PyGObject, Rsvg and Cairo). Charge states other than 70% will be generated after choosing a family.
