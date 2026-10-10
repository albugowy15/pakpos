//! Transient in-window notification presentation.
//!
//! [`Toast`] overlays short request messages without blocking interaction. Each
//! message replaces the previous timeout. Errors remain visible slightly longer and
//! the close button always allows immediate dismissal.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use gtk::{
    AccessibleRole, Align, Box as GtkBox, Button, Label, Orientation, Overlay, Revealer,
    RevealerTransitionType, Widget, glib, prelude::*,
};
use pakpos::config::{ERROR_TOAST_DURATION_SECONDS, TOAST_DURATION_SECONDS};

use super::set_accessible_label;

#[derive(Clone)]
pub(super) struct Toast {
    handles: Rc<ToastHandles>,
}

struct ToastHandles {
    revealer: Revealer,
    label: Label,
    timeout: RefCell<Option<glib::SourceId>>,
}

pub(super) struct WeakToast(Weak<ToastHandles>);

impl glib::clone::Downgrade for Toast {
    type Weak = WeakToast;

    fn downgrade(&self) -> WeakToast {
        WeakToast(Rc::downgrade(&self.handles))
    }
}

impl glib::clone::Upgrade for WeakToast {
    type Strong = Toast;

    fn upgrade(&self) -> Option<Toast> {
        self.0.upgrade().map(|handles| Toast { handles })
    }
}

impl Drop for ToastHandles {
    fn drop(&mut self) {
        if let Some(timeout) = self.timeout.get_mut().take() {
            timeout.remove();
        }
    }
}

impl Toast {
    pub(super) fn overlay(child: &impl IsA<Widget>) -> (Overlay, Self) {
        let overlay = Overlay::new();
        overlay.set_child(Some(child));

        let label = Label::builder()
            .wrap(true)
            .selectable(true)
            .max_width_chars(72)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(4)
            .margin_end(4)
            .build();
        let close = Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Dismiss notification")
            .valign(Align::Center)
            .margin_end(4)
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
            .margin_bottom(4)
            .transition_type(RevealerTransitionType::SlideUp)
            .transition_duration(150)
            .child(&content)
            .build();
        overlay.add_overlay(&revealer);

        let handles = Rc::new(ToastHandles {
            revealer,
            label,
            timeout: RefCell::default(),
        });
        close.connect_clicked(glib::clone!(
            #[weak]
            handles,
            move |_| {
                handles.cancel_timeout();
                handles.revealer.set_reveal_child(false);
            }
        ));

        (overlay, Self { handles })
    }

    pub(super) fn message(&self, message: &str) {
        self.handles.show(message, false);
    }

    pub(super) fn error(&self, message: &str) {
        self.handles.show(message, true);
    }
}

impl ToastHandles {
    fn cancel_timeout(&self) {
        if let Some(timeout) = self.timeout.borrow_mut().take() {
            timeout.remove();
        }
    }

    fn show(self: &Rc<Self>, message: &str, error: bool) {
        self.cancel_timeout();
        self.label.set_text(message);
        if error {
            self.label.add_css_class("error");
        } else {
            self.label.remove_css_class("error");
        }
        self.revealer.set_reveal_child(true);

        let duration = if error {
            ERROR_TOAST_DURATION_SECONDS
        } else {
            TOAST_DURATION_SECONDS
        };
        *self.timeout.borrow_mut() = Some(glib::timeout_add_seconds_local_once(
            duration,
            glib::clone!(
                #[weak(rename_to = handles)]
                self,
                move || {
                    handles.timeout.borrow_mut().take();
                    handles.revealer.set_reveal_child(false);
                }
            ),
        ));
    }
}

#[cfg(test)]
pub(super) fn assert_lifetimes() {
    let (overlay, toast) = Toast::overlay(&GtkBox::new(Orientation::Vertical, 0));
    let context = glib::MainContext::default();
    let mut previous: Option<glib::Source> = None;
    for _ in 0..100 {
        toast.message("Notification");
        if let Some(previous) = previous {
            assert!(previous.is_destroyed());
        }
        previous = context.find_source_by_id(toast.handles.timeout.borrow().as_ref().unwrap());
    }
    let close = toast
        .handles
        .revealer
        .child()
        .unwrap()
        .last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap();
    close.emit_clicked();
    assert!(toast.handles.timeout.borrow().is_none());
    assert!(!toast.handles.revealer.reveals_child());
    assert!(previous.unwrap().is_destroyed());

    toast.error("Error");
    let timeout = context
        .find_source_by_id(toast.handles.timeout.borrow().as_ref().unwrap())
        .unwrap();
    let weak = Rc::downgrade(&toast.handles);
    let label = toast.handles.label.downgrade();
    let revealer = toast.handles.revealer.downgrade();
    drop(toast);
    assert!(weak.upgrade().is_none());
    assert!(timeout.is_destroyed());
    // A retained child must not retain its former parent through its callback.
    drop(overlay);
    assert!(label.upgrade().is_none());
    assert!(revealer.upgrade().is_none());
    close.emit_clicked();
}
