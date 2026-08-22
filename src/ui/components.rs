use gtk::prelude::WidgetExt;

/// Shows or hides a small inline validation hint under the input box.
pub fn set_hint(label: &gtk::Label, text: Option<&str>) {
    match text {
        Some(message) => {
            label.set_text(message);
            label.set_visible(true);
        }
        None => label.set_visible(false),
    }
}
