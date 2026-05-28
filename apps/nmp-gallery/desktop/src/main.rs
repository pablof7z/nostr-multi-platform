//! `nmp-gallery-desktop` — component gallery for NMP desktop widgets.
//!
//! Run: `cargo run -p nmp-gallery-desktop`

mod components;
mod gallery;

use gallery::{update, view, GalleryApp};

fn main() -> iced::Result {
    iced::application(GalleryApp::new, update, view)
        .title("NMP Desktop Component Gallery")
        .run()
}
