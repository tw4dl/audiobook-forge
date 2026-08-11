//! Core library for the `kokoro-book` CLI.

#![forbid(unsafe_code)]

pub mod audio;
pub mod book;
pub mod chunk;
pub mod cli;
pub mod input;
pub mod m4b;
pub mod model;
pub mod narration;
mod phoneme;
pub mod pipeline;
pub mod sidecar;
pub mod synthesis;
pub mod timeline;
pub mod tts;
mod vocab;
pub mod voice;
pub mod worker;
