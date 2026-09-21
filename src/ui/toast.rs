//! Transient in-window notification presentation.
//!
//! [`Toast`] overlays short request messages without blocking interaction. Each
//! message receives a monotonically wrapping sequence number so an older timeout
//! cannot hide a newer notification. Errors remain visible slightly longer and
//! the close button always allows immediate dismissal.

use std::{cell::Cell, rc::Rc};

use gtk::{
    AccessibleRole, Align, Box as GtkBox, Button, Label, Orientation, Overlay, Revealer,
    RevealerTransitionType, Widget, glib, prelude::*,
};

use super::set_accessible_label;

#[derive(Clone)]
pub(super) struct Toast {
    revealer: Revealer,
    label: Label,
    sequence: Rc<Cell<u64>>,
}

impl Toast {
    pub(super) fn overlay(child: &impl IsA<Widget>) -> (Overlay, Self) {
        let overlay = Overlay::new();
        overlay.set_child(Some(child));

        let label = Label::builder()
            .wrap(true)
            .selectable(true)
            .max_width_chars(72)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(8)
            .margin_end(6)
            .build();
        let close = Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Dismiss notification")
            .valign(Align::Center)
            .margin_end(6)
            .build();
        set_accessible_label(&close, "Dismiss notification");
        close.add_css_class("flat");

        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .accessible_role(AccessibleRole::Status)
            .build();
        content.add_css_class("osd");
        content.append(&label);
        content.append(&close);

        let revealer = Revealer::builder()
            .halign(Align::Center)
            .valign(Align::End)
            .margin_bottom(8)
            .transition_type(RevealerTransitionType::SlideUp)
            .transition_duration(150)
            .child(&content)
            .build();
        overlay.add_overlay(&revealer);

        close.connect_clicked({
            let revealer = revealer.downgrade();
            move |_| {
                if let Some(revealer) = revealer.upgrade() {
                    revealer.set_reveal_child(false);
                }
            }
        });

        (
            overlay,
            Self {
                revealer,
                label,
                sequence: Rc::new(Cell::new(0)),
            },
        )
    }

    pub(super) fn message(&self, message: &str) {
        self.show(message, false);
    }

    pub(super) fn error(&self, message: &str) {
        self.show(message, true);
    }

    fn show(&self, message: &str, error: bool) {
        self.label.set_text(message);
        if error {
            self.label.add_css_class("error");
        } else {
            self.label.remove_css_class("error");
        }
        self.revealer.set_reveal_child(true);

        let sequence = self.sequence.get().wrapping_add(1);
        self.sequence.set(sequence);
        let current_sequence = self.sequence.clone();
        let revealer = self.revealer.downgrade();
        glib::timeout_add_seconds_local_once(if error { 6 } else { 4 }, move || {
            // Ignore a timer belonging to a message that has since been replaced.
            if current_sequence.get() == sequence
                && let Some(revealer) = revealer.upgrade()
            {
                revealer.set_reveal_child(false);
            }
        });
    }
}
