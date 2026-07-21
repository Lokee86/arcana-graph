mod error;
mod reader;
mod writer;

#[cfg(test)]
mod corruption_tests;
#[cfg(test)]
mod tests;

pub use error::SqliteError;
pub use reader::SqliteGraph;
pub use writer::{SqliteWriteSummary, write_sqlite};

pub(super) const APPLICATION_ID: i64 = 1_095_911_217;
pub(super) const FORMAT_VERSION: i64 = 1;
