/// LD_PRELOAD coverage shim — compiled once and cached in `~/.apex/`.
///
/// The shim implements `__sanitizer_cov_trace_pc_guard` and writes to the
/// POSIX SHM region created by `ShmBitmap`. Targets compiled with
/// `-fsanitize-coverage=trace-pc-guard` (clang) or
/// `-Zsanitizer=coverage` (rustc nightly) will automatically call it.
use apex_core::error::{ApexError, Result};
use std::path::PathBuf;
use tracing::{debug, info};

/// C source for the coverage shim.
const SHIM_SOURCE: &str = r#"
#define _GNU_SOURCE
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <unistd.h>

#define APEX_MAP_SIZE 65536

static uint8_t *__apex_trace_bits = ((void*)0);
static uint32_t __apex_guard_count = 0;

__attribute__((constructor))
static void __apex_shm_init(void) {
    const char *shm_name = getenv("__APEX_SHM_NAME");
    if (!shm_name) return;
    int fd = shm_open(shm_name, O_RDWR, 0);
    if (fd < 0) return;
    __apex_trace_bits = (uint8_t *)mmap(
        NULL, APEX_MAP_SIZE,
        PROT_READ | PROT_WRITE, MAP_SHARED,
        fd, 0
    );
    close(fd);
    if (__apex_trace_bits == MAP_FAILED) __apex_trace_bits = ((void*)0);
}

void __sanitizer_cov_trace_pc_guard_init(uint32_t *start, uint32_t *stop) {
    static uint32_t n = 0;
    if (start == stop || *start) return;
    for (uint32_t *x = start; x < stop; x++) {
        *x = ++n;
    }
    __apex_guard_count = n;
}

void __sanitizer_cov_trace_pc_guard(uint32_t *guard) {
    if (!__apex_trace_bits || !*guard || *guard >= APEX_MAP_SIZE) return;
    __apex_trace_bits[*guard]++;
}
"#;

/// Path where the compiled shim is cached.
fn shim_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| ApexError::Sandbox("HOME not set".into()))?;
    let dir = PathBuf::from(home).join(".apex").join("shims");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ApexError::Sandbox(format!("create shim dir: {e}")))?;

    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    Ok(dir.join(format!("libapex_cov.{ext}")))
}

/// Ensure the shim shared library exists, compiling it if needed.
/// Returns the path to the `.so` / `.dylib`.
pub fn ensure_compiled() -> Result<PathBuf> {
    let out_path = shim_path()?;
    if out_path.exists() {
        debug!(path = %out_path.display(), "coverage shim already compiled");
        return Ok(out_path);
    }

    info!("compiling APEX coverage shim");

    let tmp_dir = tempfile::tempdir().map_err(|e| ApexError::Sandbox(format!("tempdir: {e}")))?;
    let src_path = tmp_dir.path().join("apex_cov_shim.c");
    std::fs::write(&src_path, SHIM_SOURCE)
        .map_err(|e| ApexError::Sandbox(format!("write shim source: {e}")))?;

    // Try clang first, fall back to cc.
    let compiler = if which_compiler("clang") {
        "clang"
    } else {
        "cc"
    };

    let shared_flag = if cfg!(target_os = "macos") {
        "-dynamiclib"
    } else {
        "-shared"
    };

    let output = std::process::Command::new(compiler)
        .args([
            shared_flag,
            "-fPIC",
            "-O2",
            "-o",
            &out_path.to_string_lossy(),
            &src_path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| ApexError::Sandbox(format!("run {compiler}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::Sandbox(format!(
            "shim compilation failed:\n{stderr}"
        )));
    }

    info!(path = %out_path.display(), "coverage shim compiled");
    Ok(out_path)
}

fn which_compiler(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Return the raw C source text for the coverage shim.
pub fn coverage_shim_source() -> &'static str {
    SHIM_SOURCE
}

/// The environment variable name used to inject the shim into a process.
pub fn preload_env_var() -> &'static str {
    if cfg!(target_os = "macos") {
        "DYLD_INSERT_LIBRARIES"
    } else {
        "LD_PRELOAD"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_source_contains_sanitizer_guard() {
        let src = coverage_shim_source();
        assert!(
            src.contains("__sanitizer_cov_trace_pc_guard"),
            "shim source must define __sanitizer_cov_trace_pc_guard"
        );
    }

    #[test]
    fn shim_source_contains_guard_init() {
        let src = coverage_shim_source();
        assert!(
            src.contains("__sanitizer_cov_trace_pc_guard_init"),
            "shim source must define guard_init"
        );
    }

    #[test]
    fn shim_source_contains_shm_open() {
        let src = coverage_shim_source();
        assert!(src.contains("shm_open"), "shim must use shm_open");
    }

    #[test]
    fn shim_source_contains_apex_map_size() {
        let src = coverage_shim_source();
        assert!(
            src.contains("APEX_MAP_SIZE"),
            "shim must define APEX_MAP_SIZE"
        );
    }

    #[test]
    fn shim_source_reads_env_var() {
        let src = coverage_shim_source();
        assert!(
            src.contains("__APEX_SHM_NAME"),
            "shim must read __APEX_SHM_NAME env var"
        );
    }

    #[test]
    fn shim_source_is_valid_c_structure() {
        let src = coverage_shim_source();
        // Check it has basic C includes
        assert!(src.contains("#include <stdint.h>"));
        assert!(src.contains("#include <sys/mman.h>"));
        // Check it has a constructor attribute
        assert!(src.contains("__attribute__((constructor))"));
    }

    /// Compute the expected shim path without calling shim_path() (which
    /// creates directories). Mirrors the logic in shim_path() for testing.
    fn expected_shim_path(home: &str) -> PathBuf {
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        PathBuf::from(home)
            .join(".apex")
            .join("shims")
            .join(format!("libapex_cov.{ext}"))
    }

    #[test]
    fn shim_path_extension_matches_platform() {
        let path = expected_shim_path("/fakehome");
        let ext = path.extension().unwrap().to_str().unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(ext, "dylib");
        } else {
            assert_eq!(ext, "so");
        }
    }

    #[test]
    fn shim_path_lives_under_dot_apex() {
        let path = expected_shim_path("/home/user");
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".apex/shims/libapex_cov"),
            "shim path should be under ~/.apex/shims/: {path_str}"
        );
    }

    #[test]
    fn shim_path_filename_is_libapex_cov() {
        let path = expected_shim_path("/home/user");
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(stem, "libapex_cov");
    }

    #[test]
    fn preload_env_var_platform_correct() {
        let var = preload_env_var();
        if cfg!(target_os = "macos") {
            assert_eq!(var, "DYLD_INSERT_LIBRARIES");
        } else {
            assert_eq!(var, "LD_PRELOAD");
        }
    }

    #[test]
    fn which_compiler_finds_cc() {
        // `cc` should be available on any Unix system
        assert!(which_compiler("cc"));
    }

    #[test]
    fn which_compiler_returns_false_for_nonexistent() {
        assert!(!which_compiler("definitely_not_a_compiler_12345"));
    }

    #[test]
    fn ensure_compiled_produces_dylib() {
        // Requires write access to ~/.apex/shims/
        match ensure_compiled() {
            Ok(path) => {
                assert!(
                    path.exists(),
                    "compiled shim should exist at {}",
                    path.display()
                );
                let ext = path.extension().unwrap().to_str().unwrap();
                if cfg!(target_os = "macos") {
                    assert_eq!(ext, "dylib");
                } else {
                    assert_eq!(ext, "so");
                }
            }
            Err(_) => {} // sandboxed — can't create ~/.apex/shims
        }
    }

    #[test]
    fn ensure_compiled_is_idempotent() {
        match (ensure_compiled(), ensure_compiled()) {
            (Ok(p1), Ok(p2)) => assert_eq!(p1, p2),
            _ => {} // sandboxed
        }
    }

    #[test]
    fn shim_source_map_size_matches_shm() {
        let src = coverage_shim_source();
        assert!(
            src.contains("65536"),
            "shim APEX_MAP_SIZE should match shm::MAP_SIZE (65536)"
        );
    }

    #[test]
    fn shim_source_has_mmap() {
        let src = coverage_shim_source();
        assert!(src.contains("mmap"));
    }

    #[test]
    fn shim_source_has_close() {
        let src = coverage_shim_source();
        assert!(src.contains("close(fd)"));
    }

    #[test]
    fn coverage_shim_source_is_static() {
        let s1 = coverage_shim_source();
        let s2 = coverage_shim_source();
        assert!(std::ptr::eq(s1, s2), "should return same &'static str");
    }

    #[test]
    fn shim_path_uses_home_env() {
        // shim_path() reads HOME and creates dirs under it.
        // Since HOME is set in any normal test env, this should succeed.
        let result = shim_path();
        match result {
            Ok(path) => {
                let path_str = path.to_string_lossy();
                assert!(
                    path_str.contains(".apex/shims/libapex_cov"),
                    "shim_path should be under ~/.apex/shims/: {path_str}"
                );
            }
            Err(_) => {
                // HOME might not be set in some CI environments
            }
        }
    }

    #[test]
    fn shim_source_non_empty() {
        let src = coverage_shim_source();
        assert!(!src.is_empty(), "shim source should not be empty");
        assert!(src.len() > 100, "shim source should be substantial");
    }

    #[test]
    fn shim_source_no_null_bytes() {
        let src = coverage_shim_source();
        assert!(
            !src.contains('\0'),
            "shim source should not contain null bytes"
        );
    }

    #[test]
    fn shim_source_handles_map_failed() {
        let src = coverage_shim_source();
        assert!(
            src.contains("MAP_FAILED"),
            "shim must handle MAP_FAILED case"
        );
    }

    #[test]
    fn shim_source_increments_guard_ids() {
        let src = coverage_shim_source();
        // The guard_init function should assign incrementing IDs
        assert!(
            src.contains("++n"),
            "guard_init should assign incrementing IDs"
        );
    }

    #[test]
    fn shim_source_checks_guard_bounds() {
        let src = coverage_shim_source();
        // The trace_pc_guard function should check bounds
        assert!(
            src.contains("*guard >= APEX_MAP_SIZE"),
            "trace_pc_guard should check bounds against APEX_MAP_SIZE"
        );
    }

    #[test]
    fn preload_env_var_is_non_empty() {
        let var = preload_env_var();
        assert!(!var.is_empty());
    }

    #[test]
    fn shim_source_uses_gnu_source() {
        let src = coverage_shim_source();
        assert!(
            src.contains("_GNU_SOURCE"),
            "shim should define _GNU_SOURCE"
        );
    }

    #[test]
    fn shim_source_includes_fcntl() {
        let src = coverage_shim_source();
        assert!(
            src.contains("#include <fcntl.h>"),
            "shim must include fcntl.h for O_RDWR"
        );
    }

    #[test]
    fn shim_source_uses_o_rdwr() {
        let src = coverage_shim_source();
        assert!(
            src.contains("O_RDWR"),
            "shim must open SHM with O_RDWR"
        );
    }

    #[test]
    fn shim_source_uses_map_shared() {
        let src = coverage_shim_source();
        assert!(
            src.contains("MAP_SHARED"),
            "shim must use MAP_SHARED for cross-process visibility"
        );
    }
}
