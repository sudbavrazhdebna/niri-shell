# Blur cost audit — 2026-11-08

Keep the current `xray false` setting for occasional shell interactions. The test found
additional compositor work while cards were visible above moving content, but no
persistent blur workload after closing them. Battery savings from `xray true` could not
be resolved reliably in this session. No runtime code or production settings changed.

## Setup and measurements

- nshell release build at `fac3a9b`; niri 26.04; Ryzen 7 7840HS / Radeon 780M.
- A separate nested niri output, 1920 × 1080, scale 1, with the user's shell configuration
  and CSS. Bar blur was disabled, matching the real configuration.
- Static coloured stripes and continuously moving stripes behind the shell. The moving
  workload is a comparison case, not a simulation of a typical desktop's daily activity.
- Each accepted sample lasted approximately 8.4 seconds. These are single short samples,
  not repeated statistical estimates. Surface presence was checked with `niri msg layers`
  before and after; host focus was checked throughout. Moving samples also required GPU
  activity from the backdrop process. Frozen or occluded runs were excluded.
- Process CPU came from `/proc/PID/stat`. GPU execution time came from amdgpu
  `drm-engine-gfx` and `drm-engine-compute` fdinfo counters, deduplicated by DRM client ID.
  CPU percentages refer to one logical core. GPU milliseconds per second describe the
  compositor's total recorded GPU execution, including ordinary composition, not just blur.
- Battery `power_now` and amdgpu `PPT` were sampled about twice per second. PPT is an APU
  metric here; it is not isolated GPU power. The external monitor's power is not included
  in the laptop battery reading.
- Keyboard-layout OSDs were triggered in the private compositor every two seconds. They
  disappear after 1.5 seconds. This tested repeated OSD appearances without changing real
  audio or brightness; volume/brightness slider animation was not separately benchmarked.

Accepted observations are in [samples.json](samples.json). Local input helpers, full
sensor traces, layer inventories and screenshots remain in ignored `.dev/blur-audit/`.

## Moving background

| Surface | niri GPU ms/s, false | niri GPU ms/s, true | niri CPU %, false | niri CPU %, true |
| --- | ---: | ---: | ---: | ---: |
| Control center | 53.46 | 37.27 | 17.90 | 14.80 |
| Dock | 44.05 | 20.83 | 18.37 | 14.69 |
| Launcher, empty search | 76.15 | 31.62 | 14.44 | 11.94 |
| Repeated keyboard OSD | 36.18 | 20.61 | 17.40 | 14.29 |
| All four closed | 20.68 | 20.62 | 14.09 | 13.73 |

The animated application itself still costs CPU/GPU in the closed case. nshell's own
GPU time after closing was 0.000 / 0.022 ms/s and CPU was 0.000 / 0.119%. Its open,
stationary dock also recorded zero CPU/GPU in both moving-background samples. No continuing
dock animation or retained menu/OSD redraw loop was observed after closure.

The larger cost belongs to the compositor while rendering live blur. It should not be
interpreted as nshell consuming these CPU percentages itself. Dynamic clocks and the nested
backend also mean GPU-time differences are not a direct conversion to watts.

## Static background and launcher activity

Repeated OSDs used only 1.21 / 1.01 ms/s of compositor GPU time on static content. Static
dock with xray enabled recorded 0 GPU ms/s. Static control-center and false-dock attempts
lost host focus and were excluded from the accepted comparison; do not present their
low readings as a validated pair.

The launcher was the exception to otherwise quiet static content: during its first eight
seconds, nshell used 9.72 / 10.30% of one core and 38.11 / 38.35 GPU ms/s. Switching xray
did not remove its own rendering cost. GTK's smoothly fading entry cursor is a likely
contributor: its implementation installs a frame-clock callback and queues redraws while
cursor alpha changes. This is a hypothesis supported by GTK source, not an isolated causal
measurement. A hidden-caret experiment lost host focus and was discarded. The default GTK
cursor blink timeout is ten seconds; steady-state activity beyond that timeout was not
measured. Do not claim an infinite launcher redraw bug from this test.

## Battery interpretation

Whole-laptop readings for false / true were 12.14 / 12.27 W for control center,
12.43 / 12.13 W for dock, 12.29 / 11.95 W for launcher, and 12.05 / 11.77 W for OSD.
However, the closed baseline also varied from 11.07 to 11.61 W despite nearly identical
compositor GPU work. Background processes, sensor lag and short samples obscure differences
of this size. The battery energy counter was quantized too coarsely for these short runs.
These values cannot establish a per-surface watt saving, battery percentage, or runtime gain.

For rare, brief appearances, the additional live-blur work only occupies a small fraction
of use time. The measurements support keeping the preferred appearance; they do not prove
that its energy cost is exactly zero. Continuous translucent surfaces above video or a
frequently changing desktop are a better candidate for xray if battery life becomes an issue.

## Configuration pitfall

Do not blindly change the existing `^nshell-` rule to `xray true`. In the initial trial,
that also affected `nshell-menu-catcher`, the nearly transparent full-screen click catcher,
and exposed wallpaper over the application across the screen. This was visually verified.
The accepted comparison instead appended a rule restricted to the actual surfaces:

```kdl
layer-rule {
    match namespace="^nshell-(menu|dock|launcher|osd)$"
    background-effect { xray true; }
}
```

The original broad `xray false` rule remained in effect for utility surfaces. This is a
pitfall when trying xray, not a rendering bug in the current false configuration. Restore
the original configuration after the comparison. The sandbox was stopped after this audit.

## References

- [niri window effects](https://niri-wm.github.io/niri/Window-Effects.html): xray uses
  wallpaper instead of windows below and can reuse the wallpaper blur; non-xray effects
  are recomputed when relevant content or position changes. They remain experimental.
- [GTK entry cursor implementation](https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtktext.c),
  `add_blink_timeout`, `blink_cb`, `blink_alpha`.
- [GTK cursor blink timeout](https://docs.gtk.org/gtk4/property.Settings.gtk-cursor-blink-timeout.html).
