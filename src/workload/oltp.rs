//! OLTP Read/Write workload (sysbench equivalent)

use super::{ExecutionContext, Operation, OperationType, PrepareContext, Workload};
use crate::config::WorkloadConfig;
use crate::{Result, Value};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// OLTP Read/Write workload (sysbench equivalent)
pub struct OltpReadWrite {
    table_count: usize,
    table_size: usize,
    rng: ChaCha8Rng,
}

impl OltpReadWrite {
    pub fn new(config: &WorkloadConfig, seed: u64) -> Result<Self> {
        let (table_count, table_size) = match config {
            WorkloadConfig::Builtin {
                table_count,
                table_size,
                ..
            } => (*table_count, *table_size),
            _ => {
                return Err(crate::Error::Workload(
                    "Invalid config for OLTP workload".into(),
                ))
            }
        };

        Ok(Self {
            table_count,
            table_size,
            rng: ChaCha8Rng::seed_from_u64(seed),
        })
    }
}

impl Workload for OltpReadWrite {
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()> {
        // Create sbtest tables
        for i in 1..=self.table_count {
            let create_sql = format!(
                "CREATE TABLE IF NOT EXISTS sbtest{} (
                    id INT PRIMARY KEY,
                    k INT NOT NULL,
                    c CHAR(120) NOT NULL,
                    pad CHAR(60) NOT NULL,
                    INDEX k_idx (k)
                )",
                i
            );
            ctx.database.execute(&create_sql)?;

            // TODO: Insert test data
            // For M0, we can skip data loading and just use random IDs
        }
        Ok(())
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        // Deterministic operation selection
        let op_type = self.rng.gen_range(0..100);
        let table_id = (ctx.iteration % self.table_count as u64) + 1;
        let row_id = self.rng.gen_range(1..=self.table_size as i64);

        if op_type < 60 {
            // 60% reads - point select
            Ok(Operation {
                name: "point_select".into(),
                sql: format!("SELECT c FROM sbtest{} WHERE id = ?", table_id),
                params: vec![Value::Int(row_id)],
                operation_type: OperationType::Read,
            })
        } else {
            // 40% writes - update non-index column
            Ok(Operation {
                name: "update_non_index".into(),
                sql: format!("UPDATE sbtest{} SET c = ? WHERE id = ?", table_id),
                params: vec![
                    Value::String(format!("{:0<120}", ctx.iteration)),
                    Value::Int(row_id),
                ],
                operation_type: OperationType::Write,
            })
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        // No cleanup needed for M0
        Ok(())
    }

    fn name(&self) -> &str {
        "oltp_read_write"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_workload(seed: u64) -> OltpReadWrite {
        let config = WorkloadConfig::Builtin {
            name: "oltp_read_write".to_string(),
            table_count: 10,
            table_size: 1000,
        };
        OltpReadWrite::new(&config, seed).unwrap()
    }

    #[test]
    fn test_oltp_workload_creation() {
        let workload = create_test_workload(42);
        assert_eq!(workload.name(), "oltp_read_write");
        assert_eq!(workload.table_count, 10);
        assert_eq!(workload.table_size, 1000);
    }

    #[test]
    fn test_invalid_config_returns_error() {
        let config = WorkloadConfig::Lua {
            script: std::path::PathBuf::from("test.lua"),
        };
        let result = OltpReadWrite::new(&config, 42);
        assert!(result.is_err());
    }

    #[test]
    fn test_operation_generation_deterministic() {
        let mut workload1 = create_test_workload(42);
        let mut workload2 = create_test_workload(42);

        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 0,
            elapsed: Duration::from_secs(0),
        };

        // Same seed should produce same operations
        let op1 = workload1.next_operation(&ctx).unwrap();
        let op2 = workload2.next_operation(&ctx).unwrap();

        assert_eq!(op1.name, op2.name);
        assert_eq!(op1.sql, op2.sql);
        assert_eq!(op1.params.len(), op2.params.len());
    }

    #[test]
    fn test_operation_distribution() {
        let mut workload = create_test_workload(42);
        let mut read_count = 0;
        let mut write_count = 0;

        // Generate 1000 operations
        for i in 0..1000 {
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration: i,
                elapsed: Duration::from_secs(0),
            };

            let op = workload.next_operation(&ctx).unwrap();
            match op.operation_type {
                OperationType::Read => read_count += 1,
                OperationType::Write => write_count += 1,
            }
        }

        // Should be roughly 60% reads, 40% writes (with some tolerance)
        let read_percentage = read_count as f64 / 1000.0;
        assert!(read_percentage > 0.50 && read_percentage < 0.70,
                "Read percentage {} not in expected range [0.50, 0.70]", read_percentage);
    }

    #[test]
    fn test_point_select_format() {
        let mut workload = create_test_workload(42);

        // Generate operations until we get a read
        for i in 0..100 {
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration: i,
                elapsed: Duration::from_secs(0),
            };

            let op = workload.next_operation(&ctx).unwrap();
            if let OperationType::Read = op.operation_type {
                assert_eq!(op.name, "point_select");
                assert!(op.sql.starts_with("SELECT c FROM sbtest"));
                assert!(op.sql.contains("WHERE id = ?"));
                assert_eq!(op.params.len(), 1);
                break;
            }
        }
    }

    #[test]
    fn test_update_format() {
        let mut workload = create_test_workload(42);

        // Generate operations until we get a write
        for i in 0..100 {
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration: i,
                elapsed: Duration::from_secs(0),
            };

            let op = workload.next_operation(&ctx).unwrap();
            if let OperationType::Write = op.operation_type {
                assert_eq!(op.name, "update_non_index");
                assert!(op.sql.starts_with("UPDATE sbtest"));
                assert!(op.sql.contains("SET c = ?"));
                assert!(op.sql.contains("WHERE id = ?"));
                assert_eq!(op.params.len(), 2);
                break;
            }
        }
    }

    #[test]
    fn test_table_rotation() {
        let mut workload = create_test_workload(42);

        // Generate operations and verify tables rotate
        let mut tables_used = std::collections::HashSet::new();

        for i in 0..20 {
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration: i,
                elapsed: Duration::from_secs(0),
            };

            let op = workload.next_operation(&ctx).unwrap();

            // Extract table number from SQL
            if let Some(start) = op.sql.find("sbtest") {
                let table_part = &op.sql[start + 6..];
                if let Some(space_or_set) = table_part.find(|c: char| c == ' ' || c == '\n') {
                    tables_used.insert(table_part[..space_or_set].to_string());
                }
            }
        }

        // Should use multiple tables
        assert!(tables_used.len() > 1);
    }

    #[test]
    fn test_row_id_within_bounds() {
        let mut workload = create_test_workload(42);

        for i in 0..100 {
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration: i,
                elapsed: Duration::from_secs(0),
            };

            let op = workload.next_operation(&ctx).unwrap();

            // Get the row ID from params
            let row_id = match &op.params[op.params.len() - 1] {
                Value::Int(id) => *id,
                _ => panic!("Expected Int parameter"),
            };

            // Row ID should be within table size
            assert!(row_id >= 1 && row_id <= 1000);
        }
    }
}
