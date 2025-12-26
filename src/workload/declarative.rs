//! Declarative workload implementation
//!
//! Supports YAML-based workload definitions with full sysbench compatibility.

use crate::workload::{ExecutionContext, Operation, OperationType, PrepareContext, Workload};
use crate::{Result, Value};
use rand::distributions::{Distribution, WeightedIndex};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Normal, Zipf};
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
    #[serde(default)]
    pub cleanup_config: Option<CleanupConfig>,
    pub operations: Vec<OperationDefinition>,
}

/// Cleanup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    #[serde(default)]
    pub drop_tables: bool,
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
    #[serde(default)]
    pub sql: String,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    pub transaction_operations: Option<Vec<SubOperationDefinition>>,
}

/// Sub-operation within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubOperationDefinition {
    pub name: String,
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
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_range")]
    pub range: Option<[String; 2]>,
    #[serde(default)]
    pub precision: Option<usize>,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    #[serde(default)]
    pub value: Option<String>,
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

    /// Sequential counter for sequential distribution
    sequential_counter: u64,

    /// Variable substitutions (e.g., ${table_count})
    variables: HashMap<String, String>,

    /// Created table names (for cleanup)
    created_tables: Vec<String>,
}

impl DeclarativeWorkload {
    /// Create new declarative workload from YAML file
    pub fn from_file(path: &Path, seed: u64) -> Result<Self> {
        Self::from_file_with_overrides(path, None, seed)
    }

    /// Create new declarative workload from YAML file with overrides
    pub fn from_file_with_overrides(
        path: &Path,
        overrides: Option<&serde_yaml::Value>,
        seed: u64,
    ) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::Error::Workload(format!("Failed to read workload file {}: {}", path.display(), e))
        })?;

        Self::from_yaml_with_overrides(&content, overrides, seed)
    }

    /// Create new declarative workload from YAML string
    pub fn from_yaml(yaml: &str, seed: u64) -> Result<Self> {
        Self::from_yaml_with_overrides(yaml, None, seed)
    }

    /// Create new declarative workload from YAML string with overrides
    pub fn from_yaml_with_overrides(
        yaml: &str,
        overrides: Option<&serde_yaml::Value>,
        seed: u64,
    ) -> Result<Self> {
        let definition: WorkloadDefinition = serde_yaml::from_str(yaml).map_err(|e| {
            crate::Error::Workload(format!("Failed to parse workload YAML: {}", e))
        })?;

        let mut spec = definition.workload;

        // Apply overrides if provided
        if let Some(overrides) = overrides {
            Self::apply_overrides(&mut spec, overrides)?;
        }

        Self::from_definition(spec, seed)
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
            sequential_counter: 0,
            variables,
            created_tables: Vec::new(),
        })
    }

    /// Apply overrides to workload specification
    fn apply_overrides(spec: &mut WorkloadSpec, overrides: &serde_yaml::Value) -> Result<()> {
        if let serde_yaml::Value::Mapping(override_map) = overrides {
            // Override operation weights
            if let Some(operations_override) = override_map.get(&serde_yaml::Value::String("operations".to_string())) {
                if let serde_yaml::Value::Sequence(ops_seq) = operations_override {
                    for op_override in ops_seq {
                        if let serde_yaml::Value::Mapping(op_map) = op_override {
                            // Get operation name
                            if let Some(serde_yaml::Value::String(name)) = op_map.get(&serde_yaml::Value::String("name".to_string())) {
                                // Find matching operation in spec
                                if let Some(op) = spec.operations.iter_mut().find(|o| &o.name == name) {
                                    // Override weight if provided
                                    if let Some(serde_yaml::Value::Number(weight)) = op_map.get(&serde_yaml::Value::String("weight".to_string())) {
                                        if let Some(w) = weight.as_u64() {
                                            op.weight = w as u32;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Override table parameters
            if let Some(schema_override) = override_map.get(&serde_yaml::Value::String("schema".to_string())) {
                if let serde_yaml::Value::Mapping(schema_map) = schema_override {
                    if let Some(tables_override) = schema_map.get(&serde_yaml::Value::String("tables".to_string())) {
                        if let serde_yaml::Value::Sequence(tables_seq) = tables_override {
                            for (idx, table_override) in tables_seq.iter().enumerate() {
                                if let serde_yaml::Value::Mapping(table_map) = table_override {
                                    if idx < spec.schema.tables.len() {
                                        let table = &mut spec.schema.tables[idx];

                                        // Override table count
                                        if let Some(serde_yaml::Value::Number(count)) = table_map.get(&serde_yaml::Value::String("count".to_string())) {
                                            if let Some(c) = count.as_u64() {
                                                table.count = c as usize;
                                            }
                                        }

                                        // Override row count
                                        if let Some(serde_yaml::Value::Number(row_count)) = table_map.get(&serde_yaml::Value::String("row_count".to_string())) {
                                            if let Some(rc) = row_count.as_u64() {
                                                table.row_count = rc as usize;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
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
            "sequential" => {
                if let Some(ref range) = dist.range {
                    let [min, max] = self.parse_range(range)?;
                    let range_size = (max - min + 1) as u64;
                    let value = min + (self.sequential_counter % range_size) as i64;
                    self.sequential_counter += 1;
                    Ok(Value::Int(value))
                } else {
                    Err(crate::Error::Workload(
                        "Sequential distribution requires 'range' parameter".into(),
                    ))
                }
            }
            "zipfian" | "zipf" => {
                if let Some(ref range) = dist.range {
                    let [min, max] = self.parse_range(range)?;
                    let n = (max - min + 1) as usize;

                    // Zipf distribution with exponent 1.0 (can be made configurable)
                    // Note: Zipf returns values in [1, n]
                    let zipf = Zipf::new(n as u64, 1.0).map_err(|e| {
                        crate::Error::Workload(format!("Failed to create Zipf distribution: {}", e))
                    })?;

                    let sample = zipf.sample(&mut self.rng) as i64;
                    let value = min + sample - 1; // Adjust to [min, max]
                    Ok(Value::Int(value))
                } else {
                    Err(crate::Error::Workload(
                        "Zipfian distribution requires 'range' parameter".into(),
                    ))
                }
            }
            "gaussian" | "normal" => {
                if let Some(ref range) = dist.range {
                    let [min, max] = self.parse_range(range)?;

                    // Use mean at center of range, stddev = range/6 (99.7% within range)
                    let mean = (min + max) as f64 / 2.0;
                    let stddev = (max - min) as f64 / 6.0;

                    let normal = Normal::new(mean, stddev).map_err(|e| {
                        crate::Error::Workload(format!("Failed to create Normal distribution: {}", e))
                    })?;

                    let sample = normal.sample(&mut self.rng);
                    // Clamp to range
                    let value = sample.round() as i64;
                    let value = value.max(min).min(max);
                    Ok(Value::Int(value))
                } else {
                    Err(crate::Error::Workload(
                        "Gaussian distribution requires 'range' parameter".into(),
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
                } else if let Some(length) = gen.length {
                    // Generate random string of specified length
                    let chars: String = (0..length)
                        .map(|_| {
                            let idx = self.rng.gen_range(0..62);
                            match idx {
                                0..=25 => (b'a' + idx) as char,
                                26..=51 => (b'A' + (idx - 26)) as char,
                                _ => (b'0' + (idx - 52)) as char,
                            }
                        })
                        .collect();
                    Ok(Value::String(chars))
                } else {
                    Err(crate::Error::Workload(
                        "String generator requires 'template' or 'length' parameter".into(),
                    ))
                }
            }
            "integer" => {
                if let Some(ref range) = gen.range {
                    let [min, max] = self.parse_range(range)?;
                    let value = self.rng.gen_range(min..=max);
                    Ok(Value::Int(value))
                } else {
                    // Generate random integer in full range
                    let value = self.rng.gen::<i64>();
                    Ok(Value::Int(value))
                }
            }
            "decimal" | "float" => {
                if let Some(ref range) = gen.range {
                    let [min, max] = self.parse_range(range)?;
                    let value = self.rng.gen_range(min as f64..=max as f64);

                    // Apply precision if specified
                    let value = if let Some(precision) = gen.precision {
                        let multiplier = 10_f64.powi(precision as i32);
                        (value * multiplier).round() / multiplier
                    } else {
                        value
                    };

                    Ok(Value::Float(value))
                } else {
                    Err(crate::Error::Workload(
                        "Decimal generator requires 'range' parameter".into(),
                    ))
                }
            }
            "choice" => {
                if let Some(ref choices) = gen.choices {
                    if choices.is_empty() {
                        return Err(crate::Error::Workload(
                            "Choice generator requires non-empty 'choices' list".into(),
                        ));
                    }

                    let idx = self.rng.gen_range(0..choices.len());
                    Ok(Value::String(choices[idx].clone()))
                } else {
                    Err(crate::Error::Workload(
                        "Choice generator requires 'choices' parameter".into(),
                    ))
                }
            }
            "uuid" => {
                // Generate a UUID-like string (simple version)
                let uuid = format!(
                    "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                    self.rng.gen::<u32>(),
                    self.rng.gen::<u16>(),
                    self.rng.gen::<u16>(),
                    self.rng.gen::<u16>(),
                    self.rng.gen::<u64>() & 0xFFFFFFFFFFFF
                );
                Ok(Value::String(uuid))
            }
            "custom" => {
                if let Some(ref value_str) = gen.value {
                    // Try to parse as different types
                    if let Ok(int_val) = value_str.parse::<i64>() {
                        Ok(Value::Int(int_val))
                    } else if let Ok(float_val) = value_str.parse::<f64>() {
                        Ok(Value::Float(float_val))
                    } else if value_str.eq_ignore_ascii_case("null") {
                        Ok(Value::Null)
                    } else {
                        // Treat as string
                        Ok(Value::String(value_str.clone()))
                    }
                } else {
                    Err(crate::Error::Workload(
                        "Custom generator requires 'value' parameter".into(),
                    ))
                }
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

    /// Load data into a table based on data generation configuration
    fn load_table_data(
        &mut self,
        ctx: &mut PrepareContext,
        table_name: &str,
        table_def: &TableDefinition,
    ) -> Result<()> {
        let row_count = table_def.row_count;
        if row_count == 0 {
            return Ok(());
        }

        // Get data generation strategy (default to uniform)
        let strategy = self
            .spec
            .data_generation
            .as_ref()
            .map(|dg| dg.strategy.clone())
            .unwrap_or_else(|| "uniform".to_string());

        eprintln!(
            "Loading {} rows into table {} using {} strategy...",
            row_count, table_name, strategy
        );

        // Batch size for INSERT statements (adjust based on database limits)
        const BATCH_SIZE: usize = 1000;

        // Clone columns to avoid borrow checker issues
        let columns = table_def.columns.clone();

        // Generate and insert data in batches
        for batch_start in (0..row_count).step_by(BATCH_SIZE) {
            let batch_end = (batch_start + BATCH_SIZE).min(row_count);

            // Build batch INSERT statement
            let column_names: Vec<&str> = columns
                .iter()
                .filter(|col| !col.auto_increment) // Skip auto-increment columns
                .map(|col| col.name.as_str())
                .collect();

            let mut insert_sql = format!(
                "INSERT INTO {} ({}) VALUES ",
                table_name,
                column_names.join(", ")
            );

            let mut value_sets = Vec::new();

            // Generate values for each row in batch
            for row_idx in batch_start..batch_end {
                let mut values = Vec::new();

                for col in &columns {
                    if col.auto_increment {
                        continue; // Skip auto-increment columns
                    }

                    let value = self.generate_column_value(col, row_idx, &strategy)?;
                    values.push(value);
                }

                let value_str = values
                    .iter()
                    .map(|v| self.value_to_sql_literal(v))
                    .collect::<Vec<_>>()
                    .join(", ");

                value_sets.push(format!("({})", value_str));
            }

            insert_sql.push_str(&value_sets.join(", "));

            // Execute batch INSERT
            ctx.database.execute(&insert_sql)?;
        }

        Ok(())
    }

    /// Generate a value for a column during data loading
    fn generate_column_value(
        &mut self,
        col: &ColumnDefinition,
        row_idx: usize,
        strategy: &str,
    ) -> Result<Value> {
        match col.column_type.to_uppercase().as_str() {
            t if t.starts_with("INT") || t.starts_with("BIGINT") || t.starts_with("SMALLINT") => {
                if col.primary_key && !col.auto_increment {
                    // Primary key: sequential values starting from 1
                    Ok(Value::Int((row_idx + 1) as i64))
                } else {
                    // Apply distribution strategy for non-PK integers
                    self.generate_integer_with_strategy(strategy, 1, 1000000)
                }
            }
            t if t.starts_with("VARCHAR") || t.starts_with("CHAR") => {
                // Generate string based on column length
                let length = if let Some(start) = t.find('(') {
                    if let Some(end) = t.find(')') {
                        t[start + 1..end].parse::<usize>().unwrap_or(50)
                    } else {
                        50
                    }
                } else {
                    50
                };

                // Generate random alphanumeric string
                let content_length = length.min(120); // Cap at 120 for performance
                let value: String = (0..content_length)
                    .map(|_| {
                        let idx = self.rng.gen_range(0..62);
                        match idx {
                            0..=25 => (b'a' + idx) as char,
                            26..=51 => (b'A' + (idx - 26)) as char,
                            _ => (b'0' + (idx - 52)) as char,
                        }
                    })
                    .collect();

                Ok(Value::String(value))
            }
            t if t.starts_with("DECIMAL") || t.starts_with("FLOAT") || t.starts_with("DOUBLE") => {
                Ok(Value::Float(self.rng.gen_range(0.0..1000000.0)))
            }
            t if t.starts_with("TEXT") => {
                // Generate short text for TEXT columns
                let value: String = (0..100)
                    .map(|_| {
                        let idx = self.rng.gen_range(0..62);
                        match idx {
                            0..=25 => (b'a' + idx) as char,
                            26..=51 => (b'A' + (idx - 26)) as char,
                            _ => (b'0' + (idx - 52)) as char,
                        }
                    })
                    .collect();
                Ok(Value::String(value))
            }
            _ => {
                // Default: use column default or NULL
                if let Some(default) = &col.default {
                    Ok(Value::String(default.clone()))
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    /// Generate an integer value using the specified distribution strategy
    fn generate_integer_with_strategy(&mut self, strategy: &str, min: i64, max: i64) -> Result<Value> {
        match strategy {
            "uniform" => {
                Ok(Value::Int(self.rng.gen_range(min..=max)))
            }
            "zipfian" | "zipf" => {
                let n = (max - min + 1) as usize;
                let zipf = Zipf::new(n as u64, 1.0).map_err(|e| {
                    crate::Error::Workload(format!("Failed to create Zipf distribution: {}", e))
                })?;
                let sample = zipf.sample(&mut self.rng) as i64;
                let value = min + sample - 1;
                Ok(Value::Int(value))
            }
            "gaussian" | "normal" => {
                let mean = (min + max) as f64 / 2.0;
                let stddev = (max - min) as f64 / 6.0;

                let normal = Normal::new(mean, stddev).map_err(|e| {
                    crate::Error::Workload(format!("Failed to create Normal distribution: {}", e))
                })?;

                let sample = normal.sample(&mut self.rng);
                let value = sample.round() as i64;
                let value = value.max(min).min(max);
                Ok(Value::Int(value))
            }
            "sequential" => {
                let range_size = (max - min + 1) as u64;
                let value = min + (self.sequential_counter % range_size) as i64;
                self.sequential_counter += 1;
                Ok(Value::Int(value))
            }
            _ => {
                // Default to uniform if unknown strategy
                Ok(Value::Int(self.rng.gen_range(min..=max)))
            }
        }
    }

    /// Convert a Value to SQL literal for INSERT statements
    fn value_to_sql_literal(&self, value: &Value) -> String {
        match value {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => format!("'{}'", s.replace('\'', "''")), // Escape single quotes
            Value::Bytes(b) => {
                // Convert bytes to hex string
                let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
                format!("X'{}'", hex)
            }
            Value::Null => "NULL".to_string(),
        }
    }
}

impl Workload for DeclarativeWorkload {
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()> {
        // Clone table definitions to avoid borrow checker issues
        let table_defs = self.spec.schema.tables.clone();

        // Create tables based on schema definition
        for table_def in &table_defs {
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

                // Track created table for cleanup
                self.created_tables.push(table_name.clone());

                // Create indexes
                for (index_name, col_name) in indexes {
                    let index_sql = format!(
                        "CREATE INDEX {} ON {} ({})",
                        index_name, table_name, col_name
                    );
                    ctx.database.execute(&index_sql)?;
                }

                // Load data based on data_generation config
                self.load_table_data(ctx, &table_name, table_def)?;
            }
        }

        Ok(())
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        // Select operation based on weights
        let op_index = self.operation_weights.sample(&mut self.rng);

        // Clone the operation definition to avoid borrow checker issues
        let op_def = self.spec.operations[op_index].clone();

        // Determine operation type
        let operation_type = match op_def.operation_type.as_str() {
            "read" => OperationType::Read,
            "write" => OperationType::Write,
            _ => OperationType::Read, // Default to read
        };

        // Check if this is a transaction operation
        if let Some(ref tx_ops) = op_def.transaction_operations {
            // Generate transaction operation
            let mut transaction_sqls = Vec::new();
            let mut transaction_params = Vec::new();

            for sub_op in tx_ops {
                // Generate parameters for this sub-operation
                let mut sub_params = Vec::new();
                let mut named_params: Vec<(String, Value)> = Vec::new();

                for param_def in &sub_op.parameters {
                    let value = self.generate_parameter(param_def, ctx)?;
                    named_params.push((param_def.name.clone(), value.clone()));
                    sub_params.push(value);
                }

                // Build SQL with named parameter substitutions
                let param_refs: Vec<(&str, &Value)> = named_params.iter()
                    .map(|(name, value)| (name.as_str(), value))
                    .collect();
                let sql = self.build_sql(&sub_op.sql, &param_refs);

                transaction_sqls.push(sql);
                transaction_params.push(sub_params);
            }

            Ok(Operation {
                name: op_def.name,
                sql: String::new(), // Not used for transactions
                params: Vec::new(), // Not used for transactions
                operation_type,
                is_transaction: true,
                transaction_sqls,
                transaction_params,
            })
        } else {
            // Single operation (non-transaction)
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

            Ok(Operation {
                name: op_def.name,
                sql,
                params,
                operation_type,
                is_transaction: false,
                transaction_sqls: Vec::new(),
                transaction_params: Vec::new(),
            })
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        // Note: cleanup() doesn't have database access, so we can only log
        // what should be cleaned up. In a future version, cleanup should
        // receive a PrepareContext parameter.

        if let Some(cleanup_cfg) = &self.spec.cleanup_config {
            if cleanup_cfg.drop_tables && !self.created_tables.is_empty() {
                eprintln!("Cleanup: The following tables should be dropped:");
                for table in &self.created_tables {
                    eprintln!("  DROP TABLE IF EXISTS {};", table);
                }
                eprintln!("Note: Automatic table cleanup requires database access.");
                eprintln!("      Run the DROP TABLE commands manually if needed.");
            }
        }

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

    #[test]
    fn test_sequential_distribution() {
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
            type: sequential
            range: [1, 10]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        // Generate multiple operations and check sequential behavior
        let mut values = Vec::new();
        for _ in 0..15 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::Int(val) = op.params[0] {
                values.push(val);
            }
        }

        // Should go 1,2,3...10,1,2,3...10,1,2,3,4,5
        assert_eq!(values[0], 1);
        assert_eq!(values[9], 10);
        assert_eq!(values[10], 1); // Wraps around
        assert_eq!(values[14], 5);
    }

    #[test]
    fn test_zipfian_distribution() {
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
            type: zipfian
            range: [1, 100]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        // Generate many operations and check they're within range
        let mut values = Vec::new();
        for _ in 0..100 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::Int(val) = op.params[0] {
                values.push(val);
            }
        }

        // All values should be within range
        for val in &values {
            assert!(*val >= 1 && *val <= 100, "Value {} out of range", val);
        }

        // Zipfian should favor lower values (hot keys)
        // Count how many times we see values 1-10 vs 91-100
        let low_count = values.iter().filter(|&&v| v <= 10).count();
        let high_count = values.iter().filter(|&&v| v >= 91).count();

        // Lower values should be significantly more common in Zipfian distribution
        assert!(low_count > high_count, "Zipfian should favor lower values");
    }

    #[test]
    fn test_gaussian_distribution() {
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
            type: gaussian
            range: [1, 100]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        // Generate many operations
        let mut values = Vec::new();
        for _ in 0..100 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::Int(val) = op.params[0] {
                values.push(val);
            }
        }

        // All values should be within range (clamped)
        for val in &values {
            assert!(*val >= 1 && *val <= 100, "Value {} out of range", val);
        }

        // Calculate mean - should be around 50 for gaussian(1, 100)
        let mean = values.iter().sum::<i64>() as f64 / values.len() as f64;
        assert!(mean > 30.0 && mean < 70.0, "Mean {} should be near center", mean);
    }

    #[test]
    fn test_decimal_generator() {
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
      sql: "UPDATE test SET price = ? WHERE id = 1"
      parameters:
        - name: price
          generator:
            type: decimal
            range: [0, 1000]
            precision: 2
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::Float(f) = op.params[0] {
            assert!(f >= 0.0 && f <= 1000.0, "Value {} out of range", f);
            // Check precision (2 decimal places) - allow small floating point errors
            let rounded = (f * 100.0).round() / 100.0;
            assert!((f - rounded).abs() < 0.001, "Should have at most 2 decimal places, got {}", f);
        } else {
            panic!("Expected Float value");
        }
    }

    #[test]
    fn test_choice_generator() {
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
      sql: "UPDATE test SET status = ? WHERE id = 1"
      parameters:
        - name: status
          generator:
            type: choice
            choices: ["active", "inactive", "pending"]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        // Generate multiple values and check they're all valid choices
        for _ in 0..20 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::String(s) = &op.params[0] {
                assert!(
                    s == "active" || s == "inactive" || s == "pending",
                    "Invalid choice: {}",
                    s
                );
            } else {
                panic!("Expected String value");
            }
        }
    }

    #[test]
    fn test_uuid_generator() {
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
    - name: insert
      weight: 100
      type: write
      sql: "INSERT INTO test (uuid) VALUES (?)"
      parameters:
        - name: uuid
          generator:
            type: uuid
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::String(s) = &op.params[0] {
            // Check UUID format (8-4-4-4-12)
            let parts: Vec<&str> = s.split('-').collect();
            assert_eq!(parts.len(), 5, "UUID should have 5 parts");
            assert_eq!(parts[0].len(), 8, "First part should be 8 chars");
            assert_eq!(parts[1].len(), 4, "Second part should be 4 chars");
            assert_eq!(parts[2].len(), 4, "Third part should be 4 chars");
            assert_eq!(parts[3].len(), 4, "Fourth part should be 4 chars");
            assert_eq!(parts[4].len(), 12, "Fifth part should be 12 chars");
        } else {
            panic!("Expected String value");
        }
    }

    #[test]
    fn test_integer_generator_with_range() {
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
      sql: "UPDATE test SET age = ? WHERE id = 1"
      parameters:
        - name: age
          generator:
            type: integer
            range: [18, 65]
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        for _ in 0..20 {
            let op = workload.next_operation(&ctx).unwrap();
            if let Value::Int(val) = op.params[0] {
                assert!(val >= 18 && val <= 65, "Value {} out of range", val);
            } else {
                panic!("Expected Int value");
            }
        }
    }

    #[test]
    fn test_string_generator_with_length() {
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
      sql: "UPDATE test SET token = ? WHERE id = 1"
      parameters:
        - name: token
          generator:
            type: string
            length: 32
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::String(s) = &op.params[0] {
            assert_eq!(s.len(), 32, "String should be 32 characters");
            // Check it's alphanumeric
            assert!(s.chars().all(|c| c.is_alphanumeric()), "Should be alphanumeric");
        } else {
            panic!("Expected String value");
        }
    }

    #[test]
    fn test_integer_with_strategy_uniform() {
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
      sql: "SELECT 1"
      parameters: []
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();

        // Test uniform strategy
        for _ in 0..10 {
            let val = workload.generate_integer_with_strategy("uniform", 1, 100).unwrap();
            if let Value::Int(v) = val {
                assert!(v >= 1 && v <= 100, "Uniform value {} out of range", v);
            } else {
                panic!("Expected Int value");
            }
        }
    }

    #[test]
    fn test_integer_with_strategy_sequential() {
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
      sql: "SELECT 1"
      parameters: []
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();

        // Test sequential strategy
        let val1 = workload.generate_integer_with_strategy("sequential", 1, 10).unwrap();
        let val2 = workload.generate_integer_with_strategy("sequential", 1, 10).unwrap();
        let val3 = workload.generate_integer_with_strategy("sequential", 1, 10).unwrap();

        if let (Value::Int(v1), Value::Int(v2), Value::Int(v3)) = (val1, val2, val3) {
            // Sequential should increment
            assert_eq!(v1, 1, "First value should be 1");
            assert_eq!(v2, 2, "Second value should be 2");
            assert_eq!(v3, 3, "Third value should be 3");
        } else {
            panic!("Expected Int values");
        }
    }

    #[test]
    fn test_integer_with_strategy_zipfian() {
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
      sql: "SELECT 1"
      parameters: []
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();

        // Test zipfian strategy
        let mut values = Vec::new();
        for _ in 0..100 {
            let val = workload.generate_integer_with_strategy("zipfian", 1, 100).unwrap();
            if let Value::Int(v) = val {
                assert!(v >= 1 && v <= 100, "Zipfian value {} out of range", v);
                values.push(v);
            } else {
                panic!("Expected Int value");
            }
        }

        // Zipfian should favor lower values
        let low_count = values.iter().filter(|&&v| v <= 20).count();
        let high_count = values.iter().filter(|&&v| v >= 81).count();
        assert!(low_count > high_count, "Zipfian should favor lower values");
    }

    #[test]
    fn test_integer_with_strategy_gaussian() {
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
      sql: "SELECT 1"
      parameters: []
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();

        // Test gaussian strategy
        let mut values = Vec::new();
        for _ in 0..100 {
            let val = workload.generate_integer_with_strategy("gaussian", 1, 100).unwrap();
            if let Value::Int(v) = val {
                assert!(v >= 1 && v <= 100, "Gaussian value {} out of range", v);
                values.push(v);
            } else {
                panic!("Expected Int value");
            }
        }

        // Gaussian should favor middle values (around 50)
        let middle_count = values.iter().filter(|&&v| v >= 40 && v <= 60).count();
        // Should have reasonable concentration in middle
        assert!(middle_count > 20, "Gaussian should favor middle values, got {} middle values", middle_count);
    }

    #[test]
    fn test_cleanup_config_drop_tables() {
        let yaml = r#"
workload:
  name: test
  schema:
    tables:
      - name: test_table
        count: 2
        row_count: 10
        columns:
          - name: id
            type: INT
            primary_key: true
  cleanup_config:
    drop_tables: true
  operations:
    - name: select
      weight: 100
      type: read
      sql: "SELECT 1"
      parameters: []
"#;

        let workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        assert!(workload.spec.cleanup_config.is_some());
        assert!(workload.spec.cleanup_config.as_ref().unwrap().drop_tables);
    }

    #[test]
    fn test_custom_generator_integer() {
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
      sql: "SELECT * FROM test WHERE status = ?"
      parameters:
        - name: status
          generator:
            type: custom
            value: "42"
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::Int(val) = op.params[0] {
            assert_eq!(val, 42, "Custom generator should return fixed integer value");
        } else {
            panic!("Expected Int value, got {:?}", op.params[0]);
        }
    }

    #[test]
    fn test_custom_generator_string() {
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
      sql: "SELECT * FROM test WHERE name = ?"
      parameters:
        - name: name
          generator:
            type: custom
            value: "test_user"
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::String(val) = &op.params[0] {
            assert_eq!(val, "test_user", "Custom generator should return fixed string value");
        } else {
            panic!("Expected String value, got {:?}", op.params[0]);
        }
    }

    #[test]
    fn test_custom_generator_float() {
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
      sql: "SELECT * FROM test WHERE price = ?"
      parameters:
        - name: price
          generator:
            type: custom
            value: "99.99"
"#;

        let mut workload = DeclarativeWorkload::from_yaml(yaml, 42).unwrap();
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration: 1,
            elapsed: std::time::Duration::from_secs(0),
        };

        let op = workload.next_operation(&ctx).unwrap();
        if let Value::Float(val) = op.params[0] {
            assert!((val - 99.99).abs() < 0.001, "Custom generator should return fixed float value");
        } else {
            panic!("Expected Float value, got {:?}", op.params[0]);
        }
    }

    #[test]
    fn test_transaction_operation() {
        let yaml = r#"
workload:
  name: test
  schema:
    tables:
      - name: accounts
        count: 1
        row_count: 100
        columns:
          - name: id
            type: INT
            primary_key: true
          - name: balance
            type: DECIMAL(10,2)
  operations:
    - name: transfer
      weight: 100
      type: write
      transaction_operations:
        - name: debit
          sql: "UPDATE accounts SET balance = balance - ? WHERE id = ?"
          parameters:
            - name: amount
              generator:
                type: decimal
                range: [1, 100]
                precision: 2
            - name: from_id
              distribution:
                type: uniform
                range: [1, 100]
        - name: credit
          sql: "UPDATE accounts SET balance = balance + ? WHERE id = ?"
          parameters:
            - name: amount
              generator:
                type: decimal
                range: [1, 100]
                precision: 2
            - name: to_id
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

        // Should be a transaction
        assert!(op.is_transaction, "Operation should be a transaction");
        assert_eq!(op.name, "transfer");
        assert_eq!(op.transaction_sqls.len(), 2, "Should have 2 SQL statements");
        assert_eq!(op.transaction_params.len(), 2, "Should have 2 parameter sets");

        // Check first sub-operation (debit)
        assert!(op.transaction_sqls[0].contains("balance - ?"));
        assert_eq!(op.transaction_params[0].len(), 2);

        // Check second sub-operation (credit)
        assert!(op.transaction_sqls[1].contains("balance + ?"));
        assert_eq!(op.transaction_params[1].len(), 2);
    }

    #[test]
    fn test_override_operation_weights() {
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
    - name: read_op
      weight: 50
      type: read
      sql: "SELECT * FROM test WHERE id = ?"
      parameters:
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
    - name: write_op
      weight: 50
      type: write
      sql: "UPDATE test SET val = ? WHERE id = ?"
      parameters:
        - name: val
          generator:
            type: integer
            range: [1, 1000]
        - name: id
          distribution:
            type: uniform
            range: [1, 100]
"#;

        let overrides_yaml = r#"
operations:
  - name: read_op
    weight: 90
  - name: write_op
    weight: 10
"#;

        let overrides: serde_yaml::Value = serde_yaml::from_str(overrides_yaml).unwrap();

        let workload = DeclarativeWorkload::from_yaml_with_overrides(yaml, Some(&overrides), 42).unwrap();

        // Check that weights were overridden
        assert_eq!(workload.spec.operations[0].weight, 90);
        assert_eq!(workload.spec.operations[1].weight, 10);
    }

    #[test]
    fn test_override_table_parameters() {
        let yaml = r#"
workload:
  name: test
  schema:
    tables:
      - name: test
        count: 1
        row_count: 1000
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

        let overrides_yaml = r#"
schema:
  tables:
    - count: 5
      row_count: 10000
"#;

        let overrides: serde_yaml::Value = serde_yaml::from_str(overrides_yaml).unwrap();

        let workload = DeclarativeWorkload::from_yaml_with_overrides(yaml, Some(&overrides), 42).unwrap();

        // Check that table parameters were overridden
        assert_eq!(workload.spec.schema.tables[0].count, 5);
        assert_eq!(workload.spec.schema.tables[0].row_count, 10000);
    }
}
