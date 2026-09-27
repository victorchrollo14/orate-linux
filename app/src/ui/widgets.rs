// Small building blocks shared by the four pages, so every page gets the same
// title / subtitle / card rhythm without repeating builder chains.

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Box as GtkBox, Label, NaturalWrapMode, Orientation, PolicyType, ScrolledWindow,
};

/// A wrapping label whose *natural* width is the whole string, so it fills the
/// content column before it starts wrapping instead of breaking early on GTK's
/// default ~65-character heuristic.
fn wrapping_label(text: &str) -> Label {
    let label = Label::builder()
        .label(text)
        .halign(Align::Start)
        .xalign(0.0)
        .wrap(true)
        .natural_wrap_mode(NaturalWrapMode::None)
        .build();
    label
}

pub struct Page {
    pub root: ScrolledWindow,
    /// Vertical box the caller appends sections to. Already carries the header.
    pub content: GtkBox,
    /// Kept so pages whose subtitle depends on live state can update it.
    pub subtitle: Label,
}

pub fn page(title: &str, subtitle_markup: &str) -> Page {
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .margin_top(32)
        .margin_bottom(32)
        .margin_start(32)
        .margin_end(32)
        .build();

    let header = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    let title_label = Label::builder()
        .label(title)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    title_label.add_css_class("orate-title");
    header.append(&title_label);

    let subtitle = wrapping_label(subtitle_markup);
    subtitle.set_use_markup(true);
    subtitle.add_css_class("orate-subtitle");
    header.append(&subtitle);

    content.append(&header);

    let root = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();

    Page {
        root,
        content,
        subtitle,
    }
}

/// A vertical group: bold section label, optional explanation, then children.
pub fn section(title: &str, description: Option<&str>) -> GtkBox {
    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .build();
    vbox.append(&heading(title));
    if let Some(text) = description {
        vbox.append(&body_dim(text));
    }
    vbox
}

pub fn heading(text: &str) -> Label {
    let label = Label::builder()
        .label(text)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("orate-section");
    label
}

pub fn body_dim(text: &str) -> Label {
    let label = wrapping_label(text);
    label.add_css_class("orate-body-dim");
    label
}

pub fn caption(text: &str) -> Label {
    let label = wrapping_label(text);
    label.add_css_class("orate-caption");
    label
}

/// A caption that only takes up space once it has something to say.
pub fn status() -> Label {
    let label = caption("");
    label.set_visible(false);
    label
}

pub fn set_status(label: &Label, text: &str) {
    label.set_label(text);
    label.set_visible(!text.is_empty());
}

/// Caption prefixed with a small symbolic icon, like macOS's `Label(_:systemImage:)`.
pub fn caption_with_icon(icon: &str, text: &str) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Start)
        .build();
    let image = gtk4::Image::from_icon_name(icon);
    image.add_css_class("orate-caption");
    row.append(&image);
    row.append(&caption(text));
    row
}

pub fn card() -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .build();
    card.add_css_class("orate-card");
    card
}

/// Green "✓ Saved" confirmation that fades itself out, matching the transient
/// `saved` state the macOS views use after a write.
pub fn confirmation() -> Label {
    let label = Label::builder().label("").visible(false).build();
    label.add_css_class("orate-success");
    label
}

pub fn flash(label: &Label, text: &str) {
    label.set_label(&format!("\u{2713} {text}"));
    label.set_visible(true);
    let label = label.clone();
    glib::timeout_add_seconds_local_once(2, move || label.set_visible(false));
}

pub fn empty_state(icon: &str, title: &str, subtitle: &str) -> GtkBox {
    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .halign(Align::Center)
        .margin_top(48)
        .margin_bottom(48)
        .build();

    let image = gtk4::Image::from_icon_name(icon);
    image.set_pixel_size(40);
    image.add_css_class("orate-empty-icon");
    vbox.append(&image);

    let title_label = Label::new(Some(title));
    title_label.add_css_class("orate-section");
    vbox.append(&title_label);

    let subtitle_label = Label::new(Some(subtitle));
    subtitle_label.add_css_class("orate-caption");
    vbox.append(&subtitle_label);

    vbox
}
