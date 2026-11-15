//! Fade text approaching visible overlay actions, without painting
//! a gradient over the glass or changing the label's layout/accessibility.

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, gsk};

glib::wrapper! {
    pub struct FadingContent(ObjectSubclass<imp::FadingContent>)
        @extends gtk4::Box, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl FadingContent {
    pub fn new(child: &impl IsA<gtk4::Widget>) -> Self {
        let body: Self = glib::Object::builder()
            .property("orientation", gtk4::Orientation::Vertical)
            .build();
        body.append(child);
        body
    }

    pub fn set_actions(&self, actions: &gtk4::Revealer) {
        self.imp().actions.set(Some(actions));
        // Repaint on hover and once the crossfade ends. No frame callback while idle.
        let weak = self.downgrade();
        actions.connect_reveal_child_notify(move |_| {
            if let Some(body) = weak.upgrade() { body.queue_draw(); }
        });
        let weak = self.downgrade();
        actions.connect_child_revealed_notify(move |_| {
            if let Some(body) = weak.upgrade() { body.queue_draw(); }
        });
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FadingContent {
        pub actions: glib::WeakRef<gtk4::Revealer>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FadingContent {
        const NAME: &'static str = "NshellFadingContent";
        type Type = super::FadingContent;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for FadingContent {}
    impl BoxImpl for FadingContent {}

    impl WidgetImpl for FadingContent {
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let bounds = self.actions.upgrade()
                .filter(|a| a.reveals_child() || a.is_child_revealed())
                .and_then(|a| a.compute_bounds(&*obj));
            let Some(bounds) = bounds.filter(|b| b.width() > 0.0 && b.height() > 0.0
                && b.y() < obj.height() as f32 && b.x() < obj.width() as f32
                && b.y() + b.height() > 0.0 && b.x() + b.width() > 0.0) else {
                self.parent_snapshot(snapshot);
                return;
            };

            let fade = 24.0;
            let top = bounds.y() - 10.0;
            let rtl = obj.direction() == gtk4::TextDirection::Rtl;
            let (start, end) = if rtl {
                (bounds.x() + bounds.width() + fade, bounds.x() + bounds.width())
            } else {
                (bounds.x() - fade, bounds.x())
            };
            let area = graphene::Rect::new(
                bounds.x() - fade, top, bounds.width() + 2.0 * fade, bounds.height() + 20.0,
            );
            let stops = [
                gsk::ColorStop::new(0.0, gdk::RGBA::TRANSPARENT),
                gsk::ColorStop::new(1.0, gdk::RGBA::BLACK),
            ];
            let vertical = [
                gsk::ColorStop::new(0.0, gdk::RGBA::TRANSPARENT),
                gsk::ColorStop::new(10.0 / area.height(), gdk::RGBA::BLACK),
                gsk::ColorStop::new((10.0 + bounds.height()) / area.height(), gdk::RGBA::BLACK),
                gsk::ColorStop::new(1.0, gdk::RGBA::TRANSPARENT),
            ];
            // Inverted alpha leaves the surrounding text intact. Fade above and below
            // as well as beside the action, so a centered button has no hard mask edge.
            snapshot.push_mask(gsk::MaskMode::InvertedAlpha);
            snapshot.push_mask(gsk::MaskMode::Alpha);
            snapshot.append_linear_gradient(&area,
                &graphene::Point::new(0.0, top), &graphene::Point::new(0.0, top + area.height()), &vertical);
            snapshot.pop();
            snapshot.append_linear_gradient(&area,
                &graphene::Point::new(start, 0.0), &graphene::Point::new(end, 0.0), &stops);
            snapshot.pop();
            snapshot.pop();
            self.parent_snapshot(snapshot);
            snapshot.pop();
        }
    }
}
