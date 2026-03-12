//! RUSTC_WRAPPER logic for SanCov-instrumented builds.
//!
//! Generates the rustc flags needed to enable SanCov instrumentation.

/// SanCov instrumentation mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SanCovMode {
    /// trace-pc-guard: function call per edge. Most flexible.
    TracePcGuard,
    /// inline-8bit-counters: one `inc` instruction per edge. 2-5x faster.
    Inline8BitCounters,
    /// inline-bool-flag: one store per edge. Fastest, binary only.
    InlineBoolFlag,
}

/// Generate rustc flags for SanCov instrumentation.
pub fn sancov_rustc_flags(mode: SanCovMode, trace_compares: bool) -> Vec<String> {
    let mut flags = vec![
        "-C".into(),
        "passes=sancov-module".into(),
        "-C".into(),
        "llvm-args=-sanitizer-coverage-level=3".into(),
        "-C".into(),
        "codegen-units=1".into(),
    ];

    match mode {
        SanCovMode::TracePcGuard => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-trace-pc-guard".into());
        }
        SanCovMode::Inline8BitCounters => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-8bit-counters".into());
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-pc-table".into());
        }
        SanCovMode::InlineBoolFlag => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-bool-flag".into());
        }
    }

    if trace_compares {
        flags.push("-C".into());
        flags.push("llvm-args=-sanitizer-coverage-trace-compares".into());
    }

    flags
}

/// Generate a complete RUSTC_WRAPPER shell command string.
pub fn wrapper_command(rustc_path: &str, mode: SanCovMode, trace_compares: bool) -> String {
    let flags = sancov_rustc_flags(mode, trace_compares);
    let flag_str = flags.join(" ");
    format!("{rustc_path} {flag_str} \"$@\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_pc_guard_flags() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"passes=sancov-module".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-pc-guard".to_string()));
        assert!(!flags.iter().any(|f| f.contains("trace-compares")));
    }

    #[test]
    fn inline_8bit_counters_flags() {
        let flags = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-8bit-counters".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-pc-table".to_string()));
    }

    #[test]
    fn inline_bool_flag_flags() {
        let flags = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-bool-flag".to_string()));
    }

    #[test]
    fn trace_compares_added() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, true);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-compares".to_string()));
    }

    #[test]
    fn codegen_units_1() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"codegen-units=1".to_string()));
    }

    #[test]
    fn wrapper_command_format() {
        let cmd = wrapper_command("rustc", SanCovMode::TracePcGuard, false);
        assert!(cmd.starts_with("rustc"));
        assert!(cmd.contains("sancov-module"));
        assert!(cmd.ends_with("\"$@\""));
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn wrapper_command_with_trace_compares() {
        let cmd = wrapper_command("rustc", SanCovMode::TracePcGuard, true);
        assert!(cmd.contains("trace-compares"));
        assert!(cmd.contains("trace-pc-guard"));
        assert!(cmd.ends_with("\"$@\""));
    }

    #[test]
    fn wrapper_command_inline_8bit() {
        let cmd = wrapper_command("/usr/bin/rustc", SanCovMode::Inline8BitCounters, false);
        assert!(cmd.starts_with("/usr/bin/rustc"));
        assert!(cmd.contains("inline-8bit-counters"));
        assert!(cmd.contains("pc-table"));
        assert!(!cmd.contains("trace-compares"));
    }

    #[test]
    fn wrapper_command_inline_bool_flag() {
        let cmd = wrapper_command("rustc", SanCovMode::InlineBoolFlag, false);
        assert!(cmd.contains("inline-bool-flag"));
        assert!(!cmd.contains("inline-8bit-counters"));
        assert!(!cmd.contains("trace-pc-guard"));
    }

    #[test]
    fn wrapper_command_inline_bool_flag_with_compares() {
        let cmd = wrapper_command("rustc", SanCovMode::InlineBoolFlag, true);
        assert!(cmd.contains("inline-bool-flag"));
        assert!(cmd.contains("trace-compares"));
    }

    #[test]
    fn wrapper_command_inline_8bit_with_compares() {
        let cmd = wrapper_command("rustc", SanCovMode::Inline8BitCounters, true);
        assert!(cmd.contains("inline-8bit-counters"));
        assert!(cmd.contains("pc-table"));
        assert!(cmd.contains("trace-compares"));
    }

    #[test]
    fn trace_pc_guard_no_compares_flag_count() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        // Should have 4 base pairs + 1 mode pair = 8 strings
        assert_eq!(flags.len(), 8);
    }

    #[test]
    fn inline_8bit_no_compares_flag_count() {
        let flags = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        // 4 base + 2 mode pairs = 10 strings
        assert_eq!(flags.len(), 10);
    }

    #[test]
    fn inline_bool_no_compares_flag_count() {
        let flags = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        // 4 base + 1 mode pair = 8 strings
        assert_eq!(flags.len(), 8);
    }

    #[test]
    fn trace_compares_adds_two_flags() {
        let without = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        let with = sancov_rustc_flags(SanCovMode::TracePcGuard, true);
        assert_eq!(with.len(), without.len() + 2);
    }

    #[test]
    fn sancov_mode_debug_and_clone() {
        let mode = SanCovMode::TracePcGuard;
        let cloned = mode;
        assert_eq!(mode, cloned);
        assert_eq!(format!("{:?}", mode), "TracePcGuard");
        assert_eq!(
            format!("{:?}", SanCovMode::Inline8BitCounters),
            "Inline8BitCounters"
        );
        assert_eq!(
            format!("{:?}", SanCovMode::InlineBoolFlag),
            "InlineBoolFlag"
        );
    }

    #[test]
    fn sancov_mode_equality() {
        assert_eq!(SanCovMode::TracePcGuard, SanCovMode::TracePcGuard);
        assert_ne!(SanCovMode::TracePcGuard, SanCovMode::Inline8BitCounters);
        assert_ne!(SanCovMode::Inline8BitCounters, SanCovMode::InlineBoolFlag);
    }

    #[test]
    fn all_flags_come_in_pairs() {
        // Every flag should have a -C prefix followed by the actual flag
        for mode in [
            SanCovMode::TracePcGuard,
            SanCovMode::Inline8BitCounters,
            SanCovMode::InlineBoolFlag,
        ] {
            for trace_compares in [false, true] {
                let flags = sancov_rustc_flags(mode, trace_compares);
                assert_eq!(
                    flags.len() % 2,
                    0,
                    "flags should come in pairs for {mode:?}, trace_compares={trace_compares}"
                );
                for i in (0..flags.len()).step_by(2) {
                    assert_eq!(flags[i], "-C", "even-indexed flag should be -C");
                }
            }
        }
    }

    #[test]
    fn wrapper_command_custom_rustc_path() {
        let cmd = wrapper_command("/opt/rust/bin/rustc", SanCovMode::TracePcGuard, false);
        assert!(cmd.starts_with("/opt/rust/bin/rustc "));
    }

    #[test]
    fn base_flags_always_present() {
        for mode in [
            SanCovMode::TracePcGuard,
            SanCovMode::Inline8BitCounters,
            SanCovMode::InlineBoolFlag,
        ] {
            let flags = sancov_rustc_flags(mode, false);
            assert!(flags.contains(&"passes=sancov-module".to_string()));
            assert!(flags.contains(&"llvm-args=-sanitizer-coverage-level=3".to_string()));
            assert!(flags.contains(&"codegen-units=1".to_string()));
        }
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn sancov_inline8bit_has_pc_table_flag() {
        // pc-table is only added for Inline8BitCounters
        let flags = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        assert!(flags.iter().any(|f| f.contains("pc-table")));

        let flags_trace = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(!flags_trace.iter().any(|f| f.contains("pc-table")));
    }

    #[test]
    fn sancov_trace_pc_guard_not_in_others() {
        let flags_8bit = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        assert!(!flags_8bit
            .iter()
            .any(|f| f.contains("trace-pc-guard")));

        let flags_bool = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        assert!(!flags_bool
            .iter()
            .any(|f| f.contains("trace-pc-guard")));
    }

    #[test]
    fn wrapper_command_no_trace_compares_does_not_contain_flag() {
        for mode in [
            SanCovMode::TracePcGuard,
            SanCovMode::Inline8BitCounters,
            SanCovMode::InlineBoolFlag,
        ] {
            let cmd = wrapper_command("rustc", mode, false);
            assert!(
                !cmd.contains("trace-compares"),
                "trace-compares should be absent when not requested, mode={mode:?}"
            );
        }
    }

    #[test]
    fn wrapper_command_contains_dollar_at() {
        // All modes should end with "$@" for proper argument forwarding
        for mode in [
            SanCovMode::TracePcGuard,
            SanCovMode::Inline8BitCounters,
            SanCovMode::InlineBoolFlag,
        ] {
            for tc in [false, true] {
                let cmd = wrapper_command("rustc", mode, tc);
                assert!(
                    cmd.ends_with("\"$@\""),
                    "should end with \"$@\", mode={mode:?}, trace_compares={tc}"
                );
            }
        }
    }

    #[test]
    fn sancov_mode_copy_semantics() {
        // SanCovMode should be Copy
        let m = SanCovMode::TracePcGuard;
        let _copy = m; // This would move if not Copy
        assert_eq!(m, SanCovMode::TracePcGuard);
    }

    #[test]
    fn sancov_inline_bool_flag_has_correct_unique_flag() {
        let flags = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        assert!(flags
            .iter()
            .any(|f| f.contains("inline-bool-flag")));
        assert!(!flags
            .iter()
            .any(|f| f.contains("inline-8bit-counters")));
    }
}
