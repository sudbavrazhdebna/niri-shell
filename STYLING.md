# Styling nshell

nshell is drawn with GTK4, and GTK is styled with CSS — close to the CSS of web pages, with
GTK's own properties and without layout (no `display`, `flex`, `position`: the layout comes
from the code).

## Where the styles come from

1. **Built-in style**: [`src/style.css`](src/style.css), compiled into the program. It holds
   the whole look: bar, menus, OSD, notifications, dock.
2. **Your style**: `~/.config/nshell/style.css` (doesn't exist until you create it). It goes on
   top of the built-in one, so a rule there wins over the same rule built in. **Saved changes
   apply at once**, no restart. A mistake shows ⚠ on the bar with the line in its tooltip.

So: don't edit `src/style.css` for personal taste (that needs a rebuild); put overrides in your
`style.css`. Copy a rule from the built-in file and change what you need.

The font family is never set: the system font is used. Colours are not taken from the GTK theme
either: the shell looks the same whatever apps use.

## One look for everything: the shared colours

Menus, the OSD, notification popups and the dock are the same glass card (the bar has its
own `--bar`): one fill, one thin
light line along the edge, one shadow. Those, and the text and accent colours, are variables at
the top of the built-in style. Redefine any of them in your `style.css` and every part follows:

```css
:root {
    --glass: rgba(30, 31, 34, 0.72);          /* card fill; niri blurs what's behind it */
    --glass-hover: rgba(42, 43, 46, 0.82);    /* a notification under the pointer */
    --glass-edge: rgba(255, 255, 255, 0.14);  /* the 1 px line along the card's edge */
    --glass-shadow: 0 6px 16px rgba(0, 0, 0, 0.4);
    --inner: rgba(255, 255, 255, 0.06);       /* panels inside a card (sliders, player) */
    --inner-hover: rgba(255, 255, 255, 0.12);
    --text: white;
    --text-dim: rgba(255, 255, 255, 0.7);
    --text-faint: rgba(255, 255, 255, 0.5);
    --accent: rgb(10, 132, 255);              /* tiles that are on, sliders, switches, dots */
    --danger: rgb(255, 69, 58);               /* critical notifications, armed power buttons */
    --bar: rgba(30, 31, 34, 0.72);            /* the bar over a window maximized to the edges */
    --blur: blur(20px);                       /* asks niri to blur behind cards; none: no blur */
}
```

For example, warmer glass and an orange accent:

```css
:root {
    --glass: rgba(40, 32, 30, 0.75);
    --accent: rgb(255, 149, 0);
}
```

Keep the shadow within about 22 px below a card and 16 px at its sides: the windows have that
much room for it, a bigger one gets cut off at the window's edge.

## Examples

```css
/* Bigger battery icon */
.bar-content .battery { -gtk-icon-size: 28px; }

/* All bar icons more transparent */
.bar-content .icon { opacity: 0.75; }

/* Bar text: smaller and lighter */
.bar-content .module { font-size: 15px; font-weight: normal; }

/* No hover plate on the bar */
.bar-content .module.has-menu:hover { background-color: transparent; }

/* Only the menus more see-through */
.menu-card { background-color: rgba(28, 29, 32, 0.55); }

/* Only the OSD's bar in another colour */
.osd-level trough block.filled { background: #4ea1ff; }
```

Note: blur is asked for in CSS. Every card has `backdrop-filter: var(--blur)`, and GTK
(4.24+) gives niri the area of each `backdrop-filter: blur()` of 20px or more, with its
`border-radius`, so any shape works. How strongly it blurs is niri's setting; `--blur: none`
(or a radius under 20px) means no blur behind that card.

## GTK specifics worth knowing

- **Buttons** need `background`, not `background-color`, to change their fill: the theme paints
  buttons with a background image that would cover the colour.
- **Button frames** are off for every button (`button { border: none; box-shadow: none; }` at
  the top of the built-in style): the GTK theme's dark frame looked foreign on the glass. A
  rule of yours with a `box-shadow` or `border` brings one back for that button.
- **Icon size** is `-gtk-icon-size`, not `width`/`height`.
- **Shadows**: `box-shadow` for boxes, `text-shadow` for text, `-gtk-icon-shadow` for icons.
- **Sizes**: `min-width` / `min-height`, there is no `width`/`height`.
- **Hover** is `:hover`; nshell also adds classes: `.has-menu` (the module has a menu),
  `.menu-open` (its menu is open), `.active` (a tile that's on, the connected network row),
  `.focused` / `.urgent` (dock apps), `.critical` (notifications).

## Where things are

| Part | Classes |
|---|---|
| bar | `window.bar` (+ `.solid`: glass over a window maximized to the edges), `.bar-content`; every module is `.module` plus its name: `.clock`, `.window-title`, `.keyboard`, `.network`, `.bluetooth`, `.volume`, `.battery`, `.control`, `.tray-item`; icon modules also `.icon`; `.problem` is the ⚠ shown when the config or style has a mistake |
| tooltips | `tooltip` (dark card, 18 px text); clock calendar is one white text label with the full date above it and today bold/underlined |
| clock extras | `.clock-dot` (unseen notifications), `.clock-dnd` |
| control chevron | `.control-chevron` inside `.control`; rotates when the parent has `.menu-open` |
| window title menu | `.menu-action` rows throughout; Floating is a mode-toggle command without a check mark |
| calendar | `.menu-calendar` (GTK's calendar: `header`, `grid > label.day-number` + `.today`, `.other-month`; a clicked day is `:selected` but left unmarked) |
| menus | `window.nshell-menu` (+ `.upward` when it opens above a bar at the bottom or the dock), `.menu-card` (the glass card), `.menu-body` (its column of rows), `.menu-title`, `.menu-note`, `.menu-separator`, `button.menu-item` (selection rows keep `.menu-check` on the left; `.menu-action` settings commands omit it), `.menu-row-icon` (an icon before a row's text), `.menu-header` (title with a switch), `.menu-slider-row`, `.menu-value` (tabular digits, 3 em minimum width to fit 100% without shifting sliders), `.menu-trailing`, `.menu-trailing-icon`, `button.menu-icon-button` (the mute button by a slider); a slider whose device is muted or off is `.dimmed` |
| tray menus | the same rows; a submenu's way back is `button.menu-back` |
| network menu | `.connected-card`, `.connected-name`, `.connected-details`, `entry.menu-password` (the password field, its own look) inside `.password-revealer` (it slides open) |
| battery menu | `.battery-facts`, `.fact-name`, `.fact-value`, `.power-profiles` (shared horizontal profile selector and surface styling with control center) |
| sound menu | `.sound-menu` (white slider fill and small thumbs like control center; muted fill stays grey; neutral icon-button hover/press with a white keyboard focus outline), `button.sound-apps-toggle` (Apps starts collapsed, hidden when empty), `.sound-apps-count`, `.sound-apps-chevron` (rotates when `:checked`), `.app-stream-row` (one app icon, slider and percentage per row; 20 px app icon; name/mute action in the icon tooltip); no microphone controls; the Sound Settings footer opens pavucontrol |
| displays menu | `.display-head`, `.display-name` (each display's name above its slider) |
| launcher | `window.nshell-launcher`, `.launcher-card`, `entry.launcher-entry`, `.launcher-list`, `.launcher-row` (+ `.selected`), `.launcher-icon`, `.launcher-name`, `.launcher-detail`, `.launcher-badge` ("Running"), `.launcher-section` |
| media | `.media` (the module), `button.media-play`, `.media-title`; the menu: `.media-menu`, `.media-head` (cover and names), `.media-cover`, `.media-name`, `.media-artist`, `.media-album`, `.media-position`, `.media-times`, `.media-controls` (the row of `button.media-control`) |
| control center | `.control-center` (system family, 12 px regular main text / 11 px secondary text; last child has no bottom margin), `.tiles` (homogeneous row), `button.tile` (+ `.active`: blue `.tile-icon` circle), `.tile-icon` (glyph follows service state; `.keyboard-light:not(.active)` uses a grey unstruck glyph), `.tile-title`, `.tile-subtitle`, `.control-slider` (separate sound/brightness cards, white slider, title and `.menu-value` percentage; icons follow volume/brightness), `.system-stats` (`.stat-name`, `.stat-value`, `.stat-bar`), `.player` (`.player-title`: 12 px semibold / 600, `.player-artist`, `button.player-button` with explicit `:hover`, `:active` and `:disabled` states), `.power-profiles`, `.session-buttons` (shared inner surface with 8 px vertical / 10 px horizontal padding and a 12 px radius; `button.session-button` + `.armed`, `.session-caption` under each); inner surfaces share a soft shadow without a border |
| OSD | `window.nshell-osd`, `.osd-card`, `.osd-icon`, `.osd-level` (the bar: `trough`, `block.filled`), `.osd-text`; icon and bar are `.dimmed` while muted |
| notifications | `.notification` (+ `.critical`), `.notification-content` (+ `.tall` on the card), `.notification-image`, `.summary`, `.body`, `.time`, `button.close-button`, `button.action` (floats over the bottom-right corner); a card's action buttons sit in `.actions`; popups sit in `.popup-column` of `window.nshell-popups` (+ `.bottom` above a bar at the bottom), the center is `.notification-center` (its scrolled list `.notification-list`) with `.notification-stack` (+ `.deep` with two sheets; a closed stack: its card has `.stacked` + `.depth-1`/`.depth-2`, behind it `.stack-sheet.depth-1`/`.depth-2`), a stack's `.stack-header` (`.stack-app-icon`, `.stack-app-name`, `button.stack-fold`), `button.clear-all` |
| dock | `window.nshell-dock`, `.dock` (the card), `.dock-item` (+ `.focused`, `.urgent`), `.dock-icon`, `.dot`, `button.dock-button` (overview, omitted with its preceding separator when config has `dock { no-overview; }`), `.dock-hot-edge`; the apps row is `.dock-apps` |
| taskbar | the dock's apps in the bar: `.dock-apps.taskbar`, with the same `.dock-item`, `.dock-icon`, `.dot` inside (smaller: `.taskbar .dock-icon` is 24 px) |
| wallpaper | `window.nshell-wallpaper` (black around an image that isn't loaded yet); over the image `picture.wallpaper-blurred`, a blurred copy, transparent until the window gets `.overview` (niri's overview open), then it fades in. The strength is `overview-blur` in config.kdl, not CSS |
| no blur | `window.no-blur`: a part named in config's `blur { off … }`; it redefines `--glass`, `--glass-hover` and `--bar` as nearly opaque and `--blur` as `none` |
| invisible windows | `window.menu-catcher` (catches a click outside an open menu), `window.idle-inhibitor` (keeps the screen on): nearly transparent on purpose, leave them be |
| GTK's own | `flat`, `linked` come from GTK (a button without a frame, buttons joined in a row); nshell sets them on a few buttons and doesn't style them |
| animations | a menu fades in with `.menu-card.opening` (`@keyframes menu-in`; `window.nshell-menu.upward` uses `menu-in-up`), popups slide with `popup-in` and, as `.popup-column > .notification.leaving`, `popup-out`. The code waits 150 ms for them (`ANIMATION_MS` in ui.rs): a longer animation in your style.css gets cut off |
