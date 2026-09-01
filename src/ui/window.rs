use crate::{config, link::url::Source};
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView, prelude::*};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

/// Visual state of a single queue row's status chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Waiting,
    Active,
    Done,
    Error,
}

/// One row in the queue list. Wraps an `AdwActionRow` so the rest of the
/// app never has to know about its internal widget tree — it just calls
/// these methods instead of walking child widgets.
pub struct QueueRow {
    pub row: adw::ActionRow,
    pub remove: gtk::Button,
    pub status: gtk::Label,
    url: String,
    reveal: Rc<RefCell<Option<PathBuf>>>,
}

impl QueueRow {
    /// Updates the trailing status chip (e.g. "Waiting", "42%", "Done").
    pub fn set_state(&self, state: RowState, text: &str) {
        self.status.set_text(text);
        for class in ["dim-label", "accent", "success", "error"] {
            self.status.remove_css_class(class);
        }
        self.status.add_css_class(match state {
            RowState::Waiting => "dim-label",
            RowState::Active => "accent",
            RowState::Done => "success",
            RowState::Error => "error",
        });
    }

    /// Restores the subtitle back to the clip URL.
    pub fn reset_subtitle(&self) {
        self.row.set_subtitle(&self.url);
    }

    /// Attaches a detail tooltip (e.g. the saved path on success) so a
    /// row stays compact while the detail is still one hover away.
    pub fn set_tooltip(&self, detail: &str) {
        self.row.set_tooltip_text(Some(detail));
    }

    /// Makes the row clickable from this point on: clicking it reveals the
    /// given file in the system file manager.
    pub fn set_reveal_target(&self, path: PathBuf) {
        *self.reveal.borrow_mut() = Some(path);
        self.row.set_activatable(true);
    }

    /// Attaches the failure reason as a tooltip, the same way `set_tooltip`
    /// surfaces the saved path on success — the row stays compact, the
    /// reason is one hover away.
    pub fn set_error_detail(&self, detail: &str) {
        self.row
            .set_tooltip_text(Some(&format!("Failed: {detail}")));
    }
}

pub struct Ui {
    pub url_view: gtk::TextView,
    pub download_button: gtk::Button,
    pub download_label: gtk::Label,
    pub spinner: gtk::Spinner,
    pub process_stack: gtk::Stack,
    pub progress: gtk::ProgressBar,
    pub status: gtk::Label,
    pub hint: gtk::Label,
    pub queue: gtk::ListBox,
    pub view_stack: gtk::Stack,
    pub window_title: adw::WindowTitle,
    pub toast_overlay: adw::ToastOverlay,
    pub folder_button: gtk::Button,
    pub folder_label: gtk::Label,
}

pub fn build(app: &Application) -> Ui {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Clipper")
        .default_width(580)
        .default_height(680)
        .resizable(true)
        .build();
    window.add_css_class("clipper-window");

    let window_title = adw::WindowTitle::new("Clipper", "");
    let header = HeaderBar::new();
    header.set_title_widget(Some(&window_title));
    header.set_show_start_title_buttons(true);
    header.set_show_end_title_buttons(true);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(480);
    clamp.set_margin_top(18);
    clamp.set_margin_bottom(18);
    clamp.set_margin_start(18);
    clamp.set_margin_end(18);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);

    // Input: a plain card, no label, no hints.
    let input_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    input_card.add_css_class("card");
    input_card.add_css_class("clipper-input-card");

    let url_view = gtk::TextView::new();
    url_view.set_wrap_mode(gtk::WrapMode::WordChar);
    url_view.set_height_request(96);
    url_view.set_left_margin(12);
    url_view.set_right_margin(12);
    url_view.set_top_margin(10);
    url_view.set_bottom_margin(10);

    // A slim "save to" row under the input field: shows the current
    // download directory and lets the user pick a new one via a folder
    // chooser dialog.
    let folder_icon2 = gtk::Image::from_icon_name("folder-download-symbolic");
    folder_icon2.set_pixel_size(14);
    folder_icon2.add_css_class("dim-label");
    let folder_label = gtk::Label::new(Some("..."));
    folder_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    folder_label.add_css_class("dim-label");
    folder_label.set_hexpand(true);
    folder_label.set_xalign(0.0);
    let folder_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    folder_row.append(&folder_icon2);
    folder_row.append(&folder_label);
    let folder_button = gtk::Button::new();
    folder_button.set_child(Some(&folder_row));
    folder_button.set_halign(gtk::Align::Fill);
    folder_button.set_css_classes(&["flat", "clipper-folder-row"]);
    folder_button.set_tooltip_text(Some("Choose a download directory"));
    input_card.append(&url_view);
    input_card.append(&folder_button);
    root.append(&input_card);

    let hint = gtk::Label::new(None);
    hint.set_halign(gtk::Align::Start);
    hint.set_wrap(true);
    hint.add_css_class("caption");
    hint.set_visible(false);
    root.append(&hint);

    // Queue: an empty-state page when there's nothing queued yet, and a
    // boxed list of clips once URLs are recognized — the same pattern
    // Files uses for an empty folder versus a populated one.
    let status_page = adw::StatusPage::builder()
        .icon_name("video-x-generic-symbolic")
        .title("No links queued")
        .description("Paste one or more Twitch Clip or YouTube links above to add them here.")
        .build();
    status_page.set_vexpand(true);
    status_page.add_css_class("compact");
    status_page.add_css_class("clipper-empty-state");

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_vexpand(true);
    let queue = gtk::ListBox::new();
    queue.set_selection_mode(gtk::SelectionMode::None);
    queue.add_css_class("boxed-list");
    queue.add_css_class("clipper-list");
    scroller.set_child(Some(&queue));

    let view_stack = gtk::Stack::new();
    view_stack.set_vexpand(true);
    view_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    view_stack.add_named(&status_page, Some("empty"));
    view_stack.add_named(&scroller, Some("list"));
    view_stack.set_visible_child_name("empty");
    root.append(&view_stack);

    let download_button = gtk::Button::new();
    download_button.set_hexpand(true);
    download_button.add_css_class("suggested-action");
    download_button.add_css_class("pill");
    download_button.add_css_class("clipper-download-button");
    let icon = gtk::Image::from_icon_name("folder-download-symbolic");
    let spinner = gtk::Spinner::new();
    let process_stack = gtk::Stack::new();
    process_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    process_stack.set_transition_duration(100);
    process_stack.add_named(&icon, Some("icon"));
    process_stack.add_named(&spinner, Some("spinner"));
    process_stack.set_visible_child_name("icon");
    let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    button_content.set_halign(gtk::Align::Center);
    let button_label = gtk::Label::new(Some("Download"));
    button_content.append(&process_stack);
    button_content.append(&button_label);
    download_button.set_child(Some(&button_content));
    root.append(&download_button);

    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    progress.set_hexpand(true);
    progress.set_visible(false);
    progress.add_css_class("clipper-progress");
    root.append(&progress);

    let status = gtk::Label::new(None);
    status.set_halign(gtk::Align::Center);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.add_css_class("clipper-status");
    status.set_visible(false);
    root.append(&status);

    clamp.set_child(Some(&root));

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&clamp));

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));
    window.set_content(Some(&toolbar));
    window.set_default_widget(Some(&download_button));
    window.present();

    Ui {
        url_view,
        download_button,
        download_label: button_label,
        spinner,
        process_stack,
        progress,
        status,
        hint,
        queue,
        view_stack,
        window_title,
        toast_overlay,
        folder_button,
        folder_label,
    }
}

pub fn queue_row(title: &str, url: &str, source: Source) -> QueueRow {
    let (icon_name, source_class) = match source {
        Source::Twitch => ("video-x-generic-symbolic", "twitch"),
        Source::YouTube => ("media-playback-start-symbolic", "youtube"),
    };

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    icon.add_css_class("clipper-icon-chip");
    icon.add_css_class(source_class);

    let status = gtk::Label::new(Some("Waiting"));
    status.add_css_class("clipper-chip");
    status.add_css_class("dim-label");

    let remove_icon = gtk::Image::from_icon_name("window-close-symbolic");
    remove_icon.set_pixel_size(14);
    let remove = gtk::Button::new();
    remove.set_child(Some(&remove_icon));
    remove.add_css_class("flat");
    remove.add_css_class("clipper-row-remove");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some("Remove from queue"));

    let row = adw::ActionRow::builder().title(title).subtitle(url).build();
    row.add_prefix(&icon);
    row.add_suffix(&remove);
    row.add_suffix(&status);
    row.set_tooltip_text(Some(&format!("{} · {url}", source.label())));

    let reveal: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    row.connect_activate({
        let reveal = Rc::clone(&reveal);
        move |row| {
            if let Some(path) = reveal.borrow().as_ref() {
                config::reveal_in_file_manager(path);
            } else {
                row.set_activatable(false);
            }
        }
    });

    QueueRow {
        row,
        remove,
        status,
        url: url.to_string(),
        reveal,
    }
}
