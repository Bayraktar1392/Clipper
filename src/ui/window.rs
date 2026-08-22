use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView, prelude::*};

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
    status: gtk::Label,
    url: String,
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

    /// Temporarily replaces the subtitle (the clip URL) with live transfer
    /// stats while a download is in progress.
    pub fn set_transfer(&self, text: &str) {
        self.row.set_subtitle(text);
    }

    /// Restores the subtitle back to the clip URL once a download finishes.
    pub fn reset_subtitle(&self) {
        self.row.set_subtitle(&self.url);
    }

    /// Attaches a detail tooltip (e.g. the saved path on success) so a
    /// row stays compact while the detail is still one hover away.
    pub fn set_tooltip(&self, detail: &str) {
        self.row.set_tooltip_text(Some(detail));
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
    pub placeholder: gtk::Label,
    pub download_button: gtk::Button,
    pub download_label: gtk::Label,
    pub spinner: gtk::Spinner,
    pub process_stack: gtk::Stack,
    pub progress: gtk::ProgressBar,
    pub hint: gtk::Label,
    pub queue: gtk::ListBox,
    pub view_stack: gtk::Stack,
    pub window_title: adw::WindowTitle,
    pub toast_overlay: adw::ToastOverlay,
}

pub fn build(app: &Application) -> Ui {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Clipper")
        .default_width(560)
        .default_height(660)
        .resizable(true)
        .build();

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

    // Input: a plain card, no label, no hints — a placeholder overlay
    // carries the instructions until the user starts typing.
    let input_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    input_card.add_css_class("card");

    let url_view = gtk::TextView::new();
    url_view.set_wrap_mode(gtk::WrapMode::WordChar);
    url_view.set_height_request(96);
    url_view.set_left_margin(12);
    url_view.set_right_margin(12);
    url_view.set_top_margin(10);
    url_view.set_bottom_margin(10);

    let placeholder = gtk::Label::new(Some("Paste Twitch Clip URLs, one or many"));
    placeholder.add_css_class("dim-label");
    placeholder.set_halign(gtk::Align::Start);
    placeholder.set_valign(gtk::Align::Start);
    placeholder.set_margin_top(10);
    placeholder.set_margin_start(14);
    placeholder.set_can_target(false);

    let input_overlay = gtk::Overlay::new();
    input_overlay.set_child(Some(&url_view));
    input_overlay.add_overlay(&placeholder);
    input_card.append(&input_overlay);
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
        .title("No clips queued")
        .description("Paste one or more Twitch Clip URLs above to add them here.")
        .build();
    status_page.set_vexpand(true);
    status_page.add_css_class("compact");

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_vexpand(true);
    let queue = gtk::ListBox::new();
    queue.set_selection_mode(gtk::SelectionMode::None);
    queue.add_css_class("boxed-list");
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
    root.append(&progress);

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
        placeholder,
        download_button,
        download_label: button_label,
        spinner,
        process_stack,
        progress,
        hint,
        queue,
        view_stack,
        window_title,
        toast_overlay,
    }
}

pub fn queue_row(title: &str, url: &str) -> QueueRow {
    let icon = gtk::Image::from_icon_name("video-x-generic-symbolic");
    icon.add_css_class("dim-label");

    let status = gtk::Label::new(Some("Waiting"));
    status.add_css_class("caption");
    status.add_css_class("dim-label");

    let row = adw::ActionRow::builder().title(title).subtitle(url).build();
    row.add_prefix(&icon);
    row.add_suffix(&status);

    QueueRow {
        row,
        status,
        url: url.to_string(),
    }
}
