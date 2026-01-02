# RSBench Tests Directory

This directory contains integration tests, property tests, and test utilities for RSBench.

## Quick Status

See **`QUICK_STATUS.txt`** for at-a-glance test status or **`TEST_STATUS.md`** for detailed information.

## Test Organization

### Integration Tests (`tests/*.rs`)
End-to-end tests that verify module interactions:
- `event_integration_test.rs` - Event module integration (✅ 6/6 passing)
- `scenario_integration_test.rs` - Scenario execution flows (needs update)
- `test_infrastructure.rs` - Infrastructure setup (needs update)
- `test_declarative_workload.rs` - Declarative workload end-to-end
- Other integration tests (compilation errors - need API updates)

### Property Tests (`tests/property/*.rs`)
Property-based tests using `proptest` for invariant checking:
- `determinism_test.rs` - Verify deterministic operation generation
- `metrics_invariants_test.rs` - Verify metrics consistency
- Currently disabled due to compilation errors (need updates)

### Test Utilities (`tests/common/*.rs`)
Shared test infrastructure:
- `mock_driver.rs` - Mock database driver for testing
- `mock_pool.rs` - Mock connection pool
- `mock_workload.rs` - Mock workload implementations
- `test_config.rs` - Test configuration builders
- `assertions.rs` - Custom test assertions

## Running Tests

### All Unit Tests (in `src/`)
```bash
cargo test --lib                # All unit tests (204/205 passing)
cargo test --lib event          # Event module only (38/38 passing)
cargo test --lib runtime        # Runtime module only (14/14 passing)
```

### Integration Tests
```bash
# Event integration (working)
cargo test --test event_integration_test   # 6/6 passing ✅

# Other integration tests (need updates)
cargo test --test scenario_integration_test
cargo test --test test_infrastructure
```

### All Tests (with failures)
```bash
cargo test                      # Runs all tests (shows compilation errors)
```

## Test Status Summary

| Category | Status | Count | Notes |
|----------|--------|-------|-------|
| Unit Tests | ⚠️ | 204/205 | 1 pre-existing failure |
| Event Integration | ✅ | 6/6 | All passing |
| Other Integration | ❌ | - | Need API updates |
| Property Tests | ❌ | - | Need API updates |

## Known Issues

### 1. Unit Test Failure (Pre-existing)
**Test:** `workload::declarative::tests::test_round_robin_distribution`
- **Status:** Pre-existing issue (not related to recent work)
- **Impact:** Low - Edge case in round-robin distribution
- **Action:** Non-urgent fix needed

### 2. Integration Tests Compilation Errors
Integration tests need updates for recent API changes:

**PrepareContext Lifetime:**
```rust
// Old (broken)
fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()>

// New (correct)
async fn prepare(&mut self, ctx: &mut PrepareContext<'_>) -> Result<()>
```

**MetricsSnapshot Fields:**
Missing fields added during runtime module updates:
- `pool_saturation_events`
- `runtime_saturation_events`

**Action:** Medium priority - tests need updating but core functionality works

## Recent Updates

### Event Module (2026-01-07 to 2026-01-08) ✨
- **Status:** COMPLETE ✅
- **Tests Added:**
  - 38 unit tests (all passing)
  - 6 integration tests (all passing)
- **Coverage:**
  - EventManager lifecycle
  - Timer event sources
  - Configuration (YAML parsing)
  - Graceful shutdown
  - Multiple event types
  - Integration with scenario module

## Contributing Tests

### Writing Unit Tests
Unit tests go in the same file as the code they test:
```rust
// In src/my_module.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        assert_eq!(my_function(42), expected_result);
    }
}
```

### Writing Integration Tests
Create a new file in `tests/`:
```rust
// In tests/my_integration_test.rs
use rsbench::*;

#[tokio::test]
async fn test_my_integration() {
    // Test cross-module integration
}
```

### Using Test Utilities
```rust
use common::{MockDriver, MockWorkload};

#[tokio::test]
async fn my_test() {
    let driver = MockDriver::new();
    // Use mock for testing
}
```

## Continuous Integration

When adding new features:
1. ✅ Add unit tests in the module (`src/`)
2. ✅ Add integration tests if needed (`tests/`)
3. ✅ Update `TEST_STATUS.md` with new test counts
4. ✅ Ensure all tests pass: `cargo test --lib`

## Documentation

- **QUICK_STATUS.txt** - One-page test status overview
- **TEST_STATUS.md** - Detailed test status and module breakdown
- **README.md** - This file (test organization and guidelines)

---

**Last Updated:** 2026-01-08
**M0 Status:** Core modules tested and functional ✅
