use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsRuntime {
    Node,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTestRunner {
    Jest,
    Mocha,
    Vitest,
    BunTest,
    NpmScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSystem {
    CommonJS,
    ESM,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonorepoKind {
    NpmWorkspaces,
    Yarn,
    Pnpm,
    Turborepo,
    Nx,
}

#[derive(Debug, Clone)]
pub struct JsEnvironment {
    pub runtime: JsRuntime,
    pub pkg_manager: PkgManager,
    pub test_runner: JsTestRunner,
    pub module_system: ModuleSystem,
    pub is_typescript: bool,
    pub source_maps: bool,
    pub monorepo: Option<MonorepoKind>,
}

impl JsEnvironment {
    /// Detect the JS/TS project environment from the filesystem.
    pub fn detect(target: &Path) -> Option<JsEnvironment> {
        if !target.join("package.json").exists() {
            return None;
        }

        let runtime = detect_runtime(target);
        let pkg_manager = detect_pkg_manager(target, runtime);
        let test_runner = detect_test_runner(target);
        let module_system = detect_module_system(target);
        let is_typescript = detect_typescript(target);
        let source_maps = is_typescript;
        let monorepo = detect_monorepo(target);

        Some(JsEnvironment {
            runtime,
            pkg_manager,
            test_runner,
            module_system,
            is_typescript,
            source_maps,
            monorepo,
        })
    }
}

fn detect_runtime(target: &Path) -> JsRuntime {
    if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
        JsRuntime::Bun
    } else {
        JsRuntime::Node
    }
}

fn detect_pkg_manager(target: &Path, runtime: JsRuntime) -> PkgManager {
    if runtime == JsRuntime::Bun {
        return PkgManager::Bun;
    }
    if target.join("yarn.lock").exists() {
        return PkgManager::Yarn;
    }
    if target.join("pnpm-lock.yaml").exists() {
        return PkgManager::Pnpm;
    }
    PkgManager::Npm
}

/// Detect test runner from package.json content.
pub fn detect_test_runner(target: &Path) -> JsTestRunner {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    if detect_runtime(target) == JsRuntime::Bun {
        if pkg_content.contains("\"vitest\"") {
            return JsTestRunner::Vitest;
        }
        return JsTestRunner::BunTest;
    }

    if pkg_content.contains("\"jest\"") {
        return JsTestRunner::Jest;
    }
    if pkg_content.contains("\"mocha\"") {
        return JsTestRunner::Mocha;
    }
    if pkg_content.contains("\"vitest\"") {
        return JsTestRunner::Vitest;
    }
    if pkg_content.contains("\"scripts\"") && pkg_content.contains("\"test\"") {
        return JsTestRunner::NpmScript;
    }
    JsTestRunner::Jest
}

fn detect_module_system(target: &Path) -> ModuleSystem {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();
    let has_type_module =
        pkg_content.contains("\"type\": \"module\"") || pkg_content.contains("\"type\":\"module\"");

    let src_dir = target.join("src");
    let has_mjs = src_dir.join("index.mjs").exists();
    let has_cjs = src_dir.join("index.cjs").exists();

    match (has_type_module, has_mjs, has_cjs) {
        (true, _, true) => ModuleSystem::Mixed,
        (true, _, _) => ModuleSystem::ESM,
        (false, true, _) => ModuleSystem::Mixed,
        _ => ModuleSystem::CommonJS,
    }
}

fn detect_typescript(target: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("tsconfig") && name_str.ends_with(".json") {
                return true;
            }
        }
    }
    let src_dir = target.join("src");
    if let Ok(entries) = std::fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".ts") || name_str.ends_with(".tsx") {
                return true;
            }
        }
    }
    false
}

fn detect_monorepo(target: &Path) -> Option<MonorepoKind> {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    if target.join("nx.json").exists() {
        return Some(MonorepoKind::Nx);
    }
    if target.join("turbo.json").exists() {
        return Some(MonorepoKind::Turborepo);
    }
    if target.join("pnpm-workspace.yaml").exists() {
        return Some(MonorepoKind::Pnpm);
    }
    if pkg_content.contains("\"workspaces\"") {
        if target.join("yarn.lock").exists() {
            return Some(MonorepoKind::Yarn);
        }
        return Some(MonorepoKind::NpmWorkspaces);
    }
    None
}

/// Return the test command for the given environment.
pub fn test_command(env: &JsEnvironment) -> (String, Vec<String>) {
    match env.test_runner {
        JsTestRunner::Jest => (
            "npx".to_string(),
            vec!["jest".to_string(), "--passWithNoTests".to_string()],
        ),
        JsTestRunner::Mocha => ("npx".to_string(), vec!["mocha".to_string()]),
        JsTestRunner::Vitest => (
            "npx".to_string(),
            vec!["vitest".to_string(), "run".to_string()],
        ),
        JsTestRunner::BunTest => ("bun".to_string(), vec!["test".to_string()]),
        JsTestRunner::NpmScript => ("npm".to_string(), vec!["test".to_string()]),
    }
}

/// Return the install command for the given environment.
pub fn install_command(env: &JsEnvironment) -> &'static str {
    match env.pkg_manager {
        PkgManager::Npm => "npm",
        PkgManager::Yarn => "yarn",
        PkgManager::Pnpm => "pnpm",
        PkgManager::Bun => "bun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_none_without_package_json() {
        let dir = tempdir().unwrap();
        assert!(JsEnvironment::detect(dir.path()).is_none());
    }

    #[test]
    fn detect_basic_npm_project() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "test", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Node);
        assert_eq!(env.pkg_manager, PkgManager::Npm);
        assert_eq!(env.test_runner, JsTestRunner::Jest);
        assert_eq!(env.module_system, ModuleSystem::CommonJS);
        assert!(!env.is_typescript);
        assert!(env.monorepo.is_none());
    }

    #[test]
    fn detect_typescript_via_tsconfig() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts-proj"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
        assert!(env.source_maps);
    }

    #[test]
    fn detect_typescript_via_tsconfig_build() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.build.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_typescript_via_ts_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/index.ts"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_bun_runtime() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "bun-proj"}"#).unwrap();
        std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Bun);
        assert_eq!(env.pkg_manager, PkgManager::Bun);
        assert_eq!(env.test_runner, JsTestRunner::BunTest);
    }

    #[test]
    fn detect_yarn_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "yarn-proj", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Yarn);
    }

    #[test]
    fn detect_pnpm_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "pnpm"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Pnpm);
    }

    #[test]
    fn detect_esm_module_system() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "esm", "type": "module"}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.module_system, ModuleSystem::ESM);
    }

    #[test]
    fn detect_vitest_runner() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.test_runner, JsTestRunner::Vitest);
    }

    #[test]
    fn detect_npm_workspaces_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "root", "workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::NpmWorkspaces));
    }

    #[test]
    fn detect_turborepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("turbo.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Turborepo));
    }

    #[test]
    fn detect_nx_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("nx.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Nx));
    }

    #[test]
    fn detect_pnpm_workspace_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*",
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Pnpm));
    }

    #[test]
    fn test_command_jest() {
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    #[test]
    fn test_command_bun() {
        let env = JsEnvironment {
            runtime: JsRuntime::Bun,
            pkg_manager: PkgManager::Bun,
            test_runner: JsTestRunner::BunTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "bun");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn install_command_variants() {
        assert_eq!(
            install_command(&JsEnvironment {
                runtime: JsRuntime::Node,
                pkg_manager: PkgManager::Npm,
                test_runner: JsTestRunner::Jest,
                module_system: ModuleSystem::CommonJS,
                is_typescript: false,
                source_maps: false,
                monorepo: None,
            }),
            "npm"
        );

        assert_eq!(
            install_command(&JsEnvironment {
                runtime: JsRuntime::Bun,
                pkg_manager: PkgManager::Bun,
                test_runner: JsTestRunner::BunTest,
                module_system: ModuleSystem::ESM,
                is_typescript: false,
                source_maps: false,
                monorepo: None,
            }),
            "bun"
        );
    }
}
