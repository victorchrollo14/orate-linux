use std::cell::RefCell;
use std::rc::Rc;

use chrono::Local;
use gtk4::prelude::*;
use gtk4::{
    gio, Align, Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Label,
    MenuButton, Orientation, PasswordEntry, PolicyType, ScrolledWindow, Stack, StackSwitcher,
};

use crate::clipboard;
use crate::history::{self, HistoryEntry};
use crate::secret;

pub const ORATE_CLOUD_KEY: &str = "orateCloudAPIKey";

pub fn show(app: &Application) {
    if let Some(win) = app.active_window() {
        win.present();
        return;
    }

    let stack = Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .build();

    let history_view = build_history_view();
    let settings_view = build_settings_view();

    stack.add_titled(&history_view.root, Some("history"), "History");
    stack.add_titled(&settings_view, Some("settings"), "Settings");

    let switcher = StackSwitcher::builder().stack(&stack).build();
    let header = HeaderBar::builder().title_widget(&switcher).build();

    let win = ApplicationWindow::builder()
        .application(app)
        .title("Orate")
        .default_width(720)
        .default_height(560)
        .child(&stack)
        .build();
    win.set_titlebar(Some(&header));

    {
        let refresh = history_view.refresh.clone();
        win.connect_show(move |_| refresh());
    }

    win.present();
}

// MARK: - History View

struct HistoryView {
    root: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_history_view() -> HistoryView {
    let header = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();
    let welcome = Label::builder()
        .label("Welcome to Orate")
        .halign(Align::Start)
        .build();
    welcome.add_css_class("title-1");
    let hint = Label::builder()
        .label("Press Ctrl+F8 and speak. Press again to transcribe.")
        .halign(Align::Start)
        .build();
    hint.add_css_class("dim-label");
    header.append(&welcome);
    header.append(&hint);

    let section_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(16)
        .build();
    let section = Label::builder()
        .label("Recent Transcriptions")
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    section.add_css_class("heading");
    section_row.append(&section);

    let menu = gio::Menu::new();
    menu.append(Some("Older than 7 days"), Some("history.clear-7"));
    menu.append(Some("Older than 14 days"), Some("history.clear-14"));
    menu.append(Some("Older than 30 days"), Some("history.clear-30"));
    let all_section = gio::Menu::new();
    all_section.append(Some("All recordings"), Some("history.clear-all"));
    menu.append_section(None, &all_section);

    let clear_menu = MenuButton::builder()
        .label("Clear")
        .menu_model(&menu)
        .build();
    section_row.append(&clear_menu);

    let list = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .build();

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .build();

    let body = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(28)
        .margin_end(28)
        .build();
    body.append(&header);
    body.append(&section_row);
    body.append(&list);
    scroll.set_child(Some(&body));

    let root = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .build();
    root.append(&scroll);

    let list_for_refresh = list.clone();
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        rebuild_history_list(&list_for_refresh);
    });

    let actions = gio::SimpleActionGroup::new();
    let install_clear = |name: &str, days: Option<i64>, refresh: Rc<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| {
            let _ = history::delete_older_than(days);
            refresh();
        });
        action
    };
    actions.add_action(&install_clear("clear-7", Some(7), refresh.clone()));
    actions.add_action(&install_clear("clear-14", Some(14), refresh.clone()));
    actions.add_action(&install_clear("clear-30", Some(30), refresh.clone()));
    actions.add_action(&install_clear("clear-all", None, refresh.clone()));
    root.insert_action_group("history", Some(&actions));

    HistoryView { root, refresh }
}

fn rebuild_history_list(list: &GtkBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let entries = history::load_all();
    if entries.is_empty() {
        list.append(&empty_state());
        return;
    }

    for entry in entries {
        list.append(&history_row(&entry));
    }
}

fn empty_state() -> GtkBox {
    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .halign(Align::Center)
        .margin_top(48)
        .margin_bottom(48)
        .build();
    let title = Label::new(Some("No transcriptions yet"));
    title.add_css_class("dim-label");
    title.add_css_class("heading");
    let sub = Label::new(Some("Press your shortcut to record your first transcription."));
    sub.add_css_class("dim-label");
    vbox.append(&title);
    vbox.append(&sub);
    vbox
}

fn history_row(entry: &HistoryEntry) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    card.add_css_class("card");

    let transcript = Label::builder()
        .label(&entry.transcript)
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    card.append(&transcript);

    let bar = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();

    let copy = Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("flat");
    copy.set_tooltip_text(Some("Copy transcript"));
    let text = entry.transcript.clone();
    copy.connect_clicked(move |btn| {
        if clipboard::set(&text).is_ok() {
            btn.set_tooltip_text(Some("Copied!"));
        }
    });
    bar.append(&copy);

    let local_ts = entry.timestamp.with_timezone(&Local);
    let timestamp = Label::new(Some(&local_ts.format("%b %-d, %Y \u{2022} %-I:%M %p").to_string()));
    timestamp.add_css_class("dim-label");
    timestamp.add_css_class("caption");
    bar.append(&timestamp);

    let dot1 = Label::new(Some("\u{2022}"));
    dot1.add_css_class("dim-label");
    bar.append(&dot1);

    let latency = Label::new(Some(&format!("{} ms", entry.latency_ms)));
    latency.add_css_class("dim-label");
    latency.add_css_class("caption");
    bar.append(&latency);

    let dot2 = Label::new(Some("\u{2022}"));
    dot2.add_css_class("dim-label");
    bar.append(&dot2);

    let words = entry.transcript.split_whitespace().count();
    let words_label = Label::new(Some(&format!("{words} words")));
    words_label.add_css_class("dim-label");
    words_label.add_css_class("caption");
    bar.append(&words_label);

    card.append(&bar);
    card
}

// MARK: - Settings View

fn build_settings_view() -> GtkBox {
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
    let saved_label = Label::new(Some("\u{2713} API key saved"));
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
        .margin_top(28)
        .margin_bottom(28)
        .margin_start(28)
        .margin_end(28)
        .build();
    vbox.append(&title);
    vbox.append(&hint);
    vbox.append(&saved_row);
    vbox.append(&entry);
    vbox.append(&action_row);
    vbox.append(&status);
    vbox
}
