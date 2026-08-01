use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct WorkspacePopupRequest<'a> {
    id: &'a str,
    cwd: &'a str,
}

#[derive(Serialize)]
struct WorkspacePopupYaziResponse<'a> {
    status: &'static str,
    yazi_id: &'a str,
}

pub fn workspace_popup_payload(popup_id: &str, workspace_root: &str) -> Option<String> {
    let popup_id = popup_id.trim();
    let workspace_root = workspace_root.trim();
    if popup_id.is_empty() || !Path::new(workspace_root).is_absolute() {
        return None;
    }
    serde_json::to_string(&WorkspacePopupRequest {
        id: popup_id,
        cwd: workspace_root,
    })
    .ok()
}

pub fn workspace_popup_destination_id<'a>(
    expected_plugin_url: &str,
    panes: impl IntoIterator<Item = (u32, bool, Option<&'a str>)>,
) -> Option<u32> {
    let expected_plugin_url = expected_plugin_url.trim();
    if expected_plugin_url.is_empty() {
        return None;
    }
    panes
        .into_iter()
        .find(|(_, exited, plugin_url)| {
            !*exited && plugin_url.is_some_and(|url| url == expected_plugin_url)
        })
        .map(|(id, _, _)| id)
}

pub fn workspace_popup_yazi_tab_id<'a, PaneId>(
    expected_pane_title: &str,
    pane_id: &str,
    panes: impl IntoIterator<Item = (usize, PaneId, &'a str, bool)>,
) -> Option<usize>
where
    PaneId: AsRef<str>,
{
    let expected_pane_title = expected_pane_title.trim();
    let pane_id = pane_id.trim();
    if expected_pane_title.is_empty() || pane_id.is_empty() {
        return None;
    }
    panes
        .into_iter()
        .find(|(_, candidate_id, title, is_floating)| {
            *is_floating
                && candidate_id.as_ref().trim() == pane_id
                && title.trim() == expected_pane_title
        })
        .map(|(tab_id, _, _, _)| tab_id)
}

pub fn workspace_popup_yazi_response(yazi_id: &str) -> Option<String> {
    let yazi_id = yazi_id.trim();
    if yazi_id.is_empty() {
        return None;
    }
    serde_json::to_string(&WorkspacePopupYaziResponse {
        status: "ok",
        yazi_id,
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_popup_request_carries_canonical_root() {
        let payload = workspace_popup_payload(" agent ", " /repo ").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
            json!({
                "id": "agent",
                "cwd": "/repo",
            })
        );

        assert!(workspace_popup_payload("", "/repo").is_none());
        assert!(workspace_popup_payload("agent", "repo").is_none());
    }

    #[test]
    fn workspace_popup_targets_the_loaded_plugin_instance() {
        let panes = [
            (7, false, None),
            (8, true, Some("yzpp")),
            (9, false, Some("other")),
            (10, false, Some("yzpp")),
        ];
        assert_eq!(workspace_popup_destination_id(" yzpp ", panes), Some(10));
        assert_eq!(workspace_popup_destination_id("missing", panes), None);
    }

    #[test]
    fn popup_yazi_registration_requires_the_configured_floating_pane() {
        let panes = [
            (3, "terminal:8", "sidebar", false),
            (3, "terminal:9", "yazi_popup", true),
            (4, "terminal:10", "yazi_popup", true),
        ];

        assert_eq!(
            workspace_popup_yazi_tab_id("yazi_popup", "terminal:9", panes),
            Some(3)
        );
        assert_eq!(
            workspace_popup_yazi_tab_id("yazi_popup", "terminal:8", panes),
            None,
            "the tiled sidebar must never satisfy popup registration"
        );
        assert_eq!(workspace_popup_yazi_tab_id("", "terminal:9", panes), None);
    }

    #[test]
    fn popup_yazi_success_carries_only_its_address() {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &workspace_popup_yazi_response(" yazi-17 ").unwrap()
            )
            .unwrap(),
            json!({"status": "ok", "yazi_id": "yazi-17"})
        );
        assert!(workspace_popup_yazi_response(" ").is_none());
    }
}
