//! Album views: a grid of artwork that opens into an album page, and the
//! album block that the Artists view reuses.

use std::cell::RefCell;
use std::rc::Rc;

use elavo_music_core::util::{clock, length_words};
use elavo_music_core::{Album, Track};
use gtk::glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::widgets::{art_path, cover, label, year_genre, Ctx, State, StatePages};

const TILE: i32 = 176;

/// One album: its cover, title, artist, and its songs as rows. With
/// `buttons` it also has Play and Shuffle, as on the album page.
pub fn album_block(
    ctx: &Ctx,
    album: &Album,
    tracks: Vec<Track>,
    cover_size: i32,
    buttons: bool,
) -> gtk::Box {
    let block = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["music-album-block"])
        .build();

    let head = gtk::Box::builder().spacing(20).build();
    head.append(&cover(art_path(&ctx.art, &album.art), cover_size));
    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .valign(gtk::Align::End)
        .hexpand(true)
        .build();
    let title = label(&album.title, &["music-album-title"]);
    title.set_wrap(true);
    info.append(&title);
    info.append(&label(
        &album.album_artist,
        &["music-album-artist", "music-accent"],
    ));
    let meta = year_genre(album.year, &album.genre);
    if !meta.is_empty() {
        info.append(&label(&meta.to_uppercase(), &["music-meta"]));
    }
    let count = if album.track_count == 1 {
        "1 song".to_string()
    } else {
        format!("{} songs", album.track_count)
    };
    info.append(&label(
        &format!("{count}, {}", length_words(album.duration_ms)),
        &["music-meta"],
    ));
    if buttons {
        let row = gtk::Box::builder().spacing(10).margin_top(10).build();
        let play = icon_label_button("media-playback-start-symbolic", "Play");
        let shuffle = icon_label_button("media-playlist-shuffle-symbolic", "Shuffle");
        let (play_fn, list) = (ctx.play.clone(), tracks.clone());
        play.connect_clicked(move |_| play_fn(list.clone(), 0));
        let (shuffle_fn, list) = (ctx.shuffle_play.clone(), tracks.clone());
        shuffle.connect_clicked(move |_| shuffle_fn(list.clone()));
        row.append(&play);
        row.append(&shuffle);
        info.append(&row);
    }
    head.append(&info);
    block.append(&head);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(false)
        .css_classes(["music-tracklist"])
        .build();
    for track in &tracks {
        let row = gtk::Box::builder().spacing(12).build();
        let number = if track.track_no > 0 {
            track.track_no.to_string()
        } else {
            String::new()
        };
        let n = label(&number, &["music-tracknum"]);
        n.set_xalign(1.0);
        n.set_width_chars(3);
        row.append(&n);
        let t = label(&track.title, &["music-cell"]);
        t.set_hexpand(true);
        row.append(&t);
        // A different artist on a compilation or a feature.
        if track.artist != album.album_artist {
            row.append(&label(&track.artist, &["music-meta"]));
        }
        row.append(&label(&clock(track.duration_ms), &["music-tracknum"]));
        list.append(&row);
    }
    let (play_fn, all) = (ctx.play.clone(), tracks);
    list.connect_row_activated(move |_, row| play_fn(all.clone(), row.index() as usize));
    block.append(&list);
    block
}

fn icon_label_button(icon: &str, text: &str) -> gtk::Button {
    let content = gtk::Box::builder().spacing(8).build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(text)));
    gtk::Button::builder()
        .child(&content)
        .css_classes(["music-pill-button", "music-accent-button"])
        .build()
}

pub struct AlbumsView {
    pub root: gtk::Stack,
    ctx: Ctx,
    pages: StatePages,
    store: gio::ListStore,
    detail: gtk::Box,
    albums: RefCell<Vec<Album>>,
    name: String,
}

fn album_of(item: &glib::Object) -> Option<Album> {
    let boxed = item.downcast_ref::<BoxedAnyObject>()?;
    let album = boxed.borrow::<Album>().clone();
    Some(album)
}

impl AlbumsView {
    /// `name` is what the back button says: "Albums" or "Recently Added".
    pub fn new(ctx: &Ctx, name: &str) -> Rc<AlbumsView> {
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        {
            let art = ctx.art.clone();
            factory.connect_setup(|_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let tile = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(2)
                    .halign(gtk::Align::Center)
                    .css_classes(["music-tile"])
                    .build();
                let holder = gtk::Box::builder().halign(gtk::Align::Start).build();
                holder.set_size_request(TILE, TILE);
                tile.set_width_request(TILE);
                tile.append(&holder);
                let title = label("", &["music-tile-title"]);
                title.set_margin_top(8);
                title.set_max_width_chars(1);
                title.set_width_chars(1);
                tile.append(&title);
                let artist = label("", &["music-tile-artist"]);
                artist.set_max_width_chars(1);
                artist.set_width_chars(1);
                tile.append(&artist);
                item.set_child(Some(&tile));
            });
            factory.connect_bind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let Some(tile) = item.child().and_downcast::<gtk::Box>() else {
                    return;
                };
                let Some(album) = item.item().and_then(|o| album_of(&o)) else {
                    return;
                };
                let holder = tile.first_child().and_downcast::<gtk::Box>().unwrap();
                while let Some(old) = holder.first_child() {
                    holder.remove(&old);
                }
                holder.append(&cover(art_path(&art, &album.art), TILE));
                let title = holder.next_sibling().and_downcast::<gtk::Label>().unwrap();
                title.set_label(&album.title);
                let artist = title.next_sibling().and_downcast::<gtk::Label>().unwrap();
                artist.set_label(&album.album_artist);
            });
        }
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let grid = gtk::GridView::builder()
            .model(&selection)
            .factory(&factory)
            .min_columns(2)
            .max_columns(12)
            .single_click_activate(true)
            .css_classes(["music-grid"])
            .build();
        let grid_scroll = gtk::ScrolledWindow::builder()
            .child(&grid)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["music-scroll"])
            .build();
        let pages = StatePages::new(&grid_scroll, ctx);

        let detail = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .css_classes(["music-detail"])
            .build();
        let detail_scroll = gtk::ScrolledWindow::builder()
            .child(&detail)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["music-scroll"])
            .build();

        let root = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build();
        root.add_named(&pages.stack, Some("grid"));
        root.add_named(&detail_scroll, Some("detail"));

        let view = Rc::new(AlbumsView {
            root,
            ctx: ctx.clone(),
            pages,
            store,
            detail,
            albums: RefCell::new(Vec::new()),
            name: name.to_string(),
        });
        let weak = Rc::downgrade(&view);
        grid.connect_activate(move |_, position| {
            if let Some(view) = weak.upgrade() {
                let album = view.albums.borrow().get(position as usize).cloned();
                if let Some(album) = album {
                    view.open(&album);
                }
            }
        });
        view
    }

    pub fn set(&self, albums: Vec<Album>, state: State) {
        self.show_grid();
        self.store.remove_all();
        let items: Vec<BoxedAnyObject> = albums.iter().cloned().map(BoxedAnyObject::new).collect();
        self.store.extend_from_slice(&items);
        *self.albums.borrow_mut() = albums;
        self.pages.set(state);
    }

    pub fn show_grid(&self) {
        self.root.set_visible_child_name("grid");
    }

    pub fn open(&self, album: &Album) {
        let tracks = self
            .ctx
            .lib
            .album_tracks(&album.album_artist, &album.title)
            .unwrap_or_default();
        while let Some(old) = self.detail.first_child() {
            self.detail.remove(&old);
        }
        let back = gtk::Button::builder()
            .label(format!("‹  {}", self.name))
            .halign(gtk::Align::Start)
            .css_classes(["music-back"])
            .build();
        let root = self.root.clone();
        back.connect_clicked(move |_| root.set_visible_child_name("grid"));
        self.detail.append(&back);
        self.detail
            .append(&album_block(&self.ctx, album, tracks, 232, true));
        self.root.set_visible_child_name("detail");
    }

    /// The first album shown, for screenshots.
    pub fn first(&self) -> Option<Album> {
        self.albums.borrow().first().cloned()
    }
}
