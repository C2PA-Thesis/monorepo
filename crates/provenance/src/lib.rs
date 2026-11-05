//! Capture a photo, publish its left half with a crop proof and a location
//! proof in its C2PA manifest, and verify it as a reader.
//!
//! The two proofs never see each other. What binds them to one capture is the
//! receipt: the device signs the image fingerprint and the location envelope
//! together, and the reader checks both proofs against that receipt.
//!
//! A capture comes either from `demo`, which signs with the laptop's own
//! device key, or from a phone through the `serve` API, which signs on the
//! device before uploading. `publish` takes it from there either way.

pub mod attack;
pub mod capture;
pub mod demo;
pub mod image;
pub mod location;
pub mod manifest;
pub mod publish;
pub mod receipt;
pub mod serve;
pub mod verify;
mod workspace;

pub use workspace::{Step, Workspace};
