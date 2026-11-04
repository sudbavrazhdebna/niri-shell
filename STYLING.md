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

Note: blur follows the rounded shape of menus, popups, the OSD and the dock, and the code
knows their corner radius (14 px menus, 12 px notifications, 18 px OSD, 16 px dock). With a
different `border-radius` the blur corners won't match exactly.

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
| bar | `window.bar` (+ `.solid`: glass over a window maximized to the edges), `.bar-content`; every module is `.module` plus its name: `.clock`, `.window-title`, `.keyboard`, `.network`, `.bluetooth`, `.volume`, `.battery`, `.control`, `.tray-item`; icon modules also `.icon` |
| clock extras | `.clock-dot` (unseen notifications), `.clock-dnd` |
| calendar | `.menu-calendar` (GTK's calendar: `header`, `grid > label.day-number` + `.today`, `.other-month`; a clicked day is `:selected` but left unmarked) |
| menus | `.menu-card` (the glass card), `.menu-title`, `.menu-note`, `.menu-separator`, `button.menu-item`, `.menu-check`, `.menu-header` (title with a switch), `.menu-slider-row`, `.menu-value`, `.menu-trailing`, `.menu-trailing-icon` |
| network menu | `.connected-card`, `.connected-name`, `.connected-details`, `entry.menu-password` (the password field, its own look) |
| battery menu | `.battery-facts`, `.fact-name`, `.fact-value` |
| sound menu | `.app-stream-name` |
| displays menu | `.display-head`, `.display-name` (each display's name above its slider) |
| launcher | `window.nshell-launcher`, `.launcher-card`, `entry.launcher-entry`, `.launcher-list`, `.launcher-row` (+ `.selected`), `.launcher-icon`, `.launcher-name`, `.launcher-detail`, `.launcher-badge` ("Running"), `.launcher-section` |
| media | `.media` (the module), `button.media-play`, `.media-title`; the menu: `.media-menu`, `.media-cover`, `.media-name`, `.media-artist`, `.media-album`, `.media-position`, `.media-times`, `button.media-control` |
| control center | `.control-center`, `button.tile` (+ `.active`), `.tile-title`, `.tile-subtitle`, `.control-sliders`, `.system-stats`, `.stat-bar`, `.player`, `.power-profiles`, `button.session-button` (+ `.armed`) |
| OSD | `.osd-card`, `.osd-icon`, `.osd-level` (the bar: `trough`, `block.filled`), `.osd-text` |
| notifications | `.notification` (+ `.critical`), `.notification-content` (+ `.tall` on the card), `.notification-image`, `.summary`, `.body`, `.time`, `button.close-button`, `button.action` (floats over the bottom-right corner); popups sit in `.popup-column`, the center is `.notification-center` with `.notification-stack` (+ `.deep` with two sheets; a closed stack: its card has `.stacked` + `.depth-1`/`.depth-2`, behind it `.stack-sheet.depth-1`/`.depth-2`), a stack's `.stack-header` (`.stack-app-icon`, `.stack-app-name`, `button.stack-fold`), `button.clear-all` |
| dock | `.dock` (the card), `.dock-item` (+ `.focused`, `.urgent`), `.dock-icon`, `.dot`, `button.dock-button` (overview), `.dock-hot-edge`; the apps row is `.dock-apps` |
| taskbar | the dock's apps in the bar: `.dock-apps.taskbar`, with the same `.dock-item`, `.dock-icon`, `.dot` inside (smaller: `.taskbar .dock-icon` is 24 px) |
| wallpaper | `window.nshell-wallpaper` (black around an image that isn't loaded yet); over the image `picture.wallpaper-blurred`, a blurred copy, transparent until the window gets `.overview` (niri's overview open), then it fades in. The strength is `overview-blur` in config.kdl, not CSS |
| no blur | `window.no-blur`: a part named in config's `blur { off … }`; it redefines `--glass`, `--glass-hover` and `--bar` as nearly opaque |
| animations | a menu fades in with `.menu-card.opening` (`@keyframes menu-in`; `window.nshell-menu.upward` uses `menu-in-up`), popups slide with `popup-in` and, as `.popup-column > .notification.leaving`, `popup-out`. The code waits 150 ms for them (`ANIMATION_MS` in ui.rs): a longer animation in your style.css gets cut off |
