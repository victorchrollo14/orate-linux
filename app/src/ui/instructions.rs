// Custom Instructions: free-form text appended to the system prompt on every
// transcription, plus one-tap examples.

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, GestureClick, Label, Orientation, PolicyType, ScrolledWindow,
    TextView, WrapMode,
};

use super::widgets;
use crate::config;

const EXAMPLES: &[&str] = &[
    "I'm a doctor. Use proper medical terminology (e.g. \"myocardial infarction\" instead of \"heart attack\").",
    "I write code. Format technical terms in lowercase (e.g. \"kubernetes\", \"nginx\"). Spell out variable-style names as spoken.",
    "I speak with filler words. Remove \"um\", \"uh\", \"like\", and \"you know\" from my speech.",
    "I dictate in Spanish but want transcriptions in English.",
    "Always use Oxford commas and American English spelling.",
];

pub fn build() -> ScrolledWindow {
    let page = widgets::page(
        "Custom Instructions",
        "Tell Orate about yourself and how you'd like your transcriptions formatted. \
         These instructions are included with every transcription request.",
    );

    let view = TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(10)
        .right_margin(10)
        .build();
    view.buffer().set_text(&config::custom_instructions());

    let editor = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(180)
        .child(&view)
        .build();
    editor.add_css_class("orate-editor");

    let actions = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let save = Button::with_label("Save");
    save.add_css_class("suggested-action");
    let saved = widgets::confirmation();
    let clear = Button::with_label("Clear");
    clear.set_halign(Align::End);
    clear.set_hexpand(true);

    actions.append(&save);
    actions.append(&saved);
    actions.append(&clear);

    {
        let view = view.clone();
        let saved = saved.clone();
        save.connect_clicked(move |_| {
            let buffer = view.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            config::set_custom_instructions(&text);
            widgets::flash(&saved, "Saved");
        });
    }
    {
        let view = view.clone();
        let saved = saved.clone();
        clear.connect_clicked(move |_| {
            view.buffer().set_text("");
            config::set_custom_instructions("");
            widgets::flash(&saved, "Cleared");
        });
    }

    let editor_section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .build();
    editor_section.append(&editor);
    editor_section.append(&actions);
    page.content.append(&editor_section);

    let examples = widgets::section("Examples", Some("Click one to use it as a starting point."));
    for text in EXAMPLES {
        examples.append(&example_row(text, &view));
    }
    page.content.append(&examples);

    page.root
}

fn example_row(text: &str, view: &TextView) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();
    row.add_css_class("orate-example");

    let bulb = gtk4::Image::from_icon_name("dialog-information-symbolic");
    bulb.set_valign(Align::Start);
    bulb.add_css_class("orate-caption");
    row.append(&bulb);

    let label = Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .build();
    label.add_css_class("orate-body-dim");
    row.append(&label);

    let click = GestureClick::new();
    let view = view.clone();
    let text = text.to_string();
    click.connect_released(move |_, _, _, _| {
        view.buffer().set_text(&text);
        view.grab_focus();
    });
    row.add_controller(click);
    row.set_cursor_from_name(Some("pointer"));

    row
}
