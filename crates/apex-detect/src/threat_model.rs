//! Trust classification of input sources per threat model type.
//!
//! Each threat model type defines which input sources are trusted (safe to flow
//! into sinks without flagging) vs untrusted (flows should be reported).

use apex_core::config::{ThreatModelConfig, ThreatModelType};

/// Whether a source is trusted in the given threat model context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Safe — flows from this source to sinks are not flagged.
    Trusted,
    /// Dangerous — flows from this source to sinks ARE flagged.
    Untrusted,
    /// Not applicable to this threat model (source doesn't exist in this context).
    NotApplicable,
}

/// Built-in source patterns and their trust levels per threat model type.
struct SourceTrust {
    pattern: &'static str,
    cli_tool: TrustLevel,
    web_service: TrustLevel,
    library: TrustLevel,
    ci_pipeline: TrustLevel,
}

use TrustLevel::*;

const SOURCE_TRUST_TABLE: &[SourceTrust] = &[
    SourceTrust {
        pattern: "argv",
        cli_tool: Trusted,
        web_service: NotApplicable,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "arg(",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "args",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "request",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: NotApplicable,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "query",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "form",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: NotApplicable,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "param",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "input",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "stdin",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "environ",
        cli_tool: Trusted,
        web_service: Trusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "getenv",
        cli_tool: Trusted,
        web_service: Trusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "recv",
        cli_tool: Untrusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Untrusted,
    },
    SourceTrust {
        pattern: "socket",
        cli_tool: Untrusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Untrusted,
    },
    SourceTrust {
        pattern: "upload",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "file",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "user",
        cli_tool: NotApplicable,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: NotApplicable,
    },
    SourceTrust {
        pattern: "format!",
        cli_tool: Trusted,
        web_service: Trusted,
        library: Trusted,
        ci_pipeline: Trusted,
    },
    SourceTrust {
        pattern: "&str",
        cli_tool: Trusted,
        web_service: Untrusted,
        library: Untrusted,
        ci_pipeline: Trusted,
    },
];

/// Classify whether a set of user-input indicators (from a SecurityPattern) are
/// trusted or untrusted in the given threat model.
///
/// Returns `Some(true)` if ALL matched indicators are trusted (finding should be suppressed).
/// Returns `Some(false)` if ANY matched indicator is untrusted (finding should be reported).
/// Returns `None` if no threat model is configured.
pub fn should_suppress(config: &ThreatModelConfig, matched_indicators: &[&str]) -> Option<bool> {
    let model_type = config.model_type?;

    // If no indicators matched, we can't determine trust — don't suppress.
    if matched_indicators.is_empty() {
        return Some(false);
    }

    for indicator in matched_indicators {
        let indicator_lower = indicator.to_lowercase();

        // Check user-defined overrides first.
        if config
            .trusted_sources
            .iter()
            .any(|s| indicator_lower.contains(&s.to_lowercase()))
        {
            continue;
        }
        if config
            .untrusted_sources
            .iter()
            .any(|s| indicator_lower.contains(&s.to_lowercase()))
        {
            return Some(false);
        }

        // Check built-in table.
        let trust = lookup_trust(&indicator_lower, model_type);
        match trust {
            Untrusted => return Some(false),
            Trusted | NotApplicable => continue,
        }
    }

    // All matched indicators were trusted or N/A — suppress.
    Some(true)
}

fn lookup_trust(indicator: &str, model_type: ThreatModelType) -> TrustLevel {
    for entry in SOURCE_TRUST_TABLE {
        if indicator.contains(entry.pattern) {
            return match model_type {
                ThreatModelType::CliTool | ThreatModelType::ConsoleTool => entry.cli_tool,
                ThreatModelType::WebService => entry.web_service,
                ThreatModelType::Library => entry.library,
                ThreatModelType::CiPipeline => entry.ci_pipeline,
            };
        }
    }
    // Unknown indicator — treat as potentially untrusted.
    Untrusted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::CliTool),
            trusted_sources: vec![],
            untrusted_sources: vec![],
            ..Default::default()
        }
    }

    fn web_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::WebService),
            trusted_sources: vec![],
            untrusted_sources: vec![],
            ..Default::default()
        }
    }

    fn lib_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::Library),
            trusted_sources: vec![],
            untrusted_sources: vec![],
            ..Default::default()
        }
    }

    fn ci_config() -> ThreatModelConfig {
        ThreatModelConfig {
            model_type: Some(ThreatModelType::CiPipeline),
            trusted_sources: vec![],
            untrusted_sources: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn no_threat_model_returns_none() {
        let cfg = ThreatModelConfig::default();
        assert_eq!(should_suppress(&cfg, &["argv"]), None);
    }

    #[test]
    fn cli_tool_trusts_argv() {
        assert_eq!(should_suppress(&cli_config(), &["argv"]), Some(true));
    }

    #[test]
    fn cli_tool_trusts_stdin() {
        assert_eq!(should_suppress(&cli_config(), &["stdin"]), Some(true));
    }

    #[test]
    fn cli_tool_trusts_environ() {
        assert_eq!(should_suppress(&cli_config(), &["environ"]), Some(true));
    }

    #[test]
    fn cli_tool_does_not_trust_socket() {
        assert_eq!(should_suppress(&cli_config(), &["socket"]), Some(false));
    }

    #[test]
    fn web_service_does_not_trust_request() {
        assert_eq!(should_suppress(&web_config(), &["request"]), Some(false));
    }

    #[test]
    fn web_service_does_not_trust_query() {
        assert_eq!(should_suppress(&web_config(), &["query"]), Some(false));
    }

    #[test]
    fn web_service_trusts_environ() {
        assert_eq!(should_suppress(&web_config(), &["environ"]), Some(true));
    }

    #[test]
    fn library_trusts_nothing() {
        assert_eq!(should_suppress(&lib_config(), &["argv"]), Some(false));
        assert_eq!(should_suppress(&lib_config(), &["environ"]), Some(false));
        assert_eq!(should_suppress(&lib_config(), &["input"]), Some(false));
    }

    #[test]
    fn mixed_indicators_untrusted_wins() {
        assert_eq!(
            should_suppress(&cli_config(), &["argv", "socket"]),
            Some(false)
        );
    }

    #[test]
    fn empty_indicators_not_suppressed() {
        assert_eq!(should_suppress(&cli_config(), &[]), Some(false));
    }

    #[test]
    fn user_override_trusted() {
        let mut cfg = web_config();
        cfg.trusted_sources = vec!["request".into()];
        assert_eq!(should_suppress(&cfg, &["request"]), Some(true));
    }

    #[test]
    fn user_override_untrusted() {
        let mut cfg = cli_config();
        cfg.untrusted_sources = vec!["argv".into()];
        assert_eq!(should_suppress(&cfg, &["argv"]), Some(false));
    }

    #[test]
    fn unknown_indicator_treated_as_untrusted() {
        assert_eq!(
            should_suppress(&cli_config(), &["some_unknown_source"]),
            Some(false)
        );
    }

    #[test]
    fn ci_pipeline_trusts_argv_and_environ() {
        assert_eq!(should_suppress(&ci_config(), &["argv"]), Some(true));
        assert_eq!(should_suppress(&ci_config(), &["environ"]), Some(true));
    }

    #[test]
    fn ci_pipeline_does_not_trust_recv() {
        assert_eq!(should_suppress(&ci_config(), &["recv"]), Some(false));
    }

    #[test]
    fn cli_tool_trusts_format_macro() {
        assert_eq!(should_suppress(&cli_config(), &["format!"]), Some(true));
    }

    #[test]
    fn cli_tool_trusts_str_ref() {
        assert_eq!(should_suppress(&cli_config(), &["&str"]), Some(true));
    }
}
