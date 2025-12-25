//! Declarative workload implementation
//!
//! Supports YAML-based workload definitions with full sysbench compatibility.

use crate::workload::{ExecutionContext, Operation, OperationType, PrepareContext, Workload};
use crate::{Result, Value};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ============================================================================
// YAML Data Structures
// ============================================================================

/// Top-level workload definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDefinition {
    pub workload: WorkloadSpec,
}

/// Workload specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub schema: SchemaDefinition,
    #[serde(default)]
    pub data_generation: Option<DataGenerationConfig>,
    pub operations: Vec<OperationDefinition>,
}

/// Schema definition for tables and columns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDefinition {
    pub tables: Vec<TableDefinition>,
}

/// Table definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDefinition {
    pub name: String,
    #[serde(default = "default_table_count")]
    pub count: usize,
    #[serde(default = "default_row_count")]
    pub row_count: usize,
    pub columns: Vec<ColumnDefinition>,
}

fn default_table_count() -> usize {
    1
}

fn default_row_count() -> usize {
    10000
}

/// Column definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub index: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
}

/// Data generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGenerationConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub seed: Option<u64>,
}

fn default_strategy() -> String {
    "uniform".to_string()
}

/// Operation definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationDefinition {
    pub name: String,
    pub weight: u32,
    #[serde(rename = "type")]
    pub operation_type: String,
    pub sql: String,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
}

/// Parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDefinition {
    pub name: String,
    #[serde(default)]
    pub distribution: Option<DistributionConfig>,
    #[serde(default)]
    pub generator: Option<GeneratorConfig>,
}

/// Distribution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionConfig {
    #[serde(rename = "type")]
    pub distribution_type: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_range")]
    pub range: Option<[String; 2]>,
}

/// Custom deserializer for range that handles both int and string values
fn deserialize_range<'de, D>(deserializer: D) -> std::result::Result<Option<[String; 2]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    use serde_yaml::Value;

    let value: Option<Value> = Option::deserialize(deserializer)?;

    match value {
        None => Ok(None),
        Some(Value::Sequence(seq)) => {
            if seq.len() != 2 {
                return Err(D::Error::custom("Range must have exactly 2 elements"));
            }

            let min = match &seq[0] {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => return Err(D::Error::custom("Range values must be numbers or strings")),
            };

            let max = match &seq[1] {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => return Err(D::Error::custom("Range values must be numbers or strings")),
            };

            Ok(Some([min, max]))
        }
        _ => Err(D::Error::custom("Range must be a sequence")),
    }
}

/// Generator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorConfig {
    #[serde(rename = "type")]
    pub generator_type: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub length: Option<usize>,
}

// ============================================================================
// DeclarativeWorkload Implementation
// ============================================================================

/// Declarative workload - loads from YAML definition
pub struct DeclarativeWorkload {
    /// Workload name
    name: String,

    /// Workload specification
    spec: WorkloadSpec,

    /// Random number generator (deterministic)
    rng: ChaCha8Rng,

    /// Weighted index for operation selection
    operation_weights: WeightedIndex<u32>,

    /// Current table index for round-robin
    current_table: usize,

    /// Variable substitutions (e.g., ${table_count})
    variables: HashMap<String, String>,
}

impl DeclarativeWorkload {
    /// Create new declarative workload from YAML file
    pub fn from_file(path: &Path, seed: u64) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::Error::Workload(format!("Failed to read workload file {}: {}", path.display(), e))
        })?;

        Self::from_yaml(&content, seed)
    }

    /// Create new declarative workload from YAML string
    pub fn from_yaml(yaml: &str, seed: u64) -> Result<Self> {
        let definition: WorkloadDefinition = serde_yaml::from_str(yaml).map_err(|e| {
            crate::Error::Workload(format!("Failed to parse workload YAML: {}", e))
        })?;

        Self::from_definition(definition.workload, seed)
    }

    /// Create new declarative workload from WorkloadSpec
    pub fn from_definition(spec: WorkloadSpec, seed: u64) -> Result<Self> {
        // Validate workload
        if spec.operations.is_empty() {
            return Err(crate::Error::Workload(
                "Workload must have at least one operation".into(),
            ));
        }

        // Build weighted index for operation selection
        let weights: Vec<u32> = spec.operations.iter().map(|op| op.weight).collect();
        let operation_weights = WeightedIndex::new(weights).map_err(|e| {
            crate::Error::Workload(format!("Invalid operation weights: {}", e))
        })?;

        // Build variable substitutions
        let mut variables = HashMap::new();

        // Add table-related variables
        if let Some(table) = spec.schema.tables.first() {
            variables.insert("table_count".to_string(), table.count.to_string());
            variables.insert("row_count".to_string(), table.row_count.to_string());
        }

        let name = spec.name.clone();
        let rng = ChaCha8Rng::seed_from_u64(seed);

        Ok(DeclarativeWorkload {
            name,
            spec,
            rng,
            operation_weights,
            current_table: 0,
            variables,
        })
    }

    /// Apply variable substitutions to a string
    fn substitute_variables(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (key, value) in &self.variables {
            let placeholder = format!("${{{}}}", key);
            result = result.replace(&placeholder, value);
        }
        result
    }

    /// Generate parameter value based on definition
    fn generate_parameter(
        &mut self,
        param: &ParameterDefinition,
        ctx: &ExecutionContext,
    ) -> Result<Value> {
        if let Some(distribution) = &param.distribution {
            self.generate_from_distribution(distribution, ctx)
        } else if let Some(generator) = &param.generator {
            self.generate_from_generator(generator, ctx)
        } else {
            Err(crate::Error::Workload(format!(
                "Parameter '{}' must have either 'distribution' or 'generator'",
                param.name
            )))
        }
    }

    /// Parse range value and substitute variables
    fn parse_range(&self, templates: &[String; 2]) -> Result<[i64; 2]> {
        let min_str = self.substitute_variables(&templates[0]);
        let max_str = self.substitute_variables(&templates[1]);

        let min = min_str.parse::<i64>().map_err(|e| {
            crate::Error::Workload(format!("Failed to parse range min '{}': {}", min_str, e))
        })?;

        let max = max_str.parse::<i64>().map_err(|e| {
            crate::Error::Workload(format!("Failed to parse range max '{}': {}", max_str, e))
        })?;

        Ok([min, max])
    }

    /// Generate value from distribution
    fn generate_from_distribution(
        &mut self,
        dist: &DistributionConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value> {
        match dist.distribution_type.as_str() {
            "uniform" => {
                if let Some(ref range) = dist.range {
                    let [min, max] = self.parse_range(range)?;
                    let value = self.rng.gen_range(min..=max);
                    Ok(Value::Int(value))
                } else {
                    Err(crate::Error::Workload(
                        "Uniform distribution requires 'range' parameter".into(),
                    ))
                }
            }
            "round_robin" => {
                if let Some(ref range) = dist.range {
                    let [min, max] = self.parse_range(range)?;
                    let range_size = (max - min + 1) as usize;
                    let value = min + (self.current_table % range_size) as i64;
                    self.current_table += 1;
                    Ok(Value::Int(value))
                } else {
                    Err(crate::Error::Workload(
                        "Round-robin distribution requires 'range' parameter".into(),
                    ))
                }
            }
            _ => Err(crate::Error::Workload(format!(
                "Unsupported distribution type: {}",
                dist.distribution_type
            ))),
        }
    }

    /// Generate value from generator
    fn generate_from_generator(
        &mut self,
        gen: &GeneratorConfig,
        ctx: &ExecutionContext,
    ) -> Result<Value> {
        match gen.generator_type.as_str() {
            "string" => {
                if let Some(template) = &gen.template {
                    // Replace {iteration} with current iteration
                    let mut result = template.replace("{iteration}", &ctx.iteration.to_string());

                    // Handle padding format: {iteration:0>N}
                    if let Some(start) = result.find("{iteration:") {
                        if let Some(end) = result[start..].find('}') {
                            let format_spec = &result[start + 11..start + end];
                            // Parse format like "0>120"
                            if let Some(width_str) = format_spec.strip_prefix("0>") {
                                if let Ok(width) = width_str.parse::<usize>() {
                                    let padded = format!("{:0>width$}", ctx.iteration, width = width);
                                    result = result[..start].to_string() + &padded + &result[start + end + 1..];
                                }
                            }
                        }
                    }

                    Ok(Value::String(result))
                } else {
                    Err(crate::Error::Workload(
                        "String generator requires 'template' parameter".into(),
                    ))
                }
            }
            "integer" => {
                // Generate random integer
                let value = self.rng.gen::<i64>();
                Ok(Value::Int(value))
            }
            _ => Err(crate::Error::Workload(format!(
                "Unsupported generator type: {}",
                gen.generator_type
            ))),
        }
    }

    /// Build SQL with parameter substitutions
    fn build_sql(&self, template: &str, params: &[(&str, &Value)]) -> String {
        let mut sql = template.to_string();

        // Substitute table_id and other named placeholders
        for (name, value) in params {
            let placeholder = format!("{{{}}}", name);
            if sql.contains(&placeholder) {
                let value_str = match value {
                    Value::Int(i) => i.to_string(),
                    Value::String(s) => s.clone(),
                    Value::Float(f) => f.to_string(),
                    Value::Bytes(b) => format!("{:?}", b),
                    Value::Null => "NULL".to_string(),
                };
                sql = sql.replace(&placeholder, &value_str);
            }
        }

        sql
    }
}

impl Workload for DeclarativeWorkload {
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()> {
        // Create tables based on schema definition
        for table_def in &self.spec.schema.tables {
            for table_num in 1..=table_def.count {
                let table_name = if table_def.count == 1 {
                    table_def.name.clone()
                } else {
                    format!("{}{}", table_def.name, table_num)
                };

                // Build CREATE TABLE statement
                let mut create_sql = format!("CREATE TABLE IF NOT EXISTS {} (", table_name);

                let mut column_defs = Vec::new();
                let mut indexes = Vec::new();
                let mut primary_key_col = None;

                for col in &table_def.columns {
                    let mut col_def = format!("{} {}", col.name, col.column_type);

                    if col.primary_key {
                        primary_key_col = Some(col.name.clone());
                        if col.auto_increment {
                            col_def.push_str(" AUTO_INCREMENT");
                        }
                    }

                    if let Some(default) = &col.default {
                        if !default.is_empty() {
                            col_def.push_str(&format!(" DEFAULT '{}'", default));
                        }
                    }

                    column_defs.push(col_def);

                    // Collect index definitions
                    if let Some(index_name) = &col.index {
                        indexes.push((index_name.clone(), col.name.clone()));
                    }
                }

                create_sql.push_str(&column_defs.join(", "));

                // Add primary key
                if let Some(pk_col) = primary_key_col {
                    create_sql.push_str(&format!(", PRIMARY KEY ({})", pk_col));
                }

                create_sql.push(')');

                // Execute CREATE TABLE
                ctx.database.execute(&create_sql)?;

                // Create indexes
                for (index_name, col_name) in indexes {
                    let index_sql = format!(
                        "CREATE INDEX {} ON {} ({})",
                        index_name, table_name, col_name
                    );
                    ctx.database.execute(&index_sql)?;
                }

                // TODO: Load data based on data_generation config
                // For now, we'll skip data loading (will be implemented next)
            }
        }

        Ok(())
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        // Select operation based on weights
        let op_index = self.operation_weights.sample(&mut self.rng);

        // Clone the operation definition to avoid borrow checker issues
        let op_def = self.spec.operations[op_index].clone();

        // Generate parameters
        let mut params = Vec::new();
        let mut named_params: Vec<(String, Value)> = Vec::new();

        for param_def in &op_def.parameters {
            let value = self.generate_parameter(param_def, ctx)?;
            named_params.push((param_def.name.clone(), value.clone()));
            params.push(value);
        }

        // Build SQL with named parameter substitutions
        let param_refs: Vec<(&str, &Value)> = named_params.iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect();
        let sql = self.build_sql(&op_def.sql, &param_refs);

        // Determine operation type
        let operation_type = match op_def.operation_type.as_str() {
            "read" => OperationType::Read,
            "write" => OperationType::Write,
            _ => OperationType::Read, // Default to read
        };

        Ok(Operation {
            name: op_def.name,
            sql,
            params,
            operation_type,
        })
    }

    fn cleanup(&mut self) -> Result<()> {
        // TODO: Drop tables if configured to do so
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_workload() {
        let yaml = r#"
workload:
  name: test_workload
  schema:
    tables:
      - name: test_table
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: select
      weight: 100
      type: read
      sql: "SELECT * FROM test_table WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

        let result = DeclarativeWorkload::from_yaml(yaml, 42);
        assert!(result.is_ok(), "Should parse valid YAML");

        let workload = result.unwrap();
        assert_eq!(workload.name, "test_workload");
        assert_eq!(workload.spec.operations.len(), 1);
    }

    #[test]
    fn test_variable_substitution() {
        let yaml = r#"
workload:
  name: test
  schema:
    tables:
      - name: test
        count: 10
        row_count: 1000
        columns:
          - name: id
            type: INT
            primary_key: true
  operations:
    - name: select
      weight: 100
      type: read
      sql: "SELECT * FROM test"
"#;

        let workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        assert_eq!(workload.variables.get("table_count").unwrap(), "10");
        assert_eq!(workload.variables.get("row_count").unwrap(), "1000");
    }

    #[test]
    fn test_empty_operations_fails() {
        let yaml = r#"
workload:
  name: test
  schema:
    tables:
      - name: test
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
  operations: []
"#;

        let result = DeclarativeWorkload::from_yaml(yaml, 42);
        assert!(result.is_err(), "Should fail with empty operations");
    }

    #[test]
    fn test_uniform_distribution() {
        let yaml = r#"
workload:
  name: test
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
    - name: select
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
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        assert_eq!(op.name, "select");
        assert_eq!(op.params.len(), 1);

        // Check that generated value is within range
        if let Value::Int(val) = op.params[0] {
            assert!(val >= 1 && val <= 100);
        } else {
            panic!("Expected Int value");
        }
    }

    #[test]
    fn test_round_robin_distribution() {
        let yaml = r#"
workload:
  name: test
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
    - name: select
      weight: 100
      type: read
      sql: "SELECT * FROM test{table_id}"
      parameters:
        - name: table_id
          distribution:
            type: round_robin
            range: [1, 5]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        // Generate multiple operations and check round-robin behavior
        let mut values = Vec::new();
        for _ in 0..10 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::Int(val) = op.params[0] {
                values.push(val);
            }
        }

        // Should cycle through 1,2,3,4,5,1,2,3,4,5
        assert_eq!(values[0], 1);
        assert_eq!(values[1], 2);
        assert_eq!(values[4], 5);
        assert_eq!(values[5], 1);
    }

    #[test]
    fn test_string_generator() {
        let yaml = r#"
workload:
  name: test
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
    - name: update
      weight: 100
      type: write
      sql: "UPDATE test SET value = ? WHERE id = 1"
      parameters:
        - name: value
          generator:
            type: string
            template: "test_{iteration}"
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 42,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::String(s) = &op.params[0] {
            assert_eq!(s, "test_42");
        } else {
            panic!("Expected String value");
        }
    }
}
