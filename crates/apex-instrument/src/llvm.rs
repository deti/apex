/// LLVM SanitizerCoverage instrumentation (feature-gated: `llvm-instrument`).
///
/// Full implementation:
/// 1. Recompile C/Rust target with `-fsanitize-coverage=trace-pc-guard`
/// 2. Parse the resulting binary's DWARF info to extract source locations
/// 3. Map each guard index to a `BranchId` (file_id, line, col, direction)
///
/// Without the feature, returns a stub that reports zero branches discovered.
use apex_core::{
    command::{CommandRunner, RealCommandRunner},
    error::Result,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

pub struct LlvmInstrumentor {
    branch_ids: Vec<BranchId>,
    /// Command runner for subprocess execution. Currently used only with
    /// the `llvm-instrument` feature; stored here for future use.
    #[allow(dead_code)]
    runner: Arc<dyn CommandRunner>,
}

impl LlvmInstrumentor {
    pub fn new() -> Self {
        LlvmInstrumentor {
            branch_ids: Vec::new(),
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        LlvmInstrumentor {
            branch_ids: Vec::new(),
            runner,
        }
    }
}

impl Default for LlvmInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for LlvmInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        #[cfg(feature = "llvm-instrument")]
        {
            instrument_llvm(target).await
        }

        #[cfg(not(feature = "llvm-instrument"))]
        {
            warn!(
                "LlvmInstrumentor: `llvm-instrument` feature not enabled; \
                 returning empty branch set. Rebuild with \
                 --features apex-instrument/llvm-instrument for full support."
            );
            Ok(InstrumentedTarget {
                target: target.clone(),
                branch_ids: Vec::new(),
                executed_branch_ids: Vec::new(),
                file_paths: HashMap::new(),
                work_dir: target.root.clone(),
            })
        }
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}

// ---------------------------------------------------------------------------
// Full LLVM implementation (only compiled with the feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "llvm-instrument")]
async fn instrument_llvm(target: &Target) -> Result<InstrumentedTarget> {
    use addr2line::Context;
    use apex_core::error::ApexError;
    use object::{Object, ObjectSection};
    use std::path::{Path, PathBuf};

    let binary_path = target.root.join("target_apex");
    if !binary_path.exists() {
        return Err(ApexError::Instrumentation(format!(
            "instrumented binary not found at {}; \
             run `apex instrument` first",
            binary_path.display()
        )));
    }

    let data = std::fs::read(&binary_path)
        .map_err(|e| ApexError::Instrumentation(format!("read binary: {e}")))?;
    let obj = object::File::parse(&*data)
        .map_err(|e| ApexError::Instrumentation(format!("parse binary: {e}")))?;

    let ctx = Context::new(&obj)
        .map_err(|e| ApexError::Instrumentation(format!("addr2line context: {e}")))?;

    let mut branch_ids = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    if let Some(section) = obj
        .sections()
        .find(|s| s.name().map_or(false, |n| n.contains("sancov")))
    {
        let data = section
            .data()
            .map_err(|e| ApexError::Instrumentation(format!("section data: {e}")))?;

        let ptr_size = if obj.is_64() { 8usize } else { 4 };
        for chunk in data.chunks(ptr_size) {
            let addr = if ptr_size == 8 {
                u64::from_le_bytes(chunk.try_into().unwrap_or([0; 8]))
            } else {
                u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])) as u64
            };

            if let Ok(mut frames) = ctx.find_frames(addr) {
                if let Ok(Some(frame)) = frames.next() {
                    if let Some(loc) = frame.location {
                        let file = loc.file.unwrap_or("<unknown>");
                        let line = loc.line.unwrap_or(0);
                        let col = loc.column.unwrap_or(0);

                        let rel = Path::new(file)
                            .strip_prefix(&target.root)
                            .unwrap_or(Path::new(file));
                        let file_id = fnv1a_hash(&rel.to_string_lossy());
                        file_paths.insert(file_id, rel.to_path_buf());

                        branch_ids.push(BranchId::new(file_id, line, col as u16, 0));
                    }
                }
            }
        }
    }

    Ok(InstrumentedTarget {
        target: target.clone(),
        branch_ids,
        executed_branch_ids: Vec::new(),
        file_paths,
        work_dir: target.root.clone(),
    })
}

#[cfg(feature = "llvm-instrument")]
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::{CommandOutput, CommandSpec};
    use std::path::PathBuf;

    /// A test-only CommandRunner.
    struct FakeRunner;

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(
            &self,
            _spec: &CommandSpec,
        ) -> apex_core::error::Result<CommandOutput> {
            Ok(CommandOutput::success(Vec::new()))
        }
    }

    #[test]
    fn test_new_and_default() {
        let inst = LlvmInstrumentor::new();
        assert!(inst.branch_ids().is_empty());
        let inst2 = LlvmInstrumentor::default();
        assert!(inst2.branch_ids().is_empty());
    }

    #[test]
    fn test_with_runner() {
        let runner = Arc::new(FakeRunner);
        let inst = LlvmInstrumentor::with_runner(runner);
        assert!(inst.branch_ids().is_empty());
    }

    #[tokio::test]
    async fn test_instrument_without_feature_returns_empty() {
        // Without llvm-instrument feature, should return empty branch set
        let runner = Arc::new(FakeRunner);
        let inst = LlvmInstrumentor::with_runner(runner);

        let target = Target {
            root: PathBuf::from("/tmp/fake-project"),
            language: apex_core::types::Language::C,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        // Without the feature flag, always returns empty
        #[cfg(not(feature = "llvm-instrument"))]
        {
            assert!(result.branch_ids.is_empty());
            assert!(result.executed_branch_ids.is_empty());
            assert!(result.file_paths.is_empty());
            assert_eq!(result.work_dir, PathBuf::from("/tmp/fake-project"));
        }
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_branch_ids_returns_empty_slice() {
        use apex_core::traits::Instrumentor;
        let inst = LlvmInstrumentor::new();
        let ids = inst.branch_ids();
        assert!(ids.is_empty());
        assert_eq!(ids.len(), 0);
    }

    #[test]
    fn test_default_and_new_are_equivalent() {
        use apex_core::traits::Instrumentor;
        let inst1 = LlvmInstrumentor::new();
        let inst2 = LlvmInstrumentor::default();
        assert_eq!(inst1.branch_ids().len(), inst2.branch_ids().len());
    }

    #[tokio::test]
    async fn test_instrument_preserves_target_fields() {
        let runner = Arc::new(FakeRunner);
        let inst = LlvmInstrumentor::with_runner(runner);

        let target = Target {
            root: PathBuf::from("/my/project"),
            language: apex_core::types::Language::C,
            test_command: vec!["make".into(), "test".into()],
        };

        let result = inst.instrument(&target).await.unwrap();
        #[cfg(not(feature = "llvm-instrument"))]
        {
            assert_eq!(result.target.root, PathBuf::from("/my/project"));
            assert_eq!(result.target.language, apex_core::types::Language::C);
            assert_eq!(result.target.test_command, vec!["make", "test"]);
            assert_eq!(result.work_dir, PathBuf::from("/my/project"));
        }
    }

    #[tokio::test]
    async fn test_instrument_with_different_languages() {
        let runner = Arc::new(FakeRunner);
        let inst = LlvmInstrumentor::with_runner(runner);

        // LLVM instrumentor should work with any target language
        for lang in [
            apex_core::types::Language::C,
            apex_core::types::Language::Rust,
        ] {
            let target = Target {
                root: PathBuf::from("/tmp/test"),
                language: lang,
                test_command: Vec::new(),
            };

            let result = inst.instrument(&target).await.unwrap();
            #[cfg(not(feature = "llvm-instrument"))]
            {
                assert!(result.branch_ids.is_empty());
            }
        }
    }
}
