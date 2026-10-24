//! The project guides, included from the `docs/` folder so that they are
//! part of the generated documentation. The same files are readable on
//! GitHub.

#![allow(rustdoc::broken_intra_doc_links)]

/// User guide: what logirev does and how to use it (`README.md`).
#[doc = include_str!("../README.md")]
pub mod readme {}

/// Every setting of the configuration file (`docs/CONFIGURATION.md`).
#[doc = include_str!("../docs/CONFIGURATION.md")]
pub mod configuration {}

/// How logirev talks to the devices, in plain words (`docs/PROTOCOL.md`).
#[doc = include_str!("../docs/PROTOCOL.md")]
pub mod protocol {}

/// Building, code layout and releasing (`docs/DEVELOPMENT.md`).
#[doc = include_str!("../docs/DEVELOPMENT.md")]
pub mod development {}

/// The checklist used before each release (`docs/TESTING.md`).
#[doc = include_str!("../docs/TESTING.md")]
pub mod testing {}

/// What changed in each version (`CHANGELOG.md`).
#[doc = include_str!("../CHANGELOG.md")]
pub mod changelog {}
