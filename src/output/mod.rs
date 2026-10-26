//! Command-line output: human-readable tables and JSON.

pub mod json;
pub mod table;

pub use table::{print_check_table, print_table};
