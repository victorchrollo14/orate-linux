// Settings: push-to-talk shortcut, transcription provider and its
// credentials, and behaviour toggles.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    gdk, glib, Align, Box as GtkBox, Button, DropDown, Entry, EventControllerKey, Label,
    LinkButton, Orientation, PasswordEntry, PropagationPhase, ScrolledWindow, Switch,
    ToggleButton,
};

use super::widgets;
use crate::config::{self, Provider};
use crate::secret;

pub fn build(refresh_home: Rc<dyn Fn()>) -> ScrolledWindow {
    let page = widgets::page(
        "Settings",
        "Configure your shortcut, credentials and preferences.",
    );

    page.content.append(&shortcut_section(refresh_home.clone()));
    page.content.append(&provider_section(refresh_home));
    page.content.append(&preferences_section());

    page.root
}

// MARK: - Push-to-talk shortcut

fn shortcut_section(refresh_home: Rc<dyn Fn()>) -> GtkBox {
    let section = widgets::section(
        "Push-to-Talk Shortcut",
        Some(
            "Press this shortcut to start recording, press it again to transcribe. \
             Click the button below and type a new combination to change it.",
        ),
    );

    let keycap = Button::builder()
        .label(super::shortcut_label())
        .halign(Align::Start)
        .focusable(true)
        .build();
    keycap.add_css_class("orate-keycap");

    let status = widgets::status();
    let editable = config::shortcut_is_editable();

    if !editable {
        keycap.set_sensitive(false);
        widgets::set_status(&status, "The Orate GNOME Shell extension is not installed, so the shortcut cannot be changed here.");
        status.add_css_class("orate-error");
        section.append(&keycap);
        section.append(&status);
        return section;
    }

    let recording = Rc::new(Cell::new(false));

    let stop = {
        let keycap = keycap.clone();
        let recording = recording.clone();
        move || {
            recording.set(false);
            keycap.remove_css_class("recording");
            keycap.set_label(&super::shortcut_label());
        }
    };

    {
        let recording = recording.clone();
        let stop = stop.clone();
        keycap.connect_clicked(move |btn| {
            if recording.get() {
                stop();
                return;
            }
            recording.set(true);
            btn.add_css_class("recording");
            btn.set_label("Type a shortcut\u{2026} (Esc to cancel)");
            btn.grab_focus();
        });
    }

    // Capture phase so the combination never reaches the default handlers
    // (Space and Enter would otherwise activate the button itself).
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(PropagationPhase::Capture);
    {
        let recording = recording.clone();
        let stop = stop.clone();
        let status = status.clone();
        let keycap = keycap.clone();
        controller.connect_key_pressed(move |_, keyval, _keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }
            if keyval == gdk::Key::Escape {
                stop();
                return glib::Propagation::Stop;
            }
            if is_modifier(keyval) {
                return glib::Propagation::Stop;
            }

            let mods = state
                & (gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK);

            if !gtk4::accelerator_valid(keyval, mods) {
                widgets::set_status(&status, "That combination can't be used as a shortcut.");
                return glib::Propagation::Stop;
            }

            let accel = gtk4::accelerator_name(keyval, mods);
            match config::set_shortcut(&accel) {
                Ok(()) => {
                    widgets::set_status(&status, "Shortcut updated.");
                    refresh_home();
                }
                Err(e) => widgets::set_status(&status, &format!("Could not save shortcut: {e}")),
            }
            stop();
            keycap.set_label(&super::shortcut_label());
            glib::Propagation::Stop
        });
    }
    keycap.add_controller(controller);

    // Leaving the button while armed shouldn't strand it in recording mode.
    {
        let recording = recording.clone();
        let stop = stop.clone();
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            if recording.get() {
                stop();
            }
        });
        keycap.add_controller(focus);
    }

    section.append(&keycap);
    section.append(&status);
    section.append(&widgets::caption_with_icon(
        "dialog-information-symbolic",
        "Any combination the shell accepts works, e.g. Ctrl+F8 or Super+Space.",
    ));
    section
}

fn is_modifier(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::Caps_Lock
            | gdk::Key::Num_Lock
            | gdk::Key::ISO_Level3_Shift
    )
}

// MARK: - Provider

fn provider_section(refresh_home: Rc<dyn Fn()>) -> GtkBox {
    let section = widgets::section(
        "AI Provider",
        Some("Choose where to send your audio for transcription."),
    );

    // Linked toggle buttons read as a segmented control, like the macOS picker.
    let picker = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::Start)
        .build();
    picker.add_css_class("linked");

    let credentials = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .build();

    let current = config::provider();
    credentials.append(&credentials_section(current));

    let mut group: Option<ToggleButton> = None;
    for provider in Provider::ALL {
        let button = ToggleButton::with_label(provider.display_name());
        button.set_group(group.as_ref());
        button.set_active(provider == current);
        group.get_or_insert_with(|| button.clone());

        let credentials = credentials.clone();
        let refresh_home = refresh_home.clone();
        button.connect_toggled(move |btn| {
            if !btn.is_active() {
                return;
            }
            config::set_provider(provider);
            while let Some(child) = credentials.first_child() {
                credentials.remove(&child);
            }
            credentials.append(&credentials_section(provider));
            refresh_home();
        });
        picker.append(&button);
    }

    section.append(&picker);
    section.append(&credentials);
    section
}

fn credentials_section(provider: Provider) -> GtkBox {
    let section = widgets::section(&format!("{} API Key", provider.display_name()), None);

    match provider {
        Provider::OrateCloud => {
            section.append(&widgets::body_dim(
                "Orate Cloud is the easiest way to get started. Paste the key from your Orate account below.",
            ));
        }
        Provider::GoogleAI => {
            section.append(&widgets::body_dim(
                "Orate uses Google's Gemini API to transcribe your audio. You'll need an API key from Google AI Studio.",
            ));
            section.append(&link(
                "https://aistudio.google.com/apikey",
                "Get your API key from Google AI Studio",
            ));
        }
        Provider::VertexAI => {
            section.append(&widgets::body_dim(
                "Use Vertex AI through your Google Cloud project. You'll need an API key from the GCP console with Vertex AI access.",
            ));
            section.append(&link(
                "https://console.cloud.google.com/apis/credentials",
                "Create an API key in Google Cloud Console",
            ));
            section.append(&vertex_config());
        }
    }

    section.append(&api_key_editor(provider.keyring_key()));
    section
}

fn link(uri: &str, label: &str) -> LinkButton {
    let button = LinkButton::with_label(uri, label);
    button.set_halign(Align::Start);
    button
}

fn vertex_config() -> GtkBox {
    let card = widgets::card();

    card.append(&widgets::caption("Project ID"));
    let project = Entry::builder()
        .placeholder_text("your-gcp-project-id")
        .text(config::vertex_project_id())
        .build();
    project.add_css_class("orate-mono");
    project.connect_changed(|e| config::set_vertex_project_id(&e.text()));
    card.append(&project);

    card.append(&widgets::caption("Region"));
    let regions = config::VERTEX_REGIONS.iter().map(|r| (r.to_string(), r.to_string()));
    card.append(&choice_dropdown(
        regions.collect(),
        &config::vertex_region(),
        config::set_vertex_region,
    ));

    card
}

/// Dropdown over `(value, label)` pairs that persists the chosen value. A
/// current value missing from the list (set via gsettings, say) is appended so
/// it stays selected instead of being silently replaced.
fn choice_dropdown(
    mut options: Vec<(String, String)>,
    current: &str,
    on_select: fn(&str),
) -> DropDown {
    if !options.iter().any(|(value, _)| value == current) {
        options.push((current.to_string(), current.to_string()));
    }
    let labels: Vec<&str> = options.iter().map(|(_, label)| label.as_str()).collect();
    let dropdown = DropDown::from_strings(&labels);
    dropdown.set_halign(Align::Start);
    if let Some(i) = options.iter().position(|(value, _)| value == current) {
        dropdown.set_selected(i as u32);
    }
    dropdown.connect_selected_notify(move |dd| {
        if let Some((value, _)) = options.get(dd.selected() as usize) {
            on_select(value);
        }
    });
    dropdown
}

// MARK: - API key

fn api_key_editor(keyring_key: &'static str) -> GtkBox {
    let section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .build();

    let saved_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .halign(Align::Start)
        .build();
    let saved_label = Label::new(Some("\u{2713} API key saved"));
    saved_label.add_css_class("orate-success");
    let change = Button::with_label("Change");
    let remove = Button::with_label("Remove");
    remove.add_css_class("destructive-action");
    saved_row.append(&saved_label);
    saved_row.append(&change);
    saved_row.append(&remove);

    let entry = PasswordEntry::builder().show_peek_icon(true).build();
    entry.add_css_class("orate-mono");

    let status = widgets::status();

    let actions = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Start)
        .build();
    let save = Button::with_label("Save");
    save.add_css_class("suggested-action");
    let cancel = Button::with_label("Cancel");
    actions.append(&save);
    actions.append(&cancel);

    let has_key = Rc::new(RefCell::new(secret::read(keyring_key).is_some()));

    let show_saved = {
        let saved_row = saved_row.clone();
        let entry = entry.clone();
        let actions = actions.clone();
        move || {
            saved_row.set_visible(true);
            entry.set_visible(false);
            actions.set_visible(false);
        }
    };
    let show_editing = {
        let saved_row = saved_row.clone();
        let entry = entry.clone();
        let actions = actions.clone();
        let cancel = cancel.clone();
        let has_key = has_key.clone();
        move || {
            saved_row.set_visible(false);
            entry.set_visible(true);
            entry.set_text("");
            actions.set_visible(true);
            cancel.set_visible(*has_key.borrow());
        }
    };

    {
        let show_editing = show_editing.clone();
        change.connect_clicked(move |_| show_editing());
    }
    {
        let show_editing = show_editing.clone();
        let has_key = has_key.clone();
        let status = status.clone();
        remove.connect_clicked(move |_| {
            match secret::delete(keyring_key) {
                Ok(()) => {
                    *has_key.borrow_mut() = false;
                    widgets::set_status(&status, "API key removed.");
                }
                Err(e) => widgets::set_status(&status, &format!("Could not remove key: {e}")),
            }
            show_editing();
        });
    }
    {
        let show_saved = show_saved.clone();
        let has_key = has_key.clone();
        let status = status.clone();
        cancel.connect_clicked(move |_| {
            if *has_key.borrow() {
                widgets::set_status(&status, "");
                show_saved();
            }
        });
    }
    {
        let entry = entry.clone();
        let status = status.clone();
        let show_saved = show_saved.clone();
        let has_key = has_key.clone();
        save.connect_clicked(move |_| {
            let value = entry.text().trim().to_string();
            if value.is_empty() {
                widgets::set_status(&status, "Please enter a key.");
                return;
            }
            match secret::save(keyring_key, &value) {
                Ok(()) => {
                    *has_key.borrow_mut() = true;
                    widgets::set_status(&status, "");
                    show_saved();
                }
                Err(e) => widgets::set_status(&status, &format!("Error: {e}")),
            }
        });
    }

    section.append(&saved_row);
    section.append(&entry);
    section.append(&actions);
    section.append(&status);
    section.append(&widgets::caption_with_icon(
        "channel-secure-symbolic",
        "Your API key is stored in the system keyring (libsecret), never on disk in plain text.",
    ));

    if *has_key.borrow() {
        show_saved();
    } else {
        show_editing();
    }

    section
}

// MARK: - Behaviour toggles

fn preferences_section() -> GtkBox {
    let section = widgets::section("Preferences", None);

    let card = widgets::card();
    card.append(&switch_row(
        "Sound feedback",
        "Play a cue when recording starts, when the transcript lands, and on errors.",
        config::sound_feedback(),
        config::set_sound_feedback,
    ));
    card.append(&switch_row(
        "Keep recordings",
        "Save the captured audio with each history entry so you can play it back.",
        config::save_audio(),
        config::set_save_audio,
    ));
    section.append(&card);

    section
}

fn switch_row(
    title: &str,
    description: &str,
    initial: bool,
    on_toggle: fn(bool),
) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(16)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let text = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    let title_label = Label::builder()
        .label(title)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    text.append(&title_label);
    text.append(&widgets::caption(description));
    row.append(&text);

    let switch = Switch::builder()
        .active(initial)
        .valign(Align::Center)
        .build();
    switch.connect_state_set(move |_, state| {
        on_toggle(state);
        glib::Propagation::Proceed
    });
    row.append(&switch);

    row
}
