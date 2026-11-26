//! The application: one Music window per session. Window frames, title
//! bars and window buttons come from the GTK theme (Elavo, or whatever is
//! active); the app follows the system light and dark setting the way Elavo
//! Files does, with dark the default look.

use gtk::prelude::*;
use gtk::{gdk, gio};

use crate::{accent, window};

pub const APP_ID: &str = "io.elavo.Music";

pub fn run() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        load_css();
        install_icon();
        follow_color_scheme();
        gtk::Window::set_default_icon_name(APP_ID);
        // The panel's global menu reads this; the window does not draw it.
        // It leaves out an app-named menu and Quit: the panel adds both.
        app.set_menubar(Some(&crate::menu::build()));
        crate::menu::install_accels(app);
    });
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        window::build(app).present();
    });
    app.run()
}

fn load_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let css = gtk::CssProvider::new();
    css.load_from_resource("/io/elavo/Music/resources/style.css");
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Music's own accent, for the current color scheme. It is replaced when
    // the scheme changes.
    let accent_css = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &accent_css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    let apply = move || {
        let dark =
            gtk::Settings::default().is_none_or(|s| s.is_gtk_application_prefer_dark_theme());
        accent_css.load_from_string(&accent::css(dark));
    };
    apply();
    if let Some(settings) = gtk::Settings::default() {
        settings.connect_notify_local(None, move |_, param| {
            if param.name() == "gtk-application-prefer-dark-theme" {
                apply();
            }
        });
    }
}

fn install_icon() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    gtk::IconTheme::for_display(&display).add_resource_path("/io/elavo/Music/icons");
}

/// Follow the color-scheme key GNOME and Elavo write, without depending on
/// either desktop being installed.
fn follow_color_scheme() {
    let Some(source) = gio::SettingsSchemaSource::default() else {
        return;
    };
    let Some(schema) = source.lookup("org.gnome.desktop.interface", true) else {
        return;
    };
    if !schema.has_key("color-scheme") {
        return;
    }
    let iface = gio::Settings::new("org.gnome.desktop.interface");
    let apply = |iface: &gio::Settings| {
        if let Some(gtk_settings) = gtk::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(
                iface.string("color-scheme") == "prefer-dark",
            );
        }
    };
    apply(&iface);
    iface.connect_changed(Some("color-scheme"), move |settings, _| apply(settings));
    std::mem::forget(iface);
}
