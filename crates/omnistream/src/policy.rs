use crate::config::Config;

pub fn requires_approval(config: &Config, action: &str) -> bool {
    config.approvals.iter().any(|item| item == action)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_actions_require_approval() {
        let config: Config = toml::from_str("approvals = [\"git_push\"]").unwrap();
        assert!(requires_approval(&config, "git_push"));
        assert!(!requires_approval(&config, "run_test"));
    }
}
