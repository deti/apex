//! LibAFL QEMU backend for binary fuzzing (BAR 2024).
//!
//! Enables coverage-guided fuzzing of closed-source binaries via QEMU
//! user-mode emulation. Feature-gated behind `libafl-qemu`.

use std::path::PathBuf;

/// Configuration for the QEMU fuzzing backend.
#[derive(Debug, Clone)]
pub struct QemuConfig {
    pub binary_path: PathBuf,
    pub binary_args: Vec<String>,
    pub qemu_args: Vec<String>,
    pub max_input_size: usize,
    pub timeout_ms: u64,
    pub max_iterations: u64,
}

impl Default for QemuConfig {
    fn default() -> Self {
        QemuConfig {
            binary_path: PathBuf::new(),
            binary_args: Vec::new(),
            qemu_args: Vec::new(),
            max_input_size: 1024 * 1024,
            timeout_ms: 1000,
            max_iterations: 100_000,
        }
    }
}

impl QemuConfig {
    pub fn new(binary: PathBuf) -> Self {
        QemuConfig {
            binary_path: binary,
            ..Default::default()
        }
    }

    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.binary_args.push(arg.into());
        self
    }

    pub fn with_qemu_arg(mut self, arg: impl Into<String>) -> Self {
        self.qemu_args.push(arg.into());
        self
    }

    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    pub fn with_max_input_size(mut self, size: usize) -> Self {
        self.max_input_size = size;
        self
    }

    pub fn with_max_iterations(mut self, n: u64) -> Self {
        self.max_iterations = n;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.binary_path.as_os_str().is_empty() {
            return Err("binary_path must not be empty".into());
        }
        if self.timeout_ms == 0 {
            return Err("timeout_ms must be > 0".into());
        }
        if self.max_input_size == 0 {
            return Err("max_input_size must be > 0".into());
        }
        Ok(())
    }
}

/// Summary of a QEMU fuzzing run.
#[derive(Debug, Clone)]
pub struct QemuRunSummary {
    pub total_executions: u64,
    pub unique_edges: u64,
    pub crashes: u64,
    pub timeouts: u64,
    pub corpus_size: usize,
    pub duration_secs: f64,
}

impl QemuRunSummary {
    pub fn empty() -> Self {
        QemuRunSummary {
            total_executions: 0,
            unique_edges: 0,
            crashes: 0,
            timeouts: 0,
            corpus_size: 0,
            duration_secs: 0.0,
        }
    }

    pub fn execs_per_sec(&self) -> f64 {
        if self.duration_secs <= 0.0 {
            return 0.0;
        }
        self.total_executions as f64 / self.duration_secs
    }
}

/// The QEMU backend stub.
pub struct QemuBackend {
    pub config: QemuConfig,
}

impl QemuBackend {
    pub fn new(config: QemuConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(QemuBackend { config })
    }

    pub fn is_available() -> bool {
        cfg!(feature = "libafl-qemu")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_config_defaults() {
        let config = QemuConfig::default();
        assert_eq!(config.max_input_size, 1024 * 1024);
        assert_eq!(config.timeout_ms, 1000);
        assert_eq!(config.max_iterations, 100_000);
        assert!(config.binary_args.is_empty());
        assert!(config.qemu_args.is_empty());
    }

    #[test]
    fn qemu_config_new() {
        let config = QemuConfig::new(PathBuf::from("/usr/bin/target"));
        assert_eq!(config.binary_path, PathBuf::from("/usr/bin/target"));
    }

    #[test]
    fn qemu_config_builder() {
        let config = QemuConfig::new(PathBuf::from("/bin/test"))
            .with_arg("--flag")
            .with_qemu_arg("-cpu")
            .with_timeout_ms(5000)
            .with_max_input_size(2048)
            .with_max_iterations(50_000);
        assert_eq!(config.binary_args, vec!["--flag"]);
        assert_eq!(config.qemu_args, vec!["-cpu"]);
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_input_size, 2048);
        assert_eq!(config.max_iterations, 50_000);
    }

    #[test]
    fn qemu_config_validate_ok() {
        let config = QemuConfig::new(PathBuf::from("/bin/test"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn qemu_config_validate_empty_path() {
        let config = QemuConfig::default();
        assert!(config.validate().is_err());
        assert!(config.validate().unwrap_err().contains("binary_path"));
    }

    #[test]
    fn qemu_config_validate_zero_timeout() {
        let config = QemuConfig {
            binary_path: PathBuf::from("/bin/test"),
            timeout_ms: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn qemu_config_validate_zero_input_size() {
        let config = QemuConfig {
            binary_path: PathBuf::from("/bin/test"),
            max_input_size: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn qemu_run_summary_empty() {
        let summary = QemuRunSummary::empty();
        assert_eq!(summary.total_executions, 0);
        assert_eq!(summary.crashes, 0);
        assert_eq!(summary.unique_edges, 0);
    }

    #[test]
    fn qemu_run_summary_execs_per_sec() {
        let summary = QemuRunSummary {
            total_executions: 1000,
            duration_secs: 2.0,
            ..QemuRunSummary::empty()
        };
        assert!((summary.execs_per_sec() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn qemu_run_summary_execs_per_sec_zero_duration() {
        let summary = QemuRunSummary::empty();
        assert_eq!(summary.execs_per_sec(), 0.0);
    }

    #[test]
    fn qemu_backend_new_valid() {
        let config = QemuConfig::new(PathBuf::from("/bin/test"));
        assert!(QemuBackend::new(config).is_ok());
    }

    #[test]
    fn qemu_backend_new_invalid() {
        let config = QemuConfig::default();
        assert!(QemuBackend::new(config).is_err());
    }

    #[test]
    fn qemu_backend_availability() {
        // Without the feature flag, should be false
        assert!(!QemuBackend::is_available());
    }

    #[test]
    fn qemu_config_debug() {
        let config = QemuConfig::default();
        let _ = format!("{:?}", config);
    }

    #[test]
    fn qemu_run_summary_debug() {
        let summary = QemuRunSummary::empty();
        let _ = format!("{:?}", summary);
    }
}
