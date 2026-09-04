#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFamilyDirection {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFamily {
    Single,
}

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
pub struct LayoutVariant {
    pub family: LayoutFamily,
    pub sidebar_state: SidebarState,
    pub agent_state: AgentState,
}

const LAYOUT_ORDER: &[LayoutVariant] = &[
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Absent),
    LayoutVariant::new(
        LayoutFamily::Single,
        SidebarState::Closed,
        AgentState::Absent,
    ),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Open),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Closed),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Closed, AgentState::Open),
    LayoutVariant::new(
        LayoutFamily::Single,
        SidebarState::Closed,
        AgentState::Closed,
    ),
];

pub fn is_base_layout_name(active_swap_layout_name: Option<&str>) -> bool {
    active_swap_layout_name.is_none() || active_swap_layout_name == Some("BASE")
}

impl LayoutVariant {
    pub const fn new(
        family: LayoutFamily,
        sidebar_state: SidebarState,
        agent_state: AgentState,
    ) -> Self {
        Self {
            family,
            sidebar_state,
            agent_state,
        }
    }

    pub fn layout_name(self) -> &'static str {
        match (self.family, self.sidebar_state, self.agent_state) {
            (LayoutFamily::Single, SidebarState::Open, AgentState::Absent) => "single_open",
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Absent) => "single_closed",
            (LayoutFamily::Single, SidebarState::Open, AgentState::Open) => {
                "single_open_agent_open"
            }
            (LayoutFamily::Single, SidebarState::Open, AgentState::Closed) => {
                "single_open_agent_closed"
            }
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Open) => {
                "single_closed_agent_open"
            }
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Closed) => {
                "single_closed_agent_closed"
            }
        }
    }

    pub fn from_layout_name(layout_name: &str) -> Option<Self> {
        LAYOUT_ORDER
            .iter()
            .copied()
            .find(|variant| variant.layout_name() == layout_name)
    }

    pub fn is_sidebar_closed(self) -> bool {
        self.sidebar_state == SidebarState::Closed
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
            ..self
        }
    }

    pub fn with_next_family(self, direction: LayoutFamilyDirection) -> Self {
        Self {
            family: self.family.next(direction),
            ..self
        }
    }
}

impl LayoutFamily {
    fn next(self, _direction: LayoutFamilyDirection) -> Self {
        self
    }
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

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
            Some(LayoutVariant::new(
                LayoutFamily::Single,
                SidebarState::Closed,
                AgentState::Absent
            ))
        );
    }

    // Defends: managed-agent layout names carry independent left-sidebar and right-agent state.
    #[test]
    fn parses_agent_layout_names() {
        assert_eq!(
            LayoutVariant::from_layout_name("single_closed_agent_open"),
            Some(LayoutVariant::new(
                LayoutFamily::Single,
                SidebarState::Closed,
                AgentState::Open
            ))
        );
    }

    // Defends: layout-family changes are no-ops after removing the bottom-terminal family.
    #[test]
    fn layout_family_switch_is_noop() {
        let current =
            LayoutVariant::new(LayoutFamily::Single, SidebarState::Closed, AgentState::Open);
        let target = current.with_next_family(LayoutFamilyDirection::Next);

        assert_eq!(target, current);
    }
}
