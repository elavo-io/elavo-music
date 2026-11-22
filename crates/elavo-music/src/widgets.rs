//! Pieces shared by the views: the context they get, covers, status pages
//! and the empty and no-results states.

use std::path::PathBuf;
use std::rc::Rc;

use elavo_music_core::{ArtCache, Library, Track};
use gtk::pango;
use gtk::prelude::*;

/// What every view needs from the window.
#[derive(Clone)]
pub struct Ctx {
    pub lib: Rc<Library>,
    pub art: Rc<ArtCache>,
    /// Play these songs, starting at the index.
    pub play: Rc<dyn Fn(Vec<Track>, usize)>,
    /// Play these songs shuffled.
    pub shuffle_play: Rc<dyn Fn(Vec<Track>)>,
    /// Scan the music folder again.
    pub rescan: Rc<dyn Fn()>,
    pub music_dir: PathBuf,
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .css_classes(classes.iter().map(|c| c.to_string()).collect::<Vec<_>>())
        .build()
}

/// Album art at `size` pixels, with rounded corners; a note icon when there
/// is no picture.
pub fn cover(path: Option<PathBuf>, size: i32) -> gtk::Widget {
    let holder = gtk::Box::builder()
        .css_classes(["music-cover"])
        .overflow(gtk::Overflow::Hidden)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .build();
    holder.set_size_request(size, size);
    match path.and_then(|p| texture(&p, size)) {
        Some(texture) => {
            // An Image at an exact pixel size; a Picture would ask for the
            // file's full size.
            let image = gtk::Image::from_paintable(Some(&texture));
            image.set_pixel_size(size);
            holder.append(&image);
        }
        None => {
            let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
            icon.set_pixel_size((size / 2).max(16));
            icon.set_hexpand(true);
            icon.set_vexpand(true);
            icon.add_css_class("music-cover-icon");
            holder.append(&icon);
        }
    }
    holder.upcast()
}

thread_local! {
    static TEXTURES: std::cell::RefCell<std::collections::HashMap<(PathBuf, i32), gtk::gdk::Texture>> = Default::default();
}

/// The picture scaled to twice `size` (for sharp corners on a HiDPI screen),
/// kept so a grid scrolling past it again does not decode it again.
fn texture(path: &std::path::Path, size: i32) -> Option<gtk::gdk::Texture> {
    let key = (path.to_path_buf(), size);
    if let Some(found) = TEXTURES.with(|t| t.borrow().get(&key).cloned()) {
        return Some(found);
    }
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, size * 2, size * 2, true).ok()?;
    let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
    TEXTURES.with(|t| t.borrow_mut().insert(key, texture.clone()));
    Some(texture)
}

pub fn art_path(art: &ArtCache, key: &Option<String>) -> Option<PathBuf> {
    key.as_ref().map(|k| art.path(k))
}

type StatusAction<'a> = Option<(&'a str, Rc<dyn Fn()>)>;

pub fn status_page(icon: &str, title: &str, body: &str, action: StatusAction<'_>) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["music-status"])
        .build();
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(72);
    image.add_css_class("music-status-icon");
    page.append(&image);
    let heading = gtk::Label::builder()
        .label(title)
        .css_classes(["title-2"])
        .build();
    page.append(&heading);
    let text = gtk::Label::builder()
        .label(body)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(46)
        .css_classes(["dim-label"])
        .build();
    page.append(&text);
    if let Some((label, callback)) = action {
        let button = gtk::Button::builder()
            .label(label)
            .halign(gtk::Align::Center)
            .margin_top(6)
            .css_classes(["music-pill-button"])
            .build();
        button.connect_clicked(move |_| callback());
        page.append(&button);
    }
    page
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Content,
    EmptyLibrary,
    NoResults,
}

/// A view's content, with the two ways it can have nothing to show.
pub struct StatePages {
    pub stack: gtk::Stack,
}

impl StatePages {
    pub fn new(content: &impl IsA<gtk::Widget>, ctx: &Ctx) -> StatePages {
        let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        stack.add_named(content, Some("content"));
        let dir = ctx.music_dir.display().to_string();
        stack.add_named(
            &status_page(
                "folder-music-symbolic",
                "Add Music to Your Library",
                &format!("Music looks for songs in {dir}. Copy music there, then scan again."),
                Some(("Scan Music Folder", ctx.rescan.clone())),
            ),
            Some("empty"),
        );
        stack.add_named(
            &status_page(
                "system-search-symbolic",
                "No Results",
                "Try a different search, or clear the genre filter.",
                None,
            ),
            Some("no-results"),
        );
        StatePages { stack }
    }

    pub fn set(&self, state: State) {
        self.stack.set_visible_child_name(match state {
            State::Content => "content",
            State::EmptyLibrary => "empty",
            State::NoResults => "no-results",
        });
    }
}

/// `2001 · Ambient`, skipping what is blank.
pub fn year_genre(year: u32, genre: &str) -> String {
    let mut parts = Vec::new();
    if year > 0 {
        parts.push(year.to_string());
    }
    if !genre.is_empty() {
        parts.push(genre.to_string());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_genre_skips_blanks() {
        assert_eq!(year_genre(2001, "Ambient"), "2001 · Ambient");
        assert_eq!(year_genre(0, "Ambient"), "Ambient");
        assert_eq!(year_genre(1999, ""), "1999");
        assert_eq!(year_genre(0, ""), "");
    }
}
