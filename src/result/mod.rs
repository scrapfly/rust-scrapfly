//! Strongly-typed result objects for every Scrapfly endpoint.
//!
//! All result structs derive `Deserialize`; lossy fields (e.g. polymorphic
//! JS scenario output, arbitrary HTTP headers) are kept as `serde_json::Value`.

pub mod account;
pub mod crawler;
pub mod extraction;
pub mod scrape;
pub mod screenshot;
