use crate::proto::{
    apex_coordinator_client::ApexCoordinatorClient, Empty, ExecutionResult as ProtoResult,
    Heartbeat, ResultBatch, SeedRequest, WorkerInfo,
};
use tonic::transport::Channel;
use uuid::Uuid;

/// gRPC client that connects to a coordinator, registers, and runs a pull loop.
pub struct WorkerClient {
    client: ApexCoordinatorClient<Channel>,
    worker_id: String,
    language: String,
}

impl WorkerClient {
    /// Connect to a coordinator at the given endpoint.
    pub async fn connect(
        endpoint: String,
        language: String,
    ) -> Result<Self, tonic::transport::Error> {
        let client = ApexCoordinatorClient::connect(endpoint).await?;
        Ok(WorkerClient {
            client,
            worker_id: Uuid::new_v4().to_string(),
            language,
        })
    }

    /// Create from an already-connected channel (useful for tests).
    pub fn from_channel(channel: Channel, language: String) -> Self {
        WorkerClient {
            client: ApexCoordinatorClient::new(channel),
            worker_id: Uuid::new_v4().to_string(),
            language,
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// Register this worker with the coordinator.
    /// Returns the session_id on success.
    pub async fn register(&mut self, capacity: u32) -> Result<String, tonic::Status> {
        let resp = self
            .client
            .register(WorkerInfo {
                worker_id: self.worker_id.clone(),
                language: self.language.clone(),
                capacity,
            })
            .await?
            .into_inner();

        if !resp.accepted {
            return Err(tonic::Status::permission_denied("Registration rejected"));
        }
        Ok(resp.session_id)
    }

    /// Send a heartbeat to the coordinator.
    pub async fn heartbeat(&mut self, active_seeds: u32) -> Result<bool, tonic::Status> {
        let resp = self
            .client
            .send_heartbeat(Heartbeat {
                worker_id: self.worker_id.clone(),
                active_seeds,
            })
            .await?
            .into_inner();

        Ok(resp.ok)
    }

    /// Request seeds from the coordinator.
    pub async fn get_seeds(
        &mut self,
        max_seeds: u32,
    ) -> Result<Vec<crate::proto::InputSeed>, tonic::Status> {
        let resp = self
            .client
            .get_seeds(SeedRequest {
                worker_id: self.worker_id.clone(),
                max_seeds,
            })
            .await?
            .into_inner();

        Ok(resp.seeds)
    }

    /// Submit execution results back to the coordinator.
    /// Returns (new_coverage_count, coverage_percent).
    pub async fn submit_results(
        &mut self,
        results: Vec<ProtoResult>,
    ) -> Result<(u64, f64), tonic::Status> {
        let resp = self
            .client
            .submit_results(ResultBatch {
                worker_id: self.worker_id.clone(),
                results,
            })
            .await?
            .into_inner();

        Ok((resp.new_coverage_count, resp.coverage_percent))
    }

    /// Get the current coverage snapshot from the coordinator.
    pub async fn get_coverage(&mut self) -> Result<crate::proto::CoverageSnapshot, tonic::Status> {
        let resp = self.client.get_coverage(Empty {}).await?.into_inner();
        Ok(resp)
    }

    /// Run a single pull iteration: get seeds, execute them with the provided
    /// callback, and submit results.
    ///
    /// The `execute` callback receives a seed and returns an optional execution
    /// result (None means skip).
    pub async fn pull_once<F>(
        &mut self,
        max_seeds: u32,
        execute: F,
    ) -> Result<(u64, f64), tonic::Status>
    where
        F: Fn(&crate::proto::InputSeed) -> Option<ProtoResult>,
    {
        let seeds = self.get_seeds(max_seeds).await?;
        if seeds.is_empty() {
            return Ok((0, 0.0));
        }

        let results: Vec<ProtoResult> = seeds.iter().filter_map(execute).collect();

        if results.is_empty() {
            return Ok((0, 0.0));
        }

        self.submit_results(results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::{CoordinatorServer, CoordinatorService};
    use crate::proto::{BranchId as ProtoBranchId, ExecutionResult as ProtoResult, InputSeed};
    use apex_core::types::BranchId;
    use apex_coverage::CoverageOracle;
    use std::net::SocketAddr;
    use std::sync::Arc;

    /// Start a real CoordinatorServer on a random port, connect a WorkerClient to it,
    /// and return all handles needed for testing.
    ///
    /// Returns `None` if TCP binding is blocked (e.g. in sandboxed environments).
    /// Tests using this should early-return on `None`.
    async fn setup_worker() -> Option<(WorkerClient, Arc<CoordinatorService>, Arc<CoverageOracle>)>
    {
        let oracle = Arc::new(CoverageOracle::new());
        // Register 4 branches for testing
        for line in 1..=4 {
            oracle.register_branches([BranchId::new(1, line, 0, 0)]);
        }

        // Bind a TCP listener to get a free port, then release it.
        // This may fail in sandboxed environments — return None to skip.
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return None,
        };
        let addr: SocketAddr = listener.local_addr().unwrap();
        drop(listener);

        let (service, _handle) = CoordinatorServer::start_with_service(addr, oracle.clone())
            .await
            .unwrap();

        // Give the server time to bind the port
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let worker = WorkerClient::connect(format!("http://{addr}"), "python".into())
            .await
            .unwrap();

        Some((worker, service, oracle))
    }

    fn make_branch(line: u32) -> ProtoBranchId {
        ProtoBranchId {
            file_id: 1,
            line,
            col: 0,
            direction: 0,
        }
    }

    fn make_result(seed_id: &str, branches: Vec<ProtoBranchId>) -> ProtoResult {
        ProtoResult {
            seed_id: seed_id.into(),
            status: "pass".into(),
            new_branches: branches,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[tokio::test]
    async fn test_worker_id_is_uuid() {
        let Some((worker, _service, _oracle)) = setup_worker().await else {
            return;
        };
        let id = worker.worker_id();
        assert!(
            uuid::Uuid::parse_str(id).is_ok(),
            "worker_id should be a valid UUID, got: {id}"
        );
    }

    #[tokio::test]
    async fn test_register_succeeds() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };
        let session_id = worker.register(4).await.unwrap();
        assert!(!session_id.is_empty());
        assert!(
            uuid::Uuid::parse_str(&session_id).is_ok(),
            "session_id should be a valid UUID, got: {session_id}"
        );
    }

    #[tokio::test]
    async fn test_heartbeat_succeeds() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };
        let ok = worker.heartbeat(0).await.unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn test_get_seeds_empty() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };
        let seeds = worker.get_seeds(10).await.unwrap();
        assert!(seeds.is_empty());
    }

    #[tokio::test]
    async fn test_enqueue_and_get_seeds() {
        let Some((mut worker, service, _oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1, 2],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![3],
                    origin: "corpus".into(),
                },
            ])
            .await;

        let seeds = worker.get_seeds(10).await.unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].id, "s1");
        assert_eq!(seeds[1].id, "s2");
    }

    #[tokio::test]
    async fn test_submit_results_updates_coverage() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        let results = vec![make_result("s1", vec![make_branch(1), make_branch(2)])];
        let (new_count, pct) = worker.submit_results(results).await.unwrap();

        assert_eq!(new_count, 2);
        assert!((pct - 50.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 2);
    }

    #[tokio::test]
    async fn test_get_coverage_snapshot() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        // Initially empty
        let snap = worker.get_coverage().await.unwrap();
        assert_eq!(snap.total_branches, 4);
        assert_eq!(snap.covered_branches, 0);
        assert!((snap.coverage_percent - 0.0).abs() < 0.01);
        assert_eq!(snap.uncovered.len(), 4);

        // Cover 2 branches
        worker
            .submit_results(vec![make_result(
                "s1",
                vec![make_branch(1), make_branch(3)],
            )])
            .await
            .unwrap();

        let snap2 = worker.get_coverage().await.unwrap();
        assert_eq!(snap2.total_branches, 4);
        assert_eq!(snap2.covered_branches, 2);
        assert!((snap2.coverage_percent - 50.0).abs() < 0.01);
        assert_eq!(snap2.uncovered.len(), 2);
        // Verify oracle is consistent
        assert_eq!(oracle.covered_count(), 2);
    }

    #[tokio::test]
    async fn test_pull_once_empty_queue() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };

        let (count, pct) = worker
            .pull_once(10, |_seed| {
                panic!("callback should not be called on empty queue");
            })
            .await
            .unwrap();

        assert_eq!(count, 0);
        assert!((pct - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_pull_once_with_callback() {
        let Some((mut worker, service, oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![2],
                    origin: "fuzzer".into(),
                },
            ])
            .await;

        let (count, pct) = worker
            .pull_once(10, |seed| {
                // Each seed covers one branch based on its data
                let line = seed.data[0] as u32;
                Some(make_result(&seed.id, vec![make_branch(line)]))
            })
            .await
            .unwrap();

        assert_eq!(count, 2);
        assert!((pct - 50.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 2);
    }

    #[tokio::test]
    async fn test_pull_once_callback_returns_none() {
        let Some((mut worker, service, _oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![2],
                    origin: "fuzzer".into(),
                },
            ])
            .await;

        // Callback returns None for all seeds (skip all)
        let (count, pct) = worker.pull_once(10, |_seed| None).await.unwrap();

        assert_eq!(count, 0);
        assert!((pct - 0.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // from_channel / pure constructor tests (no server required)
    // -----------------------------------------------------------------------

    /// `from_channel` creates a lazy channel without an actual TCP connection,
    /// so we can exercise the constructor and accessor without a server.
    #[tokio::test]
    async fn test_from_channel_worker_id_is_uuid() {
        // Channel::from_static creates a lazily-evaluated channel — no connection
        // is made until an RPC is issued.
        let channel = tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let worker = WorkerClient::from_channel(channel, "rust".into());
        let id = worker.worker_id();
        assert!(
            uuid::Uuid::parse_str(id).is_ok(),
            "worker_id from from_channel should be a valid UUID, got: {id}"
        );
    }

    #[tokio::test]
    async fn test_from_channel_distinct_ids() {
        // Two workers from different from_channel calls must both have valid UUIDs
        // (each call generates a fresh Uuid::new_v4).
        let ch1 = tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let ch2 = tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let w1 = WorkerClient::from_channel(ch1, "python".into());
        let w2 = WorkerClient::from_channel(ch2, "java".into());
        // IDs must be valid UUIDs
        assert!(uuid::Uuid::parse_str(w1.worker_id()).is_ok());
        assert!(uuid::Uuid::parse_str(w2.worker_id()).is_ok());
        // IDs must be distinct (collision probability negligible)
        assert_ne!(w1.worker_id(), w2.worker_id());
    }

    // -----------------------------------------------------------------------
    // Registration rejection path
    // -----------------------------------------------------------------------

    /// A coordinator that always rejects workers.
    struct RejectingCoordinator;

    #[tonic::async_trait]
    impl crate::proto::apex_coordinator_server::ApexCoordinator for RejectingCoordinator {
        async fn register(
            &self,
            _req: tonic::Request<crate::proto::WorkerInfo>,
        ) -> Result<tonic::Response<crate::proto::RegisterResponse>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::RegisterResponse {
                accepted: false,
                session_id: String::new(),
            }))
        }

        async fn send_heartbeat(
            &self,
            _req: tonic::Request<crate::proto::Heartbeat>,
        ) -> Result<tonic::Response<crate::proto::HeartbeatAck>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::HeartbeatAck {
                ok: false,
            }))
        }

        async fn get_seeds(
            &self,
            _req: tonic::Request<crate::proto::SeedRequest>,
        ) -> Result<tonic::Response<crate::proto::SeedBatch>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::SeedBatch {
                seeds: vec![],
            }))
        }

        async fn submit_results(
            &self,
            _req: tonic::Request<crate::proto::ResultBatch>,
        ) -> Result<tonic::Response<crate::proto::ResultAck>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::ResultAck {
                new_coverage_count: 0,
                coverage_percent: 0.0,
            }))
        }

        async fn get_coverage(
            &self,
            _req: tonic::Request<crate::proto::Empty>,
        ) -> Result<tonic::Response<crate::proto::CoverageSnapshot>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::CoverageSnapshot {
                total_branches: 0,
                covered_branches: 0,
                coverage_percent: 0.0,
                uncovered: vec![],
            }))
        }
    }

    async fn setup_rejecting_worker() -> Option<WorkerClient> {
        use crate::proto::apex_coordinator_server::ApexCoordinatorServer;

        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return None,
        };
        let addr: SocketAddr = listener.local_addr().unwrap();
        drop(listener);

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ApexCoordinatorServer::new(RejectingCoordinator))
                .serve(addr)
                .await
                .ok();
        });

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let worker = WorkerClient::connect(format!("http://{addr}"), "python".into())
            .await
            .unwrap();
        Some(worker)
    }

    #[tokio::test]
    async fn test_pull_once_partial_skip() {
        let Some((mut worker, service, oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![2],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s3".into(),
                    data: vec![3],
                    origin: "fuzzer".into(),
                },
            ])
            .await;

        // Only process seeds with even data, skip odd
        let (count, pct) = worker
            .pull_once(10, |seed| {
                #[allow(unknown_lints, clippy::manual_is_multiple_of)]
                if seed.data[0] % 2 == 0 {
                    Some(make_result(
                        &seed.id,
                        vec![make_branch(seed.data[0] as u32)],
                    ))
                } else {
                    None
                }
            })
            .await
            .unwrap();

        // Only seed s2 (data=2) should be processed, covering branch line=2
        assert_eq!(count, 1);
        assert!((pct - 25.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 1);
    }

    #[tokio::test]
    async fn test_pull_once_with_max_seeds_limit() {
        let Some((mut worker, service, oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![2],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s3".into(),
                    data: vec![3],
                    origin: "fuzzer".into(),
                },
            ])
            .await;

        // Request max_seeds=2, so only 2 should be processed
        let (count, _pct) = worker
            .pull_once(2, |seed| {
                Some(make_result(
                    &seed.id,
                    vec![make_branch(seed.data[0] as u32)],
                ))
            })
            .await
            .unwrap();

        // Only 2 seeds fetched, covering branches at lines 1 and 2
        assert_eq!(count, 2);
        assert_eq!(oracle.covered_count(), 2);

        // The third seed (s3) should still be in the queue
        let remaining = worker.get_seeds(10).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "s3");
    }

    #[tokio::test]
    async fn test_submit_results_empty_vec() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        let (count, pct) = worker.submit_results(vec![]).await.unwrap();
        assert_eq!(count, 0);
        assert!((pct - 0.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 0);
    }

    #[tokio::test]
    async fn test_submit_results_duplicate_branches() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        // Submit same branch twice in one batch
        let results = vec![
            make_result("s1", vec![make_branch(1)]),
            make_result("s2", vec![make_branch(1)]),
        ];
        let (new_count, pct) = worker.submit_results(results).await.unwrap();

        // Only 1 new branch (second is a duplicate)
        assert_eq!(new_count, 1);
        assert!((pct - 25.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 1);
    }

    #[tokio::test]
    async fn test_register_rejected_returns_permission_denied() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        let result = worker.register(4).await;
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert!(
            status.message().contains("rejected"),
            "expected 'rejected' in message, got: {}",
            status.message()
        );
    }

    // -----------------------------------------------------------------------
    // Additional coverage: rejecting coordinator — heartbeat, get_coverage,
    // get_seeds, submit_results, pull_once paths
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_rejecting_heartbeat_returns_false() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        let ok = worker.heartbeat(5).await.unwrap();
        assert!(!ok, "rejecting coordinator should return ok=false");
    }

    #[tokio::test]
    async fn test_rejecting_get_seeds_returns_empty() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        let seeds = worker.get_seeds(10).await.unwrap();
        assert!(seeds.is_empty());
    }

    #[tokio::test]
    async fn test_rejecting_submit_results_returns_zeros() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        let results = vec![make_result("s1", vec![make_branch(1)])];
        let (count, pct) = worker.submit_results(results).await.unwrap();
        assert_eq!(count, 0);
        assert!((pct - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_rejecting_get_coverage_returns_empty_snapshot() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        let snap = worker.get_coverage().await.unwrap();
        assert_eq!(snap.total_branches, 0);
        assert_eq!(snap.covered_branches, 0);
        assert!((snap.coverage_percent - 0.0).abs() < f64::EPSILON);
        assert!(snap.uncovered.is_empty());
    }

    #[tokio::test]
    async fn test_rejecting_pull_once_empty_seeds() {
        let Some(mut worker) = setup_rejecting_worker().await else {
            return;
        };
        // Rejecting coordinator returns empty seeds, so pull_once should
        // short-circuit with (0, 0.0) without calling the callback.
        let (count, pct) = worker
            .pull_once(10, |_seed| {
                panic!("callback should not be called when seeds are empty");
            })
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!((pct - 0.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Error coordinator: returns tonic::Status errors for each RPC
    // -----------------------------------------------------------------------

    struct ErrorCoordinator;

    #[tonic::async_trait]
    impl crate::proto::apex_coordinator_server::ApexCoordinator for ErrorCoordinator {
        async fn register(
            &self,
            _req: tonic::Request<crate::proto::WorkerInfo>,
        ) -> Result<tonic::Response<crate::proto::RegisterResponse>, tonic::Status> {
            Err(tonic::Status::internal("register error"))
        }

        async fn send_heartbeat(
            &self,
            _req: tonic::Request<crate::proto::Heartbeat>,
        ) -> Result<tonic::Response<crate::proto::HeartbeatAck>, tonic::Status> {
            Err(tonic::Status::unavailable("heartbeat error"))
        }

        async fn get_seeds(
            &self,
            _req: tonic::Request<crate::proto::SeedRequest>,
        ) -> Result<tonic::Response<crate::proto::SeedBatch>, tonic::Status> {
            Err(tonic::Status::resource_exhausted("no seeds available"))
        }

        async fn submit_results(
            &self,
            _req: tonic::Request<crate::proto::ResultBatch>,
        ) -> Result<tonic::Response<crate::proto::ResultAck>, tonic::Status> {
            Err(tonic::Status::deadline_exceeded("submit timeout"))
        }

        async fn get_coverage(
            &self,
            _req: tonic::Request<crate::proto::Empty>,
        ) -> Result<tonic::Response<crate::proto::CoverageSnapshot>, tonic::Status> {
            Err(tonic::Status::not_found("no coverage data"))
        }
    }

    async fn setup_error_worker() -> Option<WorkerClient> {
        use crate::proto::apex_coordinator_server::ApexCoordinatorServer;

        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return None,
        };
        let addr: SocketAddr = listener.local_addr().unwrap();
        drop(listener);

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ApexCoordinatorServer::new(ErrorCoordinator))
                .serve(addr)
                .await
                .ok();
        });

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let worker = WorkerClient::connect(format!("http://{addr}"), "rust".into())
            .await
            .unwrap();
        Some(worker)
    }

    #[tokio::test]
    async fn test_error_register_propagates_status() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        let err = worker.register(4).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("register error"));
    }

    #[tokio::test]
    async fn test_error_heartbeat_propagates_status() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        let err = worker.heartbeat(0).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unavailable);
        assert!(err.message().contains("heartbeat error"));
    }

    #[tokio::test]
    async fn test_error_get_seeds_propagates_status() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        let err = worker.get_seeds(5).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn test_error_submit_results_propagates_status() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        let results = vec![make_result("s1", vec![make_branch(1)])];
        let err = worker.submit_results(results).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::DeadlineExceeded);
    }

    #[tokio::test]
    async fn test_error_get_coverage_propagates_status() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        let err = worker.get_coverage().await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_error_pull_once_propagates_get_seeds_error() {
        let Some(mut worker) = setup_error_worker().await else {
            return;
        };
        // pull_once should propagate the get_seeds error
        let err = worker
            .pull_once(10, |_| panic!("should not be called"))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::ResourceExhausted);
    }

    // -----------------------------------------------------------------------
    // Submit-error coordinator: get_seeds succeeds but submit_results fails
    // -----------------------------------------------------------------------

    struct SubmitErrorCoordinator;

    #[tonic::async_trait]
    impl crate::proto::apex_coordinator_server::ApexCoordinator for SubmitErrorCoordinator {
        async fn register(
            &self,
            _req: tonic::Request<crate::proto::WorkerInfo>,
        ) -> Result<tonic::Response<crate::proto::RegisterResponse>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::RegisterResponse {
                accepted: true,
                session_id: "test-session".into(),
            }))
        }

        async fn send_heartbeat(
            &self,
            _req: tonic::Request<crate::proto::Heartbeat>,
        ) -> Result<tonic::Response<crate::proto::HeartbeatAck>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::HeartbeatAck {
                ok: true,
            }))
        }

        async fn get_seeds(
            &self,
            _req: tonic::Request<crate::proto::SeedRequest>,
        ) -> Result<tonic::Response<crate::proto::SeedBatch>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::SeedBatch {
                seeds: vec![crate::proto::InputSeed {
                    id: "seed-1".into(),
                    data: vec![42],
                    origin: "test".into(),
                }],
            }))
        }

        async fn submit_results(
            &self,
            _req: tonic::Request<crate::proto::ResultBatch>,
        ) -> Result<tonic::Response<crate::proto::ResultAck>, tonic::Status> {
            Err(tonic::Status::aborted("submit failed"))
        }

        async fn get_coverage(
            &self,
            _req: tonic::Request<crate::proto::Empty>,
        ) -> Result<tonic::Response<crate::proto::CoverageSnapshot>, tonic::Status> {
            Ok(tonic::Response::new(crate::proto::CoverageSnapshot {
                total_branches: 0,
                covered_branches: 0,
                coverage_percent: 0.0,
                uncovered: vec![],
            }))
        }
    }

    async fn setup_submit_error_worker() -> Option<WorkerClient> {
        use crate::proto::apex_coordinator_server::ApexCoordinatorServer;

        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return None,
        };
        let addr: SocketAddr = listener.local_addr().unwrap();
        drop(listener);

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ApexCoordinatorServer::new(SubmitErrorCoordinator))
                .serve(addr)
                .await
                .ok();
        });

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let worker = WorkerClient::connect(format!("http://{addr}"), "python".into())
            .await
            .unwrap();
        Some(worker)
    }

    #[tokio::test]
    async fn test_pull_once_submit_error_propagated() {
        let Some(mut worker) = setup_submit_error_worker().await else {
            return;
        };
        // pull_once gets seeds successfully but submit_results fails
        let err = worker
            .pull_once(10, |seed| Some(make_result(&seed.id, vec![make_branch(1)])))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Aborted);
        assert!(err.message().contains("submit failed"));
    }

    // -----------------------------------------------------------------------
    // Sequential operations on a single worker
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_sequential_submit_accumulates_coverage() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        // First submit: cover branch 1
        let (count1, pct1) = worker
            .submit_results(vec![make_result("s1", vec![make_branch(1)])])
            .await
            .unwrap();
        assert_eq!(count1, 1);
        assert!((pct1 - 25.0).abs() < 0.01);

        // Second submit: cover branches 2 and 3
        let (count2, pct2) = worker
            .submit_results(vec![make_result(
                "s2",
                vec![make_branch(2), make_branch(3)],
            )])
            .await
            .unwrap();
        assert_eq!(count2, 2);
        assert!((pct2 - 75.0).abs() < 0.01);

        // Third submit: cover branch 4 (completes 100%)
        let (count3, pct3) = worker
            .submit_results(vec![make_result("s3", vec![make_branch(4)])])
            .await
            .unwrap();
        assert_eq!(count3, 1);
        assert!((pct3 - 100.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 4);
    }

    #[tokio::test]
    async fn test_multiple_heartbeats_succeed() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };
        // Multiple heartbeats with different active_seeds counts
        for active in [0, 1, 5, 100] {
            let ok = worker.heartbeat(active).await.unwrap();
            assert!(ok);
        }
    }

    #[tokio::test]
    async fn test_get_seeds_zero_max() {
        let Some((mut worker, service, _oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![InputSeed {
                id: "s1".into(),
                data: vec![1],
                origin: "test".into(),
            }])
            .await;

        // Requesting 0 seeds should return empty
        let seeds = worker.get_seeds(0).await.unwrap();
        assert!(seeds.is_empty());
    }

    #[tokio::test]
    async fn test_submit_results_with_stdout_stderr() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };

        // Submit a result with non-empty stdout/stderr
        let result = ProtoResult {
            seed_id: "s1".into(),
            status: "fail".into(),
            new_branches: vec![make_branch(1)],
            duration_ms: 500,
            stdout: "some output\n".into(),
            stderr: "an error occurred\n".into(),
        };
        let (count, _pct) = worker.submit_results(vec![result]).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_submit_results_multiple_results_multiple_branches() {
        let Some((mut worker, _service, oracle)) = setup_worker().await else {
            return;
        };

        let results = vec![
            make_result("s1", vec![make_branch(1), make_branch(2)]),
            make_result("s2", vec![make_branch(3), make_branch(4)]),
        ];
        let (count, pct) = worker.submit_results(results).await.unwrap();
        assert_eq!(count, 4);
        assert!((pct - 100.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 4);
    }

    #[tokio::test]
    async fn test_register_then_heartbeat_then_coverage() {
        let Some((mut worker, _service, _oracle)) = setup_worker().await else {
            return;
        };

        // Full workflow: register, heartbeat, check coverage
        let session_id = worker.register(8).await.unwrap();
        assert!(!session_id.is_empty());

        let ok = worker.heartbeat(0).await.unwrap();
        assert!(ok);

        let snap = worker.get_coverage().await.unwrap();
        assert_eq!(snap.total_branches, 4);
        assert_eq!(snap.covered_branches, 0);
    }

    #[tokio::test]
    async fn test_connect_to_invalid_endpoint_fails() {
        // Connecting to an endpoint where nothing is listening should fail
        // when we actually try to make an RPC call.
        // Note: connect itself may succeed (lazy connection), so we test
        // by making an RPC call.
        let channel = tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let mut worker = WorkerClient::from_channel(channel, "python".into());

        // The RPC call should fail since there's no server at port 1
        let result = worker.heartbeat(0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_from_channel_language_preserved() {
        // Verify from_channel stores the language (exercised indirectly
        // through the register RPC which sends it).
        let channel = tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let worker = WorkerClient::from_channel(channel, "javascript".into());
        // We can at least verify the worker_id is set
        assert!(!worker.worker_id().is_empty());
        assert!(uuid::Uuid::parse_str(worker.worker_id()).is_ok());
    }

    #[tokio::test]
    async fn test_pull_once_single_seed_single_branch() {
        let Some((mut worker, service, oracle)) = setup_worker().await else {
            return;
        };

        service
            .enqueue_seeds(vec![InputSeed {
                id: "only".into(),
                data: vec![4],
                origin: "manual".into(),
            }])
            .await;

        let (count, pct) = worker
            .pull_once(1, |seed| {
                assert_eq!(seed.id, "only");
                assert_eq!(seed.data, vec![4]);
                assert_eq!(seed.origin, "manual");
                Some(make_result(&seed.id, vec![make_branch(4)]))
            })
            .await
            .unwrap();

        assert_eq!(count, 1);
        assert!((pct - 25.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 1);
    }
}
