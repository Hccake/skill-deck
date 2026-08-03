pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const INSTALL_WIZARD_LABEL: &str = "install-wizard";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowRole {
    Main,
    InstallWizard,
    Unknown,
}

impl WindowRole {
    pub(crate) fn from_label(label: &str) -> Self {
        match label {
            MAIN_WINDOW_LABEL => Self::Main,
            INSTALL_WIZARD_LABEL => Self::InstallWizard,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_window_labels_and_rejects_unknown_labels() {
        assert_eq!(WindowRole::from_label(MAIN_WINDOW_LABEL), WindowRole::Main);
        assert_eq!(
            WindowRole::from_label(INSTALL_WIZARD_LABEL),
            WindowRole::InstallWizard
        );
        assert_eq!(WindowRole::from_label("other"), WindowRole::Unknown);
    }
}
