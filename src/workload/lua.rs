//! Lua-based workload (sysbench compatibility)

use super::{ExecutionContext, Operation, OperationType, PrepareContext, Workload};
use crate::{Result, Value};
use mlua::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::path::Path;

/// Lua-based workload (sysbench compatibility)
pub struct LuaWorkload {
    lua: Lua,
    #[allow(dead_code)]
    rng: ChaCha8Rng,
}

impl LuaWorkload {
    pub fn new(script_path: &Path, seed: u64) -> Result<Self> {
        let lua = Lua::new();
        let script = std::fs::read_to_string(script_path)?;

        // Load script
        lua.load(&script)
            .exec()
            .map_err(|e| crate::Error::Workload(format!("Lua error: {}", e)))?;

        Ok(Self {
            lua,
            rng: ChaCha8Rng::seed_from_u64(seed),
        })
    }
}

impl Workload for LuaWorkload {
    fn prepare(&mut self, _ctx: &mut PrepareContext) -> Result<()> {
        // Call Lua prepare() function if exists
        let prepare: Option<LuaFunction> = self.lua.globals().get("prepare").ok();
        if let Some(prepare_fn) = prepare {
            prepare_fn
                .call::<_, ()>(())
                .map_err(|e| crate::Error::Workload(format!("Lua prepare error: {}", e)))?;
        }
        Ok(())
    }

    fn next_operation(&mut self, _ctx: &ExecutionContext) -> Result<Operation> {
        // Call Lua event() function
        let event: LuaFunction = self
            .lua
            .globals()
            .get("event")
            .map_err(|e| crate::Error::Workload(format!("Lua event function not found: {}", e)))?;

        let result: LuaTable = event
            .call(())
            .map_err(|e| crate::Error::Workload(format!("Lua event error: {}", e)))?;

        // Extract operation from Lua table
        let sql: String = result
            .get("sql")
            .map_err(|e| crate::Error::Workload(format!("Lua result missing 'sql': {}", e)))?;
        let name: String = result.get("name").unwrap_or_else(|_| "lua_op".into());

        Ok(Operation {
            name,
            sql,
            params: vec![], // M0: basic support only
            operation_type: OperationType::Read, // M0: simplified
            is_transaction: false,
            transaction_sqls: Vec::new(),
            transaction_params: Vec::new(),
        })
    }

    fn cleanup(&mut self) -> Result<()> {
        // Call Lua cleanup() if exists
        if let Ok(cleanup) = self.lua.globals().get::<_, LuaFunction>("cleanup") {
            cleanup
                .call::<_, ()>(())
                .map_err(|e| crate::Error::Workload(format!("Lua cleanup error: {}", e)))?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "lua_workload"
    }
}
