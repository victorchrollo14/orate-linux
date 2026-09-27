// Vocabulary: exact spellings for names, brands and jargon, sent to the model
// as spelling hints with every request.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, FlowBox, Label, Orientation, ScrolledWindow, SelectionMode,
};

use super::widgets;
use crate::config;

/// The widgets `render` rewrites. Cloning is cheap (GObject refcounts) and lets
/// each chip's remove handler re-render without a closure cycle.
#[derive(Clone)]
struct Views {
    flow: FlowBox,
    header: GtkBox,
    empty: GtkBox,
    count: Label,
}

type Words = Rc<RefCell<Vec<String>>>;

pub fn build() -> ScrolledWindow {
    let page = widgets::page(
        "Vocabulary",
        "Add custom words so Orate spells them correctly \u{2014} names, brands, \
         technical terms, or anything unique to you.",
    );

    let words: Words = Rc::new(RefCell::new(config::vocabulary()));

    // MARK: - Add field

    let entry = Entry::builder()
        .placeholder_text("Add a word (e.g. Kubernetes, LangChain, Anthropic)")
        .hexpand(true)
        .build();
    entry.add_css_class("orate-field");

    let add = Button::with_label("Add");
    add.add_css_class("suggested-action");
    add.set_sensitive(false);

    let saved = widgets::confirmation();

    let add_row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();
    add_row.append(&entry);
    add_row.append(&add);
    add_row.append(&saved);
    page.content.append(&add_row);

    // MARK: - Word list

    let count = widgets::caption("");
    count.set_halign(Align::End);
    count.set_hexpand(true);

    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    header.append(&widgets::heading("Custom Words"));
    header.append(&count);

    let flow = FlowBox::builder()
        .selection_mode(SelectionMode::None)
        .row_spacing(8)
        .column_spacing(8)
        .max_children_per_line(30)
        .homogeneous(false)
        .build();

    let empty = widgets::empty_state(
        "accessories-dictionary-symbolic",
        "No custom words yet",
        "Add words that Orate should recognize and spell correctly.",
    );

    let section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .build();
    section.append(&header);
    section.append(&flow);
    section.append(&empty);
    page.content.append(&section);

    let views = Views {
        flow,
        header,
        empty,
        count,
    };

    // MARK: - Wiring

    let commit = {
        let entry = entry.clone();
        let words = words.clone();
        let saved = saved.clone();
        let views = views.clone();
        move || {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            entry.set_text("");
            if words.borrow().iter().any(|w| w.eq_ignore_ascii_case(&text)) {
                widgets::flash(&saved, "Already added");
                return;
            }
            words.borrow_mut().push(text);
            config::set_vocabulary(&words.borrow());
            render(&views, &words);
            widgets::flash(&saved, "Saved");
        }
    };

    {
        let commit = commit.clone();
        add.connect_clicked(move |_| commit());
    }
    {
        let commit = commit.clone();
        entry.connect_activate(move |_| commit());
    }
    {
        let add = add.clone();
        entry.connect_changed(move |e| add.set_sensitive(!e.text().trim().is_empty()));
    }

    render(&views, &words);
    page.root
}

fn render(views: &Views, words: &Words) {
    while let Some(child) = views.flow.first_child() {
        views.flow.remove(&child);
    }

    let n = {
        let list = words.borrow();
        for word in list.iter() {
            views.flow.insert(&chip(word, words.clone(), views.clone()), -1);
        }
        list.len()
    };

    views
        .count
        .set_label(&format!("{n} word{}", if n == 1 { "" } else { "s" }));
    views.flow.set_visible(n > 0);
    views.header.set_visible(n > 0);
    views.empty.set_visible(n == 0);
}

fn chip(word: &str, words: Words, views: Views) -> GtkBox {
    let chip = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();
    chip.add_css_class("orate-chip");
    chip.append(&Label::new(Some(word)));

    let remove = Button::from_icon_name("window-close-symbolic");
    remove.set_tooltip_text(Some("Remove"));
    let word = word.to_string();
    remove.connect_clicked(move |_| {
        words.borrow_mut().retain(|w| w != &word);
        config::set_vocabulary(&words.borrow());
        render(&views, &words);
    });
    chip.append(&remove);

    chip
}
