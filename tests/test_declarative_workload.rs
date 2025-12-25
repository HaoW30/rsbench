//! Integration tests for declarative workload loading

use rsbench::workload::{DeclarativeWorkload, Workload, ExecutionContext};
use std::path::Path;
use std::time::Duration;

#[test]
fn test_load_oltp_read_write_workload() {
    let path = Path::new("workloads/oltp_read_write.yaml");

    // Skip test if file doesn't exist (e.g., in CI without workload files)
    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let result = DeclarativeWorkload::from_file(path, 42);
    assert!(result.is_ok(), "Should load oltp_read_write.yaml successfully");

    let mut workload = result.unwrap();
    assert_eq!(workload.name(), "oltp_read_write");

    // Test operation generation
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    let op = workload.next_operation(&ctx);
    assert!(op.is_ok(), "Should generate operation successfully");

    let operation = op.unwrap();
    assert!(operation.name == "point_select" || operation.name == "update_non_index");
    assert!(!operation.sql.is_empty());
}

#[test]
fn test_load_oltp_point_select_workload() {
    let path = Path::new("workloads/oltp_point_select.yaml");

    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let result = DeclarativeWorkload::from_file(path, 42);
    assert!(result.is_ok(), "Should load oltp_point_select.yaml successfully");

    let mut workload = result.unwrap();
    assert_eq!(workload.name(), "oltp_point_select");

    // Test operation generation - should only generate point_select
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    for _ in 0..10 {
        let op = workload.next_operation(&ctx).unwrap();
        assert_eq!(op.name, "point_select", "Should only generate point_select operations");
    }
}

#[test]
fn test_operation_distribution() {
    let path = Path::new("workloads/oltp_read_write.yaml");

    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let mut workload = DeclarativeWorkload::from_file(path, 42).unwrap();
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    // Generate many operations and check distribution
    let mut point_select_count = 0;
    let mut update_count = 0;

    for _ in 0..1000 {
        let op = workload.next_operation(&ctx).unwrap();
        match op.name.as_str() {
            "point_select" => point_select_count += 1,
            "update_non_index" => update_count += 1,
            _ => panic!("Unexpected operation: {}", op.name),
        }
    }

    // Should be roughly 60% reads, 40% writes (allow ±10% variance)
    let read_ratio = point_select_count as f64 / 1000.0;
    assert!(read_ratio > 0.50 && read_ratio < 0.70,
        "Read ratio should be around 60%, got {:.1}%", read_ratio * 100.0);
}

#[test]
fn test_deterministic_operation_sequence() {
    let path = Path::new("workloads/oltp_read_write.yaml");

    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    // Create two workloads with same seed
    let mut workload1 = DeclarativeWorkload::from_file(path, 12345).unwrap();
    let mut workload2 = DeclarativeWorkload::from_file(path, 12345).unwrap();

    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    // Generate operations from both - should be identical
    for i in 0..20 {
        let op1 = workload1.next_operation(&ctx).unwrap();
        let op2 = workload2.next_operation(&ctx).unwrap();

        assert_eq!(op1.name, op2.name,
            "Operation {} name mismatch: {} vs {}", i, op1.name, op2.name);
        assert_eq!(op1.sql, op2.sql,
            "Operation {} SQL mismatch", i);
        assert_eq!(op1.params.len(), op2.params.len(),
            "Operation {} param count mismatch", i);
    }
}
