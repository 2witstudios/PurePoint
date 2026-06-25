use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Broken,
}

impl AgentStatus {
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Running)
    }
}

impl Serialize for AgentStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = match self {
            AgentStatus::Running => "running",
            AgentStatus::Broken => "broken",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for AgentStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "running" => Ok(AgentStatus::Running),
            "broken" => Ok(AgentStatus::Broken),
            // Backward compat: map old status values
            "streaming" | "waiting" | "spawning" | "idle" | "suspended" => Ok(AgentStatus::Running),
            "completed" | "failed" | "killed" | "lost" | "stopped" => Ok(AgentStatus::Broken),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["running", "broken"],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerState {
    Active,
    Gating,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: AgentStatus,
    pub prompt: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspended_at: Option<DateTime<Utc>>,
    /// Whether this agent is currently suspended. Only meaningful when `status.is_alive()`.
    /// Invariant: custom Deserialize infers `true` when `suspended_at` is present (backward
    /// compat with old manifests). The engine sets both `suspended` and `suspended_at`
    /// atomically on suspend/resume.
    #[serde(default)]
    pub suspended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plan_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_seq_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_state: Option<TriggerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "crate::serde_defaults::is_false")]
    pub no_trigger: bool,
    /// Name of the trigger definition this agent is bound to.
    /// Set at spawn time so the sequence is stable across ticks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_name: Option<String>,
}

impl<'de> Deserialize<'de> for AgentEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            id: String,
            name: String,
            agent_type: String,
            status: AgentStatus,
            prompt: Option<String>,
            started_at: DateTime<Utc>,
            completed_at: Option<DateTime<Utc>>,
            exit_code: Option<i32>,
            error: Option<String>,
            pid: Option<u32>,
            session_id: Option<String>,
            suspended_at: Option<DateTime<Utc>>,
            #[serde(default)]
            suspended: bool,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            plan_mode: bool,
            #[serde(default)]
            trigger_seq_index: Option<u32>,
            #[serde(default)]
            trigger_state: Option<TriggerState>,
            #[serde(default)]
            trigger_total: Option<u32>,
            #[serde(default)]
            gate_attempts: Option<u32>,
            #[serde(default)]
            no_trigger: bool,
            #[serde(default)]
            trigger_name: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // Backward compat: old manifests have suspended_at set but no suspended field.
        // Ensure suspended is true when suspended_at is present.
        let suspended = raw.suspended || raw.suspended_at.is_some();
        Ok(AgentEntry {
            id: raw.id,
            name: raw.name,
            agent_type: raw.agent_type,
            status: raw.status,
            prompt: raw.prompt,
            started_at: raw.started_at,
            completed_at: raw.completed_at,
            exit_code: raw.exit_code,
            error: raw.error,
            pid: raw.pid,
            session_id: raw.session_id,
            suspended_at: raw.suspended_at,
            suspended,
            command: raw.command,
            plan_mode: raw.plan_mode,
            trigger_seq_index: raw.trigger_seq_index,
            trigger_state: raw.trigger_state,
            trigger_total: raw.trigger_total,
            gate_attempts: raw.gate_attempts,
            no_trigger: raw.no_trigger,
            trigger_name: raw.trigger_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // --- AgentStatus ---

    #[test]
    fn given_agent_status_running_should_serialize() {
        let json = serde_json::to_string(&AgentStatus::Running).unwrap();
        assert_eq!(json, r#""running""#);
    }

    #[test]
    fn given_agent_status_broken_should_serialize() {
        let json = serde_json::to_string(&AgentStatus::Broken).unwrap();
        assert_eq!(json, r#""broken""#);
    }

    #[test]
    fn given_all_agent_statuses_should_round_trip_json() {
        let statuses = vec![AgentStatus::Running, AgentStatus::Broken];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn given_old_status_values_should_deserialize_to_running() {
        // streaming, waiting, spawning, idle, suspended → Running
        for value in &[
            "streaming",
            "waiting",
            "spawning",
            "idle",
            "suspended",
            "running",
        ] {
            assert_eq!(
                serde_json::from_str::<AgentStatus>(&format!(r#""{value}""#)).unwrap(),
                AgentStatus::Running,
                "{value} should deserialize to Running"
            );
        }
    }

    #[test]
    fn given_old_broken_status_values_should_deserialize_to_broken() {
        // completed, failed, killed, lost → Broken
        for value in &["completed", "failed", "killed", "lost", "broken"] {
            assert_eq!(
                serde_json::from_str::<AgentStatus>(&format!(r#""{value}""#)).unwrap(),
                AgentStatus::Broken,
                "{value} should deserialize to Broken"
            );
        }
    }

    #[test]
    fn given_broken_status_should_not_be_alive() {
        assert!(!AgentStatus::Broken.is_alive());
    }

    #[test]
    fn given_running_status_should_be_alive() {
        assert!(AgentStatus::Running.is_alive());
    }

    // --- AgentEntry ---

    #[test]
    fn given_agent_entry_should_serialize_camel_case_keys() {
        let entry = AgentEntry {
            id: "ag-abc".into(),
            name: "claude".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Running,
            prompt: Some("fix bug".into()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(1234),
            session_id: None,
            suspended_at: None,
            suspended: false,
            command: None,
            plan_mode: false,
            trigger_seq_index: None,
            trigger_state: None,
            trigger_total: None,
            gate_attempts: None,
            no_trigger: false,
            trigger_name: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        // Should use camelCase per manifest compat
        assert!(json.contains("agentType"));
        assert!(json.contains("startedAt"));
        // Optional None fields with skip_serializing_if should be absent
        assert!(!json.contains("completedAt"));
        assert!(!json.contains("exitCode"));
        assert!(!json.contains("sessionId"));
        // plan_mode false should be omitted
        assert!(!json.contains("planMode"));
    }

    #[test]
    fn given_agent_entry_with_suspended_at_but_no_suspended_field_should_infer_suspended() {
        let json = r#"{
            "id": "ag-old",
            "name": "claude",
            "agentType": "claude",
            "status": "running",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z",
            "suspendedAt": "2026-03-01T01:00:00Z"
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert!(
            entry.suspended,
            "suspended should be true when suspendedAt is present"
        );
        assert!(entry.suspended_at.is_some());
    }

    #[test]
    fn given_agent_entry_json_should_deserialize_camel_case() {
        let json = r#"{
            "id": "ag-xyz",
            "name": "claude",
            "agentType": "claude",
            "status": "idle",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z",
            "pid": 5678
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, "ag-xyz");
        assert_eq!(entry.status, AgentStatus::Running);
        assert_eq!(entry.pid, Some(5678));
    }

    #[test]
    fn given_agent_entry_with_plan_mode_true_should_serialize() {
        // given
        let entry = AgentEntry {
            id: "ag-plan".into(),
            name: "claude".into(),
            agent_type: "claude".into(),
            status: AgentStatus::Running,
            prompt: Some("research this".into()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(1234),
            session_id: None,
            suspended_at: None,
            suspended: false,
            command: None,
            plan_mode: true,
            trigger_seq_index: None,
            trigger_state: None,
            trigger_total: None,
            gate_attempts: None,
            no_trigger: false,
            trigger_name: None,
        };

        // when
        let json = serde_json::to_string(&entry).unwrap();

        // then
        assert!(json.contains("planMode"));
        let parsed: AgentEntry = serde_json::from_str(&json).unwrap();
        assert!(parsed.plan_mode);
    }

    #[test]
    fn given_old_manifest_without_plan_mode_should_default_to_false() {
        // given: JSON without planMode field (backward compat with old manifests)
        let json = r#"{
            "id": "ag-old",
            "name": "claude",
            "agentType": "claude",
            "status": "running",
            "prompt": "hello",
            "startedAt": "2026-03-01T00:00:00Z",
            "pid": 1234
        }"#;

        // when
        let entry: AgentEntry = serde_json::from_str(json).unwrap();

        // then
        assert!(!entry.plan_mode);
    }

    // --- TriggerState ---

    #[test]
    fn given_trigger_state_should_round_trip_json() {
        let states = vec![
            TriggerState::Active,
            TriggerState::Gating,
            TriggerState::Completed,
            TriggerState::Failed,
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let parsed: TriggerState = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn given_agent_entry_without_trigger_fields_should_deserialize_with_defaults() {
        let json = r#"{
            "id": "ag-old",
            "name": "claude",
            "agentType": "claude",
            "status": "running",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z"
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert!(entry.trigger_seq_index.is_none());
        assert!(entry.trigger_state.is_none());
        assert!(entry.gate_attempts.is_none());
        assert!(!entry.no_trigger);
    }

    #[test]
    fn given_agent_entry_with_trigger_fields_should_round_trip() {
        let json = r#"{
            "id": "ag-new",
            "name": "claude",
            "agentType": "claude",
            "status": "running",
            "prompt": null,
            "startedAt": "2026-03-01T00:00:00Z",
            "triggerSeqIndex": 2,
            "triggerState": "active",
            "gateAttempts": 1,
            "noTrigger": false
        }"#;
        let entry: AgentEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.trigger_seq_index, Some(2));
        assert_eq!(entry.trigger_state, Some(TriggerState::Active));
        assert_eq!(entry.gate_attempts, Some(1));
        assert!(!entry.no_trigger);
    }
}
