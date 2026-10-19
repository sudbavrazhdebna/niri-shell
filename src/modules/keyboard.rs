//! Keyboard layout: EN / RU. Click or scroll switches to the next one.

use gtk4::prelude::*;
use niri_ipc::{Action, LayoutSwitchTarget};

use super::{Context, LEFT, RIGHT, on_click, on_scroll};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let label = gtk4::Label::new(None);
    label.add_css_class("module");
    label.add_css_class("keyboard");
    // EN and RU differ in width; keep neighbours from shifting on switch.
    label.set_width_chars(2);

    let niri = ctx.services.niri.clone();
    niri.state.bind(&label, |label, state| {
        let current = state
            .keyboard_layouts
            .as_ref()
            .and_then(|layouts| layouts.names.get(usize::from(layouts.current_idx)));
        label.set_visible(current.is_some());
        if let Some(name) = current {
            label.set_label(&short_name(name));
        }
    });

    let switch = move |target: LayoutSwitchTarget| {
        let niri = niri.clone();
        move || niri.action(Action::SwitchLayout { layout: target })
    };
    // Left click opens the layout menu once menus exist (step 5).
    on_click(&label, LEFT, switch(LayoutSwitchTarget::Next));
    on_click(&label, RIGHT, switch(LayoutSwitchTarget::Next));
    let niri = ctx.services.niri.clone();
    on_scroll(&label, move |steps| {
        let layout = if steps > 0 {
            LayoutSwitchTarget::Prev
        } else {
            LayoutSwitchTarget::Next
        };
        niri.action(Action::SwitchLayout { layout });
    });
    label.upcast()
}

/// "English (US)" → "EN", "Russian" → "RU".
fn short_name(xkb_name: &str) -> String {
    xkb_name.chars().take(2).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_names() {
        assert_eq!(super::short_name("English (US)"), "EN");
        assert_eq!(super::short_name("Russian"), "RU");
    }
}
