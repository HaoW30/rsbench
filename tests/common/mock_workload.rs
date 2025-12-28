//! Mock workload implementation for testing

use rsbench::workload::{
    ExecutionContext, Operation, OperationType, PrepareContext, Workload,
};
use rsbench::{Result, Value};

/// Mock workload that returns a predefined sequence of operations
pub struct MockWorkload {
    name: String,
    operations: Vec<Operation>,
    current_index: usize,
    prepare_called: bool,
    cleanup_called: bool,
}

impl MockWorkload {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            operations: Vec::new(),
            current_index: 0,
            prepare_called: false,
            cleanup_called: false,
        }
    }

    pub fn with_operations(mut self, operations: Vec<Operation>) -> Self {
        self.operations = operations;
        self
    }

    pub fn with_simple_operation(mut self, sql: &str, op_type: OperationType) -> Self {
        self.operations.push(Operation {
            name: "test_op".to_string(),
            sql: sql.to_string(),
            params: vec![],
            operation_type: op_type,
            is_transaction: false,
            transaction_sqls: vec![],
            transaction_params: vec![],
        });
        self
    }

    pub fn was_prepare_called(&self) -> bool {
        self.prepare_called
    }

    pub fn was_cleanup_called(&self) -> bool {
        self.cleanup_called
    }

    pub fn operations_generated(&self) -> usize {
        self.current_index
    }
}

impl Workload for MockWorkload {
    fn prepare(&mut self, _ctx: &mut PrepareContext) -> Result<()> {
        self.prepare_called = true;
        Ok(())
    }

    fn next_operation(&mut self, _ctx: &ExecutionContext) -> Result<Operation> {
        if self.operations.is_empty() {
            // Generate a default operation if no operations provided
            Ok(Operation {
                name: "default_op".to_string(),
                sql: "SELECT 1".to_string(),
                params: vec![],
                operation_type: OperationType::Read,
                is_transaction: false,
                transaction_sqls: vec![],
                transaction_params: vec![],
            })
        } else {
            // Cycle through the provided operations
            let op = self.operations[self.current_index % self.operations.len()].clone();
            self.current_index += 1;
            Ok(op)
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        self.cleanup_called = true;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Builder for creating mock operations easily
pub struct MockOperationBuilder {
    name: String,
    sql: String,
    params: Vec<Value>,
    operation_type: OperationType,
}

impl MockOperationBuilder {
    pub fn new() -> Self {
        Self {
            name: "test_op".to_string(),
            sql: "SELECT 1".to_string(),
            params: vec![],
            operation_type: OperationType::Read,
        }
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn sql(mut self, sql: &str) -> Self {
        self.sql = sql.to_string();
        self
    }

    pub fn param(mut self, value: Value) -> Self {
        self.params.push(value);
        self
    }

    pub fn operation_type(mut self, op_type: OperationType) -> Self {
        self.operation_type = op_type;
        self
    }

    pub fn build(self) -> Operation {
        Operation {
            name: self.name,
            sql: self.sql,
            params: self.params,
            operation_type: self.operation_type,
            is_transaction: false,
            transaction_sqls: vec![],
            transaction_params: vec![],
        }
    }
}

impl Default for MockOperationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_workload_default() {
        let mut workload = MockWorkload::new("test");
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 0,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        assert_eq!(op.name, "default_op");
        assert_eq!(op.sql, "SELECT 1");
    }

    #[test]
    fn test_mock_workload_with_operations() {
        let ops = vec![
            MockOperationBuilder::new()
                .name("op1")
                .sql("SELECT * FROM test1")
                .build(),
            MockOperationBuilder::new()
                .name("op2")
                .sql("SELECT * FROM test2")
                .build(),
        ];

        let mut workload = MockWorkload::new("test").with_operations(ops);
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 0,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op1 = workload.next_operation(&ctx).unwrap();
        assert_eq!(op1.name, "op1");

        let op2 = workload.next_operation(&ctx).unwrap();
        assert_eq!(op2.name, "op2");

        // Should cycle back
        let op3 = workload.next_operation(&ctx).unwrap();
        assert_eq!(op3.name, "op1");
    }

    #[test]
    fn test_mock_workload_lifecycle() {
        let mut workload = MockWorkload::new("test");
        assert!(!workload.was_prepare_called());
        assert!(!workload.was_cleanup_called());

        // Note: We can't call prepare without a PrepareContext
        // This would require a mock database connection
        // For now, just test the flags are tracked

        let _result = workload.cleanup();
        assert!(workload.was_cleanup_called());
    }
}
