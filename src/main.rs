mod app;
mod error;
mod state;
mod twitch;
mod ui;

use gtk::gio::prelude::{ApplicationExt, ApplicationExtManual};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    app.connect_activate(app::build_ui);
    app.run()
}
