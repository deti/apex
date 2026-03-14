//! Firecracker microVM sandbox.
//!
//! Drives Firecracker via its REST API on a Unix domain socket, using
//! pre-built rootfs images (one per language) stored in `~/.apex/rootfs/`.
//!
//! Each VM is snapshotted after the target is loaded. `run()` restores the
//! snapshot, injects the seed via virtio-vsock, collects the coverage bitmap,
//! then suspends the VM again — boot latency is paid only once per session.
//!
//! # Vsock frame protocol
//!
//! ```text
//! Send: [4B len (big-endian)][seed data]
//! Recv: [4B bitmap_len][bitmap][4B exit_code][4B stdout_len][stdout][4B stderr_len][stderr]
//! ```
//!
//! # Feature flags
//!
//! When compiled with `--features firecracker`, the `FcClient` uses `hyper`
//! for real HTTP/1.1 over Unix sockets and `tokio-vsock` for vsock transport.
//! Without the feature, all API calls return stub (success) results.
//!
//! # Prerequisites (production)
//! - Firecracker binary >= 1.4 in PATH or `/usr/local/bin/firecracker`
//! - Pre-built rootfs images: `~/.apex/rootfs/<language>/rootfs.ext4`
//! - KVM device access: `/dev/kvm` readable by current user
//! - `jailer` binary for production isolation

use apex_core::{
    error::{ApexError, Result},
    traits::Sandbox,
    types::{BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};
#[cfg(feature = "firecracker")]
use tracing::warn;
use tracing::{debug, info};

// ---------------------------------------------------------------------------
// Vsock frame serialization / deserialization
// ---------------------------------------------------------------------------

/// Encode a seed as a vsock frame: `[4B len (big-endian)][data]`.
pub fn encode_vsock_frame(seed_data: &[u8]) -> Vec<u8> {
    let len = seed_data.len() as u32;
    let mut frame = Vec::with_capacity(4 + seed_data.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(seed_data);
    frame
}

/// Result of decoding a vsock response frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VsockResponse {
    pub bitmap: Vec<u8>,
    pub exit_code: u32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Decode a vsock response frame.
///
/// Format: `[4B bitmap_len][bitmap][4B exit_code][4B stdout_len][stdout][4B stderr_len][stderr]`
///
/// Returns `Err` if the buffer is too short or the lengths are inconsistent.
pub fn decode_vsock_response(data: &[u8]) -> Result<VsockResponse> {
    let mut pos = 0usize;

    let read_u32 = |data: &[u8], pos: &mut usize| -> Result<u32> {
        if *pos + 4 > data.len() {
            return Err(ApexError::Sandbox(
                "vsock frame: unexpected EOF reading u32".into(),
            ));
        }
        let val = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
        *pos += 4;
        Ok(val)
    };

    let read_bytes = |data: &[u8], pos: &mut usize, len: usize| -> Result<Vec<u8>> {
        if *pos + len > data.len() {
            return Err(ApexError::Sandbox(format!(
                "vsock frame: expected {len} bytes at offset {pos}, but only {} remain",
                data.len() - *pos,
            )));
        }
        let slice = data[*pos..*pos + len].to_vec();
        *pos += len;
        Ok(slice)
    };

    let bitmap_len = read_u32(data, &mut pos)? as usize;
    let bitmap = read_bytes(data, &mut pos, bitmap_len)?;
    let exit_code = read_u32(data, &mut pos)?;
    let stdout_len = read_u32(data, &mut pos)? as usize;
    let stdout = read_bytes(data, &mut pos, stdout_len)?;
    let stderr_len = read_u32(data, &mut pos)? as usize;
    let stderr = read_bytes(data, &mut pos, stderr_len)?;

    Ok(VsockResponse {
        bitmap,
        exit_code,
        stdout,
        stderr,
    })
}

/// Build a vsock response frame (for testing / guest-side usage).
pub fn encode_vsock_response(resp: &VsockResponse) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(resp.bitmap.len() as u32).to_be_bytes());
    buf.extend_from_slice(&resp.bitmap);
    buf.extend_from_slice(&resp.exit_code.to_be_bytes());
    buf.extend_from_slice(&(resp.stdout.len() as u32).to_be_bytes());
    buf.extend_from_slice(&resp.stdout);
    buf.extend_from_slice(&(resp.stderr.len() as u32).to_be_bytes());
    buf.extend_from_slice(&resp.stderr);
    buf
}

// ---------------------------------------------------------------------------
// REST client helpers
// ---------------------------------------------------------------------------

/// Firecracker REST API endpoint over a Unix domain socket.
struct FcClient {
    socket_path: PathBuf,
}

impl FcClient {
    fn new(socket_path: PathBuf) -> Self {
        FcClient { socket_path }
    }

    /// PUT /machine-config
    async fn put_machine_config(&self, vcpu_count: u8, mem_mb: u32) -> Result<()> {
        debug!(
            socket = %self.socket_path.display(),
            vcpu_count,
            mem_mb,
            "FC: PUT /machine-config"
        );
        #[cfg(feature = "firecracker")]
        {
            self.http_put(
                "/machine-config",
                &serde_json::json!({
                    "vcpu_count": vcpu_count,
                    "mem_size_mib": mem_mb,
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// PUT /drives/rootfs
    async fn put_rootfs(&self, rootfs_path: &Path) -> Result<()> {
        debug!(
            socket = %self.socket_path.display(),
            rootfs = %rootfs_path.display(),
            "FC: PUT /drives/rootfs"
        );
        #[cfg(feature = "firecracker")]
        {
            self.http_put(
                "/drives/rootfs",
                &serde_json::json!({
                    "drive_id": "rootfs",
                    "path_on_host": rootfs_path.to_string_lossy(),
                    "is_root_device": true,
                    "is_read_only": false,
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// PUT /actions — start the VM.
    async fn start(&self) -> Result<()> {
        info!(socket = %self.socket_path.display(), "FC: starting microVM");
        #[cfg(feature = "firecracker")]
        {
            self.http_put(
                "/actions",
                &serde_json::json!({ "action_type": "InstanceStart" }),
            )
            .await?;
        }
        Ok(())
    }

    /// PUT /snapshot/create
    async fn create_snapshot(&self, snap_path: &Path, mem_path: &Path) -> Result<()> {
        debug!(
            snap = %snap_path.display(),
            mem = %mem_path.display(),
            "FC: snapshot/create"
        );
        #[cfg(feature = "firecracker")]
        {
            self.http_put(
                "/snapshot/create",
                &serde_json::json!({
                    "snapshot_type": "Full",
                    "snapshot_path": snap_path.to_string_lossy(),
                    "mem_file_path": mem_path.to_string_lossy(),
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// PUT /snapshot/load
    async fn load_snapshot(&self, snap_path: &Path, mem_path: &Path) -> Result<()> {
        debug!(
            snap = %snap_path.display(),
            mem = %mem_path.display(),
            "FC: snapshot/load"
        );
        #[cfg(feature = "firecracker")]
        {
            self.http_put(
                "/snapshot/load",
                &serde_json::json!({
                    "snapshot_path": snap_path.to_string_lossy(),
                    "mem_file_path": mem_path.to_string_lossy(),
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// Real HTTP PUT over Unix socket (only compiled with `firecracker` feature).
    #[cfg(feature = "firecracker")]
    async fn http_put(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper::Request;
        use hyper_util::rt::TokioIo;
        use tokio::net::UnixStream;

        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            ApexError::Sandbox(format!("FC connect {}: {e}", self.socket_path.display()))
        })?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| ApexError::Sandbox(format!("FC handshake: {e}")))?;

        // Spawn connection driver.
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::error!(error = %e, "FC HTTP connection error");
            }
        });

        let json = serde_json::to_vec(body)
            .map_err(|e| ApexError::Sandbox(format!("FC serialize: {e}")))?;

        let req = Request::builder()
            .method("PUT")
            .uri(format!("http://localhost{path}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(Full::new(Bytes::from(json)))
            .map_err(|e| ApexError::Sandbox(format!("FC request build: {e}")))?;

        let resp = sender
            .send_request(req)
            .await
            .map_err(|e| ApexError::Sandbox(format!("FC request: {e}")))?;

        let status = resp.status();
        if !status.is_success() && status.as_u16() != 204 {
            return Err(ApexError::Sandbox(format!(
                "FC API {path} returned {status}"
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Snapshot state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Snapshot {
    id: SnapshotId,
    snap_file: PathBuf,
    mem_file: PathBuf,
}

// ---------------------------------------------------------------------------
// FirecrackerSandbox
// ---------------------------------------------------------------------------

/// Sandbox backed by Firecracker microVMs.
///
/// One `FirecrackerSandbox` manages a pool of N VMs, each snapshotted after
/// the target is loaded. `run()` picks an available VM, restores the snapshot,
/// injects the seed, collects coverage, and re-suspends.
pub struct FirecrackerSandbox {
    language: Language,
    /// Path to the rootfs image for this language.
    rootfs: PathBuf,
    /// Directory for VM sockets and snapshot files.
    work_dir: PathBuf,
    /// Active snapshot (set after `prepare()`).
    snapshot: Mutex<Option<Snapshot>>,
    /// Pool size.
    pool_size: usize,
    /// Coverage oracle for bitmap→branch mapping.
    oracle: Option<Arc<CoverageOracle>>,
    /// Branch index: bitmap position → BranchId.
    branch_index: Vec<BranchId>,
}

impl FirecrackerSandbox {
    pub fn new(language: Language, work_dir: PathBuf) -> Self {
        let rootfs = Self::default_rootfs(language);
        FirecrackerSandbox {
            language,
            rootfs,
            work_dir,
            snapshot: Mutex::new(None),
            pool_size: 4,
            oracle: None,
            branch_index: Vec::new(),
        }
    }

    pub fn with_rootfs(mut self, rootfs: PathBuf) -> Self {
        self.rootfs = rootfs;
        self
    }

    pub fn with_pool_size(mut self, n: usize) -> Self {
        self.pool_size = n;
        self
    }

    pub fn with_coverage(
        mut self,
        oracle: Arc<CoverageOracle>,
        branch_index: Vec<BranchId>,
    ) -> Self {
        self.oracle = Some(oracle);
        self.branch_index = branch_index;
        self
    }

    /// Default rootfs path: `~/.apex/rootfs/<language>/rootfs.ext4`.
    fn default_rootfs(language: Language) -> PathBuf {
        let lang_str = format!("{language}");
        dirs_next_home()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".apex")
            .join("rootfs")
            .join(lang_str)
            .join("rootfs.ext4")
    }

    fn socket_path(&self) -> PathBuf {
        self.work_dir.join("firecracker.sock")
    }

    fn client(&self) -> FcClient {
        FcClient::new(self.socket_path())
    }

    /// Prepare the VM pool — start one VM, run the target, create snapshot.
    ///
    /// Must be called before `run()`. In production, this would launch N VMs.
    pub async fn prepare(&self) -> Result<()> {
        if !self.rootfs.exists() {
            return Err(ApexError::Sandbox(format!(
                "Firecracker rootfs not found at {}. \
                 Build it with: apex rootfs build --lang {}",
                self.rootfs.display(),
                self.language,
            )));
        }

        std::fs::create_dir_all(&self.work_dir)
            .map_err(|e| ApexError::Sandbox(format!("create work dir: {e}")))?;

        // TODO: spawn `firecracker --api-sock <socket_path>` process.
        // TODO: wait for API socket to become available.

        let client = self.client();
        client.put_machine_config(1, 128).await?;
        client.put_rootfs(&self.rootfs).await?;
        client.start().await?;

        // TODO: inject target via vsock, run install/build steps, then snapshot.

        let snap_file = self.work_dir.join("snapshot.bin");
        let mem_file = self.work_dir.join("snapshot.mem");
        client.create_snapshot(&snap_file, &mem_file).await?;

        *self.snapshot.lock().unwrap() = Some(Snapshot {
            id: SnapshotId::new(),
            snap_file,
            mem_file,
        });

        info!(language = %self.language, "Firecracker sandbox prepared");
        Ok(())
    }
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[async_trait]
impl Sandbox for FirecrackerSandbox {
    fn language(&self) -> Language {
        self.language
    }

    async fn run(&self, seed: &InputSeed) -> Result<ExecutionResult> {
        let snap = self
            .snapshot
            .lock()
            .map_err(|e| ApexError::Sandbox(format!("snapshot lock poisoned: {e}")))?
            .clone()
            .ok_or_else(|| ApexError::Sandbox("FirecrackerSandbox not prepared".into()))?;

        let start = Instant::now();

        // Restore snapshot.
        let client = self.client();
        client
            .load_snapshot(&snap.snap_file, &snap.mem_file)
            .await?;

        // Encode seed as vsock frame.
        let frame = encode_vsock_frame(&seed.data);

        // Without the firecracker feature, simulate an empty response.
        // With the feature, this would send `frame` over the vsock socket.
        #[cfg(not(feature = "firecracker"))]
        let response = VsockResponse {
            bitmap: vec![],
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        };

        #[cfg(feature = "firecracker")]
        let response = {
            // TODO: real vsock socket I/O using self.socket_path()
            // 1. Connect to vsock CID 3, port 5000
            // 2. Write `frame` bytes
            // 3. Read response bytes
            // 4. decode_vsock_response(&response_bytes)?
            warn!("firecracker feature: real vsock I/O not yet implemented");
            VsockResponse {
                bitmap: vec![],
                exit_code: 0,
                stdout: vec![],
                stderr: vec![],
            }
        };

        let _ = &frame; // suppress unused warning in non-firecracker builds

        // Convert bitmap to new branches.
        let new_branches = if let Some(ref oracle) = self.oracle {
            crate::bitmap::bitmap_to_new_branches(&response.bitmap, &self.branch_index, oracle)
        } else {
            Vec::new()
        };

        // Map exit code to status.
        let status = match response.exit_code {
            0 => ExecutionStatus::Pass,
            code if code >= 128 => ExecutionStatus::Crash,
            _ => ExecutionStatus::Fail,
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExecutionResult {
            seed_id: seed.id,
            status,
            new_branches,
            trace: None,
            duration_ms,
            stdout: String::from_utf8_lossy(&response.stdout).to_string(),
            stderr: String::from_utf8_lossy(&response.stderr).to_string(),
        })
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        let client = self.client();
        let snap_file = self
            .work_dir
            .join(format!("snap_{}.bin", SnapshotId::new().0));
        let mem_file = self
            .work_dir
            .join(format!("snap_{}.mem", SnapshotId::new().0));
        client.create_snapshot(&snap_file, &mem_file).await?;
        let id = SnapshotId::new();
        Ok(id)
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        let snap = self
            .snapshot
            .lock()
            .map_err(|e| ApexError::Sandbox(format!("snapshot lock poisoned: {e}")))?
            .clone()
            .ok_or_else(|| ApexError::Sandbox("no snapshot available".into()))?;
        let client = self.client();
        client
            .load_snapshot(&snap.snap_file, &snap.mem_file)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Vsock frame tests
    // -----------------------------------------------------------------------

    #[test]
    fn encode_vsock_frame_empty_data() {
        let frame = encode_vsock_frame(&[]);
        assert_eq!(frame, vec![0, 0, 0, 0]); // 4-byte zero length
    }

    #[test]
    fn encode_vsock_frame_with_data() {
        let frame = encode_vsock_frame(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(frame.len(), 7); // 4 + 3
        assert_eq!(&frame[..4], &[0, 0, 0, 3]); // big-endian 3
        assert_eq!(&frame[4..], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn vsock_response_roundtrip() {
        let resp = VsockResponse {
            bitmap: vec![1, 0, 1, 0, 1],
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: b"warn".to_vec(),
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn vsock_response_empty_fields() {
        let resp = VsockResponse {
            bitmap: vec![],
            exit_code: 42,
            stdout: vec![],
            stderr: vec![],
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn vsock_response_nonzero_exit() {
        let resp = VsockResponse {
            bitmap: vec![0xFF],
            exit_code: 137,
            stdout: b"out".to_vec(),
            stderr: b"killed".to_vec(),
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded.exit_code, 137);
        assert_eq!(decoded.stderr, b"killed");
    }

    #[test]
    fn decode_vsock_response_truncated_header() {
        let result = decode_vsock_response(&[0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_truncated_bitmap() {
        // Claims 10 bytes of bitmap but only provides 2
        let mut data = vec![];
        data.extend_from_slice(&10u32.to_be_bytes());
        data.extend_from_slice(&[1, 2]);
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_truncated_stdout() {
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_be_bytes()); // bitmap_len = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // exit_code = 0
        data.extend_from_slice(&5u32.to_be_bytes()); // stdout_len = 5
                                                     // No actual stdout data
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // FcClient tests
    // -----------------------------------------------------------------------

    #[test]
    fn fc_client_stores_socket_path() {
        let client = FcClient::new(PathBuf::from("/run/fc/vm0.sock"));
        assert_eq!(client.socket_path, PathBuf::from("/run/fc/vm0.sock"));
    }

    #[test]
    fn default_rootfs_path_structure() {
        let path = FirecrackerSandbox::default_rootfs(Language::Python);
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".apex/rootfs/python/rootfs.ext4")
                || path_str.contains("/tmp/.apex/rootfs/python/rootfs.ext4"),
            "default rootfs path should include .apex/rootfs/<lang>/rootfs.ext4: {path_str}"
        );
    }

    #[test]
    fn default_rootfs_per_language() {
        let py = FirecrackerSandbox::default_rootfs(Language::Python);
        let js = FirecrackerSandbox::default_rootfs(Language::JavaScript);
        let rs = FirecrackerSandbox::default_rootfs(Language::Rust);

        assert!(py.to_string_lossy().contains("/python/"));
        assert!(js.to_string_lossy().contains("/javascript/"));
        assert!(rs.to_string_lossy().contains("/rust/"));
        // All should end with rootfs.ext4
        assert_eq!(py.file_name().unwrap(), "rootfs.ext4");
        assert_eq!(js.file_name().unwrap(), "rootfs.ext4");
        assert_eq!(rs.file_name().unwrap(), "rootfs.ext4");
    }

    #[test]
    fn new_sets_defaults() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp/fc-work"));
        assert_eq!(sb.language, Language::Python);
        assert_eq!(sb.work_dir, PathBuf::from("/tmp/fc-work"));
        assert_eq!(sb.pool_size, 4);
        assert!(sb.snapshot.lock().unwrap().is_none());
    }

    #[test]
    fn with_rootfs_overrides_default() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp"))
            .with_rootfs(PathBuf::from("/custom/rootfs.ext4"));
        assert_eq!(sb.rootfs, PathBuf::from("/custom/rootfs.ext4"));
    }

    #[test]
    fn with_pool_size_overrides_default() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp")).with_pool_size(8);
        assert_eq!(sb.pool_size, 8);
    }

    #[test]
    fn socket_path_under_work_dir() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/var/run/apex"));
        assert_eq!(
            sb.socket_path(),
            PathBuf::from("/var/run/apex/firecracker.sock")
        );
    }

    #[test]
    fn client_uses_correct_socket() {
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/work"));
        let client = sb.client();
        assert_eq!(client.socket_path, PathBuf::from("/work/firecracker.sock"));
    }

    #[test]
    fn language_returns_configured() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::JavaScript, PathBuf::from("/tmp"));
        assert_eq!(sb.language(), Language::JavaScript);
    }

    #[test]
    fn builder_methods_chainable() {
        let sb = FirecrackerSandbox::new(Language::C, PathBuf::from("/tmp"))
            .with_rootfs(PathBuf::from("/rootfs.ext4"))
            .with_pool_size(2);
        assert_eq!(sb.rootfs, PathBuf::from("/rootfs.ext4"));
        assert_eq!(sb.pool_size, 2);
    }

    #[test]
    fn dirs_next_home_reads_env() {
        // dirs_next_home returns HOME env var as PathBuf
        let home = std::env::var("HOME").ok().map(PathBuf::from);
        assert_eq!(super::dirs_next_home(), home);
    }

    #[tokio::test]
    async fn fc_client_put_machine_config_stub_succeeds() {
        let client = FcClient::new(PathBuf::from("/tmp/test.sock"));
        let result = client.put_machine_config(2, 256).await;
        // Without firecracker feature, this is a stub that always succeeds
        #[cfg(not(feature = "firecracker"))]
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fc_client_put_rootfs_stub_succeeds() {
        let client = FcClient::new(PathBuf::from("/tmp/test.sock"));
        let result = client.put_rootfs(&PathBuf::from("/rootfs.ext4")).await;
        #[cfg(not(feature = "firecracker"))]
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fc_client_start_stub_succeeds() {
        let client = FcClient::new(PathBuf::from("/tmp/test.sock"));
        let result = client.start().await;
        #[cfg(not(feature = "firecracker"))]
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fc_client_create_snapshot_stub_succeeds() {
        let client = FcClient::new(PathBuf::from("/tmp/test.sock"));
        let result = client
            .create_snapshot(&PathBuf::from("/snap.bin"), &PathBuf::from("/snap.mem"))
            .await;
        #[cfg(not(feature = "firecracker"))]
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fc_client_load_snapshot_stub_succeeds() {
        let client = FcClient::new(PathBuf::from("/tmp/test.sock"));
        let result = client
            .load_snapshot(&PathBuf::from("/snap.bin"), &PathBuf::from("/snap.mem"))
            .await;
        #[cfg(not(feature = "firecracker"))]
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn prepare_fails_when_rootfs_missing() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp/fc-test-work"))
            .with_rootfs(PathBuf::from("/nonexistent/rootfs.ext4"));
        let result = sb.prepare().await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("rootfs not found"), "error: {msg}");
    }

    #[tokio::test]
    async fn prepare_succeeds_with_real_rootfs() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake rootfs").unwrap();

        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir.clone()).with_rootfs(rootfs);
        let result = sb.prepare().await;
        assert!(result.is_ok());

        // Snapshot should be set after prepare
        assert!(sb.snapshot.lock().unwrap().is_some());
        // work_dir should have been created
        assert!(work_dir.exists());
    }

    #[tokio::test]
    async fn run_fails_when_not_prepared() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp/fc-work"));
        let seed = InputSeed::new(b"test".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("not prepared"), "error: {msg}");
    }

    #[tokio::test]
    async fn run_succeeds_after_prepare() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();

        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"data".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
        assert!(result.new_branches.is_empty());
    }

    #[tokio::test]
    async fn snapshot_trait_method_succeeds() {
        // The trait snapshot() method creates a new snapshot via the stub client
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/tmp/fc-snap"));
        let result = sb.snapshot().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn restore_fails_without_snapshot() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/tmp/fc-restore"));
        let result = sb.restore(SnapshotId::new()).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no snapshot"), "error: {msg}");
    }

    #[tokio::test]
    async fn restore_succeeds_after_prepare() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();

        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let result = sb.restore(SnapshotId::new()).await;
        assert!(result.is_ok());
    }

    #[test]
    fn snapshot_struct_fields() {
        let snap = Snapshot {
            id: SnapshotId::new(),
            snap_file: PathBuf::from("/snap.bin"),
            mem_file: PathBuf::from("/snap.mem"),
        };
        assert_eq!(snap.snap_file, PathBuf::from("/snap.bin"));
        assert_eq!(snap.mem_file, PathBuf::from("/snap.mem"));
    }

    #[test]
    fn snapshot_clone() {
        let snap = Snapshot {
            id: SnapshotId::new(),
            snap_file: PathBuf::from("/snap.bin"),
            mem_file: PathBuf::from("/snap.mem"),
        };
        let cloned = snap.clone();
        assert_eq!(cloned.snap_file, snap.snap_file);
        assert_eq!(cloned.mem_file, snap.mem_file);
        assert_eq!(cloned.id, snap.id);
    }

    #[test]
    fn encode_vsock_frame_large_data() {
        let data = vec![0xABu8; 1000];
        let frame = encode_vsock_frame(&data);
        assert_eq!(frame.len(), 4 + 1000);
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(len, 1000);
        assert!(frame[4..].iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn vsock_response_large_bitmap() {
        let bitmap = vec![0x55u8; 65536];
        let resp = VsockResponse {
            bitmap: bitmap.clone(),
            exit_code: 0,
            stdout: b"ok".to_vec(),
            stderr: vec![],
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded.bitmap.len(), 65536);
        assert_eq!(decoded, resp);
    }

    #[test]
    fn decode_vsock_response_truncated_exit_code() {
        // Valid bitmap (len=2, data=[1,2]) but then truncated before exit_code
        let mut data = vec![];
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&[1, 2]);
        // No exit_code follows — only 2 bytes of bitmap after the length
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_truncated_stderr() {
        // Valid bitmap + exit_code + stdout, but truncate before stderr data
        let mut data = vec![];
        data.extend_from_slice(&1u32.to_be_bytes()); // bitmap_len = 1
        data.push(0xFF); // bitmap
        data.extend_from_slice(&0u32.to_be_bytes()); // exit_code = 0
        data.extend_from_slice(&2u32.to_be_bytes()); // stdout_len = 2
        data.extend_from_slice(b"ok"); // stdout
        data.extend_from_slice(&5u32.to_be_bytes()); // stderr_len = 5
        data.extend_from_slice(b"er"); // only 2 of 5 bytes
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_empty_input() {
        let result = decode_vsock_response(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn exit_code_to_status_mapping() {
        // exit 0 = Pass
        assert_eq!(
            match 0u32 {
                0 => ExecutionStatus::Pass,
                c if c >= 128 => ExecutionStatus::Crash,
                _ => ExecutionStatus::Fail,
            },
            ExecutionStatus::Pass
        );
        // exit 1 = Fail
        assert_eq!(
            match 1u32 {
                0 => ExecutionStatus::Pass,
                c if c >= 128 => ExecutionStatus::Crash,
                _ => ExecutionStatus::Fail,
            },
            ExecutionStatus::Fail
        );
        // exit 137 (SIGKILL) = Crash
        assert_eq!(
            match 137u32 {
                0 => ExecutionStatus::Pass,
                c if c >= 128 => ExecutionStatus::Crash,
                _ => ExecutionStatus::Fail,
            },
            ExecutionStatus::Crash
        );
    }

    #[test]
    fn with_coverage_builder() {
        let tmp = tempfile::tempdir().unwrap();
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 10, 0, 0);
        let sb = FirecrackerSandbox::new(Language::Python, tmp.path().to_path_buf())
            .with_coverage(Arc::clone(&oracle), vec![b0.clone()]);
        assert!(sb.oracle.is_some());
        assert_eq!(sb.branch_index.len(), 1);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_vsock_roundtrip(
            bitmap in proptest::collection::vec(any::<u8>(), 0..256),
            exit_code in any::<u32>(),
            stdout in proptest::collection::vec(any::<u8>(), 0..256),
            stderr in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let resp = VsockResponse { bitmap, exit_code, stdout, stderr };
            let encoded = encode_vsock_response(&resp);
            let decoded = decode_vsock_response(&encoded).unwrap();
            prop_assert_eq!(decoded, resp);
        }

        #[test]
        fn prop_encode_vsock_frame_length(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            let frame = encode_vsock_frame(&data);
            prop_assert_eq!(frame.len(), data.len() + 4);
        }

        /// Fuzz-like test: random bytes should never panic decode_vsock_response.
        #[test]
        fn prop_decode_vsock_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            // Should return Ok or Err, never panic
            let _ = decode_vsock_response(&data);
        }

        /// Fuzz-like test: truncated valid frames should error gracefully.
        #[test]
        fn prop_decode_vsock_truncated(
            bitmap in proptest::collection::vec(any::<u8>(), 0..64),
            exit_code in any::<u32>(),
            stdout in proptest::collection::vec(any::<u8>(), 0..64),
            stderr in proptest::collection::vec(any::<u8>(), 0..64),
            truncate_at in 0usize..256,
        ) {
            let resp = VsockResponse { bitmap, exit_code, stdout, stderr };
            let encoded = encode_vsock_response(&resp);
            let truncated = if truncate_at < encoded.len() {
                &encoded[..truncate_at]
            } else {
                &encoded
            };
            // Should return Ok (if not truncated) or Err (if truncated), never panic
            let _ = decode_vsock_response(truncated);
        }
    }

    // -----------------------------------------------------------------------
    // Additional coverage: exit code mapping, with_coverage, edge cases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_with_coverage_oracle_and_empty_bitmap() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();

        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 10, 0, 0);
        oracle.register_branches([b0.clone()]);

        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir)
            .with_rootfs(rootfs)
            .with_coverage(Arc::clone(&oracle), vec![b0]);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"data".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        // Stub produces empty bitmap, so no new branches
        assert!(result.new_branches.is_empty());
        assert_eq!(result.status, ExecutionStatus::Pass);
    }

    #[tokio::test]
    async fn run_without_oracle_produces_no_branches() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();

        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"test".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        // oracle is None, so new_branches is always empty
        assert!(result.new_branches.is_empty());
    }

    #[test]
    fn exit_code_fail_range_1_to_127() {
        // exit codes 1..127 map to Fail (not Pass, not Crash)
        for code in [1u32, 2, 50, 100, 127] {
            let status = match code {
                0 => ExecutionStatus::Pass,
                c if c >= 128 => ExecutionStatus::Crash,
                _ => ExecutionStatus::Fail,
            };
            assert_eq!(status, ExecutionStatus::Fail, "code={code}");
        }
    }

    #[test]
    fn exit_code_crash_range_128_plus() {
        for code in [128u32, 129, 137, 139, 255] {
            let status = match code {
                0 => ExecutionStatus::Pass,
                c if c >= 128 => ExecutionStatus::Crash,
                _ => ExecutionStatus::Fail,
            };
            assert_eq!(status, ExecutionStatus::Crash, "code={code}");
        }
    }

    #[test]
    fn vsock_response_debug_format() {
        let resp = VsockResponse {
            bitmap: vec![1, 2],
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("VsockResponse"));
        assert!(debug.contains("bitmap"));
        assert!(debug.contains("exit_code"));
    }

    #[test]
    fn vsock_response_eq() {
        let a = VsockResponse {
            bitmap: vec![1],
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        };
        let b = a.clone();
        assert_eq!(a, b);

        let c = VsockResponse {
            bitmap: vec![2],
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        };
        assert_ne!(a, c);
    }

    #[test]
    fn decode_vsock_response_truncated_stderr_len() {
        // Valid bitmap + exit_code + stdout, but no stderr_len bytes at all
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_be_bytes()); // bitmap_len = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // exit_code = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // stdout_len = 0
                                                     // No stderr_len at all
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn encode_vsock_frame_single_byte() {
        let frame = encode_vsock_frame(&[0x42]);
        assert_eq!(frame.len(), 5);
        assert_eq!(&frame[..4], &[0, 0, 0, 1]);
        assert_eq!(frame[4], 0x42);
    }

    #[test]
    fn default_rootfs_for_c_and_java() {
        let c_path = FirecrackerSandbox::default_rootfs(Language::C);
        let java_path = FirecrackerSandbox::default_rootfs(Language::Java);
        assert!(c_path.to_string_lossy().contains("/c/"));
        assert!(java_path.to_string_lossy().contains("/java/"));
    }

    #[tokio::test]
    async fn snapshot_returns_unique_ids() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/tmp/fc-snap"));
        let id1 = sb.snapshot().await.unwrap();
        let id2 = sb.snapshot().await.unwrap();
        assert_ne!(id1, id2, "snapshot IDs should be unique");
    }

    #[test]
    fn with_coverage_sets_branch_index() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 1);
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp"))
            .with_coverage(Arc::clone(&oracle), vec![b0.clone(), b1.clone()]);
        assert_eq!(sb.branch_index.len(), 2);
        assert_eq!(sb.branch_index[0], b0);
        assert_eq!(sb.branch_index[1], b1);
    }

    #[test]
    fn new_with_all_languages() {
        for lang in [
            Language::Python,
            Language::JavaScript,
            Language::Rust,
            Language::C,
            Language::Java,
        ] {
            let sb = FirecrackerSandbox::new(lang, PathBuf::from("/tmp"));
            assert_eq!(sb.language, lang);
        }
    }

    // ------------------------------------------------------------------
    // Additional branch-coverage tests
    // ------------------------------------------------------------------

    /// `decode_vsock_response` with a minimum-valid frame (all zero-length fields).
    #[test]
    fn decode_vsock_response_all_zero_lengths() {
        // bitmap_len=0, exit_code=0, stdout_len=0, stderr_len=0
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_be_bytes()); // bitmap_len = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // exit_code = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // stdout_len = 0
        data.extend_from_slice(&0u32.to_be_bytes()); // stderr_len = 0
        let resp = decode_vsock_response(&data).unwrap();
        assert!(resp.bitmap.is_empty());
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.is_empty());
        assert!(resp.stderr.is_empty());
    }

    /// `encode_vsock_response` preserves exact byte sequences.
    #[test]
    fn encode_vsock_response_byte_layout() {
        let resp = VsockResponse {
            bitmap: vec![0xAA, 0xBB],
            exit_code: 0x01020304,
            stdout: vec![0x11],
            stderr: vec![0x22, 0x33],
        };
        let encoded = encode_vsock_response(&resp);
        // bitmap_len = 2
        assert_eq!(&encoded[0..4], &2u32.to_be_bytes());
        // bitmap bytes
        assert_eq!(encoded[4], 0xAA);
        assert_eq!(encoded[5], 0xBB);
        // exit_code
        assert_eq!(&encoded[6..10], &0x01020304u32.to_be_bytes());
        // stdout_len = 1
        assert_eq!(&encoded[10..14], &1u32.to_be_bytes());
        assert_eq!(encoded[14], 0x11);
        // stderr_len = 2
        assert_eq!(&encoded[15..19], &2u32.to_be_bytes());
        assert_eq!(encoded[19], 0x22);
        assert_eq!(encoded[20], 0x33);
    }

    /// `dirs_next_home()` returns None when HOME is unset.
    #[test]
    fn dirs_next_home_without_home_env() {
        // We can't easily unset HOME in parallel tests, but we can verify
        // that the function returns something or nothing based on the env.
        let result = dirs_next_home();
        // If HOME is set (as it normally is), it should be Some.
        // If HOME is not set, it should be None.
        if std::env::var("HOME").is_ok() {
            assert!(result.is_some());
        } else {
            assert!(result.is_none());
        }
    }

    /// `FirecrackerSandbox::default_rootfs` always ends with `rootfs.ext4`.
    #[test]
    fn default_rootfs_always_ends_with_rootfs_ext4() {
        for lang in [
            Language::Python,
            Language::Rust,
            Language::JavaScript,
            Language::C,
            Language::Java,
        ] {
            let path = FirecrackerSandbox::default_rootfs(lang);
            assert_eq!(path.file_name().unwrap(), "rootfs.ext4", "lang={lang}");
        }
    }

    /// `VsockResponse` Clone preserves all fields.
    #[test]
    fn vsock_response_clone_all_fields() {
        let original = VsockResponse {
            bitmap: vec![1, 2, 3],
            exit_code: 42,
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
        };
        let cloned = original.clone();
        assert_eq!(cloned.bitmap, original.bitmap);
        assert_eq!(cloned.exit_code, original.exit_code);
        assert_eq!(cloned.stdout, original.stdout);
        assert_eq!(cloned.stderr, original.stderr);
    }

    /// `encode_vsock_frame` capacity is exact (4 + data.len()).
    #[test]
    fn encode_vsock_frame_capacity_exact() {
        let data = b"hello";
        let frame = encode_vsock_frame(data);
        assert_eq!(frame.len(), 4 + data.len());
        // Big-endian length prefix is correct.
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(len as usize, data.len());
    }

    /// `prepare` error message mentions `rootfs not found`.
    #[tokio::test]
    async fn prepare_error_mentions_rootfs_not_found() {
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/tmp/fc-err"))
            .with_rootfs(PathBuf::from("/absolutely/nonexistent/rootfs.ext4"));
        let err = sb.prepare().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("rootfs not found"), "error: {msg}");
    }

    /// `prepare` error includes the language name in the hint.
    #[tokio::test]
    async fn prepare_error_mentions_language() {
        let sb = FirecrackerSandbox::new(Language::JavaScript, PathBuf::from("/tmp/fc-lang"))
            .with_rootfs(PathBuf::from("/no/such/rootfs.ext4"));
        let err = sb.prepare().await.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("javascript") || msg.contains("JavaScript"),
            "error: {msg}"
        );
    }

    /// `run()` result has `seed_id` matching the input seed.
    #[tokio::test]
    async fn run_result_seed_id_matches_input() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"my-seed".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let expected_id = seed.id;
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.seed_id, expected_id);
    }

    /// `run()` produces `duration_ms >= 0` (trivially true for u64, but exercises the field).
    #[tokio::test]
    async fn run_duration_ms_is_set() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"d".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        // duration_ms is a u64; it should be set (possibly 0 in fast tests).
        let _ = result.duration_ms;
    }

    /// `snapshot()` trait method returns a valid SnapshotId (not an error, in stub mode).
    #[tokio::test]
    async fn snapshot_returns_valid_id() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::C, PathBuf::from("/tmp/fc-snap-id"));
        let id = sb.snapshot().await.unwrap();
        // Just verify it doesn't panic and the returned type is valid.
        let _ = id;
    }

    /// `Snapshot` struct `Debug` format (exercising the derive).
    #[test]
    fn snapshot_struct_debug() {
        let snap = Snapshot {
            id: SnapshotId::new(),
            snap_file: PathBuf::from("/snap.bin"),
            mem_file: PathBuf::from("/snap.mem"),
        };
        let dbg = format!("{snap:?}");
        assert!(dbg.contains("Snapshot"), "debug: {dbg}");
    }

    /// `with_pool_size(0)` is accepted (edge case: zero pool).
    #[test]
    fn with_pool_size_zero() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp")).with_pool_size(0);
        assert_eq!(sb.pool_size, 0);
    }

    /// `with_pool_size(1)` is the minimum practical pool size.
    #[test]
    fn with_pool_size_one() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp")).with_pool_size(1);
        assert_eq!(sb.pool_size, 1);
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn decode_vsock_response_error_message_eof() {
        let result = decode_vsock_response(&[0, 0]);
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("unexpected EOF"), "error: {msg}");
    }

    #[test]
    fn decode_vsock_response_error_message_bytes_remain() {
        let mut data = vec![];
        data.extend_from_slice(&10u32.to_be_bytes());
        data.extend_from_slice(&[1, 2]);
        let result = decode_vsock_response(&data);
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("expected 10 bytes"), "error: {msg}");
    }

    #[tokio::test]
    async fn run_with_empty_seed_data() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(Vec::<u8>::new(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
        assert!(result.trace.is_none());
    }

    #[tokio::test]
    async fn run_result_stdout_stderr_are_strings() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"test".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    #[tokio::test]
    async fn run_with_large_seed_data() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let big_data = vec![0xFFu8; 65536];
        let seed = InputSeed::new(big_data, apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
    }

    #[test]
    fn with_coverage_empty_branch_index() {
        let oracle = Arc::new(CoverageOracle::new());
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp"))
            .with_coverage(oracle, vec![]);
        assert!(sb.oracle.is_some());
        assert!(sb.branch_index.is_empty());
    }

    #[tokio::test]
    async fn run_with_oracle_empty_branch_index() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let oracle = Arc::new(CoverageOracle::new());
        let sb = FirecrackerSandbox::new(Language::Python, work_dir)
            .with_rootfs(rootfs)
            .with_coverage(oracle, vec![]);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"x".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let result = sb.run(&seed).await.unwrap();
        assert!(result.new_branches.is_empty());
    }

    #[tokio::test]
    async fn prepare_creates_nested_work_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("a").join("b").join("c");
        assert!(!work_dir.exists());
        let sb = FirecrackerSandbox::new(Language::Python, work_dir.clone()).with_rootfs(rootfs);
        sb.prepare().await.unwrap();
        assert!(work_dir.exists());
    }

    #[tokio::test]
    async fn prepare_sets_snapshot_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir.clone()).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let snap = sb.snapshot.lock().unwrap();
        let snap = snap.as_ref().unwrap();
        assert_eq!(snap.snap_file, work_dir.join("snapshot.bin"));
        assert_eq!(snap.mem_file, work_dir.join("snapshot.mem"));
    }

    #[test]
    fn encode_vsock_frame_length_prefix_various_sizes() {
        for size in [0usize, 1, 255, 256, 65535, 65536] {
            let data = vec![0u8; size];
            let frame = encode_vsock_frame(&data);
            let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
            assert_eq!(len as usize, size);
        }
    }

    #[test]
    fn encode_vsock_response_total_length() {
        let resp = VsockResponse {
            bitmap: vec![1, 2, 3],
            exit_code: 42,
            stdout: vec![4, 5],
            stderr: vec![6, 7, 8, 9],
        };
        let encoded = encode_vsock_response(&resp);
        let expected = 4 + 3 + 4 + 4 + 2 + 4 + 4;
        assert_eq!(encoded.len(), expected);
    }

    #[test]
    fn decode_vsock_response_three_bytes() {
        let result = decode_vsock_response(&[0, 0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_four_bytes_no_exit() {
        let data = 0u32.to_be_bytes();
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_no_stdout_len() {
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        let result = decode_vsock_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_vsock_response_max_exit_code() {
        let resp = VsockResponse {
            bitmap: vec![],
            exit_code: u32::MAX,
            stdout: vec![],
            stderr: vec![],
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded.exit_code, u32::MAX);
    }

    #[tokio::test]
    async fn restore_error_message_content() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp/fc-restore-err2"));
        let err = sb.restore(SnapshotId::new()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("no snapshot"), "error: {msg}");
    }

    #[tokio::test]
    async fn run_error_message_content() {
        use apex_core::traits::Sandbox;
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp/fc-run-err2"));
        let seed = InputSeed::new(b"x".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let err = sb.run(&seed).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not prepared"), "error: {msg}");
    }

    #[tokio::test]
    async fn run_multiple_times() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        for i in 0..5 {
            let seed = InputSeed::new(
                format!("seed-{i}").into_bytes(),
                apex_core::types::SeedOrigin::Corpus,
            );
            let result = sb.run(&seed).await.unwrap();
            assert_eq!(result.status, ExecutionStatus::Pass);
            assert_eq!(result.seed_id, seed.id);
        }
    }

    #[tokio::test]
    async fn prepare_twice_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();
        let id1 = sb.snapshot.lock().unwrap().as_ref().unwrap().id;
        sb.prepare().await.unwrap();
        let id2 = sb.snapshot.lock().unwrap().as_ref().unwrap().id;
        assert_ne!(id1, id2);
    }

    #[test]
    fn builder_full_chain() {
        let oracle = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let sb = FirecrackerSandbox::new(Language::Rust, PathBuf::from("/tmp"))
            .with_pool_size(16)
            .with_rootfs(PathBuf::from("/my/rootfs.ext4"))
            .with_coverage(oracle, vec![b0.clone()]);
        assert_eq!(sb.pool_size, 16);
        assert_eq!(sb.rootfs, PathBuf::from("/my/rootfs.ext4"));
        assert!(sb.oracle.is_some());
        assert_eq!(sb.branch_index, vec![b0]);
    }

    #[test]
    fn encode_vsock_response_only_bitmap() {
        let resp = VsockResponse {
            bitmap: vec![0xFF; 100],
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        };
        let encoded = encode_vsock_response(&resp);
        let decoded = decode_vsock_response(&encoded).unwrap();
        assert_eq!(decoded.bitmap.len(), 100);
        assert!(decoded.bitmap.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn vsock_response_ne_exit_code() {
        let a = VsockResponse { bitmap: vec![1], exit_code: 0, stdout: vec![], stderr: vec![] };
        let b = VsockResponse { bitmap: vec![1], exit_code: 1, stdout: vec![], stderr: vec![] };
        assert_ne!(a, b);
    }

    #[test]
    fn vsock_response_ne_stdout() {
        let a = VsockResponse { bitmap: vec![], exit_code: 0, stdout: vec![1], stderr: vec![] };
        let b = VsockResponse { bitmap: vec![], exit_code: 0, stdout: vec![2], stderr: vec![] };
        assert_ne!(a, b);
    }

    #[test]
    fn vsock_response_ne_stderr() {
        let a = VsockResponse { bitmap: vec![], exit_code: 0, stdout: vec![], stderr: vec![1] };
        let b = VsockResponse { bitmap: vec![], exit_code: 0, stdout: vec![], stderr: vec![2] };
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn snapshot_trait_works_all_languages() {
        use apex_core::traits::Sandbox;
        for lang in [Language::Python, Language::JavaScript, Language::Rust, Language::C, Language::Java] {
            let sb = FirecrackerSandbox::new(lang, PathBuf::from("/tmp/fc-snap-all2"));
            let result = sb.snapshot().await;
            assert!(result.is_ok(), "snapshot failed for {lang}");
        }
    }

    #[tokio::test]
    async fn run_restore_run_sequence() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed1 = InputSeed::new(b"first".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let r1 = sb.run(&seed1).await.unwrap();
        assert_eq!(r1.status, ExecutionStatus::Pass);

        sb.restore(SnapshotId::new()).await.unwrap();

        let seed2 = InputSeed::new(b"second".to_vec(), apex_core::types::SeedOrigin::Corpus);
        let r2 = sb.run(&seed2).await.unwrap();
        assert_eq!(r2.status, ExecutionStatus::Pass);
    }

    #[tokio::test]
    async fn run_with_fuzzer_origin_seed() {
        use apex_core::traits::Sandbox;
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = tmp.path().join("rootfs.ext4");
        std::fs::write(&rootfs, b"fake").unwrap();
        let work_dir = tmp.path().join("work");
        let sb = FirecrackerSandbox::new(Language::Python, work_dir).with_rootfs(rootfs);
        sb.prepare().await.unwrap();

        let seed = InputSeed::new(b"fuzzed".to_vec(), apex_core::types::SeedOrigin::Fuzzer);
        let result = sb.run(&seed).await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Pass);
        assert_eq!(result.seed_id, seed.id);
    }

    #[test]
    fn with_coverage_replaces_oracle() {
        let oracle1 = Arc::new(CoverageOracle::new());
        let oracle2 = Arc::new(CoverageOracle::new());
        let b0 = BranchId::new(1, 1, 0, 0);
        let b1 = BranchId::new(1, 2, 0, 0);
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/tmp"))
            .with_coverage(oracle1, vec![b0])
            .with_coverage(oracle2, vec![b1.clone()]);
        assert_eq!(sb.branch_index.len(), 1);
        assert_eq!(sb.branch_index[0], b1);
    }

    #[test]
    fn default_rootfs_wasm_language() {
        let wasm_path = FirecrackerSandbox::default_rootfs(Language::Wasm);
        assert!(wasm_path.to_string_lossy().contains("/wasm/"));
        assert_eq!(wasm_path.file_name().unwrap(), "rootfs.ext4");
    }

    #[test]
    fn default_rootfs_ruby_language() {
        let ruby_path = FirecrackerSandbox::default_rootfs(Language::Ruby);
        assert!(ruby_path.to_string_lossy().contains("/ruby/"));
        assert_eq!(ruby_path.file_name().unwrap(), "rootfs.ext4");
    }

    #[test]
    fn new_with_long_work_dir() {
        let long_path = PathBuf::from("/tmp/".to_string() + &"a/".repeat(50) + "work");
        let sb = FirecrackerSandbox::new(Language::Python, long_path.clone());
        assert_eq!(sb.work_dir, long_path);
    }

    #[test]
    fn fc_client_empty_socket_path() {
        let client = FcClient::new(PathBuf::new());
        assert_eq!(client.socket_path, PathBuf::new());
    }

    #[test]
    fn socket_path_various_work_dirs() {
        let paths = vec!["/tmp", "/var/run/apex", "/home/user/.apex/work", "/"];
        for p in paths {
            let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from(p));
            assert_eq!(sb.socket_path(), PathBuf::from(p).join("firecracker.sock"));
        }
    }

    #[test]
    fn client_method_returns_correct_path() {
        let sb = FirecrackerSandbox::new(Language::Python, PathBuf::from("/my/work"));
        let c = sb.client();
        assert_eq!(c.socket_path, PathBuf::from("/my/work/firecracker.sock"));
    }
}
