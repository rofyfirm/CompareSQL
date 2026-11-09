//! Selisih core: bandingkan dan sinkronkan schema + data MySQL/MariaDB.
//!
//! Alur: sumber (file dump atau DB live) -> model `Schema` + `RowSource`
//! -> normalisasi -> schema diff + data diff -> script sinkronisasi.

pub mod data_diff;
pub mod ddl;
pub mod dump;
pub mod engine;
pub mod error;
pub mod lexer;
pub mod model;
pub mod normalize;
pub mod progress;
pub mod render;
pub mod row;
pub mod schema_diff;
pub mod script;
pub mod value;

#[cfg(feature = "live")]
pub mod live;

pub use error::{Error, Result};
