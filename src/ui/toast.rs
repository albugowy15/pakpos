//! Transient libadwaita notifications with one current message per window.
//!
//! A new message dismisses the previous toast. Libadwaita owns the presentation,
//! close button, accessibility, and timeout; errors remain visible longer.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use adw::prelude::*;
use gtk::{Widget, glib};
use pakpos::config::{ERROR_TOAST_DURATION_SECONDS, TOAST_DURATION_SECONDS};

#[derive(Clone)]
pub(super) struct Toast {
    handles: Rc<ToastHandles>,
}

struct ToastHandles {
    overlay: adw::ToastOverlay,
    current: RefCell<Option<adw::Toast>>,
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
        if let Some(toast) = self.current.get_mut().take() {
            toast.dismiss();
        }
    }
}

impl Toast {
    pub(super) fn overlay(child: &impl IsA<Widget>) -> (adw::ToastOverlay, Self) {
        let overlay = adw::ToastOverlay::new();
        overlay.set_child(Some(child));
        let handles = Rc::new(ToastHandles {
            overlay: overlay.clone(),
            current: RefCell::default(),
        });
        (overlay, Self { handles })
    }

    pub(super) fn message(&self, message: &str) {
        self.show(message, false);
    }

    pub(super) fn error(&self, message: &str) {
        self.show(message, true);
    }

    #[cfg(test)]
    pub(super) fn current(&self) -> Option<adw::Toast> {
        self.handles.current.borrow().clone()
    }

    fn show(&self, message: &str, error: bool) {
        if let Some(previous) = self.handles.current.borrow_mut().take() {
            previous.dismiss();
        }
        let duration = if error {
            ERROR_TOAST_DURATION_SECONDS
        } else {
            TOAST_DURATION_SECONDS
        };
        let toast = adw::Toast::builder()
            .title(message)
            // Validation errors and URLs can contain markup metacharacters.
            .use_markup(false)
            .timeout(duration)
            .build();
        self.handles.current.replace(Some(toast.clone()));
        self.handles.overlay.add_toast(toast);
    }
}

#[cfg(test)]
pub(super) fn assert_lifetimes() {
    use std::cell::Cell;

    let child = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let weak_child = child.downgrade();
    let (overlay, toast) = Toast::overlay(&child);
    drop(child);
    toast.message("<literal> & notification");
    let previous = toast.handles.current.borrow().as_ref().unwrap().clone();
    assert!(!previous.uses_markup());
    assert_eq!(previous.timeout(), TOAST_DURATION_SECONDS);
    assert_eq!(
        previous.title().as_deref(),
        Some("<literal> & notification")
    );
    let dismissed = Rc::new(Cell::new(false));
    previous.connect_dismissed({
        let dismissed = dismissed.clone();
        move |_| dismissed.set(true)
    });
    toast.error("Error");
    assert!(
        dismissed.get(),
        "a new notification must dismiss the previous one"
    );
    let current = toast.handles.current.borrow().as_ref().unwrap().clone();
    assert_eq!(current.timeout(), ERROR_TOAST_DURATION_SECONDS);
    dismissed.set(false);
    current.connect_dismissed({
        let dismissed = dismissed.clone();
        move |_| dismissed.set(true)
    });
    let weak = Rc::downgrade(&toast.handles);
    let weak_overlay = overlay.downgrade();
    drop(toast);
    assert!(weak.upgrade().is_none());
    assert!(
        dismissed.get(),
        "dropping the presenter must dismiss its toast"
    );
    drop(overlay);
    assert!(weak_overlay.upgrade().is_none());
    assert!(weak_child.upgrade().is_none());
}
