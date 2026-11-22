//! The parts of Elavo Music that do not need a window: the library
//! database, the folder scanner, tag and artwork reading, the play queue,
//! and the settings model.

pub mod art;
pub mod db;
pub mod model;
pub mod paths;
pub mod queue;
pub mod scan;
pub mod settings;
pub mod tags;
pub mod util;

pub use art::ArtCache;
pub use db::{AlbumOrder, Library, TrackOrder};
pub use model::{Album, Artist, Filter, Track};
pub use queue::{Queue, Repeat};
