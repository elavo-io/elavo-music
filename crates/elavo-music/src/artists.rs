//! The Artists split view: the artist list on the left, that artist's
//! albums on the right.

use std::cell::RefCell;
use std::rc::Rc;

use elavo_music_core::{AlbumOrder, Artist, Filter};
use gtk::prelude::*;

use crate::albums::album_block;
use crate::widgets::{label, Ctx, State, StatePages};

pub struct ArtistsView {
    pub root: gtk::Stack,
    ctx: Ctx,
    pages: StatePages,
    list: gtk::ListBox,
    detail: gtk::Box,
    artists: RefCell<Vec<Artist>>,
    /// The "Find in" text, so the right side shows only matching albums.
    filter: RefCell<Filter>,
}

impl ArtistsView {
    pub fn new(ctx: &Ctx) -> Rc<ArtistsView> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["music-artist-list"])
            .build();
        let left = gtk::ScrolledWindow::builder()
            .child(&list)
            .width_request(260)
            .vexpand(true)
            .css_classes(["music-scroll", "music-artist-pane"])
            .build();
        let detail = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .css_classes(["music-detail"])
            .build();
        let right = gtk::ScrolledWindow::builder()
            .child(&detail)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["music-scroll"])
            .build();
        let paned = gtk::Paned::builder()
            .start_child(&left)
            .end_child(&right)
            .position(260)
            .shrink_start_child(false)
            .resize_start_child(false)
            .wide_handle(false)
            .build();
        let pages = StatePages::new(&paned, ctx);
        let view = Rc::new(ArtistsView {
            root: pages.stack.clone(),
            ctx: ctx.clone(),
            pages,
            list,
            detail,
            artists: RefCell::new(Vec::new()),
            filter: RefCell::new(Filter::default()),
        });
        let weak = Rc::downgrade(&view);
        view.list.connect_row_selected(move |_, row| {
            if let (Some(view), Some(row)) = (weak.upgrade(), row) {
                view.show_artist(row.index() as usize);
            }
        });
        view
    }

    pub fn set(&self, artists: Vec<Artist>, filter: &Filter, state: State) {
        *self.filter.borrow_mut() = filter.clone();
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for artist in &artists {
            let cell = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(1)
                .build();
            cell.append(&label(&artist.name, &["music-artist-name"]));
            let albums = if artist.album_count == 1 {
                "1 album".to_string()
            } else {
                format!("{} albums", artist.album_count)
            };
            cell.append(&label(&albums, &["music-meta"]));
            self.list.append(&cell);
        }
        *self.artists.borrow_mut() = artists;
        self.pages.set(state);
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        } else {
            self.clear_detail();
        }
    }

    fn clear_detail(&self) {
        while let Some(old) = self.detail.first_child() {
            self.detail.remove(&old);
        }
    }

    fn show_artist(&self, index: usize) {
        let Some(artist) = self.artists.borrow().get(index).cloned() else {
            return;
        };
        self.clear_detail();
        self.detail
            .append(&label(&artist.name, &["music-artist-heading"]));
        let filter = Filter {
            album_artist: Some(artist.name.clone()),
            ..self.filter.borrow().clone()
        };
        let albums = self
            .ctx
            .lib
            .albums(&filter, AlbumOrder::Title, None)
            .unwrap_or_default();
        for album in albums {
            let tracks = self
                .ctx
                .lib
                .album_tracks(&album.album_artist, &album.title)
                .unwrap_or_default();
            self.detail
                .append(&album_block(&self.ctx, &album, tracks, 128, false));
        }
    }
}
