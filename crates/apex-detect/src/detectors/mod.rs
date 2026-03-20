pub mod bandit;
pub mod broken_access;
pub mod cegar;
pub mod command_injection;
pub mod crypto_failure;
pub mod data_transform_spec;
pub mod dep_audit;
pub mod dual_encoder;
pub mod flag_hygiene;
pub mod hagnn;
pub mod hardcoded_secret;
pub mod insecure_deserialization;
pub mod license_scan;
pub mod panic_pattern;
pub mod path_normalize;
pub mod path_traversal;
pub mod secret_scan;
pub mod security_pattern;
pub mod session_security;
pub mod spec_miner;
pub mod sql_injection;
pub mod ssrf;
pub mod static_analysis;
pub mod timeout;
pub mod unsafe_reach;
pub mod util;

// Dig 2 high-confidence detectors
pub mod blocking_io_in_async;
pub mod broad_exception;
pub mod error_context_loss;
pub mod regex_in_loop;
pub mod string_concat_in_loop;
pub mod swallowed_errors;

// P1 concurrency detectors
pub mod ffi_panic;
pub mod mutex_across_await;
pub mod open_without_with;
pub mod unbounded_queue;

// Rust self-analysis detectors
pub mod discarded_async_result;
pub mod duplicated_fn;
pub mod mixed_bool_ops;
pub mod partial_cmp_unwrap;
pub mod process_exit_in_lib;
pub mod substring_security;
pub mod unsafe_send_sync;
pub mod vecdeque_partial;

// JS/TS detectors
pub mod js_command_injection;
pub mod js_crypto_failure;
pub mod js_insecure_deser;
pub mod js_path_traversal;
pub mod js_sql_injection;
pub mod js_ssrf;
pub mod js_timeout;

pub use bandit::BanditRuleDetector;
pub use data_transform_spec::DataTransformSpecMiner;
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

// Dig 2 high-confidence detectors
pub use blocking_io_in_async::BlockingIoInAsyncDetector;
pub use broad_exception::BroadExceptionDetector;
pub use error_context_loss::ErrorContextLossDetector;
pub use regex_in_loop::RegexInLoopDetector;
pub use string_concat_in_loop::StringConcatInLoopDetector;
pub use swallowed_errors::SwallowedErrorsDetector;

// P1 concurrency detectors
pub use ffi_panic::FfiPanicDetector;
pub use mutex_across_await::MutexAcrossAwaitDetector;
pub use open_without_with::OpenWithoutWithDetector;
pub use unbounded_queue::UnboundedQueueDetector;

// Rust self-analysis detectors
pub use discarded_async_result::DiscardedAsyncResultDetector;
pub use duplicated_fn::DuplicatedFnDetector;
pub use mixed_bool_ops::MixedBoolOpsDetector;
pub use partial_cmp_unwrap::PartialCmpUnwrapDetector;
pub use process_exit_in_lib::ProcessExitInLibDetector;
pub use substring_security::SubstringSecurityDetector;
pub use unsafe_send_sync::UnsafeSendSyncDetector;
pub use vecdeque_partial::VecDequePartialDetector;

// JS/TS detectors
pub use js_command_injection::JsCommandInjectionDetector;
pub use js_crypto_failure::JsCryptoFailureDetector;
pub use js_insecure_deser::JsInsecureDeserDetector;
pub use js_path_traversal::JsPathTraversalDetector;
pub use js_sql_injection::JsSqlInjectionDetector;
pub use js_ssrf::JsSsrfDetector;
pub use js_timeout::JsTimeoutDetector;
