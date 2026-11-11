//! The vault drawn as `git log --graph` draws history.

mod dag;
mod layout;

pub use dag::{Dag, Note, Source};
pub use layout::{layout, Line};
