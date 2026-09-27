// The main window: a sidebar of pages on the left, the selected page on the
// right — the same shape as the macOS app's NavigationSplitView.

mod home;
mod instructions;
mod settings;
mod vocabulary;
mod widgets;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, HeaderBar, Image, Label,
    ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow, SelectionMode, Separator, Stack,
    StackTransitionType,
};

/// (stack name, sidebar label, symbolic icon)
const PAGES: &[(&str, &str, &str)] = &[
    ("home", "Home", "user-home-symbolic"),
    ("instructions", "Instructions", "document-edit-symbolic"),
    ("vocabulary", "Vocabulary", "accessories-dictionary-symbolic"),
    ("settings", "Settings", "preferences-system-symbolic"),
];

pub fn show(app: &Application) {
    if let Some(win) = app.active_window() {
        win.present();
        return;
    }

    load_css();

    let stack = Stack::builder()
        .transition_type(StackTransitionType::Crossfade)
        .transition_duration(120)
        .hexpand(true)
        .vexpand(true)
        .build();

    let home = home::build();
    let refresh_home = home.refresh.clone();

    stack.add_named(&home.root, Some("home"));
    stack.add_named(&instructions::build(), Some("instructions"));
    stack.add_named(&vocabulary::build(), Some("vocabulary"));
    stack.add_named(&settings::build(refresh_home.clone()), Some("settings"));

    let (sidebar, nav) = build_sidebar(&stack);

    let split = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .build();
    split.append(&sidebar);
    split.append(&Separator::new(Orientation::Vertical));
    split.append(&stack);

    let header = HeaderBar::builder().build();

    let win = ApplicationWindow::builder()
        .application(app)
        .title("Orate")
        .default_width(900)
        .default_height(650)
        .child(&split)
        .build();
    win.set_size_request(700, 500);
    win.set_titlebar(Some(&header));

    // The history list is rebuilt on show and whenever Home comes back into
    // view, so a transcription made while the window sat open still appears.
    {
        let refresh = refresh_home.clone();
        win.connect_show(move |_| refresh());
    }
    {
        let refresh = refresh_home.clone();
        stack.connect_visible_child_name_notify(move |stack| {
            if stack.visible_child_name().as_deref() == Some("home") {
                refresh();
            }
        });
    }

    win.present();
    // Without this the first focusable widget on the visible page takes focus,
    // which scrolls a page past its own header the moment it opens.
    nav.grab_focus();
}

fn build_sidebar(stack: &Stack) -> (GtkBox, ListBox) {
    let list = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .build();

    for (_, title, icon) in PAGES {
        let row_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
        row_box.append(&Image::from_icon_name(icon));
        row_box.append(
            &Label::builder()
                .label(*title)
                .halign(Align::Start)
                .xalign(0.0)
                .build(),
        );
        let row = ListBoxRow::builder().child(&row_box).build();
        list.append(&row);
    }

    // Guard against the reentrancy of selecting a row programmatically.
    let syncing = Rc::new(RefCell::new(false));
    {
        let stack = stack.clone();
        let syncing = syncing.clone();
        list.connect_row_selected(move |_, row| {
            if *syncing.borrow() {
                return;
            }
            if let Some(row) = row {
                if let Some((name, _, _)) = PAGES.get(row.index() as usize) {
                    stack.set_visible_child_name(name);
                }
            }
        });
    }

    if let Some(first) = list.row_at_index(0) {
        *syncing.borrow_mut() = true;
        list.select_row(Some(&first));
        *syncing.borrow_mut() = false;
        stack.set_visible_child_name("home");
    }

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .child(&list)
        .build();

    let sidebar = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .width_request(190)
        .build();
    sidebar.add_css_class("orate-sidebar");

    let brand = Label::builder()
        .label("ORATE")
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    brand.add_css_class("orate-brand");
    sidebar.append(&brand);
    sidebar.append(&scroll);

    (sidebar, list)
}

/// The configured shortcut rendered the way a user reads it ("Ctrl+F8"), not
/// the way GSettings stores it ("&lt;Control&gt;F8").
pub(crate) fn shortcut_label() -> String {
    let accel = crate::config::shortcut();
    match gtk4::accelerator_parse(&accel) {
        Some((key, mods)) => gtk4::accelerator_get_label(key, mods).to_string(),
        None => accel,
    }
}

fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
