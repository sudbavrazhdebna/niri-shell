//! Shared power-mode selector for the battery popup and control center.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::services::Services;
use crate::ui;

pub fn build(services: &Rc<Services>) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    row.add_css_class("linked");
    row.add_css_class("power-profiles");
    row.set_homogeneous(true);
    let weak = Rc::downgrade(services);
    services.power().state.bind(&row, move |row, state| {
        ui::clear(row);
        row.set_visible(!state.profiles.is_empty());
        let mut first = None::<gtk4::ToggleButton>;
        for profile in &state.profiles {
            let (label, icon) = match profile.as_str() {
                "power-saver" => ("Saver", "power-profile-power-saver-symbolic"),
                "balanced" => ("Balanced", "power-profile-balanced-symbolic"),
                "performance" => ("Performance", "power-profile-performance-symbolic"),
                other => (other, "power-profile-balanced-symbolic"),
            };
            let button = gtk4::ToggleButton::new();
            if let Some(first) = &first {
                button.set_group(Some(first));
            } else {
                first = Some(button.clone());
            }
            let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            content.set_halign(gtk4::Align::Center);
            content.append(&gtk4::Image::from_icon_name(icon));
            content.append(&gtk4::Label::new(Some(label)));
            button.set_child(Some(&content));
            button.set_active(*profile == state.active);
            let (services, name) = (weak.clone(), profile.clone());
            button.connect_clicked(move |button| {
                // Re-clicking the selected mode must not leave an empty selection,
                // including machines which expose only one profile.
                button.set_active(true);
                if let Some(services) = services.upgrade() {
                    let power = services.power();
                    if power.state.get().active != name {
                        power.set(&name);
                    }
                }
            });
            row.append(&button);
        }
    });
    row
}
