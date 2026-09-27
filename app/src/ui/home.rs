// Home: welcome header, remaining word balance, and the transcription history.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::Local;
use gtk4::prelude::*;
use gtk4::{gio, Align, Box as GtkBox, Button, Label, MenuButton, Orientation, ScrolledWindow};
use log::warn;

use super::widgets;
use crate::clipboard;
use crate::history::{self, HistoryEntry};
use crate::player::Player;

pub struct HomeView {
    pub root: ScrolledWindow,
    pub refresh: Rc<dyn Fn()>,
}

pub fn build() -> HomeView {
    let page = widgets::page("Welcome to Orate", &welcome_markup());

    let balance = Label::builder()
        .label("")
        .halign(Align::Start)
        .visible(false)
        .build();
    balance.add_css_class("orate-badge");
    let balance_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::Start)
        .build();
    balance_row.append(&balance);
    page.content.append(&balance_row);

    let section_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    let heading = widgets::heading("Recent Transcriptions");
    heading.set_hexpand(true);
    section_row.append(&heading);

    let menu = gio::Menu::new();
    menu.append(Some("Older than 7 days"), Some("history.clear-7"));
    menu.append(Some("Older than 14 days"), Some("history.clear-14"));
    menu.append(Some("Older than 30 days"), Some("history.clear-30"));
    let all_section = gio::Menu::new();
    all_section.append(Some("All recordings"), Some("history.clear-all"));
    menu.append_section(None, &all_section);
    section_row.append(&MenuButton::builder().label("Clear").menu_model(&menu).build());

    let cleared = widgets::confirmation();
    section_row.append(&cleared);

    let list = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .build();

    let history_section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .build();
    history_section.append(&section_row);
    history_section.append(&list);
    page.content.append(&history_section);

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let balance = balance.clone();
        let subtitle = page.subtitle.clone();
        Rc::new(move || {
            subtitle.set_markup(&welcome_markup());
            rebuild(&list, &balance);
        })
    };

    install_clear_actions(&page.content, refresh.clone(), &cleared);

    HomeView {
        root: page.root,
        refresh,
    }
}

fn welcome_markup() -> String {
    format!(
        "Press <b>{}</b> and speak. Press again to transcribe.",
        glib_escape(&super::shortcut_label())
    )
}

fn glib_escape(text: &str) -> String {
    gtk4::glib::markup_escape_text(text).to_string()
}

fn install_clear_actions(anchor: &GtkBox, refresh: Rc<dyn Fn()>, cleared: &Label) {
    let actions = gio::SimpleActionGroup::new();
    for (name, days) in [
        ("clear-7", Some(7i64)),
        ("clear-14", Some(14)),
        ("clear-30", Some(30)),
        ("clear-all", None),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let refresh = refresh.clone();
        let cleared = cleared.clone();
        action.connect_activate(move |_, _| {
            let deleted = history::delete_older_than(days);
            refresh();
            let message = match deleted {
                0 => "Nothing to clear".to_string(),
                1 => "1 recording cleared".to_string(),
                n => format!("{n} recordings cleared"),
            };
            widgets::flash(&cleared, &message);
        });
        actions.add_action(&action);
    }
    anchor.insert_action_group("history", Some(&actions));
}

fn rebuild(list: &GtkBox, balance: &Label) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let entries = history::load_all();

    // Entries are newest-first, so the first one carrying a balance is current.
    // Only Orate Cloud has a balance; on the Gemini providers an old one would
    // be stale.
    let balance_words = (crate::config::provider() == crate::config::Provider::OrateCloud)
        .then(|| entries.iter().find_map(|e| e.words_remaining))
        .flatten();
    match balance_words {
        Some(n) => {
            balance.set_label(&format!("{} words remaining", format_thousands(n)));
            balance.set_visible(true);
        }
        None => balance.set_visible(false),
    }

    if entries.is_empty() {
        list.append(&widgets::empty_state(
            "audio-input-microphone-symbolic",
            "No transcriptions yet",
            "Press your shortcut to record your first transcription.",
        ));
        return;
    }

    for entry in entries {
        list.append(&history_row(&entry));
    }
}

fn format_thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn history_row(entry: &HistoryEntry) -> GtkBox {
    let card = widgets::card();

    let transcript = Label::builder()
        .label(&entry.transcript)
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    card.append(&transcript);

    let bar = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();

    bar.append(&play_button(entry));

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
    let words = entry.transcript.split_whitespace().count();
    let meta = format!(
        "{}  \u{2022}  {} ms  \u{2022}  {} words",
        local_ts.format("%b %-d, %Y at %-I:%M %p"),
        entry.latency_ms,
        words
    );
    // One line, never wrapped — it sits beside the buttons in a tight row.
    let meta_label = widgets::caption(&meta);
    meta_label.set_wrap(false);
    meta_label.set_hexpand(true);
    bar.append(&meta_label);

    card.append(&bar);
    card
}

fn play_button(entry: &HistoryEntry) -> Button {
    let play = Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("flat");

    let has_audio = entry.has_audio();
    play.set_sensitive(has_audio);
    play.set_tooltip_text(Some(if has_audio {
        "Play recording"
    } else {
        "Recording not saved"
    }));
    if !has_audio {
        return play;
    }

    let audio_path = entry.audio_path();
    // One slot per row. Storing a new Player drops the old one, which stops it;
    // on_finished clears the slot so the icon flips back at end of file.
    let slot: Rc<RefCell<Option<Player>>> = Rc::new(RefCell::new(None));
    play.connect_clicked(move |btn| {
        if slot.borrow().is_some() {
            slot.borrow_mut().take();
            btn.set_icon_name("media-playback-start-symbolic");
            btn.set_tooltip_text(Some("Play recording"));
            return;
        }

        let slot_done = slot.clone();
        let btn_done = btn.clone();
        let on_finished = move || {
            slot_done.borrow_mut().take();
            btn_done.set_icon_name("media-playback-start-symbolic");
            btn_done.set_tooltip_text(Some("Play recording"));
        };

        match Player::play(&audio_path, on_finished) {
            Ok(player) => {
                slot.borrow_mut().replace(player);
                btn.set_icon_name("media-playback-stop-symbolic");
                btn.set_tooltip_text(Some("Stop"));
            }
            Err(e) => {
                warn!("playback failed: {e}");
                btn.set_tooltip_text(Some("Playback failed"));
            }
        }
    });

    play
}
