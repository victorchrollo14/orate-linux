mod clipboard;
mod dbus_service;
mod history;
mod recorder;
mod secret;
mod settings_window;
mod transcription;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};

const APP_ID: &str = "com.orate.App";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let service_only = args.iter().any(|a| a == "--service");

    if service_only {
        if let Err(e) = dbus_service::run_blocking() {
            eprintln!("[orate] {e}");
            return glib::ExitCode::FAILURE;
        }
        return glib::ExitCode::SUCCESS;
    }

    dbus_service::start_in_background();

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(settings_window::show);
    app.run()
}
