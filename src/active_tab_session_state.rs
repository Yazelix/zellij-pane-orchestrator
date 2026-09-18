//! Versioned JSON snapshot for `get_active_tab_session_state`.
//!
//! Compatibility policy:
//! - A schema version may add optional fields, but must not rename or remove existing fields.
//! - Breaking field shape changes require a new schema version and a coordinated consumer update.
//! - The schema carries session facts only. Presentation strings, colors, and bar/widget formatting
//!   belong to consumers such as `yazelix_bar`.

use serde::{Deserialize, Serialize};

pub const ACTIVE_TAB_SESSION_SCHEMA_VERSION: i32 = 2;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionWorkspace {
    /// Owned live state: workspace root selected by the orchestrator for this tab.
    pub root: String,
    /// Adapter state: where the root came from, currently `explicit` or `bootstrap`.
    pub source: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionManagedPanes {
    /// Owned live state: managed editor pane identity, when present.
    pub editor_pane_id: Option<String>,
    /// Owned live state: managed sidebar pane identity, when present.
    pub sidebar_pane_id: Option<String>,
    /// Owned live state: managed agent pane identity, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pane_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionLayout {
    /// Derived state: active Zellij swap layout name reported for the active tab.
    pub active_swap_layout_name: Option<String>,
    /// Derived state: sidebar visibility resolved from the active Yazelix layout family.
    pub sidebar_collapsed: Option<bool>,
    /// Derived state: right agent sidebar visibility, when the managed agent exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_collapsed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionTransientPane {
    /// Derived state: transient pane identity discovered from the live pane manifest.
    pub pane_id: String,
    /// Derived state: whether the transient pane currently owns terminal focus.
    pub is_focused: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionTransientPanes {
    /// Derived state: currently visible Yazelix popup pane, if any.
    pub popup: Option<SessionTransientPane>,
    /// Derived state: currently visible Yazelix menu pane, if any.
    pub menu: Option<SessionTransientPane>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionAiPaneActivityState {
    #[default]
    Unknown,
    Inactive,
    Active,
    Thinking,
    Stale,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionAiPaneActivity {
    /// Adapter state: tab position this activity fact belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_position: Option<usize>,
    /// Legacy v2 provider label, retained only for payload compatibility.
    #[serde(default)]
    pub provider: String,
    /// Adapter state: pane identity associated with the activity signal.
    #[serde(default)]
    pub pane_id: String,
    /// Adapter state: stable activity token retained for existing consumers.
    #[serde(default)]
    pub activity: String,
    /// Adapter state: normalized activity state for status-bus consumers.
    #[serde(default)]
    pub state: SessionAiPaneActivityState,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionStatusExtensions {
    /// Legacy v2 wire field. Nova no longer tracks activity and always emits an empty list.
    #[serde(default)]
    pub ai_pane_activity: Vec<SessionAiPaneActivity>,
}

/// Stable v2 payload for the active tab. Serialized to JSON for the pipe response.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ActiveTabSessionStateV2 {
    pub schema_version: i32,
    pub active_tab_position: usize,
    pub workspace: Option<SessionWorkspace>,
    pub managed_panes: SessionManagedPanes,
    pub focus_context: String,
    pub layout: SessionLayout,
    #[serde(default)]
    pub transient_panes: SessionTransientPanes,
    #[serde(default)]
    pub extensions: SessionStatusExtensions,
}

#[cfg(test)]
mod tests {
    // Test lane: default
    use super::*;
    use serde_json::json;

    // Defends: additive v2 fields remain readable by consumers replaying older active-tab payload fixtures.
    #[test]
    fn deserializes_older_v2_payloads_with_default_extension_fields() {
        let decoded: ActiveTabSessionStateV2 = serde_json::from_value(json!({
            "schema_version": ACTIVE_TAB_SESSION_SCHEMA_VERSION,
            "active_tab_position": 1,
            "workspace": null,
            "managed_panes": {
                "editor_pane_id": null,
                "sidebar_pane_id": null
            },
            "focus_context": "other",
            "layout": {
                "active_swap_layout_name": null,
                "sidebar_collapsed": null
            },
        }))
        .unwrap();

        assert_eq!(decoded.transient_panes, SessionTransientPanes::default());
        assert_eq!(decoded.extensions, SessionStatusExtensions::default());
    }

    // Defends: the status bus exposes stable session facts without embedding bar/zjstatus formatting.
    #[test]
    fn serializes_representative_payload_without_presentation_formatting() {
        let snapshot = ActiveTabSessionStateV2 {
            schema_version: ACTIVE_TAB_SESSION_SCHEMA_VERSION,
            active_tab_position: 2,
            workspace: Some(SessionWorkspace {
                root: "/repo".into(),
                source: "explicit".into(),
            }),
            managed_panes: SessionManagedPanes {
                editor_pane_id: Some("terminal:1".into()),
                sidebar_pane_id: Some("terminal:2".into()),
                agent_pane_id: Some("terminal:3".into()),
            },
            focus_context: "editor".into(),
            layout: SessionLayout {
                active_swap_layout_name: Some("single_open_agent_closed".into()),
                sidebar_collapsed: Some(false),
                agent_collapsed: Some(true),
            },
            transient_panes: SessionTransientPanes {
                popup: None,
                menu: Some(SessionTransientPane {
                    pane_id: "terminal:9".into(),
                    is_focused: true,
                }),
            },
            extensions: SessionStatusExtensions::default(),
        };

        let serialized = serde_json::to_string(&snapshot).unwrap();
        let decoded: ActiveTabSessionStateV2 = serde_json::from_str(&serialized).unwrap();
        let value = serde_json::to_value(&snapshot).unwrap();

        assert_eq!(decoded, snapshot);
        assert_eq!(
            value,
            json!({
                "schema_version": ACTIVE_TAB_SESSION_SCHEMA_VERSION,
                "active_tab_position": 2,
                "workspace": {
                    "root": "/repo",
                    "source": "explicit"
                },
                "managed_panes": {
                    "editor_pane_id": "terminal:1",
                    "sidebar_pane_id": "terminal:2",
                    "agent_pane_id": "terminal:3"
                },
                "focus_context": "editor",
                "layout": {
                    "active_swap_layout_name": "single_open_agent_closed",
                    "sidebar_collapsed": false,
                    "agent_collapsed": true
                },
                "transient_panes": {
                    "popup": null,
                    "menu": {
                        "pane_id": "terminal:9",
                        "is_focused": true
                    }
                },
                "extensions": {
                    "ai_pane_activity": []
                }
            })
        );
        assert!(!serialized.contains("#["));
        assert!(!serialized.contains("command_cpu"));
        assert!(!serialized.contains("zjstatus"));
    }
}
