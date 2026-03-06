pub mod compat;
pub mod discovery;
pub mod runner;

pub use discovery::{discover_tests, filter_tests, resolve_suite, DiscoveredTest};
pub use runner::run_test_suite;
