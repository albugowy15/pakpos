//! Pakpos desktop executable and composition root.
//!
//! The binary owns the two GTK-aware layers that are intentionally absent from
//! the library crate: [`ui`] builds the window and translates widget events,
//! while [`runtime`] executes application effects away from GTK's main thread.

mod runtime;
mod ui;

use adw::{Application, prelude::*};
use gtk::gio;
use pakpos::config::APPLICATION_ID;

fn main() -> gtk::glib::ExitCode {
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(ui::build);
    application.run()
}
