//! RelichHelper local agent library.
//!
//! Phase 1 provides EE.log location ([`paths`]) and parsing/following
//! ([`eelog`]). Later phases add the reference-data sync, OCR, and the overlay
//! bridge.

pub mod eelog;
pub mod inventory;
pub mod paths;
pub mod refdata;
pub mod session;
