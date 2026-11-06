//! Foyer patch engine.
//!
//! A game build is described by a [`Manifest`]: every file in the build
//! directory, its size, and its BLAKE3 hash. Comparing two manifests yields a
//! [`PatchPlan`] that tells a client which files to download, which to delete,
//! and which it already has.
//!
//! This crate is shared by the CLI (which creates manifests when a developer
//! pushes a build) and the desktop client (which applies plans to update an
//! installed game). Chunk-level deltas will build on top of these types.

mod manifest;
mod plan;

pub use manifest::{FileEntry, Manifest, ManifestError};
pub use plan::{PatchPlan, diff};
