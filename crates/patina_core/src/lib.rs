//! Patina project model.
//!
//! This crate holds plain data only: no GPU and no UI code, so it can be used
//! by the editor, the headless CLI and tests alike.

mod channel;
mod image;
mod project;

pub use channel::Channel;
pub use image::{Image, ImageError};
pub use project::{NormalFormat, Project, TextureSet};
