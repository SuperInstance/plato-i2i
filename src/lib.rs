use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single I2I inter-agent message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct I2IMessage {
    /// Identifier of the sending agent.
    pub sender: String,
    /// Identifier of the intended receiving agent.
    pub recipient: String,
    /// Logical message type (e.g., `QUERY`, `RESPONSE`).
    pub msg_type: String,
    /// Payload of the message.
    pub content: String,
    /// Priority level -- `P0`, `P1`, or `P2` (lowest).
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Unix epoch timestamp (float) when the message was created.
    #[serde(default = "current_timestamp")]
    pub timestamp: f64,
    /// Additional key/value context attached to the message.
    #[serde(default = "HashMap::new")]
    pub metadata: HashMap<String, serde_json::Value>,
}

fn default_priority() -> String {
    "P2".to_string()
}

fn current_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

impl I2IMessage {
    /// Create a new I2IMessage with the current timestamp.
    pub fn new(
        sender: impl Into<String>,
        recipient: impl Into<String>,
        msg_type: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            sender: sender.into(),
            recipient: recipient.into(),
            msg_type: msg_type.into(),
            content: content.into(),
            priority: default_priority(),
            timestamp: current_timestamp(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new I2IMessage with a specific priority.
    pub fn with_priority(
        sender: impl Into<String>,
        recipient: impl Into<String>,
        msg_type: impl Into<String>,
        content: impl Into<String>,
        priority: impl Into<String>,
    ) -> Self {
        Self {
            sender: sender.into(),
            recipient: recipient.into(),
            msg_type: msg_type.into(),
            content: content.into(),
            priority: priority.into(),
            timestamp: current_timestamp(),
            metadata: HashMap::new(),
        }
    }
}

/// Message formatting, parsing, serialization, and validation utilities.
pub struct I2IProtocol;

impl I2IProtocol {
    /// Create a fully populated [`I2IMessage`].
    pub fn format_message(
        sender: impl Into<String>,
        recipient: impl Into<String>,
        msg_type: impl Into<String>,
        content: impl Into<String>,
        priority: impl Into<String>,
    ) -> I2IMessage {
        I2IMessage::with_priority(sender, recipient, msg_type, content, priority)
    }

    /// Parse a human-readable `[I2I:TYPE] sender -> recipient — content` string.
    ///
    /// # Errors
    /// Returns an error if *raw* does not conform to the expected pattern.
    pub fn parse_message(raw: &str) -> Result<I2IMessage, String> {
        if !raw.starts_with("[I2I:") {
            return Err("Message must start with '[I2I:'".to_string());
        }

        let type_end = raw[5..].find(']').ok_or("Missing closing ']' for message type")? + 5;
        let msg_type = raw[5..type_end].trim().to_string();
        let remainder = raw[type_end + 1..].trim();

        let arrow = remainder.find("->").ok_or("Missing '->' separator")?;
        let sender = remainder[..arrow].trim().to_string();
        let after_arrow = remainder[arrow + 2..].trim();

        let em_dash = after_arrow.find('—');
        let (recipient, content) = match em_dash {
            Some(idx) => (
                after_arrow[..idx].trim().to_string(),
                after_arrow[idx + 1..].trim().to_string(),
            ),
            None => {
                let fallback = after_arrow.find("- ").ok_or("Missing '—' content separator")?;
                (
                    after_arrow[..fallback].trim().to_string(),
                    after_arrow[fallback + 1..].trim().to_string(),
                )
            }
        };

        Ok(I2IMessage {
            sender,
            recipient,
            msg_type,
            content,
            priority: default_priority(),
            timestamp: current_timestamp(),
            metadata: HashMap::new(),
        })
    }

    /// Convert an [`I2IMessage`] to a wire-format JSON string.
    pub fn serialize(message: &I2IMessage) -> String {
        serde_json::to_string(message).unwrap_or_default()
    }

    /// Reconstruct an [`I2IMessage`] from a JSON wire-format string.
    ///
    /// # Errors
    /// Returns an error if *raw* is not valid JSON or missing required keys.
    pub fn deserialize(raw: &str) -> Result<I2IMessage, String> {
        let payload: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("Invalid JSON: {e}"))?;

        let required = ["sender", "recipient", "msg_type", "content"];
        for key in &required {
            if payload.get(key).is_none() {
                return Err(format!("Missing required field: {key}"));
            }
        }

        let message: I2IMessage = serde_json::from_value(payload)
            .map_err(|e| format!("Invalid message structure: {e}"))?;
        Ok(message)
    }

    /// Check whether *message* has all required fields populated.
    ///
    /// A field is considered populated when it is a non-empty string.
    pub fn validate(message: &I2IMessage) -> bool {
        !message.sender.trim().is_empty()
            && !message.recipient.trim().is_empty()
            && !message.msg_type.trim().is_empty()
            && !message.content.trim().is_empty()
    }

    /// Block messages whose priority is below the supplied threshold.
    ///
    /// Lower numeric rank == higher priority (`P0` > `P1` > `P2`).
    pub fn priority_gate(message: &I2IMessage, min_priority: &str) -> bool {
        let rank = |p: &str| match p {
            "P0" => 0,
            "P1" => 1,
            "P2" => 2,
            _ => 99,
        };
        rank(&message.priority) <= rank(min_priority)
    }
}

/// Routes messages to the most trusted agent among a set of candidates.
///
/// Trust levels are floats in the inclusive range `0.0` to `1.0`.
#[derive(Debug, Clone, Default)]
pub struct TrustRouter {
    trust: HashMap<String, f64>,
}

impl TrustRouter {
    /// Initialise an empty trust table.
    pub fn new() -> Self {
        Self {
            trust: HashMap::new(),
        }
    }

    /// Register or update an agent's trust level.
    ///
    /// # Errors
    /// Returns an error if *trust_level* is outside the `[0.0, 1.0]` range.
    pub fn add_trust(&mut self, agent_id: impl Into<String>, trust_level: f64) -> Result<(), String> {
        if !(0.0..=1.0).contains(&trust_level) {
            return Err("trust_level must be between 0.0 and 1.0".to_string());
        }
        self.trust.insert(agent_id.into(), trust_level);
        Ok(())
    }

    /// Return the stored trust level for *agent_id*.
    ///
    /// Returns `0.0` if the agent is not known.
    pub fn get_trust(&self, agent_id: &str) -> f64 {
        self.trust.get(agent_id).copied().unwrap_or(0.0)
    }

    /// Pick the most-trusted agent from *available_agents*.
    ///
    /// Returns the identifier of the most trusted available agent, or `None` if
    /// no candidates are supplied or none are trusted.
    pub fn route(&self, _message: &I2IMessage, available_agents: &[String]) -> Option<String> {
        if available_agents.is_empty() {
            return None;
        }

        let mut best_agent: Option<String> = None;
        let mut best_score = -1.0;

        for agent in available_agents {
            let score = self.get_trust(agent);
            if score > best_score {
                best_score = score;
                best_agent = Some(agent.clone());
            }
        }

        if best_score <= 0.0 {
            return None;
        }

        best_agent
    }

    /// Check whether *agent_id* meets the minimum trust threshold.
    pub fn is_trusted(&self, agent_id: &str, min_trust: f64) -> bool {
        self.get_trust(agent_id) >= min_trust
    }

    /// Return aggregate statistics for the trust table.
    pub fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();
        if self.trust.is_empty() {
            result.insert("count".to_string(), serde_json::json!(0));
            result.insert("average_trust".to_string(), serde_json::json!(0.0));
            result.insert("max_trust".to_string(), serde_json::json!(0.0));
            result.insert("min_trust".to_string(), serde_json::json!(0.0));
            return result;
        }

        let values: Vec<f64> = self.trust.values().copied().collect();
        let count = values.len() as i64;
        let sum: f64 = values.iter().sum();
        let avg = sum / values.len() as f64;
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));

        result.insert("count".to_string(), serde_json::json!(count));
        result.insert("average_trust".to_string(), serde_json::json!(avg));
        result.insert("max_trust".to_string(), serde_json::json!(max));
        result.insert("min_trust".to_string(), serde_json::json!(min));
        result
    }
}
