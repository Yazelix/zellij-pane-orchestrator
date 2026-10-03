#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Absent,
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContentMode {
    Stacked,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutVariant {
    pub sidebar_state: SidebarState,
    pub agent_state: AgentState,
    content_mode: ContentMode,
    bottom_hints_hidden: bool,
}

const LAYOUT_ORDER: &[LayoutVariant] = &[
    LayoutVariant::new(SidebarState::Open, AgentState::Absent),
    LayoutVariant::new(SidebarState::Closed, AgentState::Absent),
    LayoutVariant::new(SidebarState::Open, AgentState::Open),
    LayoutVariant::new(SidebarState::Open, AgentState::Closed),
    LayoutVariant::new(SidebarState::Closed, AgentState::Open),
    LayoutVariant::new(SidebarState::Closed, AgentState::Closed),
    LayoutVariant::columns(SidebarState::Open),
    LayoutVariant::columns(SidebarState::Closed),
];

pub fn is_base_layout_name(active_swap_layout_name: Option<&str>) -> bool {
    active_swap_layout_name.is_none() || active_swap_layout_name == Some("BASE")
}

impl LayoutVariant {
    pub const fn new(sidebar_state: SidebarState, agent_state: AgentState) -> Self {
        Self {
            sidebar_state,
            agent_state,
            content_mode: ContentMode::Stacked,
            bottom_hints_hidden: false,
        }
    }

    const fn columns(sidebar_state: SidebarState) -> Self {
        Self {
            sidebar_state,
            agent_state: AgentState::Absent,
            content_mode: ContentMode::Columns,
            bottom_hints_hidden: false,
        }
    }

    pub fn layout_name(self) -> &'static str {
        if self.bottom_hints_hidden {
            return match (self.content_mode, self.sidebar_state) {
                (ContentMode::Stacked, SidebarState::Open) => "single_open_no_hints",
                (ContentMode::Stacked, SidebarState::Closed) => "single_closed_no_hints",
                (ContentMode::Columns, SidebarState::Open) => "columns_open_no_hints",
                (ContentMode::Columns, SidebarState::Closed) => "columns_closed_no_hints",
            };
        }
        match (self.content_mode, self.sidebar_state, self.agent_state) {
            (ContentMode::Columns, SidebarState::Open, AgentState::Absent) => "columns_open",
            (ContentMode::Columns, SidebarState::Closed, AgentState::Absent) => "columns_closed",
            (_, SidebarState::Open, AgentState::Absent) => "single_open",
            (_, SidebarState::Closed, AgentState::Absent) => "single_closed",
            (_, SidebarState::Open, AgentState::Open) => "single_open_agent_open",
            (_, SidebarState::Open, AgentState::Closed) => "single_open_agent_closed",
            (_, SidebarState::Closed, AgentState::Open) => "single_closed_agent_open",
            (_, SidebarState::Closed, AgentState::Closed) => "single_closed_agent_closed",
        }
    }

    pub fn from_layout_name(layout_name: &str) -> Option<Self> {
        let (name, hidden) = layout_name
            .strip_suffix("_no_hints")
            .map(|name| (name, true))
            .unwrap_or((layout_name, false));
        let variant = LAYOUT_ORDER
            .iter()
            .copied()
            .find(|variant| variant.layout_name() == name)?;
        if hidden {
            variant.with_bottom_hints_hidden(true)
        } else {
            Some(variant)
        }
    }

    pub fn bottom_hints_hidden(self) -> bool {
        self.bottom_hints_hidden
    }

    pub fn with_bottom_hints_hidden(self, hidden: bool) -> Option<Self> {
        (self.agent_state == AgentState::Absent).then_some(Self {
            bottom_hints_hidden: hidden,
            ..self
        })
    }

    pub fn is_sidebar_closed(self) -> bool {
        self.sidebar_state == SidebarState::Closed
    }

    pub fn is_columns(self) -> bool {
        self.content_mode == ContentMode::Columns
    }

    pub fn agent_is_closed(self) -> Option<bool> {
        match self.agent_state {
            AgentState::Absent => None,
            AgentState::Open => Some(false),
            AgentState::Closed => Some(true),
        }
    }

    pub fn with_sidebar_state(self, sidebar_state: SidebarState) -> Self {
        Self {
            sidebar_state,
            ..self
        }
    }

    pub fn with_agent_state(self, agent_state: AgentState) -> Self {
        Self {
            agent_state,
            bottom_hints_hidden: self.bottom_hints_hidden && agent_state == AgentState::Absent,
            content_mode: if agent_state == AgentState::Absent {
                self.content_mode
            } else {
                ContentMode::Stacked
            },
            ..self
        }
    }

    pub fn toggle_content_mode(self) -> Self {
        if self.agent_state != AgentState::Absent {
            return self;
        }
        Self {
            content_mode: match self.content_mode {
                ContentMode::Stacked => ContentMode::Columns,
                ContentMode::Columns => ContentMode::Stacked,
            },
            ..self
        }
    }
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_hints_visibility_survives_sidebar_and_content_changes() {
        for name in [
            "single_open",
            "single_closed",
            "columns_open",
            "columns_closed",
        ] {
            let visible = LayoutVariant::from_layout_name(name).unwrap();
            let hidden = visible.with_bottom_hints_hidden(true).unwrap();
            assert_eq!(hidden.layout_name(), format!("{name}_no_hints"));
            assert_eq!(
                LayoutVariant::from_layout_name(hidden.layout_name()),
                Some(hidden)
            );
            assert!(hidden.toggle_content_mode().bottom_hints_hidden());
            assert!(hidden
                .with_sidebar_state(SidebarState::Closed)
                .bottom_hints_hidden());
            assert_eq!(hidden.with_bottom_hints_hidden(false), Some(visible));
        }
        assert!(LayoutVariant::from_layout_name("custom_no_hints").is_none());
        assert!(LayoutVariant::new(SidebarState::Open, AgentState::Open)
            .with_bottom_hints_hidden(true)
            .is_none());
    }

    #[test]
    fn absent_swap_layout_name_is_the_base_layout() {
        assert!(is_base_layout_name(None));
        assert!(is_base_layout_name(Some("BASE")));
        assert!(!is_base_layout_name(Some("single_open")));
    }

    // Defends: existing layout names continue to parse as no-agent variants for current sessions.
    #[test]
    fn parses_existing_no_agent_layout_names() {
        assert_eq!(
            LayoutVariant::from_layout_name("single_closed"),
            Some(LayoutVariant::new(SidebarState::Closed, AgentState::Absent))
        );
    }

    // Defends: managed-agent layout names carry independent left-sidebar and right-agent state.
    #[test]
    fn parses_agent_layout_names() {
        assert_eq!(
            LayoutVariant::from_layout_name("single_closed_agent_open"),
            Some(LayoutVariant::new(SidebarState::Closed, AgentState::Open))
        );
    }

    #[test]
    fn content_mode_preserves_sidebar_state() {
        let stacked = LayoutVariant::new(SidebarState::Open, AgentState::Absent);
        let columns = stacked.toggle_content_mode();
        assert_eq!(columns.layout_name(), "columns_open");
        assert_eq!(
            columns
                .with_sidebar_state(SidebarState::Closed)
                .layout_name(),
            "columns_closed"
        );
        assert_eq!(
            LayoutVariant::from_layout_name("columns_open"),
            Some(columns)
        );
        assert_eq!(columns.toggle_content_mode(), stacked);
        assert_eq!(
            LayoutVariant::new(SidebarState::Open, AgentState::Open).toggle_content_mode(),
            LayoutVariant::new(SidebarState::Open, AgentState::Open)
        );
    }
}
