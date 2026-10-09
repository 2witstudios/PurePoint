//! Generate the cross-client fixture with the real Rust wire serializer.
use pu_core::protocol::{
    ChannelAuthor, ChannelMessage, ChannelReaction, ChannelReference, Response,
};
use std::collections::BTreeMap;

fn fixture() -> Response {
    let human = ChannelAuthor {
        id: "human:501".into(),
        name: "jono".into(),
        kind: "human".into(),
        agent_type: None,
        worktree_id: None,
        branch: None,
    };
    let agent = ChannelAuthor {
        id: "ag-fixture".into(),
        name: "Channel builder".into(),
        kind: "agent".into(),
        agent_type: Some("codex".into()),
        worktree_id: Some("wt-fixture".into()),
        branch: Some("pu/channel".into()),
    };
    Response::ChannelHistory {
        messages: vec![
            ChannelMessage {
                id: "msg-parent".into(),
                sequence: 1,
                parent_id: None,
                author: human,
                text: "Ready for `review`?".into(),
                created_at: "2026-10-09T17:00:00+00:00".into(),
                edited_at: Some("2026-10-09T17:01:00+00:00".into()),
                references: vec![ChannelReference {
                    kind: "pr".into(),
                    value: "42".into(),
                    label: Some("Review PR".into()),
                }],
                reactions: vec![ChannelReaction {
                    emoji: "👍".into(),
                    author_ids: vec!["ag-fixture".into(), "human:501".into()],
                }],
            },
            ChannelMessage {
                id: "msg-reply".into(),
                sequence: 7,
                parent_id: Some("msg-parent".into()),
                author: agent,
                text: "Yes — checks passed.\nRésumé included.".into(),
                created_at: "2026-10-09T17:02:00+00:00".into(),
                edited_at: None,
                references: vec![ChannelReference {
                    kind: "commit".into(),
                    value: "abcdef0123456789".into(),
                    label: None,
                }],
                reactions: vec![],
            },
        ],
        revision: 11,
        latest_sequence: 9,
        has_more: true,
        oldest_sequence: Some(7),
        unchanged: false,
        self_author_id: "human:501".into(),
        reply_counts: BTreeMap::from([("msg-parent".into(), 3)]),
    }
}
fn main() {
    println!("{}", serde_json::to_string_pretty(&fixture()).unwrap());
}
#[cfg(test)]
mod tests {
    #[test]
    fn given_rust_response_should_match_shared_swift_fixture() {
        let actual = serde_json::to_string_pretty(&super::fixture()).unwrap() + "\n";
        assert_eq!(
            actual,
            include_str!("../../../docs/reference/fixtures/channel-history.json")
        );
        let round_trip: pu_core::protocol::Response = serde_json::from_str(&actual).unwrap();
        assert_eq!(
            serde_json::to_string_pretty(&round_trip).unwrap() + "\n",
            actual
        );
    }
}
