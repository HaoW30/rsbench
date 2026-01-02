//! Event configuration types
//!
//! Configuration structures for event sources and event manager.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for all event sources
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EventConfig {
    /// List of event sources to enable
    #[serde(default)]
    pub sources: Vec<EventSourceConfig>,
}

/// Configuration for a single event source
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum EventSourceConfig {
    /// Timer-based event source
    #[serde(rename = "timer")]
    Timer {
        /// Schedule of events to emit
        schedule: Vec<TimerEventConfig>,
    },

    // Future event sources (M1+)
    // #[cfg(feature = "k8s")]
    // #[serde(rename = "k8s")]
    // K8s { ... },
    //
    // #[cfg(feature = "webhook")]
    // #[serde(rename = "webhook")]
    // Webhook { ... },
}

/// Configuration for a single timer event
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimerEventConfig {
    /// Time offset from start (e.g., "0s", "30s", "1m", "1h")
    pub at: String,

    /// Event to emit at this time
    pub event: EventTypeConfig,
}

/// Configuration for event types
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum EventTypeConfig {
    /// Change scenario rate
    #[serde(rename = "rate_change")]
    RateChange {
        /// New rate in operations per second
        rate: u64,
    },

    /// Phase transition (pause, resume, shutdown)
    #[serde(rename = "phase_transition")]
    PhaseTransition {
        /// Phase to transition to ("pause", "resume", "shutdown")
        phase: String,
    },

    /// Trigger metrics snapshot
    #[serde(rename = "metrics_snapshot")]
    MetricsSnapshot,

    /// Custom event with arbitrary data
    #[serde(rename = "custom")]
    Custom {
        /// Custom event data
        data: serde_json::Value,
    },
}

impl EventConfig {
    /// Check if any event sources are configured
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Count of event sources
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
}

/// Parse duration string (e.g., "30s", "1m", "1h")
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();

    if s.is_empty() {
        return Err("Empty duration string".to_string());
    }

    // Find where the number ends
    let num_end = s.chars().position(|c| !c.is_ascii_digit())
        .unwrap_or(s.len());

    if num_end == 0 {
        return Err(format!("Invalid duration: '{}'", s));
    }

    let num_str = &s[..num_end];
    let unit_str = &s[num_end..];

    let num: u64 = num_str.parse()
        .map_err(|_| format!("Invalid number in duration: '{}'", num_str))?;

    let duration = match unit_str {
        "s" | "sec" | "second" | "seconds" => Duration::from_secs(num),
        "ms" | "millis" | "millisecond" | "milliseconds" => Duration::from_millis(num),
        "m" | "min" | "minute" | "minutes" => Duration::from_secs(num * 60),
        "h" | "hour" | "hours" => Duration::from_secs(num * 3600),
        "" => Duration::from_secs(num), // Default to seconds if no unit
        _ => return Err(format!("Unknown duration unit: '{}'", unit_str)),
    };

    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("1sec").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("60seconds").unwrap(), Duration::from_secs(60));
    }

    #[test]
    fn test_parse_duration_milliseconds() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("1000millis").unwrap(), Duration::from_millis(1000));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1min").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("2minutes").unwrap(), Duration::from_secs(120));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("2hours").unwrap(), Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_duration_no_unit() {
        // Default to seconds
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn test_parse_duration_whitespace() {
        assert_eq!(parse_duration("  30s  ").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("30x").is_err());
        assert!(parse_duration("-5s").is_err());
    }

    #[test]
    fn test_event_config_empty() {
        let config = EventConfig::default();
        assert!(config.is_empty());
        assert_eq!(config.source_count(), 0);
    }

    #[test]
    fn test_event_config_deserialization() {
        let yaml = r#"
sources:
  - type: timer
    schedule:
      - at: "0s"
        event:
          type: rate_change
          rate: 100
      - at: "30s"
        event:
          type: phase_transition
          phase: "shutdown"
"#;

        let config: EventConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.source_count(), 1);
        assert!(!config.is_empty());

        match &config.sources[0] {
            EventSourceConfig::Timer { schedule } => {
                assert_eq!(schedule.len(), 2);
                assert_eq!(schedule[0].at, "0s");
                assert_eq!(schedule[1].at, "30s");
            }
        }
    }

    #[test]
    fn test_timer_event_config_all_types() {
        let yaml = r#"
sources:
  - type: timer
    schedule:
      - at: "0s"
        event:
          type: rate_change
          rate: 1000
      - at: "10s"
        event:
          type: phase_transition
          phase: "pause"
      - at: "20s"
        event:
          type: metrics_snapshot
      - at: "30s"
        event:
          type: custom
          data: {"action": "checkpoint"}
"#;

        let config: EventConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.source_count(), 1);

        match &config.sources[0] {
            EventSourceConfig::Timer { schedule } => {
                assert_eq!(schedule.len(), 4);

                // Check all event types
                assert!(matches!(schedule[0].event, EventTypeConfig::RateChange { .. }));
                assert!(matches!(schedule[1].event, EventTypeConfig::PhaseTransition { .. }));
                assert!(matches!(schedule[2].event, EventTypeConfig::MetricsSnapshot));
                assert!(matches!(schedule[3].event, EventTypeConfig::Custom { .. }));
            }
        }
    }

    #[test]
    fn test_empty_sources_deserialization() {
        let yaml = r#"
sources: []
"#;
        let config: EventConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.is_empty());
    }
}
