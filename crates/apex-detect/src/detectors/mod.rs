pub mod bandit;
pub mod cegar;
pub mod command_injection;
pub mod dep_audit;
pub mod dual_encoder;
pub mod flag_hygiene;
pub mod hagnn;
pub mod hardcoded_secret;
pub mod license_scan;
pub mod panic_pattern;
pub mod path_normalize;
pub mod path_traversal;
pub mod secret_scan;
pub mod security_pattern;
pub mod session_security;
pub mod spec_miner;
pub mod sql_injection;
pub mod static_analysis;
pub mod timeout;
pub mod unsafe_reach;
pub mod util;

// Rust self-analysis detectors
pub mod discarded_async_result;
pub mod duplicated_fn;
pub mod mixed_bool_ops;
pub mod partial_cmp_unwrap;
pub mod process_exit_in_lib;
pub mod substring_security;
pub mod unsafe_send_sync;
pub mod vecdeque_partial;

pub use bandit::BanditRuleDetector;
pub use dep_audit::DependencyAuditDetector;
pub use flag_hygiene::FlagHygieneDetector;
pub use hardcoded_secret::HardcodedSecretDetector;
pub use license_scan::LicenseScanDetector;
pub use panic_pattern::PanicPatternDetector;
pub use path_normalize::PathNormalizationDetector;
pub use secret_scan::SecretScanDetector;
pub use security_pattern::SecurityPatternDetector;
pub use session_security::SessionSecurityDetector;
pub use static_analysis::StaticAnalysisDetector;
pub use timeout::MissingTimeoutDetector;
pub use unsafe_reach::UnsafeReachabilityDetector;

// Rust self-analysis detectors
pub use discarded_async_result::DiscardedAsyncResultDetector;
pub use duplicated_fn::DuplicatedFnDetector;
pub use mixed_bool_ops::MixedBoolOpsDetector;
pub use partial_cmp_unwrap::PartialCmpUnwrapDetector;
pub use process_exit_in_lib::ProcessExitInLibDetector;
pub use substring_security::SubstringSecurityDetector;
pub use unsafe_send_sync::UnsafeSendSyncDetector;
pub use vecdeque_partial::VecDequePartialDetector;
