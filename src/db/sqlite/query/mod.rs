//! SQL query definitions for SQLite.

pub(crate) mod entries;
pub(crate) mod feeds;
pub(crate) mod sync;
pub(crate) mod tags;

pub(crate) fn sql_placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
}
