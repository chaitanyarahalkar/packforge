//! Format-independent planning primitives for Packforge.

/// The current implementation stage exposed by the planning scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStage {
    /// Architecture and release gates are defined; packing is not implemented.
    Planning,
}

impl ProjectStage {
    /// Returns the stable command-line representation of the stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
        }
    }
}

/// Returns the current project stage.
#[must_use]
pub const fn project_stage() -> ProjectStage {
    ProjectStage::Planning
}

#[cfg(test)]
mod tests {
    use super::{ProjectStage, project_stage};

    #[test]
    fn scaffold_reports_planning_stage() {
        assert_eq!(project_stage(), ProjectStage::Planning);
        assert_eq!(project_stage().as_str(), "planning");
    }
}
