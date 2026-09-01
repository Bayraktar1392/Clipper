mod app;
mod config;
mod error;
mod link;
mod state;
mod ui;

use gtk::gio::prelude::{ApplicationExt, ApplicationExtManual};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Loads the app's minimal stylesheet on top of whatever system theme is
/// active. Every color it uses is one of libadwaita's own named colors, so
/// light/dark mode and the user's accent color still come from the platform —
/// the stylesheet only adjusts a few shapes and motion cues.
fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../assets/style.css"));

    match gtk::gdk::Display::default() {
        Some(display) => gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        ),
        None => tracing::warn!("no default display available; skipping custom styling"),
    }
}

fn main() -> gtk::glib::ExitCode {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "clipper=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let app = adw::Application::builder()
        .application_id("io.github.clipper")
        .build();

    app.connect_startup(|_| load_css());
    app.connect_activate(app::build_ui);
    app.run()
}
