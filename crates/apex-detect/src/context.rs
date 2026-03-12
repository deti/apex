use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use apex_core::types::{BugReport, Language};
use apex_coverage::CoverageOracle;

use crate::config::DetectConfig;

#[derive(Clone)]
pub struct AnalysisContext {
    pub target_root: PathBuf,
    pub language: Language,
    pub oracle: Arc<CoverageOracle>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub known_bugs: Vec<BugReport>,
    pub source_cache: HashMap<PathBuf, String>,
    pub fuzz_corpus: Option<PathBuf>,
    pub config: DetectConfig,
}

impl fmt::Debug for AnalysisContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisContext")
            .field("target_root", &self.target_root)
            .field("language", &self.language)
            .field("file_paths", &self.file_paths.len())
            .field("source_cache", &self.source_cache.len())
            .field("fuzz_corpus", &self.fuzz_corpus)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_impl_does_not_dump_full_cache() {
        let ctx = AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: Language::Python,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: {
                let mut m = HashMap::new();
                m.insert(PathBuf::from("a.py"), "code".into());
                m
            },
            fuzz_corpus: Some(PathBuf::from("/corpus")),
            config: DetectConfig::default(),
        };
        let dbg = format!("{:?}", ctx);
        assert!(dbg.contains("AnalysisContext"));
        assert!(dbg.contains("/tmp/test"));
        assert!(dbg.contains("Python"));
        // source_cache shows count, not full contents
        assert!(dbg.contains("1"));
        assert!(dbg.contains("/corpus"));
    }

    #[test]
    fn debug_impl_with_no_corpus() {
        let ctx = AnalysisContext {
            target_root: PathBuf::from("/proj"),
            language: Language::Rust,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: HashMap::new(),
            fuzz_corpus: None,
            config: DetectConfig::default(),
        };
        let dbg = format!("{:?}", ctx);
        assert!(dbg.contains("None"));
    }
}
