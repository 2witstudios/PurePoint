use pu_core::signals::*;
use serde_json::Value;

pub(super) fn kind(tool: &str, payload: &Value) -> Option<(SignalEventKind, SignalConfidence)> {
    use SignalConfidence as C;
    use SignalEventKind as K;
    let hook = payload["hook_event_name"].as_str().unwrap_or("");
    let intent = |kind| K::AttentionIntent {
        kind,
        request_id: payload["tool_use_id"].as_str().map(str::to_owned),
    };
    if tool == "pi"
        && matches!(
            payload["type"].as_str(),
            Some("ui_prompt_start" | "ui_prompt_end")
        )
        && !payload["request_id"]
            .as_str()
            .is_some_and(crate::read_source::valid_session_id)
    {
        return None;
    }
    Some(match tool {
        "claude" => match hook {
            "UserPromptSubmit" => (K::Start, C::SourceConfirmed),
            "Stop" => (K::StopAttempt, C::SourceConfirmed),
            "StopFailure" => (
                K::Outcome {
                    kind: OutcomeKind::Errored,
                },
                if native_turn(payload).is_some() {
                    C::Authoritative
                } else {
                    C::SourceConfirmed
                },
            ),
            "PermissionRequest" => (intent(AttentionKind::Permission), C::SourceConfirmed),
            "PreToolUse" if payload["tool_name"] == "AskUserQuestion" => {
                (intent(AttentionKind::Question), C::SourceConfirmed)
            }
            "Elicitation" => (intent(AttentionKind::Question), C::SourceConfirmed),
            "SessionEnd" => (K::Disconnected, C::Authoritative),
            _ => return None,
        },
        "opencode" => {
            let data = &payload["data"];
            match payload["type"].as_str()? {
                "session.execution.started" => (K::Start, C::Authoritative),
                "session.execution.succeeded" => (
                    K::Outcome {
                        kind: OutcomeKind::Completed,
                    },
                    C::Authoritative,
                ),
                "session.execution.failed" => (
                    K::Outcome {
                        kind: OutcomeKind::Errored,
                    },
                    C::Authoritative,
                ),
                "session.execution.interrupted" => (
                    K::Outcome {
                        kind: OutcomeKind::Interrupted,
                    },
                    C::Authoritative,
                ),
                "permission.asked" => (
                    K::AttentionPending {
                        request: pending(data["id"].as_str()?, AttentionKind::Permission, payload),
                    },
                    C::Authoritative,
                ),
                "permission.replied" => (
                    K::AttentionResolved {
                        request_id: data["requestID"].as_str()?.into(),
                    },
                    C::Authoritative,
                ),
                "form.created" => (
                    K::AttentionPending {
                        request: pending(
                            data["form"]["id"].as_str()?,
                            AttentionKind::Question,
                            payload,
                        ),
                    },
                    C::Authoritative,
                ),
                "form.replied" | "form.cancelled" => (
                    K::AttentionResolved {
                        request_id: data["id"].as_str()?.into(),
                    },
                    C::Authoritative,
                ),
                _ => return None,
            }
        }
        "pi" => match payload["type"].as_str()? {
            "observer_loss" => (K::Loss, C::Authoritative),
            "agent_start" => (K::Start, C::Authoritative),
            "agent_settled" => {
                let aborted = payload["aborted"].as_bool()?;
                let outcome = if aborted {
                    OutcomeKind::Interrupted
                } else {
                    match payload["outcome"].as_str()? {
                        "error" => OutcomeKind::Errored,
                        "completed" => OutcomeKind::Completed,
                        _ => return None,
                    }
                };
                (K::Outcome { kind: outcome }, C::Authoritative)
            }
            "ui_prompt_start"
                if matches!(
                    payload["kind"].as_str(),
                    Some("select" | "confirm" | "input" | "editor" | "custom")
                ) =>
            {
                (
                    K::AttentionPending {
                        request: pending(
                            payload["request_id"].as_str()?,
                            AttentionKind::Question,
                            payload,
                        ),
                    },
                    C::Authoritative,
                )
            }
            "ui_prompt_end"
                if matches!(
                    payload["kind"].as_str(),
                    Some("select" | "confirm" | "input" | "editor" | "custom")
                ) =>
            {
                (
                    K::AttentionResolved {
                        request_id: payload["request_id"].as_str()?.into(),
                    },
                    C::Authoritative,
                )
            }
            _ => return None,
        },
        "gemini" => match hook {
            "BeforeAgent" => (K::Start, C::SourceConfirmed),
            "AfterAgent" => (K::StopAttempt, C::SourceConfirmed),
            "Notification" if payload["notification_type"] == "ToolPermission" => {
                let k = match payload["details"]["type"].as_str()? {
                    "ask_user" => AttentionKind::Question,
                    "edit" | "exec" | "mcp" | "info" => AttentionKind::Permission,
                    _ => return None,
                };
                (intent(k), C::SourceConfirmed)
            }
            _ => return None,
        },
        "codex" => match hook {
            "UserPromptSubmit" => (K::Start, C::SourceConfirmed),
            "Stop" => (K::StopAttempt, C::SourceConfirmed),
            "PermissionRequest" => (intent(AttentionKind::Permission), C::SourceConfirmed),
            "Interrupt" => (
                K::Outcome {
                    kind: OutcomeKind::Interrupted,
                },
                C::SourceConfirmed,
            ),
            _ => return None,
        },
        "cursor" if hook == "stop" => (K::StopAttempt, C::SourceConfirmed),
        _ => return None,
    })
}

/// Never attribute an uncorrelated outcome to the adapter's most recent turn.
pub(super) fn native_turn(payload: &Value) -> Option<String> {
    let turn = payload["turn_id"].as_str();
    let prompt = payload["prompt_id"].as_str();
    if matches!((turn,prompt),(Some(a),Some(b)) if a!=b) {
        return None;
    }
    turn.or(prompt)
        .filter(|id| crate::read_source::valid_session_id(id))
        .map(str::to_owned)
}
pub(super) fn claude_event_id(payload: &Value) -> String {
    // These hooks are one boundary per native prompt/request. Duplicate raw
    // deliveries keep their identity; receipt order is not event identity.
    serde_json::to_string(&serde_json::json!([
        payload["hook_event_name"].as_str(),
        native_turn(payload),
        payload["tool_use_id"].as_str()
    ]))
    .unwrap()
}

fn pending(id: &str, kind: AttentionKind, payload: &Value) -> PendingAttention {
    PendingAttention {
        request_id: id.into(),
        kind,
        event_id: payload["id"].as_str().unwrap_or(id).into(),
        turn_id: None,
        confidence: SignalConfidence::Authoritative,
        actionability: AttentionActionability::default(),
    }
}

pub(super) fn session<'a>(tool: &str, payload: &'a Value) -> Option<&'a str> {
    if payload["agent_id"].as_str().is_some() || payload["parent_session_id"].as_str().is_some() {
        return None;
    }
    if tool == "opencode" {
        payload["data"]["sessionID"]
            .as_str()
            .or_else(|| payload["data"]["form"]["sessionID"].as_str())
    } else if tool == "cursor" {
        payload["conversation_id"].as_str()
    } else {
        payload["session_id"].as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn given_claude_stop_and_intent_should_never_invent_finished_or_pending() {
        assert!(matches!(
            kind("claude", &json!({"hook_event_name":"Stop"})),
            Some((SignalEventKind::StopAttempt, _))
        ));
        for (hook, tool) in [
            ("PermissionRequest", "Bash"),
            ("PreToolUse", "AskUserQuestion"),
            ("Elicitation", ""),
        ] {
            assert!(matches!(
                kind("claude", &json!({"hook_event_name":hook,"tool_name":tool})),
                Some((SignalEventKind::AttentionIntent { .. }, _))
            ));
        }
        assert!(kind("claude", &json!({"hook_event_name":"SubagentStop"})).is_none());
        assert!(session("claude", &json!({"session_id":"root","agent_id":"child"})).is_none());
    }
    #[test]
    fn given_opencode_nonblocking_form_should_retain_request_without_claiming_platform_actionability()
     {
        let (SignalEventKind::AttentionPending{request},_) = kind("opencode",&json!({"id":"event","type":"form.created","data":{"form":{"id":"form","sessionID":"root"}}})).unwrap() else { panic!() };
        assert_eq!(request.request_id, "form");
        assert_eq!(request.actionability, AttentionActionability::default());
        assert!(kind("opencode", &json!({"type":"session.tool.failed"})).is_none());
        assert!(kind("opencode", &json!({"type":"session.error"})).is_none());
    }
    #[test]
    fn given_other_native_boundaries_should_preserve_their_limits() {
        assert!(matches!(
            kind("gemini", &json!({"hook_event_name":"AfterAgent"})),
            Some((SignalEventKind::StopAttempt, _))
        ));
        assert!(matches!(
            kind(
                "gemini",
                &json!({"hook_event_name":"Notification","notification_type":"ToolPermission","details":{"type":"ask_user"}})
            ),
            Some((
                SignalEventKind::AttentionIntent {
                    kind: AttentionKind::Question,
                    ..
                },
                _
            ))
        ));
        assert!(matches!(
            kind(
                "cursor",
                &json!({"hook_event_name":"stop","status":"completed"})
            ),
            Some((SignalEventKind::StopAttempt, _))
        ));
        assert!(kind("codex", &json!({"last_assistant_message":"why?"})).is_none());
        assert!(
            kind(
                "pi",
                &json!({"type":"agent_before_settle","outcome":"error"})
            )
            .is_none()
        );
        assert!(matches!(
            kind(
                "pi",
                &json!({"type":"agent_settled","outcome":"completed","aborted":false})
            ),
            Some((
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed
                },
                _
            ))
        ));
        assert!(matches!(
            kind(
                "pi",
                &json!({"type":"ui_prompt_start","kind":"confirm","request_id":"id"})
            ),
            Some((
                SignalEventKind::AttentionPending {
                    request: PendingAttention {
                        kind: AttentionKind::Question,
                        ..
                    }
                },
                _
            ))
        ));
    }
}
