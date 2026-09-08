mod ui;

use gtk::{Application, gio, prelude::*};

const APPLICATION_ID: &str = "com.bughowi.Pakpos";

fn main() -> gtk::glib::ExitCode {
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(ui::build);
    application.run()
}
