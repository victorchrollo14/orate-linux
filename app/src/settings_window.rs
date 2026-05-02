use std::cell::RefCell;
use std::rc::Rc;

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

    let saved_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Start)
        .build();
    let saved_label = Label::builder().label("\u{2713} API key saved").build();
    saved_label.add_css_class("success");
    let change_button = Button::with_label("Change");
    saved_row.append(&saved_label);
    saved_row.append(&change_button);

    let entry = PasswordEntry::builder().show_peek_icon(true).build();
    let status = Label::builder().label("").halign(Align::Start).build();
    status.add_css_class("dim-label");

    let action_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Start)
        .build();
    let save = Button::with_label("Save");
    save.add_css_class("suggested-action");
    let cancel = Button::with_label("Cancel");
    action_row.append(&save);
    action_row.append(&cancel);

    let has_existing = Rc::new(RefCell::new(secret::read(ORATE_CLOUD_KEY).is_some()));

    let show_saved = {
        let saved_row = saved_row.clone();
        let entry = entry.clone();
        let action_row = action_row.clone();
        move || {
            saved_row.set_visible(true);
            entry.set_visible(false);
            action_row.set_visible(false);
        }
    };
    let show_editing = {
        let saved_row = saved_row.clone();
        let entry = entry.clone();
        let action_row = action_row.clone();
        let cancel = cancel.clone();
        let has_existing = has_existing.clone();
        move || {
            saved_row.set_visible(false);
            entry.set_visible(true);
            entry.set_text("");
            action_row.set_visible(true);
            cancel.set_visible(*has_existing.borrow());
        }
    };

    {
        let show_editing = show_editing.clone();
        change_button.connect_clicked(move |_| show_editing());
    }
    {
        let show_saved = show_saved.clone();
        let has_existing = has_existing.clone();
        let status = status.clone();
        cancel.connect_clicked(move |_| {
            if *has_existing.borrow() {
                status.set_text("");
                show_saved();
            }
        });
    }
    {
        let entry = entry.clone();
        let status = status.clone();
        let show_saved = show_saved.clone();
        let has_existing = has_existing.clone();
        save.connect_clicked(move |_| {
            let value = entry.text().to_string();
            if value.is_empty() {
                status.set_text("Please enter a key.");
                return;
            }
            match secret::save(ORATE_CLOUD_KEY, &value) {
                Ok(()) => {
                    *has_existing.borrow_mut() = true;
                    status.set_text("");
                    show_saved();
                }
                Err(e) => status.set_text(&format!("Error: {e}")),
            }
        });
    }

    if *has_existing.borrow() {
        show_saved();
    } else {
        show_editing();
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
    vbox.append(&saved_row);
    vbox.append(&entry);
    vbox.append(&action_row);
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
