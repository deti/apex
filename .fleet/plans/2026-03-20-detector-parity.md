<!-- status: ACTIVE -->

# Security Detector Feature Parity Across All 11 Languages

**Goal:** Close 75 security detector gaps so all 11 languages have full coverage of all 10 security detectors.

**Date:** 2026-03-20
**Crew:** security-detect (all tasks)
**Reviewers:** lang-jvm (Java/Kotlin sinks), lang-go (Go sinks), lang-c-cpp (C/C++ sinks), lang-dotnet (C# sinks), lang-swift (Swift sinks), lang-ruby (Ruby sinks), lang-rust (Rust sinks)

## Architecture Analysis

Two detector architectures coexist today:

1. **JS Detector trait impls** (`js_command_injection.rs`, `js_sql_injection.rs`, etc.) -- implement `Detector` trait, wired into `DetectorPipeline::from_config()` with `&& lang == Language::JavaScript` guards. Well-structured with compiled regex patterns.

2. **Python standalone scanners** (`command_injection.rs`, `sql_injection.rs`, `crypto_failure.rs`, `ssrf.rs`, `path_traversal.rs`, `insecure_deserialization.rs`) -- `pub fn scan_*()` functions, Python-centric patterns, NOT wired into the pipeline (dead code from pipeline perspective, only tested via unit tests).

**Strategy:** For each of the 7 detector categories, create a single unified multi-language detector that:
- Implements the `Detector` trait (like the JS detectors do)
- Contains per-language source/sink/sanitizer tables via `match lang { ... }`
- Absorbs patterns from both the existing JS detector AND the Python standalone scanner
- Replaces the JS-only guard in `pipeline.rs` with the unified detector
- Keeps the old JS detector and Python scanner as deprecated (removed in a follow-up)

## File Map

| Crew | Files |
|------|-------|
| security-detect | `crates/apex-detect/src/detectors/multi_command_injection.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/multi_sql_injection.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/multi_crypto_failure.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/multi_insecure_deser.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/multi_ssrf.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/multi_path_traversal.rs` (new) |
| security-detect | `crates/apex-detect/src/detectors/hardcoded_secret.rs` (extend) |
| security-detect | `crates/apex-detect/src/detectors/secret_scan.rs` (extend) |
| security-detect | `crates/apex-detect/src/detectors/path_normalize.rs` (extend) |
| security-detect | `crates/apex-detect/src/detectors/mod.rs` (wire new modules) |
| security-detect | `crates/apex-detect/src/pipeline.rs` (register unified detectors) |

## Wave 1 -- Unified Multi-Language Detectors (parallel, 6 subtasks)

Each subtask creates one new `multi_*.rs` file implementing `Detector` trait with per-language pattern dispatch. All 6 are independent and can run in parallel.

### Task 1.1 -- multi_command_injection.rs

**Files:** `crates/apex-detect/src/detectors/multi_command_injection.rs` (new)
**CWE:** 78

Per-language command execution sinks:

| Language | Sinks |
|----------|-------|
| Python | `subprocess.run`, `subprocess.call`, `subprocess.Popen` with `shell=True`; `os.system(`, `os.popen(`, `commands.getoutput(` |
| JavaScript | `child_process.exec`, `child_process.execSync`, `shelljs.exec`, template-literal exec |
| Java | `Runtime.getRuntime().exec(`, `ProcessBuilder(`, `new ProcessBuilder(` |
| Go | `exec.Command(`, `exec.CommandContext(`, `syscall.Exec(` |
| C/C++ | `system(`, `popen(`, `execl(`, `execv(`, `execvp(` |
| C# | `Process.Start(`, `ProcessStartInfo(`, `new Process(` |
| Swift | `Process()`, `NSTask(`, `Process.launchedProcess(` |
| Kotlin | `Runtime.getRuntime().exec(`, `ProcessBuilder(` |
| Ruby | `` `...` ``, `system(`, `exec(`, `spawn(`, `%x{`, `IO.popen(`, `Open3.` |
| Rust | `Command::new(`, `std::process::Command` |

Per-language safe patterns (skip detection):
- Python: `subprocess.run([...])` without `shell=True`
- JS: `execFile(`, `execFileSync(`, `spawn(` with array args
- Java: `ProcessBuilder` with list constructor (not string)
- Go: `exec.Command` always takes arg array (lower severity)
- Rust: `Command::new` always takes arg array (lower severity)

Steps:
- [ ] Create `multi_command_injection.rs` with `MultiCommandInjectionDetector` struct
- [ ] Implement per-language sink tables using `fn sinks_for(lang: Language) -> &'static [&'static str]`
- [ ] Implement per-language safe-pattern tables
- [ ] Port JS regex patterns from `js_command_injection.rs` under `Language::JavaScript` branch
- [ ] Port Python patterns from `command_injection.rs` under `Language::Python` branch
- [ ] Write tests: 1 positive + 1 negative per language (22 tests minimum)
- [ ] Run `cargo nextest run -p apex-detect --filter multi_command`, confirm all pass
- [ ] Commit

### Task 1.2 -- multi_sql_injection.rs

**Files:** `crates/apex-detect/src/detectors/multi_sql_injection.rs` (new)
**CWE:** 89

Per-language SQL injection patterns:

| Language | Patterns |
|----------|----------|
| Python | f-string SQL, `%`-format SQL, concatenation SQL (from `sql_injection.rs`) |
| JavaScript | Template literal SQL, string concat SQL (from `js_sql_injection.rs`) |
| Java | `Statement.execute(` + string concat, `createQuery(` + concat, `"SELECT..."+` |
| Go | `db.Query(fmt.Sprintf(`, `db.Exec("SELECT..."+`, `Queryx(` + concat |
| C/C++ | `sprintf(` + SQL keywords, `strcat(` + SQL keywords, `mysql_query(` + variable |
| C# | `SqlCommand(` + concat, `ExecuteReader(` + concat, `$"SELECT...{` |
| Swift | `sqlite3_exec(` + concat, string interpolation + SQL keywords |
| Kotlin | `createQuery(` + concat, `$"SELECT...${`, template string SQL |
| Ruby | `ActiveRecord` + string interpolation, `execute(` + concat, `"SELECT...#{` |
| Rust | `query(` + `format!(` + SQL keywords, `execute(` + `format!(` |

Steps:
- [ ] Create `multi_sql_injection.rs` with `MultiSqlInjectionDetector`
- [ ] Implement per-language concat/interpolation pattern detection
- [ ] Implement per-language safe patterns (parameterized queries, prepared statements)
- [ ] Port JS patterns from `js_sql_injection.rs`
- [ ] Port Python patterns from `sql_injection.rs`
- [ ] Write tests: 1 positive + 1 negative per language (22 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.3 -- multi_crypto_failure.rs

**Files:** `crates/apex-detect/src/detectors/multi_crypto_failure.rs` (new)
**CWE:** 327, 328, 330

Per-language crypto patterns (weak hashes, weak ciphers, insecure random):

| Language | Weak Hashes | Weak Ciphers | Insecure Random |
|----------|-------------|--------------|-----------------|
| Python | `hashlib.md5`, `hashlib.sha1` | `DES`, `RC4`, `Blowfish`, `ECB` | `random.random()` |
| JavaScript | `createHash('md5')`, `createHash('sha1')` | `createCipher('des')`, `'aes-128-ecb'` | `Math.random()` |
| Java | `MessageDigest.getInstance("MD5")`, `"SHA-1"` | `Cipher.getInstance("DES")`, `"/ECB/"` | `java.util.Random()`, `new Random()` |
| Go | `md5.New()`, `sha1.New()`, `crypto/md5`, `crypto/sha1` | `des.NewCipher`, `rc4.NewCipher` | `math/rand`, `rand.Intn(` |
| C/C++ | `MD5(`, `MD5_Init`, `SHA1(`, `SHA1_Init` | `DES_set_key`, `EVP_des_`, `EVP_rc4` | `rand()`, `srand(` |
| C# | `MD5.Create()`, `SHA1.Create()`, `new MD5CryptoServiceProvider` | `DES.Create()`, `RC2.Create()`, `CipherMode.ECB` | `new Random()` |
| Swift | `CC_MD5(`, `CC_SHA1(`, `Insecure.MD5`, `Insecure.SHA1` | `CCAlgorithm(kCCAlgorithmDES)` | `arc4random()` in security ctx |
| Kotlin | same as Java | same as Java | `java.util.Random()`, `kotlin.random.Random` |
| Ruby | `Digest::MD5`, `Digest::SHA1`, `OpenSSL::Digest::MD5` | `OpenSSL::Cipher::DES`, `'des-ecb'` | `rand()`, `Random.new` |
| Rust | (generally safe, flag `md-5` crate or `md5` crate usage) | (flag `des` crate usage) | `rand::thread_rng()` in security ctx |

Steps:
- [ ] Create `multi_crypto_failure.rs` with `MultiCryptoFailureDetector`
- [ ] Implement weak-hash detection with per-language patterns
- [ ] Implement weak-cipher detection with per-language patterns
- [ ] Implement insecure-random detection with security-context check
- [ ] Implement hardcoded key/IV detection (already in Python scanner, generalize)
- [ ] Port existing patterns from `crypto_failure.rs` and `js_crypto_failure.rs`
- [ ] Write tests: 1 weak-hash + 1 weak-cipher + 1 insecure-random per language (33 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.4 -- multi_insecure_deser.rs

**Files:** `crates/apex-detect/src/detectors/multi_insecure_deser.rs` (new)
**CWE:** 502

Per-language unsafe deserialization patterns:

| Language | Unsafe Patterns | Safe Alternatives |
|----------|----------------|-------------------|
| Python | `pickle.loads(`, `pickle.load(`, `marshal.loads(`, `yaml.load(` w/o SafeLoader | `yaml.safe_load(`, `json.loads(` |
| JavaScript | `node-serialize`, `eval(`, `Function(`, `unserialize(` | `JSON.parse(` |
| Java | `ObjectInputStream(`, `readObject(`, `XMLDecoder(`, `XStream.fromXML(` | `ObjectInputFilter`, whitelisting |
| Go | `gob.Decode(`, `encoding/gob` with untrusted input | `json.Unmarshal(` with typed struct |
| C/C++ | `unserialize(`, custom binary deserialization | typed parsing |
| C# | `BinaryFormatter.Deserialize(`, `SoapFormatter.Deserialize(`, `ObjectStateFormatter`, `LosFormatter`, `NetDataContractSerializer` | `JsonSerializer`, `XmlSerializer` with known types |
| Swift | `NSKeyedUnarchiver.unarchiveObject(` w/o `unarchivedObject(ofClass:` | `JSONDecoder()`, `Codable` |
| Kotlin | same as Java | same as Java |
| Ruby | `Marshal.load(`, `YAML.load(` w/o `safe_load`, `Oj.load(` w/o safe mode | `YAML.safe_load(`, `JSON.parse(` |
| Rust | (generally safe due to type system, flag `bincode::deserialize` from untrusted) | `serde_json::from_str(` with typed |

Steps:
- [ ] Create `multi_insecure_deser.rs` with `MultiInsecureDeserDetector`
- [ ] Implement per-language unsafe pattern tables
- [ ] Implement per-language safe-alternative skip logic
- [ ] Port patterns from `insecure_deserialization.rs` and `js_insecure_deser.rs`
- [ ] Write tests: 1 positive + 1 negative per language (22 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.5 -- multi_ssrf.rs

**Files:** `crates/apex-detect/src/detectors/multi_ssrf.rs` (new)
**CWE:** 918

Per-language HTTP request sinks and user-input indicators:

| Language | HTTP Sinks | User Input Indicators |
|----------|------------|----------------------|
| Python | `requests.get(`, `urllib.request.urlopen(`, `httpx.get(`, `aiohttp` | `request.args`, `request.form`, `sys.argv`, `os.environ` |
| JavaScript | `fetch(`, `axios(`, `http.get(`, `request(` | `req.body`, `req.query`, `req.params`, `process.argv` |
| Java | `HttpURLConnection`, `URL(`, `HttpClient.newHttpClient()`, `RestTemplate` | `request.getParameter(`, `@RequestParam`, `@PathVariable` |
| Go | `http.Get(`, `http.Post(`, `http.NewRequest(`, `client.Do(` | `r.URL.Query()`, `r.FormValue(`, `os.Args` |
| C/C++ | `curl_easy_setopt(`, `CURLOPT_URL`, `connect(`, `getaddrinfo(` | `argv`, `getenv(`, `fgets(` |
| C# | `HttpClient.GetAsync(`, `WebClient.DownloadString(`, `WebRequest.Create(` | `Request.Query[`, `Request.Form[`, `args[` |
| Swift | `URLSession.shared.dataTask(`, `URL(string:` | `request.url`, `UserDefaults` |
| Kotlin | same as Java | same as Java |
| Ruby | `Net::HTTP.get(`, `open-uri`, `HTTParty.get(`, `Faraday.get(` | `params[`, `request.env[`, `ARGV` |
| Rust | `reqwest::get(`, `reqwest::Client`, `hyper::Client` | `std::env::args()`, `env::var(` |

Steps:
- [ ] Create `multi_ssrf.rs` with `MultiSsrfDetector`
- [ ] Implement per-language HTTP sink tables
- [ ] Implement per-language user-input indicator tables
- [ ] Implement per-language sanitization indicator tables (URL validation, allowlists)
- [ ] Port patterns from `ssrf.rs` and `js_ssrf.rs`
- [ ] Write tests: 1 positive + 1 negative per language (22 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 1.6 -- multi_path_traversal.rs

**Files:** `crates/apex-detect/src/detectors/multi_path_traversal.rs` (new)
**CWE:** 22

Per-language file access patterns and sanitization:

| Language | File Access Sinks | Sanitization |
|----------|------------------|--------------|
| Python | `open(`, `Path(`, `os.path.join(` | `os.path.realpath`, `os.path.abspath`, `.resolve()` |
| JavaScript | `fs.readFile(`, `fs.createReadStream(`, `path.join(` | `path.resolve(`, `path.normalize(` |
| Java | `new File(`, `new FileInputStream(`, `Paths.get(`, `Files.readAllBytes(` | `getCanonicalPath()`, `toRealPath()`, `normalize()` |
| Go | `os.Open(`, `os.ReadFile(`, `ioutil.ReadFile(`, `filepath.Join(` | `filepath.Clean(`, `filepath.Abs(` |
| C/C++ | `fopen(`, `open(`, `ifstream(` | `realpath(`, `canonicalize_file_name(` |
| C# | `File.ReadAllText(`, `new FileStream(`, `File.Open(`, `Path.Combine(` | `Path.GetFullPath(`, `GetCanonicalPath` |
| Swift | `FileManager.default.contents(`, `Data(contentsOf:`, `String(contentsOfFile:` | `standardizedFileURL`, `resolvingSymlinksInPath` |
| Kotlin | same as Java | same as Java |
| Ruby | `File.read(`, `File.open(`, `IO.read(`, `File.join(` | `File.realpath(`, `File.expand_path(`, `Pathname.new(...).cleanpath` |
| Rust | `std::fs::read(`, `File::open(`, `std::fs::read_to_string(` | `.canonicalize()`, `fs::canonicalize(` |

Steps:
- [ ] Create `multi_path_traversal.rs` with `MultiPathTraversalDetector`
- [ ] Implement per-language file-access sink tables
- [ ] Implement per-language sanitization skip tables
- [ ] Implement safe-variable-prefix skip logic (generalized from Python scanner)
- [ ] Port patterns from `path_traversal.rs` and `js_path_traversal.rs`
- [ ] Write tests: 1 positive + 1 negative per language (22 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

## Wave 2 -- Wiring and Existing Detector Extension (depends on Wave 1)

### Task 2.1 -- Wire unified detectors into pipeline

**Files:** `crates/apex-detect/src/detectors/mod.rs`, `crates/apex-detect/src/pipeline.rs`

- [ ] Add `pub mod multi_command_injection;` etc. to `mod.rs` (6 new modules)
- [ ] Add `pub use` for all 6 new detector structs
- [ ] In `pipeline.rs`, add registration for each unified detector WITHOUT a language guard:
  ```rust
  if cfg.enabled.contains(&"command-injection".into()) {
      detectors.push(Box::new(MultiCommandInjectionDetector));
  }
  ```
- [ ] Keep old JS detector registrations but mark with `// DEPRECATED: replaced by multi_*`
- [ ] Run `cargo nextest run -p apex-detect`, confirm all tests pass (old + new)
- [ ] Run `cargo clippy --workspace -- -D warnings`, confirm clean
- [ ] Commit

### Task 2.2 -- Extend hardcoded_secret.rs for missing languages

**Files:** `crates/apex-detect/src/detectors/hardcoded_secret.rs`

The `HardcodedSecretDetector` already works across all languages (regex-based, language-agnostic patterns like AWS keys, GitHub tokens, Stripe keys). The gap is in language-specific assignment syntax recognition.

- [ ] Add language-specific assignment patterns to `ASSIGNMENT_RE` in `scan_hardcoded_secrets`:
  - Java/Kotlin: `String secret = "..."`, `val secret = "..."`
  - Go: `secret := "..."`, `var secret = "..."`
  - C/C++: `const char* secret = "..."`, `std::string secret = "..."`
  - C#: `string secret = "..."`, `const string secret = "..."`
  - Swift: `let secret = "..."`, `var secret = "..."`
- [ ] Add language-specific env-var markers to `ENV_VAR_MARKERS`:
  - Java: `System.getenv(`, Go: `os.Getenv(`, C#: `Environment.GetEnvironmentVariable(`
  - Swift: `ProcessInfo.processInfo.environment[`, Kotlin: `System.getenv(`
- [ ] Write tests for each newly covered language (7 tests minimum)
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.3 -- Extend secret_scan.rs for missing languages

**Files:** `crates/apex-detect/src/detectors/secret_scan.rs`

The `SecretScanDetector` is regex-based and mostly language-agnostic. Gaps are in language-specific comment syntax and string literal recognition.

- [ ] Verify `is_comment()` utility handles all 11 language comment styles
- [ ] Add Java/Kotlin `/** */` doc-comment handling if missing
- [ ] Add C# `///` doc-comment handling if missing
- [ ] Add Swift `///` doc-comment handling if missing
- [ ] Write tests verifying secret detection in Java, C, C++, C#, Swift, Kotlin files
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.4 -- Extend path_normalize.rs for missing languages

**Files:** `crates/apex-detect/src/detectors/path_normalize.rs`

Currently supports Python, JavaScript, Rust. Needs 8 more languages.

- [ ] Add function-keyword detection for Java, Go, C, C++, C#, Swift, Kotlin, Ruby
- [ ] Add normalization call patterns for each:
  - Java: `Paths.get(...).normalize()`, `new File(...).getCanonicalPath()`, `toRealPath()`
  - Go: `filepath.Clean(`, `filepath.Abs(`
  - C/C++: `realpath(`, `canonicalize_file_name(`
  - C#: `Path.GetFullPath(`, `Path.GetRelativePath(`
  - Swift: `standardizedFileURL`, `resolvingSymlinksInPath`, `standardized`
  - Kotlin: same as Java
  - Ruby: `File.realpath(`, `File.expand_path(`, `.cleanpath`
- [ ] Write tests: 1 positive (missing normalization) + 1 negative (has normalization) per language (16 tests)
- [ ] Run tests, confirm pass
- [ ] Commit

## Wave 3 -- Integration Verification (depends on Wave 2)

### Task 3.1 -- Full workspace build and test

- [ ] Run `cargo check --workspace`
- [ ] Run `cargo nextest run --workspace`
- [ ] Run `cargo clippy --workspace -- -D warnings`
- [ ] Fix any compilation or test failures
- [ ] Commit fixes if needed

### Task 3.2 -- Coverage matrix verification

- [ ] Write a test in `crates/apex-detect/src/detectors/mod.rs` that verifies every (detector, language) combination is covered:
  ```rust
  #[test]
  fn all_security_detectors_cover_all_languages() {
      // For each multi_* detector, verify it handles all 11 languages
  }
  ```
- [ ] Confirm the gap matrix is now 0 gaps
- [ ] Commit

## Gap Closure Summary

After completion, the matrix should look like:

```
Detector             Rust Pyth JS   Java  Go    C   C++  C#  Swift Kotlin Ruby
security-pattern       Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (already done)
hardcoded-secret       Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 2.2)
secret-scan            Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 2.3)
path-normalize         Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 2.4)
command-injection      Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.1)
sql-injection          Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.2)
crypto-failure         Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.3)
insecure-deser         Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.4)
ssrf                   Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.5)
path-traversal         Y   Y   Y    Y    Y    Y    Y   Y    Y    Y     Y   (Task 1.6)
```

75 gaps -> 0 gaps.

## Estimated Scope

- **Wave 1:** 6 new files, ~300-400 lines each = ~2100 lines of detection logic + ~1300 lines of tests
- **Wave 2:** 4 tasks modifying existing files, ~200-300 lines of additions + tests
- **Wave 3:** Verification only, minimal new code
- **Total:** ~3900 lines of new/modified code
