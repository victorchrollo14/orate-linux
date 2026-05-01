use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation,
    PasswordEntry,
};

use crate::secret;

pub const ORATE_CLOUD_KEY: &str = "orateCloudAPIKey";

pub fn show(app: &Application) {
    if let Some(win) = app.active_window() {
        win.present();
        return;
    }

    let title = Label::builder()
        .label("Orate Cloud API Key")
        .halign(Align::Start)
        .build();
    title.add_css_class("title-4");

    let hint = Label::builder()
        .label("Stored in GNOME Keyring (libsecret).")
        .halign(Align::Start)
        .build();
    hint.add_css_class("dim-label");

    let entry = PasswordEntry::builder().show_peek_icon(true).build();
    if let Some(existing) = secret::read(ORATE_CLOUD_KEY) {
        entry.set_text(&existing);
    }

    let status = Label::builder().label("").halign(Align::Start).build();
    status.add_css_class("dim-label");

    let save = Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_halign(Align::Start);
    {
        let entry = entry.clone();
        let status = status.clone();
        save.connect_clicked(move |_| {
            let value = entry.text().to_string();
            match secret::save(ORATE_CLOUD_KEY, &value) {
                Ok(()) => status.set_text("Saved."),
                Err(e) => status.set_text(&format!("Error: {e}")),
            }
        });
    }

    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    vbox.append(&title);
    vbox.append(&hint);
    vbox.append(&entry);
    vbox.append(&save);
    vbox.append(&status);

    let win = ApplicationWindow::builder()
        .application(app)
        .title("Orate")
        .default_width(440)
        .default_height(220)
        .resizable(false)
        .child(&vbox)
        .build();

    win.present();
}
