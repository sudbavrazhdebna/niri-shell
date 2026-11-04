//! One bar window per monitor, kept in sync with monitor hotplug and config reloads.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::Config;
use crate::modules;
use crate::services::Services;

pub struct Bars {
    app: gtk4::Application,
    services: Rc<Services>,
    config: RefCell<Rc<Config>>,
    /// Shown as ⚠ on the bar, with the text in the tooltip.
    problems: RefCell<Vec<String>>,
    /// Per monitor: the bar, and the dock if it is on.
    windows: RefCell<Vec<(gdk::Monitor, gtk4::ApplicationWindow, Option<gtk4::ApplicationWindow>)>>,
    /// Plugged-in monitors whose connector name isn't known yet (see `sync`).
    waiting: RefCell<Vec<gdk::Monitor>>,
}

impl Bars {
    pub fn start(app: &gtk4::Application, config: Rc<Config>, services: Rc<Services>) -> Rc<Bars> {
        let bars = Rc::new(Bars {
            app: app.clone(),
            services,
            config: RefCell::new(config),
            problems: RefCell::new(Vec::new()),
            windows: RefCell::new(Vec::new()),
            waiting: RefCell::new(Vec::new()),
        });

        if let Some(display) = gdk::Display::default() {
            let monitors = display.monitors();
            bars.sync(&monitors);
            let this = bars.clone();
            monitors.connect_items_changed(move |monitors, _, _, _| this.sync(monitors));
        } else {
            tracing::error!("no display");
        }
        bars
    }

    /// Rebuilds every bar with new settings.
    pub fn set_config(self: &Rc<Self>, config: Rc<Config>, problems: Vec<String>) {
        self.services.idle().configure(&config.idle);
        crate::displays::configure(config.laptop_screen_auto);
        crate::wallpaper::configure(config.wallpaper.clone(), config.overview_blur);
        crate::osd::set_layout_rename(&config.layout_rename);
        crate::launcher::configure(&config.launcher_paths);
        crate::popups::set_bar_at_bottom(config.bar.bottom);
        crate::blur::configure(&config.blur_off);
        self.config.replace(config);
        self.problems.replace(problems);
        self.rebuild();
    }

    /// Updates the ⚠ indicator; an empty list hides it.
    pub fn set_problems(self: &Rc<Self>, problems: Vec<String>) {
        if *self.problems.borrow() == problems {
            return;
        }
        self.problems.replace(problems);
        self.rebuild();
    }

    fn rebuild(self: &Rc<Self>) {
        // An open menu belongs to a module that is about to go away.
        crate::menu::close();
        for (_, bar, dock) in self.windows.take() {
            bar.destroy();
            if let Some(dock) = dock {
                dock.destroy();
            }
        }
        if let Some(display) = gdk::Display::default() {
            self.sync(&display.monitors());
        }
    }

    /// Opens a bar on every new monitor and closes bars of the removed ones.
    fn sync(self: &Rc<Self>, monitors: &gio::ListModel) {
        let current: Vec<gdk::Monitor> = (0..monitors.n_items())
            .filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>())
            .collect();

        let mut windows = self.windows.borrow_mut();
        windows.retain(|(monitor, bar, dock)| {
            let keep = current.contains(monitor);
            if !keep {
                bar.destroy();
                if let Some(dock) = dock {
                    dock.destroy();
                }
            }
            keep
        });

        for monitor in current {
            if windows.iter().any(|(m, _, _)| *m == monitor) {
                continue;
            }
            // A monitor just plugged in tells its connector a moment later. Built before
            // that, the window title followed no output (it stayed empty) and the dock
            // answered to no name: wait for it.
            let Some(output) = monitor.connector().map(String::from) else {
                self.wait_for_connector(&monitor);
                continue;
            };
            tracing::info!(output, "bar opened");
            let ctx = modules::Context {
                config: self.config.borrow().clone(),
                services: self.services.clone(),
                output,
            };
            let bar = self.build_window(&monitor, &ctx);
            let dock = ctx
                .config
                .dock
                .enabled
                .then(|| crate::dock::build(&self.app, &monitor, &ctx.config, &self.services.niri));
            windows.push((monitor, bar, dock));
        }
    }

    fn wait_for_connector(self: &Rc<Self>, monitor: &gdk::Monitor) {
        if self.waiting.borrow().contains(monitor) {
            return;
        }
        self.waiting.borrow_mut().push(monitor.clone());
        let weak = Rc::downgrade(self);
        monitor.connect_connector_notify(move |monitor| {
            let Some(this) = weak.upgrade() else { return };
            this.waiting.borrow_mut().retain(|m| m != monitor);
            if let Some(display) = gdk::Display::default() {
                this.sync(&display.monitors());
            }
        });
    }

    fn build_window(
        &self,
        monitor: &gdk::Monitor,
        ctx: &modules::Context,
    ) -> gtk4::ApplicationWindow {
        let height = ctx.config.bar.height;
        let window = gtk4::ApplicationWindow::new(&self.app);
        window.add_css_class("bar");

        window.init_layer_shell();
        window.set_namespace(Some("nshell-bar"));
        window.set_layer(Layer::Top);
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::None);
        let bottom = ctx.config.bar.bottom;
        for edge in [if bottom { Edge::Bottom } else { Edge::Top }, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        if bottom {
            window.add_css_class("bottom");
        }
        window.auto_exclusive_zone_enable();
        window.set_default_size(-1, height);

        let content = self.build_content(ctx);
        // Glass with blur while a window is maximized to the edges right under the bar
        // (the `solid` class, style.css); transparent over the wallpaper otherwise.
        let solid = Rc::new(std::cell::Cell::new(false));
        crate::blur::part(&window, "bar");
        window.set_child(Some(&content));
        let (output, monitor) = (ctx.output.clone(), monitor.clone());
        self.services.niri.state.bind(&window, move |window, state| {
            let width = f64::from(monitor.geometry().width());
            // In the overview windows are shrunk away from the bar: nothing to cover.
            let covered = !state.overview_open && if bottom {
                state.covers_bottom(&output, width)
            } else {
                state.covers_top(&output, width)
            };
            if solid.replace(covered) != covered {
                if covered {
                    window.add_css_class("solid");
                } else {
                    window.remove_css_class("solid");
                }
            }
        });
        crate::menu::watch_bar(window.upcast_ref());
        window.set_visible(true);
        #[cfg(debug_assertions)]
        crate::menu::dev_open(window.upcast_ref());
        window
    }

    /// Three zones like a menu bar: left, center, right. The center stays centered
    /// on the screen no matter how wide the sides get.
    fn build_content(&self, ctx: &modules::Context) -> gtk4::Box {
        let bar = &ctx.config.bar;
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        content.add_css_class("bar-content");
        content.set_height_request(bar.height);
        content.set_layout_manager(Some(zones_layout()));

        let zone = |class: &str, names: &[String]| {
            let zone = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            zone.add_css_class(class);
            zone.set_valign(gtk4::Align::Center);
            for name in names {
                if let Some(widget) = modules::build(name, ctx) {
                    widget.set_valign(gtk4::Align::Center);
                    zone.append(&widget);
                }
            }
            zone
        };
        let left = zone("left", &bar.left);
        if let Some(warning) = self.problem_indicator() {
            left.prepend(&warning);
        }
        content.append(&left);
        content.append(&zone("center", &bar.center));
        content.append(&zone("right", &bar.right));
        content
    }

    fn problem_indicator(&self) -> Option<gtk4::Label> {
        let problems = self.problems.borrow();
        if problems.is_empty() {
            return None;
        }
        let label = gtk4::Label::new(Some("⚠"));
        label.add_css_class("module");
        label.add_css_class("problem");
        label.set_tooltip_text(Some(&problems.join("\n")));
        Some(label)
    }
}

/// Lays out the left, center and right zones. The center sits in the middle of the bar
/// and the sides get at most the room beside it: a long window title is cut short (its
/// label shrinks to "…") instead of pushing the clock aside, as GTK's CenterBox did.
/// A side that can't shrink that far (many tray icons) still gets its minimum.
fn zones_layout() -> gtk4::LayoutManager {
    glib::Object::new::<zones::ZonesLayout>().upcast()
}

mod zones {
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use gtk4::glib;

    glib::wrapper! {
        pub struct ZonesLayout(ObjectSubclass<imp::ZonesLayout>)
            @extends gtk4::LayoutManager;
    }

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct ZonesLayout;

        #[glib::object_subclass]
        impl ObjectSubclass for ZonesLayout {
            const NAME: &'static str = "NshellZonesLayout";
            type Type = super::ZonesLayout;
            type ParentType = gtk4::LayoutManager;
        }

        impl ObjectImpl for ZonesLayout {}

        impl LayoutManagerImpl for ZonesLayout {
            fn request_mode(&self, _: &gtk4::Widget) -> gtk4::SizeRequestMode {
                gtk4::SizeRequestMode::ConstantSize
            }

            fn measure(
                &self,
                widget: &gtk4::Widget,
                orientation: gtk4::Orientation,
                _for_size: i32,
            ) -> (i32, i32, i32, i32) {
                let (mut min, mut nat) = (0, 0);
                let mut child = widget.first_child();
                while let Some(c) = child {
                    let (m, n, _, _) = c.measure(orientation, -1);
                    if orientation == gtk4::Orientation::Horizontal {
                        min += m;
                        nat += n;
                    } else {
                        min = min.max(m);
                        nat = nat.max(n);
                    }
                    child = c.next_sibling();
                }
                (min, nat, -1, -1)
            }

            fn allocate(&self, widget: &gtk4::Widget, width: i32, height: i32, _baseline: i32) {
                let Some(left) = widget.first_child() else { return };
                let Some(center) = left.next_sibling() else { return };
                let Some(right) = center.next_sibling() else { return };
                let size = |w: &gtk4::Widget| w.measure(gtk4::Orientation::Horizontal, -1);
                let (_, center_nat, _, _) = size(&center);
                let center_width = center_nat.min(width);
                let center_x = (width - center_width) / 2;
                let side = |w: &gtk4::Widget| {
                    let (min, nat, _, _) = size(w);
                    nat.min(center_x).max(min)
                };
                let (left_width, right_width) = (side(&left), side(&right));
                let place = |w: &gtk4::Widget, x: i32, width: i32| {
                    w.size_allocate(&gtk4::Allocation::new(x, 0, width, height), -1);
                };
                place(&left, 0, left_width);
                place(&center, center_x, center_width);
                place(&right, width - right_width, right_width);
            }
        }
    }
}
