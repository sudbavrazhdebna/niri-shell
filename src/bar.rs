//! One bar window per monitor, kept in sync with monitor hotplug.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// Bar height in pixels. Moves to the config in step 2.
const HEIGHT: i32 = 34;

pub struct Bars {
    app: gtk4::Application,
    windows: RefCell<Vec<(gdk::Monitor, gtk4::ApplicationWindow)>>,
}

impl Bars {
    pub fn start(app: &gtk4::Application) {
        let Some(display) = gdk::Display::default() else {
            tracing::error!("no display");
            return;
        };

        let bars = Rc::new(Bars {
            app: app.clone(),
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
            tracing::info!(connector = ?monitor.connector(), "bar opened");
            let window = build_window(&self.app, &monitor);
            windows.push((monitor, window));
        }
    }
}

fn build_window(app: &gtk4::Application, monitor: &gdk::Monitor) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::new(app);
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
    window.set_default_size(-1, HEIGHT);

    window.set_child(Some(&build_content()));
    window.present();
    window
}

/// Three zones like a menu bar: left, center, right. The center stays centered
/// on the screen no matter how wide the sides get.
fn build_content() -> gtk4::CenterBox {
    let content = gtk4::CenterBox::new();
    content.add_css_class("bar-content");
    content.set_height_request(HEIGHT);

    let zone = |name: &str| {
        let zone = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        zone.add_css_class(name);
        zone.set_valign(gtk4::Align::Center);
        zone
    };
    let (left, center, right) = (zone("left"), zone("center"), zone("right"));

    left.append(&gtk4::Label::new(Some("nshell")));
    center.append(&gtk4::Label::new(Some(&glib::DateTime::now_local().map_or_else(
        |_| String::new(),
        |now| now.format("%H:%M").map(String::from).unwrap_or_default(),
    ))));

    content.set_start_widget(Some(&left));
    content.set_center_widget(Some(&center));
    content.set_end_widget(Some(&right));
    content
}
