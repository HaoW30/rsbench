//! Common test utilities for RSBench
//!
//! This module provides shared testing infrastructure including:
//! - Mock implementations of core traits
//! - Test configuration builders
//! - Custom assertions for metrics and results

pub mod mock_driver;
pub mod mock_workload;
pub mod test_config;
pub mod assertions;

// Re-export commonly used items
pub use mock_driver::{MockDriver, MockConnection};
pub use mock_workload::MockWorkload;
pub use test_config::TestConfigBuilder;
pub use assertions::*;
