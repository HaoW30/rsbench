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
