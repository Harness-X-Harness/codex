mod extension;
mod history;
mod output;
mod schema;
mod tool;

pub use extension::install;

#[cfg(test)]
#[path = "schema_bounds_tests.rs"]
mod schema_bounds_tests;
