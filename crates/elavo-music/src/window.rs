//! The Music window: a sidebar column with the window buttons over it, a
//! per-view header with a filter button and a "Find in" field, the views,
//! and the floating player bar over the bottom of the content column.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elavo_music_core::settings::Settings;
use elavo_music_core::{paths, AlbumOrder, ArtCache, Filter, Library, TrackOrder};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::albums::AlbumsView;
use crate::artists::ArtistsView;
use crate::keys;
use crate::player::Player;
use crate::songs::SongsView;
use crate::widgets::{status_page, Ctx, State};

const SIDEBAR_WIDTH: i32 = 232;
const MIN_WIDTH: i32 = 800;
const MIN_HEIGHT: i32 = 520;

/// id, title, icon, "Find in" text. `None` for views with no find field.
const LIBRARY_VIEWS: &[(&str, &str, &str)] = &[
    ("recent", "Recently Added", "document-open-recent-symbolic"),
    ("artists", "Artists", "avatar-default-symbolic"),
    ("albums", "Albums", "media-optical-symbolic"),
    ("songs", "Songs", "audio-x-generic-symbolic"),
];

struct Music {
    ctx: Ctx,
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    title: gtk::Label,
    search: gtk::SearchEntry,
    filter_button: gtk::MenuButton,
    genre_list: gtk::ListBox,
    popover: gtk::Popover,
    sidebar: gtk::ListBox,
    scan_bar: gtk::Revealer,
    scan_label: gtk::Label,
    recent: Rc<AlbumsView>,
    albums: Rc<AlbumsView>,
    artists: Rc<ArtistsView>,
    songs: SongsView,
    view: RefCell<String>,
    filter: RefCell<Filter>,
    scanning: Cell<bool>,
    music_dir: PathBuf,
    player: Rc<Player>,
}

fn pill() -> gtk::Box {
    gtk::Box::builder()
        .css_classes(["music-pill-group"])
        .valign(gtk::Align::Center)
        .build()
}

fn sidebar_row(id: &str, title: &str, icon: &str) -> gtk::ListBoxRow {
    let row = gtk::Box::builder().spacing(10).build();
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("music-side-icon");
    row.append(&image);
    row.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    let row = gtk::ListBoxRow::builder().child(&row).build();
    row.set_widget_name(id);
    row
}

fn section_row(title: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["music-section-title"])
        .build();
    gtk::ListBoxRow::builder()
        .child(&label)
        .selectable(false)
        .activatable(false)
        .css_classes(["music-section"])
        .build()
}

pub fn build(app: &gtk::Application) -> gtk::ApplicationWindow {
    let settings = Settings::load(&paths::settings_file(), paths::default_music_dir());
    let lib = Rc::new(Library::open(&paths::database()).unwrap_or_else(|err| {
        eprintln!("elavo-music: cannot open the library database ({err}); using a temporary one");
        Library::open_in_memory().expect("in-memory database")
    }));
    let art = Rc::new(ArtCache::new(paths::art_dir()));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Music")
        .default_width(1180)
        .default_height(760)
        .show_menubar(false)
        .build();
    // Add the class instead of setting css-classes, which would drop GTK's
    // own "background" class and leave the window see-through.
    window.add_css_class("music-window");
    window.set_size_request(MIN_WIDTH, MIN_HEIGHT);
    // The title band lives in each column, so the window has no title bar
    // of its own. An empty, hidden widget keeps GTK from adding the default one.
    let hidden_titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hidden_titlebar.set_visible(false);
    window.set_titlebar(Some(&hidden_titlebar));

    // The player and the callbacks the views use to reach it. The window
    // exists by the time any of them runs.
    let player = {
        let lib = lib.clone();
        Player::new(art.clone(), move |id| {
            let _ = lib.record_play(id);
        })
    };
    let music_cell: Rc<RefCell<Option<std::rc::Weak<Music>>>> = Rc::default();
    let ctx = Ctx {
        lib: lib.clone(),
        art: art.clone(),
        play: {
            let player = player.clone();
            Rc::new(move |tracks, start| player.play_tracks(tracks, start))
        },
        shuffle_play: {
            let player = player.clone();
            Rc::new(move |tracks| player.shuffle_tracks(tracks))
        },
        rescan: {
            let cell = music_cell.clone();
            Rc::new(move || {
                if let Some(music) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    music.rescan();
                }
            })
        },
        music_dir: settings.music_folder.clone(),
    };

    // Title bar: window buttons over the sidebar, the view header over the content.
    let band = gtk::Box::builder()
        .width_request(SIDEBAR_WIDTH)
        .css_classes(["music-sidebar-band"])
        .build();
    let start_controls = gtk::WindowControls::new(gtk::PackType::Start);
    start_controls.set_valign(gtk::Align::Center);
    band.append(&start_controls);

    let title = gtk::Label::builder()
        .label("Songs")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["music-view-title"])
        .build();
    let genre_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["music-genre-list"])
        .build();
    let genre_scroll = gtk::ScrolledWindow::builder()
        .child(&genre_list)
        .min_content_height(120)
        .max_content_height(320)
        .min_content_width(200)
        .propagate_natural_height(true)
        .build();
    let popover_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    popover_box.append(
        &gtk::Label::builder()
            .label("Filter by Genre")
            .xalign(0.0)
            .css_classes(["music-section-title"])
            .margin_start(6)
            .build(),
    );
    popover_box.append(&genre_scroll);
    let popover = gtk::Popover::builder()
        .child(&popover_box)
        .css_classes(["music-menu"])
        .build();
    let filter_button = gtk::MenuButton::builder()
        .icon_name("music-filter-symbolic")
        .tooltip_text("Filter")
        .popover(&popover)
        .build();
    let filter_pill = pill();
    filter_pill.append(&filter_button);
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Find in Songs")
        .search_delay(180)
        .width_chars(20)
        .valign(gtk::Align::Center)
        .css_classes(["music-find"])
        .build();
    let toolbar = gtk::Box::builder()
        .spacing(10)
        .hexpand(true)
        .css_classes(["music-toolbar"])
        .build();
    toolbar.append(&title);
    toolbar.append(&filter_pill);
    toolbar.append(&search);
    let end_controls = gtk::WindowControls::new(gtk::PackType::End);
    end_controls.set_valign(gtk::Align::Center);
    toolbar.append(&end_controls);

    // Sidebar.
    let sidebar = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["music-side-list"])
        .build();
    sidebar.append(&section_row("Library"));
    for (id, name, icon) in LIBRARY_VIEWS {
        sidebar.append(&sidebar_row(id, name, icon));
    }
    sidebar.append(&section_row("Playlists"));
    sidebar.append(&sidebar_row(
        "playlists",
        "All Playlists",
        "view-list-symbolic",
    ));
    sidebar.append(&section_row("Devices"));
    sidebar.append(&sidebar_row(
        "devices",
        "No Devices",
        "multimedia-player-symbolic",
    ));
    let side_scroll = gtk::ScrolledWindow::builder()
        .child(&sidebar)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    // Views.
    let recent = AlbumsView::new(&ctx, "Recently Added");
    let albums = AlbumsView::new(&ctx, "Albums");
    let artists = ArtistsView::new(&ctx);
    let songs = SongsView::new(&ctx);
    let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    stack.add_named(&recent.root, Some("recent"));
    stack.add_named(&artists.root, Some("artists"));
    stack.add_named(&albums.root, Some("albums"));
    stack.add_named(&songs.root, Some("songs"));
    stack.add_named(
        &status_page(
            "view-list-symbolic",
            "No Playlists",
            "Playlists you make will appear here. Playlists arrive in a later version.",
            None,
        ),
        Some("playlists"),
    );
    stack.add_named(
        &status_page(
            "multimedia-player-symbolic",
            "No Devices",
            "Connect an iPod or another player and it will appear here. Syncing arrives in a later version.",
            None,
        ),
        Some("devices"),
    );

    let scan_label = gtk::Label::builder()
        .label("Scanning…")
        .css_classes(["music-scan-label"])
        .build();
    let scan_bar = gtk::Revealer::builder()
        .child(&scan_label)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .build();

    let overlay = gtk::Overlay::builder()
        .child(&stack)
        .hexpand(true)
        .vexpand(true)
        .overflow(gtk::Overflow::Hidden)
        .build();
    overlay.add_overlay(&scan_bar);
    // Add the pill itself as the overlay, not a full-size holder with
    // can_target(false). GTK's pick() does not descend into a widget
    // that cannot be a target, so that holder made the play button
    // unclickable even though the child asked to receive events.
    let pill = player.widget();
    pill.set_valign(gtk::Align::End);
    pill.set_halign(gtk::Align::Center);
    pill.set_margin_bottom(18);
    pill.set_can_target(true);
    overlay.add_overlay(pill);

    // One sidebar column and one content column, so the window buttons
    // sit over the sidebar and the two sides cannot drift apart or overlap.
    let sidebar_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(SIDEBAR_WIDTH)
        .hexpand(false)
        .css_classes(["music-sidebar"])
        .build();
    sidebar_col.append(&gtk::WindowHandle::builder().child(&band).build());
    sidebar_col.append(&side_scroll);

    let content_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    content_col.append(&gtk::WindowHandle::builder().child(&toolbar).build());
    content_col.append(&overlay);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&sidebar_col);
    body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    body.append(&content_col);
    window.set_child(Some(&body));

    let music = Rc::new(Music {
        ctx,
        window: window.clone(),
        stack,
        title,
        search,
        filter_button,
        genre_list,
        popover,
        sidebar,
        scan_bar,
        scan_label,
        recent,
        albums,
        artists,
        songs,
        view: RefCell::new("songs".into()),
        filter: RefCell::new(Filter::default()),
        scanning: Cell::new(false),
        music_dir: settings.music_folder.clone(),
        player,
    });
    *music_cell.borrow_mut() = Some(Rc::downgrade(&music));
    music.wire();

    // Keep the floating player bar inside the content column, shrinking
    // or hiding its secondary parts before it would overflow.
    let hooked = Rc::new(Cell::new(false));
    window.connect_realize({
        let player = music.player.clone();
        let overlay = overlay.clone();
        move |window| {
            if hooked.replace(true) {
                return;
            }
            let Some(surface) = window.surface() else {
                return;
            };
            let player = player.clone();
            let overlay = overlay.clone();
            surface.connect_layout(move |_, _, _| {
                let width = overlay.width();
                if width > 0 {
                    player.adapt_to_width(width);
                }
            });
        }
    });

    // Where to start: the last view would be remembered later; for now
    // Songs, or the one a screenshot asks for.
    let start = std::env::var("ELAVO_MUSIC_VIEW").unwrap_or_else(|_| "songs".into());
    music.select_view(&start);
    music.rescan();
    window.connect_close_request({
        let music = music.clone();
        move |_| {
            // Keep the Music alive until the window goes.
            let _ = &music;
            glib::Propagation::Proceed
        }
    });
    window
}

impl Music {
    fn wire(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.sidebar.connect_row_selected({
            let this = this.clone();
            move |_, row| {
                if let (Some(music), Some(row)) = (this.upgrade(), row) {
                    let id = row.widget_name().to_string();
                    music.show_view(&id);
                }
            }
        });
        self.search.connect_search_changed({
            let this = this.clone();
            move |entry| {
                if let Some(music) = this.upgrade() {
                    // GTK also reports this when nothing was typed; reloading
                    // then would close an open album page.
                    let text = entry.text().trim().to_string();
                    if music.filter.borrow().text != text {
                        music.filter.borrow_mut().text = text;
                        music.refresh();
                    }
                }
            }
        });
        self.popover.connect_show({
            let this = this.clone();
            move |_| {
                if let Some(music) = this.upgrade() {
                    music.fill_genres();
                }
            }
        });
        self.genre_list.connect_row_activated({
            let this = this.clone();
            move |_, row| {
                if let Some(music) = this.upgrade() {
                    let name = row.widget_name().to_string();
                    music.filter.borrow_mut().genre =
                        if name.is_empty() { None } else { Some(name) };
                    music.popover.popdown();
                    music.refresh();
                }
            }
        });
        // Ctrl+F goes to Find.
        let find = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>f"),
            Some(gtk::CallbackAction::new({
                let search = self.search.clone();
                move |_, _| {
                    search.grab_focus();
                    glib::Propagation::Stop
                }
            })),
        );
        let controller = gtk::ShortcutController::new();
        controller.set_scope(gtk::ShortcutScope::Global);
        controller.add_shortcut(find);
        self.window.add_controller(controller);

        // Space and the audio keys are handled here, in capture, so they
        // pause or skip instead of activating a row or leaving the play
        // button stuck after a Space press on the button itself. Typematic
        // repeats are ignored until the key is released (gdk4 0.11 has no
        // KeyEvent::is_repeat binding).
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let held = Rc::new(Cell::new(None::<gtk::gdk::Key>));
        keys.connect_key_pressed({
            let window = self.window.clone();
            let player = self.player.clone();
            let held = held.clone();
            move |_, key, _, _| {
                let is_repeat = held.get() == Some(key);
                let editable = gtk::prelude::GtkWindowExt::focus(&window)
                    .is_some_and(|w| keys::focus_is_editable(&w));
                match keys::media_key_action(key, editable, is_repeat) {
                    Some(action) => {
                        held.set(Some(key));
                        player.handle_media(action);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                }
            }
        });
        keys.connect_key_released({
            let held = held.clone();
            move |_, key, _, _| {
                if held.get() == Some(key) {
                    held.set(None);
                }
            }
        });
        self.window.add_controller(keys);
    }

    fn select_view(&self, id: &str) {
        let mut index = 0;
        while let Some(row) = self.sidebar.row_at_index(index) {
            if row.widget_name() == id {
                self.sidebar.select_row(Some(&row));
                return;
            }
            index += 1;
        }
    }

    fn show_view(&self, id: &str) {
        *self.view.borrow_mut() = id.to_string();
        let (title, find) = match id {
            "recent" => ("Recently Added", Some("Find in Recently Added")),
            "artists" => ("Artists", Some("Find in Artists")),
            "albums" => ("Albums", Some("Find in Albums")),
            "songs" => ("Songs", Some("Find in Songs")),
            "playlists" => ("Playlists", None),
            _ => ("Devices", None),
        };
        self.title.set_label(title);
        if let Some(parent) = self.filter_button.parent() {
            parent.set_visible(find.is_some());
        }
        self.search.set_visible(find.is_some());
        if let Some(find) = find {
            self.search.set_placeholder_text(Some(find));
        }
        self.stack.set_visible_child_name(id);
        self.refresh();
    }

    fn fill_genres(&self) {
        while let Some(row) = self.genre_list.first_child() {
            self.genre_list.remove(&row);
        }
        let current = self.filter.borrow().genre.clone();
        let mut entries = vec![(String::new(), "All Genres".to_string())];
        entries.extend(
            self.ctx
                .lib
                .genres()
                .unwrap_or_default()
                .into_iter()
                .map(|g| (g.clone(), g)),
        );
        for (value, text) in entries {
            let active = current.as_deref().unwrap_or("") == value;
            let line = gtk::Box::builder().spacing(8).build();
            line.append(
                &gtk::Label::builder()
                    .label(&text)
                    .xalign(0.0)
                    .hexpand(true)
                    .build(),
            );
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_opacity(if active { 1.0 } else { 0.0 });
            check.add_css_class("music-accent");
            line.append(&check);
            let row = gtk::ListBoxRow::builder().child(&line).build();
            row.set_widget_name(&value);
            self.genre_list.append(&row);
        }
    }

    /// Reload the visible view from the database.
    fn refresh(&self) {
        let filter = self.filter.borrow().clone();
        let total = self.ctx.lib.count().unwrap_or(0);
        let lib = &self.ctx.lib;
        let state = |shown: usize| {
            if total == 0 {
                State::EmptyLibrary
            } else if shown == 0 {
                State::NoResults
            } else {
                State::Content
            }
        };
        if filter.genre.is_some() {
            self.filter_button.add_css_class("active");
        } else {
            self.filter_button.remove_css_class("active");
        }
        match self.view.borrow().as_str() {
            "recent" => {
                let list = lib
                    .albums(&filter, AlbumOrder::RecentlyAdded, Some(60))
                    .unwrap_or_default();
                let s = state(list.len());
                self.recent.set(list, s);
            }
            "albums" => {
                let list = lib
                    .albums(&filter, AlbumOrder::Artist, None)
                    .unwrap_or_default();
                let s = state(list.len());
                self.albums.set(list, s);
            }
            "artists" => {
                let list = lib.artists(&filter).unwrap_or_default();
                let s = state(list.len());
                self.artists.set(list, &filter, s);
            }
            "songs" => {
                let list = lib.tracks(&filter, TrackOrder::Library).unwrap_or_default();
                let s = state(list.len());
                self.songs.set(list, s);
            }
            _ => {}
        }
    }

    /// Scan the music folder on a worker thread, then reload.
    fn rescan(self: &Rc<Self>) {
        if self.scanning.replace(true) {
            return;
        }
        let progress = Arc::new(AtomicUsize::new(0));
        let folder = self.music_dir.clone();
        let handle = gio::spawn_blocking({
            let progress = progress.clone();
            move || {
                let lib = Library::open(&paths::database())?;
                let art = ArtCache::new(paths::art_dir());
                elavo_music_core::scan::scan_folder(&lib, &folder, &art, |n| {
                    progress.store(n, Ordering::Relaxed)
                })
            }
        });
        self.scan_bar.set_reveal_child(true);
        let finished = Rc::new(Cell::new(false));
        {
            let finished = finished.clone();
            let label = self.scan_label.clone();
            glib::timeout_add_local(Duration::from_millis(150), move || {
                if finished.get() {
                    return glib::ControlFlow::Break;
                }
                label.set_label(&format!(
                    "Scanning… {} files",
                    progress.load(Ordering::Relaxed)
                ));
                glib::ControlFlow::Continue
            });
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = handle.await;
            finished.set(true);
            this.scanning.set(false);
            match result {
                Ok(Ok(report)) => {
                    if !report.failed.is_empty() {
                        eprintln!(
                            "elavo-music: {} files could not be read",
                            report.failed.len()
                        );
                    }
                }
                Ok(Err(err)) => eprintln!("elavo-music: scan failed: {err}"),
                Err(_) => eprintln!("elavo-music: scan stopped unexpectedly"),
            }
            this.scan_bar.set_reveal_child(false);
            this.refresh();
            this.demo();
        });
    }

    /// Screenshot hooks, set from the environment by `scripts/nested-session.sh`:
    /// ELAVO_MUSIC_OPEN_ALBUM=1 opens the first album, ELAVO_MUSIC_PLAY=N
    /// starts the Nth song of the view, and ELAVO_MUSIC_PROGRESS_MS=N then
    /// seeks into it. They run once, after the first scan.
    fn demo(&self) {
        thread_local! { static DONE: Cell<bool> = const { Cell::new(false) }; }
        if DONE.with(|d| d.replace(true)) {
            return;
        }
        if std::env::var("ELAVO_MUSIC_OPEN_ALBUM").is_ok() {
            let view = self.view.borrow().clone();
            let target = if view == "recent" {
                &self.recent
            } else {
                &self.albums
            };
            if let Some(album) = target.first() {
                target.open(&album);
            }
        }
        if let Some(n) = std::env::var("ELAVO_MUSIC_PLAY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            let tracks = self
                .ctx
                .lib
                .tracks(&Filter::default(), TrackOrder::Library)
                .unwrap_or_default();
            (self.ctx.play)(tracks, n);
            if let Some(ms) = std::env::var("ELAVO_MUSIC_PROGRESS_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
            {
                self.player.seek_ms_for_demo(ms);
            }
        }
        if let Ok(script) = std::env::var("ELAVO_MUSIC_THEN") {
            if !script.is_empty() {
                self.player.run_then_script(&script);
            }
        }
    }
}
