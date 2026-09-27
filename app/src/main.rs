mod clipboard;
mod config;
mod dbus_service;
mod history;
mod logger;
mod player;
mod recorder;
mod secret;
mod sound;
mod transcription;
mod ui;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};
use log::{error, info};

const APP_ID: &str = "com.orate.App";

fn main() -> glib::ExitCode {
    logger::init();

    let args: Vec<String> = std::env::args().collect();
    let service_only = args.iter().any(|a| a == "--service");
    info!(
        "orate starting (pid={}, mode={})",
        std::process::id(),
        if service_only { "service-only" } else { "gui+service" }
    );

    if service_only {
        if let Err(e) = dbus_service::run_blocking() {
            error!("dbus service exited with error: {e}");
            return glib::ExitCode::FAILURE;
        }
        return glib::ExitCode::SUCCESS;
    }

    dbus_service::start_in_background();

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(ui::show);
    app.run()
}
