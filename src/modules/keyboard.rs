//! Keyboard layout: EN / RU. Left click: menu of layouts, right click or scroll:
//! next layout.

use gtk4::prelude::*;
use niri_ipc::{Action, LayoutSwitchTarget};

use super::{Context, RIGHT, on_click, on_scroll};
use crate::{menu, ui};

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

    let n = niri.clone();
    menu::attach(&label, move |anchor| {
        let column = ui::column();
        column.append(&ui::title("Keyboard layout"));
        let state = n.state.get();
        if let Some(layouts) = &state.keyboard_layouts {
            for (idx, name) in layouts.names.iter().enumerate() {
                let n = n.clone();
                let Ok(idx) = u8::try_from(idx) else { continue };
                column.append(&ui::item(name, idx == layouts.current_idx, move || {
                    n.action(Action::SwitchLayout {
                        layout: LayoutSwitchTarget::Index(idx),
                    });
                    menu::close();
                }));
            }
        }
        menu::show(anchor, &column);
    });

    let n = niri.clone();
    on_click(&label, RIGHT, move || {
        n.action(Action::SwitchLayout {
            layout: LayoutSwitchTarget::Next,
        });
    });
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
