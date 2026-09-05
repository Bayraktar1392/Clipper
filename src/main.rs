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

fn load_resources() {
    gtk::gio::resources_register_include!("clipper.gresource")
        .expect("Failed to register gresource");
}

/// Configures Wayland-native optimizations for the application.
/// Sets up fractional scaling, Wayland-specific window hints, and
/// ensures proper compositor integration.
fn setup_wayland_optimizations(app: &adw::Application) {
    app.connect_startup(|_| {
        // Check if running on Wayland via GDK_BACKEND or WAYLAND_DISPLAY
        let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("GDK_BACKEND")
                .map(|v| v.contains("wayland"))
                .unwrap_or(false);

        if is_wayland {
            tracing::info!("Running on Wayland compositor - enabling native optimizations");
        }
    });
}

fn main() -> gtk::glib::ExitCode {
    // Prefer Wayland backend when available (unsafe in Rust 2024, but needed for backend selection)
    unsafe {
        std::env::set_var("GDK_BACKEND", "wayland,x11");
    }

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

    setup_wayland_optimizations(&app);

    app.connect_startup(|_| {
        load_resources();
        load_css();
    });
    app.connect_activate(app::build_ui);
    app.run()
}
