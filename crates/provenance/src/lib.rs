//! Capture a photo, publish its left half with a crop proof and a location
//! proof in its C2PA manifest, and verify it as a reader.
//!
//! The two proofs never see each other. What binds them to one capture is the
//! receipt: the device signs the image fingerprint and the location envelope
//! together, and the reader checks both proofs against that receipt.

pub mod attack;
pub mod capture;
pub mod demo;
pub mod location;
pub mod manifest;
pub mod verify;
mod workspace;

pub use workspace::{Step, Workspace};
