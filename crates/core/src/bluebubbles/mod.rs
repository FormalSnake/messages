//! The BlueBubbles server: REST under `/api/v1` with `?password=`, and a socket.io v4 event stream.

pub mod client;
pub mod map;
pub mod socket;

pub use client::{BlueBubblesOptions, BlueBubblesTransport};
