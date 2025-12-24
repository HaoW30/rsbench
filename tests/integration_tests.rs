//! Integration test runner
//!
//! This file serves as the entry point for integration tests.
//! It imports the common test utilities and integration test modules.

mod common;
mod integration;

// Re-export common utilities for integration tests
use common::*;
