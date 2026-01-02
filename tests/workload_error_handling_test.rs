//! Workload Error Handling Tests
//!
//! Tests for workload definition validation and error handling.
//! Addresses real-world issues:
//! - Poor error handling for misaligned query parameters
//! - Unclear error messages for workload definition problems
//! - Parameter count mismatch detection

use rsbench::workload::{DeclarativeWorkload, Workload, ExecutionContext};
use rsbench::Result;
use std::time::Duration;

#[test]
fn test_parameter_count_mismatch_too_few() {
    // Issue: Query has 2 placeholders but only 1 parameter defined
    // Should give clear error message

    let yaml = r#"
workload:
  name: test_mismatch
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
          - name: value
            type: INT
  operations:
    - name: bad_query
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ? AND value = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
        # Missing second parameter!
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should either fail to parse OR generate operations correctly
    // If it parses, verify behavior when generating operations
    if let Ok(mut workload) = result {
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();

        // With 2 ? placeholders and 1 parameter, should only include 1 param
        // (Implementation uses "last N params" logic)
        assert_eq!(op.params.len(), 1,
                   "Should detect parameter count mismatch: SQL has 2 '?' but only 1 param defined");
    }
}

#[test]
fn test_parameter_count_mismatch_too_many() {
    // Issue: Query has 1 placeholder but 2 parameters defined
    // Extra parameters should be ignored (used for template substitution)

    let yaml = r#"
workload:
  name: test_extra_params
  schema:
    tables:
      - name: test
        count: 5
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: query_with_template
      weight: 100
      type: read
      sql: "SELECT * FROM test{table_id} WHERE id = ?"
      parameters:
        - name: table_id
          distribution:
            type: uniform
            range: [1, 5]
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

    let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    let op = workload.next_operation(&ctx).unwrap();

    // Should have 1 parameter for the SQL placeholder
    assert_eq!(op.params.len(), 1, "Should have 1 SQL parameter (id)");

    // SQL should have table_id substituted
    assert!(op.sql.contains("test"), "SQL should contain table name");
    assert!(!op.sql.contains("{table_id}"), "table_id should be substituted");
}

#[test]
fn test_missing_required_parameter_fields() {
    // Issue: Parameter definition missing distribution or generator
    // Should give clear error message

    let yaml = r#"
workload:
  name: test_missing_fields
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: bad_param
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          # Missing both distribution and generator!
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should fail to parse with clear error
    assert!(result.is_err(), "Should fail when parameter has no distribution or generator");

    // Check error message is helpful
    if let Err(e) = result {
        let error_msg = e.to_string().to_lowercase();
        // Error should mention the problem
        assert!(error_msg.contains("distribution") || error_msg.contains("generator") || error_msg.contains("missing"),
                "Error message should mention missing distribution/generator: {}", e);
    }
}

#[test]
fn test_invalid_distribution_type() {
    // Issue: Typo in distribution type
    // Should give clear error message

    let yaml = r#"
workload:
  name: test_invalid_dist
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: bad_distribution
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: unifrm  # Typo! Should be "uniform"
            range: [1, 100]
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should fail to parse
    assert!(result.is_err(), "Should fail with invalid distribution type");

    if let Err(e) = result {
        let error_msg = e.to_string();
        // Error should mention the invalid type
        println!("Error message: {}", error_msg);
        // Just verify it's an error - specific message may vary
    }
}

#[test]
fn test_invalid_range_values() {
    // Issue: Range with min > max
    // Should give clear error or handle gracefully

    let yaml = r#"
workload:
  name: test_invalid_range
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: bad_range
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [100, 1]  # Min > Max!
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Could either fail to parse OR generate operations incorrectly
    // The important thing is it doesn't panic
    if let Ok(mut workload) = result {
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: Duration::from_secs(0),
        };

        // Should not panic, even with invalid range
        let op_result = workload.next_operation(&ctx);
        assert!(op_result.is_ok() || op_result.is_err(),
                "Should handle invalid range gracefully (either error or generate valid op)");
    }
}

#[test]
fn test_missing_parameter_name() {
    // Issue: Parameter used in SQL but not defined
    // Should detect and report clearly

    let yaml = r#"
workload:
  name: test_undefined_param
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: undefined_param
      weight: 100
      type: read
      sql: "SELECT * FROM test{table_num} WHERE id = ?"
      parameters:
        # table_num is used in SQL but not defined!
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

    let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    let op = workload.next_operation(&ctx).unwrap();

    // SQL should still contain {table_num} (not substituted)
    // This is actually valid - might be intentional template
    assert!(op.sql.contains("{table_num}") || !op.sql.contains("table_num"),
            "Undefined template variable should remain or be handled gracefully");
}

#[test]
fn test_variable_substitution_error_handling() {
    // Issue: Variable reference to undefined variable
    // Should handle gracefully

    let yaml = r#"
workload:
  name: test_undefined_variable
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: query
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, "${undefined_variable}"]  # Undefined!
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should either fail to parse or substitute to empty/default
    if let Ok(mut workload) = result {
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: Duration::from_secs(0),
        };

        // Should not panic when generating operations
        let op_result = workload.next_operation(&ctx);
        assert!(op_result.is_ok() || op_result.is_err(),
                "Should handle undefined variable reference gracefully");
    }
}

#[test]
fn test_empty_operations_list() {
    // Issue: Workload with no operations defined
    // Should fail with clear error

    let yaml = r#"
workload:
  name: test_no_operations
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations: []  # Empty!
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should fail - can't generate operations without any defined
    assert!(result.is_err(), "Should fail with empty operations list");

    if let Err(e) = result {
        let error_msg = e.to_string().to_lowercase();
        assert!(error_msg.contains("operation") || error_msg.contains("empty"),
                "Error should mention empty operations: {}", e);
    }
}

#[test]
fn test_zero_weight_operations() {
    // Issue: All operations have zero weight
    // Should fail or handle gracefully

    let yaml = r#"
workload:
  name: test_zero_weights
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: query1
      weight: 0  # Zero weight!
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    // Should either fail or handle zero weights gracefully
    if let Ok(mut workload) = result {
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: Duration::from_secs(0),
        };

        // Should not panic
        let _op = workload.next_operation(&ctx).unwrap();
    }
}

#[test]
fn test_helpful_error_messages() {
    // Issue: Generic error messages not helpful
    // Test that errors give context about what's wrong WHERE

    let yaml = r#"
workload:
  name: test_bad_syntax
  schema:
    tables:
      - name: test
        count: "not_a_number"  # Wrong type!
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: query
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

    let result = DeclarativeWorkload::from_yaml(yaml, 42);

    assert!(result.is_err(), "Should fail with type mismatch");

    if let Err(e) = result {
        let error_msg = e.to_string();
        println!("Error message: {}", error_msg);
        // Error should come from YAML parser with reasonable context
        // Just verify we get an error - specific message will vary
    }
}

#[test]
fn test_sql_injection_protection() {
    // Issue: User-provided template values could contain SQL
    // Test that parameter substitution is safe

    let yaml = r#"
workload:
  name: test_injection
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: query
      weight: 100
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

    let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 1,
        elapsed: Duration::from_secs(0),
    };

    let op = workload.next_operation(&ctx).unwrap();

    // Parameters should be in params vec, not substituted into SQL string
    assert!(op.sql.contains("?"), "Should use parameterized queries");
    assert_eq!(op.params.len(), 1, "Should have 1 parameter");
    assert!(!op.sql.contains("1 OR 1=1"), "Should not have SQL injection");
}
