//! `nmp-gallery-desktop` — component gallery for NMP desktop widgets.
//!
//! Run: `cargo run -p nmp-gallery-desktop`

mod bridge;
mod components;
mod gallery;

fn main() -> iced::Result {
    iced::application(gallery::GalleryApp::new, gallery::update, gallery::view)
        .subscription(gallery::subscription)
        .title("NMP Desktop Component Gallery")
        .run()
}
