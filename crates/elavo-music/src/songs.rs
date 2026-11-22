//! The Songs table: title, time, artist, album, genre and plays, sortable
//! by any column. Double-click plays from that song.

use std::cmp::Ordering;

use elavo_music_core::util::clock;
use elavo_music_core::Track;
use gtk::glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::widgets::{label, Ctx, State, StatePages};

pub struct SongsView {
    pub root: gtk::Stack,
    pages: StatePages,
    store: gio::ListStore,
}

fn track_of(item: &glib::Object) -> Option<Track> {
    let boxed = item.downcast_ref::<BoxedAnyObject>()?;
    let track = boxed.borrow::<Track>().clone();
    Some(track)
}

fn text_column(
    title: &str,
    width: i32,
    expand: bool,
    numeric: bool,
    text: fn(&Track) -> String,
    cmp: fn(&Track, &Track) -> Ordering,
) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let cell = label("", &["music-cell"]);
        if numeric {
            cell.set_xalign(1.0);
        }
        item.set_child(Some(&cell));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(cell) = item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        if let Some(track) = item.item().and_then(|o| track_of(&o)) {
            cell.set_label(&text(&track));
        }
    });
    let sorter = gtk::CustomSorter::new(move |a, b| match (track_of(a), track_of(b)) {
        (Some(a), Some(b)) => cmp(&a, &b).into(),
        _ => gtk::Ordering::Equal,
    });
    let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    column.set_sorter(Some(&sorter));
    column.set_resizable(true);
    column.set_expand(expand);
    if width > 0 {
        column.set_fixed_width(width);
    }
    column
}

fn ci(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

impl SongsView {
    pub fn new(ctx: &Ctx) -> SongsView {
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let view = gtk::ColumnView::builder()
            .show_row_separators(false)
            .show_column_separators(false)
            .reorderable(false)
            .css_classes(["music-songs"])
            .build();
        view.append_column(&text_column(
            "Title",
            240,
            true,
            false,
            |t| t.title.clone(),
            |a, b| ci(&a.title, &b.title),
        ));
        view.append_column(&text_column(
            "Time",
            64,
            false,
            true,
            |t| clock(t.duration_ms),
            |a, b| a.duration_ms.cmp(&b.duration_ms),
        ));
        view.append_column(&text_column(
            "Artist",
            170,
            true,
            false,
            |t| t.artist.clone(),
            |a, b| ci(&a.artist, &b.artist),
        ));
        view.append_column(&text_column(
            "Album",
            170,
            true,
            false,
            |t| t.album.clone(),
            |a, b| ci(&a.album, &b.album),
        ));
        view.append_column(&text_column(
            "Genre",
            100,
            false,
            false,
            |t| t.genre.clone(),
            |a, b| ci(&a.genre, &b.genre),
        ));
        view.append_column(&text_column(
            "Plays",
            64,
            false,
            true,
            |t| {
                if t.plays == 0 {
                    String::new()
                } else {
                    t.plays.to_string()
                }
            },
            |a, b| a.plays.cmp(&b.plays),
        ));

        let sorted = gtk::SortListModel::new(Some(store.clone()), view.sorter());
        let selection = gtk::SingleSelection::new(Some(sorted.clone()));
        view.set_model(Some(&selection));

        let play = ctx.play.clone();
        view.connect_activate(move |_, position| {
            // Play what is on screen, in the order it is sorted, from the
            // song that was opened.
            let tracks: Vec<Track> = (0..sorted.n_items())
                .filter_map(|i| sorted.item(i).and_then(|o| track_of(&o)))
                .collect();
            play(tracks, position as usize);
        });

        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["music-scroll"])
            .build();
        let pages = StatePages::new(&scroller, ctx);
        SongsView {
            root: pages.stack.clone(),
            pages,
            store,
        }
    }

    pub fn set(&self, tracks: Vec<Track>, state: State) {
        self.store.remove_all();
        let items: Vec<BoxedAnyObject> = tracks.into_iter().map(BoxedAnyObject::new).collect();
        self.store.extend_from_slice(&items);
        self.pages.set(state);
    }
}
