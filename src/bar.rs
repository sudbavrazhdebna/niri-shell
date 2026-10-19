//! One bar window per monitor, kept in sync with monitor hotplug.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::Config;
use crate::modules;
use crate::niri::Niri;

pub struct Bars {
    app: gtk4::Application,
    config: Rc<Config>,
    niri: Niri,
    windows: RefCell<Vec<(gdk::Monitor, gtk4::ApplicationWindow)>>,
}

impl Bars {
    pub fn start(app: &gtk4::Application, config: Rc<Config>, niri: Niri) {
        let Some(display) = gdk::Display::default() else {
            tracing::error!("no display");
            return;
        };

        let bars = Rc::new(Bars {
            app: app.clone(),
            config,
            niri,
            windows: RefCell::new(Vec::new()),
        });

        let monitors = display.monitors();
        bars.sync(&monitors);

        // The handler owns the bars: they live as long as the display does.
        monitors.connect_items_changed(move |monitors, _, _, _| bars.sync(monitors));
    }

    /// Opens a bar on every new monitor and closes bars of the removed ones.
    fn sync(&self, monitors: &gio::ListModel) {
        let current: Vec<gdk::Monitor> = (0..monitors.n_items())
            .filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>())
            .collect();

        let mut windows = self.windows.borrow_mut();
        windows.retain(|(monitor, window)| {
            let keep = current.contains(monitor);
            if !keep {
                window.destroy();
            }
            keep
        });

        for monitor in current {
            if windows.iter().any(|(m, _)| *m == monitor) {
                continue;
            }
            let output = monitor.connector().map(String::from).unwrap_or_default();
            tracing::info!(output, "bar opened");
            let ctx = modules::Context {
                config: self.config.clone(),
                niri: self.niri.clone(),
                output,
            };
            let window = self.build_window(&monitor, &ctx);
            windows.push((monitor, window));
        }
    }

    fn build_window(&self, monitor: &gdk::Monitor, ctx: &modules::Context) -> gtk4::ApplicationWindow {
        let height = self.config.bar.height;
        let window = gtk4::ApplicationWindow::new(&self.app);
        window.add_css_class("bar");

        window.init_layer_shell();
        window.set_namespace(Some("nshell-bar"));
        window.set_layer(Layer::Top);
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::None);
        for edge in [Edge::Top, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.auto_exclusive_zone_enable();
        window.set_default_size(-1, height);

        window.set_child(Some(&self.build_content(ctx)));
        window.present();
        window
    }

    /// Three zones like a menu bar: left, center, right. The center stays centered
    /// on the screen no matter how wide the sides get.
    fn build_content(&self, ctx: &modules::Context) -> gtk4::CenterBox {
        let content = gtk4::CenterBox::new();
        content.add_css_class("bar-content");
        content.set_height_request(self.config.bar.height);

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
        let bar = &self.config.bar;
        content.set_start_widget(Some(&zone("left", &bar.left)));
        content.set_center_widget(Some(&zone("center", &bar.center)));
        content.set_end_widget(Some(&zone("right", &bar.right)));
        content
    }
}
