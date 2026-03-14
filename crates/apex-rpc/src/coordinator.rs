use crate::proto::{
    apex_coordinator_server::{ApexCoordinator, ApexCoordinatorServer},
    BranchId as ProtoBranchId, CoverageSnapshot, Empty, Heartbeat, HeartbeatAck, InputSeed,
    RegisterResponse, ResultAck, ResultBatch, SeedBatch, SeedRequest, WorkerInfo,
};
use apex_core::types::{BranchId, SeedId};
use apex_coverage::CoverageOracle;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use tracing::instrument;
use uuid::Uuid;

/// Convert a proto BranchId to the core BranchId type.
fn proto_to_core_branch(pb: &ProtoBranchId) -> BranchId {
    BranchId::new(pb.file_id, pb.line, pb.col as u16, pb.direction as u8)
}

/// Convert a core BranchId to the proto BranchId type.
fn core_to_proto_branch(b: &BranchId) -> ProtoBranchId {
    ProtoBranchId {
        file_id: b.file_id,
        line: b.line,
        col: b.col as u32,
        direction: b.direction as u32,
    }
}

pub struct CoordinatorService {
    oracle: Arc<CoverageOracle>,
    seed_queue: Arc<Mutex<VecDeque<InputSeed>>>,
}

impl CoordinatorService {
    pub fn new(oracle: Arc<CoverageOracle>) -> Self {
        CoordinatorService {
            oracle,
            seed_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Enqueue seeds for workers to consume.
    pub async fn enqueue_seeds(&self, seeds: Vec<InputSeed>) {
        let mut queue = self.seed_queue.lock().await;
        for seed in seeds {
            queue.push_back(seed);
        }
    }
}

#[tonic::async_trait]
impl ApexCoordinator for CoordinatorService {
    #[instrument(skip(self, request))]
    async fn register(
        &self,
        request: Request<WorkerInfo>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let info = request.into_inner();
        tracing::info!(
            worker_id = %info.worker_id,
            language = %info.language,
            capacity = info.capacity,
            "Worker registered"
        );
        Ok(Response::new(RegisterResponse {
            accepted: true,
            session_id: Uuid::new_v4().to_string(),
        }))
    }

    #[instrument(skip(self, request))]
    async fn send_heartbeat(
        &self,
        request: Request<Heartbeat>,
    ) -> Result<Response<HeartbeatAck>, Status> {
        let hb = request.into_inner();
        tracing::debug!(
            worker_id = %hb.worker_id,
            active_seeds = hb.active_seeds,
            "Heartbeat received"
        );
        Ok(Response::new(HeartbeatAck { ok: true }))
    }

    #[instrument(skip(self, request))]
    async fn get_seeds(
        &self,
        request: Request<SeedRequest>,
    ) -> Result<Response<SeedBatch>, Status> {
        let req = request.into_inner();
        let mut queue = self.seed_queue.lock().await;
        let count = (req.max_seeds as usize).min(queue.len());
        let seeds: Vec<InputSeed> = queue.drain(..count).collect();
        Ok(Response::new(SeedBatch { seeds }))
    }

    #[instrument(skip(self, request))]
    async fn submit_results(
        &self,
        request: Request<ResultBatch>,
    ) -> Result<Response<ResultAck>, Status> {
        let batch = request.into_inner();
        let mut new_coverage_count: u64 = 0;

        for result in &batch.results {
            let seed_id = SeedId::new();
            for pb_branch in &result.new_branches {
                let branch = proto_to_core_branch(pb_branch);
                if self.oracle.mark_covered(&branch, seed_id) {
                    new_coverage_count += 1;
                }
            }
        }

        Ok(Response::new(ResultAck {
            new_coverage_count,
            coverage_percent: self.oracle.coverage_percent(),
        }))
    }

    #[instrument(skip(self, _request))]
    async fn get_coverage(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<CoverageSnapshot>, Status> {
        let uncovered = self
            .oracle
            .uncovered_branches()
            .iter()
            .map(core_to_proto_branch)
            .collect();

        Ok(Response::new(CoverageSnapshot {
            total_branches: self.oracle.total_count() as u64,
            covered_branches: self.oracle.covered_count() as u64,
            coverage_percent: self.oracle.coverage_percent(),
            uncovered,
        }))
    }
}

/// Helper to start the coordinator gRPC server.
pub struct CoordinatorServer;

impl CoordinatorServer {
    /// Start the gRPC server on the given address.
    /// Returns a future that runs the server.
    pub async fn start(
        addr: SocketAddr,
        oracle: Arc<CoverageOracle>,
    ) -> Result<(), tonic::transport::Error> {
        let service = CoordinatorService::new(oracle);
        tracing::info!(%addr, "Starting coordinator gRPC server");
        match tonic::transport::Server::builder()
            .add_service(ApexCoordinatorServer::new(service))
            .serve(addr)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::error!(%addr, error = %e, "Coordinator server failed — check if port is already in use");
                Err(e)
            }
        }
    }

    /// Start the gRPC server and return the service handle and the bound address.
    /// Useful for tests where you need to know the actual port.
    pub async fn start_with_service(
        addr: SocketAddr,
        oracle: Arc<CoverageOracle>,
    ) -> Result<
        (
            Arc<CoordinatorService>,
            tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
        ),
        tonic::transport::Error,
    > {
        let service = Arc::new(CoordinatorService::new(oracle));
        let svc_clone = service.clone();

        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ApexCoordinatorServer::from_arc(svc_clone))
                .serve(addr)
                .await
        });

        Ok((service, handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::apex_coordinator_server::ApexCoordinator;
    use crate::proto::{BranchId as ProtoBranchId, ExecutionResult as ProtoResult, ResultBatch};

    fn make_oracle() -> Arc<CoverageOracle> {
        let oracle = CoverageOracle::new();
        // Register 4 branches
        for line in 1..=4 {
            oracle.register_branches([BranchId::new(1, line, 0, 0)]);
        }
        Arc::new(oracle)
    }

    #[tokio::test]
    async fn test_register_returns_accepted() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .register(Request::new(WorkerInfo {
                worker_id: "w1".into(),
                language: "python".into(),
                capacity: 4,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.accepted);
        assert!(!resp.session_id.is_empty());
        // session_id should be a valid UUID
        assert!(Uuid::parse_str(&resp.session_id).is_ok());
    }

    #[tokio::test]
    async fn test_get_seeds_empty_queue() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_get_seeds_returns_enqueued() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        // Enqueue 3 seeds
        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1, 2, 3],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![4, 5],
                    origin: "corpus".into(),
                },
                InputSeed {
                    id: "s3".into(),
                    data: vec![6],
                    origin: "agent".into(),
                },
            ])
            .await;

        // Request only 2
        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 2,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.seeds.len(), 2);
        assert_eq!(resp.seeds[0].id, "s1");
        assert_eq!(resp.seeds[1].id, "s2");

        // 1 seed should remain
        let resp2 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp2.seeds.len(), 1);
        assert_eq!(resp2.seeds[0].id, "s3");
    }

    #[tokio::test]
    async fn test_submit_results_merges_coverage() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Submit results covering branches at line 1 and 2
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![
                        ProtoBranchId {
                            file_id: 1,
                            line: 1,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 2,
                            col: 0,
                            direction: 0,
                        },
                    ],
                    duration_ms: 42,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 2);
        assert!((resp.coverage_percent - 50.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 2);
    }

    #[tokio::test]
    async fn test_submit_results_idempotent() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        let branch = ProtoBranchId {
            file_id: 1,
            line: 1,
            col: 0,
            direction: 0,
        };

        // First submission
        let resp1 = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![branch.clone()],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp1.new_coverage_count, 1);

        // Second submission of the same branch
        let resp2 = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s2".into(),
                    status: "pass".into(),
                    new_branches: vec![branch],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp2.new_coverage_count, 0); // already covered
    }

    #[tokio::test]
    async fn test_get_coverage_snapshot() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Initially no coverage
        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(snap.total_branches, 4);
        assert_eq!(snap.covered_branches, 0);
        assert!((snap.coverage_percent - 0.0).abs() < 0.01);
        assert_eq!(snap.uncovered.len(), 4);

        // Cover 2 branches
        service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![
                        ProtoBranchId {
                            file_id: 1,
                            line: 1,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 3,
                            col: 0,
                            direction: 0,
                        },
                    ],
                    duration_ms: 5,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap();

        let snap2 = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(snap2.total_branches, 4);
        assert_eq!(snap2.covered_branches, 2);
        assert!((snap2.coverage_percent - 50.0).abs() < 0.01);
        assert_eq!(snap2.uncovered.len(), 2);
    }

    #[tokio::test]
    async fn test_heartbeat() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .send_heartbeat(Request::new(Heartbeat {
                worker_id: "w1".into(),
                active_seeds: 3,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.ok);
    }

    #[tokio::test]
    async fn test_proto_branch_conversion_roundtrip() {
        let core_branch = BranchId::new(42, 100, 5, 1);
        let proto = core_to_proto_branch(&core_branch);
        let back = proto_to_core_branch(&proto);
        assert_eq!(core_branch.file_id, back.file_id);
        assert_eq!(core_branch.line, back.line);
        assert_eq!(core_branch.col, back.col);
        assert_eq!(core_branch.direction, back.direction);
    }

    #[tokio::test]
    async fn test_enqueue_seeds_empty_vec() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        service.enqueue_seeds(vec![]).await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_get_seeds_zero_max() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        service
            .enqueue_seeds(vec![InputSeed {
                id: "s1".into(),
                data: vec![1],
                origin: "fuzzer".into(),
            }])
            .await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_submit_results_empty_batch() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 0);
    }

    #[tokio::test]
    async fn test_submit_results_no_branches() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 0);
    }

    #[tokio::test]
    async fn test_enqueue_then_drain_completely() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let seeds: Vec<InputSeed> = (0..5)
            .map(|i| InputSeed {
                id: format!("s{i}"),
                data: vec![i as u8],
                origin: "corpus".into(),
            })
            .collect();

        service.enqueue_seeds(seeds).await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 5,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.seeds.len(), 5);
        for (i, seed) in resp.seeds.iter().enumerate() {
            assert_eq!(seed.id, format!("s{i}"));
        }

        // Queue should now be empty
        let resp2 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp2.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_proto_branch_conversion_edge_values() {
        // Test col/direction truncation: col is u32→u16, direction is u32→u8
        let proto = ProtoBranchId {
            file_id: u64::MAX,
            line: u32::MAX,
            col: 65535,     // max u16
            direction: 255, // max u8
        };
        let core = proto_to_core_branch(&proto);
        assert_eq!(core.file_id, u64::MAX);
        assert_eq!(core.line, u32::MAX);
        assert_eq!(core.col, 65535);
        assert_eq!(core.direction, 255);

        // Roundtrip
        let back = core_to_proto_branch(&core);
        assert_eq!(back.file_id, u64::MAX);
        assert_eq!(back.line, u32::MAX);
        assert_eq!(back.col, 65535);
        assert_eq!(back.direction, 255);
    }

    #[tokio::test]
    async fn test_proto_branch_conversion_zeros() {
        let proto = ProtoBranchId {
            file_id: 0,
            line: 0,
            col: 0,
            direction: 0,
        };
        let core = proto_to_core_branch(&proto);
        assert_eq!(core.file_id, 0);
        assert_eq!(core.line, 0);
        assert_eq!(core.col, 0);
        assert_eq!(core.direction, 0);
    }

    #[tokio::test]
    async fn test_submit_results_multiple_results_in_batch() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Submit two results, each covering different branches
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![
                    ProtoResult {
                        seed_id: "s1".into(),
                        status: "pass".into(),
                        new_branches: vec![ProtoBranchId {
                            file_id: 1,
                            line: 1,
                            col: 0,
                            direction: 0,
                        }],
                        duration_ms: 10,
                        stdout: String::new(),
                        stderr: String::new(),
                    },
                    ProtoResult {
                        seed_id: "s2".into(),
                        status: "pass".into(),
                        new_branches: vec![
                            ProtoBranchId {
                                file_id: 1,
                                line: 2,
                                col: 0,
                                direction: 0,
                            },
                            ProtoBranchId {
                                file_id: 1,
                                line: 3,
                                col: 0,
                                direction: 0,
                            },
                        ],
                        duration_ms: 20,
                        stdout: String::new(),
                        stderr: String::new(),
                    },
                ],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 3);
        assert!((resp.coverage_percent - 75.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 3);
    }

    #[tokio::test]
    async fn test_submit_results_auto_registers_unknown_branch() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Submit a branch that was never explicitly registered (file_id=99).
        // mark_covered auto-registers unknown branches.
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![ProtoBranchId {
                        file_id: 99,
                        line: 999,
                        col: 0,
                        direction: 0,
                    }],
                    duration_ms: 5,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        // Auto-registered and covered
        assert_eq!(resp.new_coverage_count, 1);
        assert_eq!(oracle.covered_count(), 1);
        // Total count increased by auto-registration
        assert_eq!(oracle.total_count(), 5); // 4 original + 1 auto-registered
    }

    #[tokio::test]
    async fn test_get_coverage_all_covered() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Cover all 4 branches
        service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![
                        ProtoBranchId {
                            file_id: 1,
                            line: 1,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 2,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 3,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 4,
                            col: 0,
                            direction: 0,
                        },
                    ],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap();

        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(snap.total_branches, 4);
        assert_eq!(snap.covered_branches, 4);
        assert!((snap.coverage_percent - 100.0).abs() < 0.01);
        assert!(snap.uncovered.is_empty());
    }

    #[tokio::test]
    async fn test_register_multiple_workers() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let r1 = service
            .register(Request::new(WorkerInfo {
                worker_id: "w1".into(),
                language: "python".into(),
                capacity: 4,
            }))
            .await
            .unwrap()
            .into_inner();

        let r2 = service
            .register(Request::new(WorkerInfo {
                worker_id: "w2".into(),
                language: "java".into(),
                capacity: 8,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(r1.accepted);
        assert!(r2.accepted);
        // Each registration should get a distinct session_id
        assert_ne!(r1.session_id, r2.session_id);
    }

    #[tokio::test]
    async fn test_get_seeds_partial_drain() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        // Enqueue 5 seeds
        let seeds: Vec<InputSeed> = (0..5)
            .map(|i| InputSeed {
                id: format!("s{i}"),
                data: vec![i as u8],
                origin: "test".into(),
            })
            .collect();
        service.enqueue_seeds(seeds).await;

        // Get 2
        let r1 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 2,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r1.seeds.len(), 2);
        assert_eq!(r1.seeds[0].id, "s0");
        assert_eq!(r1.seeds[1].id, "s1");

        // Get 2 more
        let r2 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w2".into(),
                max_seeds: 2,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r2.seeds.len(), 2);
        assert_eq!(r2.seeds[0].id, "s2");
        assert_eq!(r2.seeds[1].id, "s3");

        // Get remaining
        let r3 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r3.seeds.len(), 1);
        assert_eq!(r3.seeds[0].id, "s4");
    }

    #[tokio::test]
    async fn test_multiple_enqueue_calls_accumulate() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "a1".into(),
                    data: vec![1],
                    origin: "fuzzer".into(),
                },
                InputSeed {
                    id: "a2".into(),
                    data: vec![2],
                    origin: "fuzzer".into(),
                },
            ])
            .await;

        service
            .enqueue_seeds(vec![InputSeed {
                id: "b1".into(),
                data: vec![3],
                origin: "corpus".into(),
            }])
            .await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.seeds.len(), 3);
        assert_eq!(resp.seeds[0].id, "a1");
        assert_eq!(resp.seeds[1].id, "a2");
        assert_eq!(resp.seeds[2].id, "b1");
    }

    #[tokio::test]
    async fn test_coordinator_service_new_starts_empty_queue() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Queue should be empty initially
        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 100,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.seeds.is_empty());

        // Oracle should have 4 branches, 0 covered
        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(snap.total_branches, 4);
        assert_eq!(snap.covered_branches, 0);
    }

    #[tokio::test]
    async fn test_core_to_proto_branch_preserves_all_fields() {
        let core = BranchId::new(123, 456, 789, 2);
        let proto = core_to_proto_branch(&core);
        assert_eq!(proto.file_id, 123);
        assert_eq!(proto.line, 456);
        assert_eq!(proto.col, 789);
        assert_eq!(proto.direction, 2);
    }

    #[tokio::test]
    async fn test_proto_to_core_branch_truncates_large_col() {
        // col is u32 in proto, u16 in core — test with value that fits in u16
        let proto = ProtoBranchId {
            file_id: 1,
            line: 1,
            col: 300,
            direction: 1,
        };
        let core = proto_to_core_branch(&proto);
        assert_eq!(core.col, 300);
    }

    #[tokio::test]
    async fn test_submit_results_covers_all_branches_then_check_coverage() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Cover all 4 branches in a single batch with multiple results
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![
                    ProtoResult {
                        seed_id: "s1".into(),
                        status: "pass".into(),
                        new_branches: vec![
                            ProtoBranchId {
                                file_id: 1,
                                line: 1,
                                col: 0,
                                direction: 0,
                            },
                            ProtoBranchId {
                                file_id: 1,
                                line: 2,
                                col: 0,
                                direction: 0,
                            },
                        ],
                        duration_ms: 5,
                        stdout: "out1".into(),
                        stderr: "err1".into(),
                    },
                    ProtoResult {
                        seed_id: "s2".into(),
                        status: "fail".into(),
                        new_branches: vec![
                            ProtoBranchId {
                                file_id: 1,
                                line: 3,
                                col: 0,
                                direction: 0,
                            },
                            ProtoBranchId {
                                file_id: 1,
                                line: 4,
                                col: 0,
                                direction: 0,
                            },
                        ],
                        duration_ms: 15,
                        stdout: String::new(),
                        stderr: "error".into(),
                    },
                ],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 4);
        assert!((resp.coverage_percent - 100.0).abs() < 0.01);

        // Verify via get_coverage
        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(snap.covered_branches, 4);
        assert!(snap.uncovered.is_empty());
    }

    #[tokio::test]
    async fn test_get_seeds_max_exceeds_queue_length() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        // Enqueue 2 seeds but request 100
        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "s1".into(),
                    data: vec![1],
                    origin: "test".into(),
                },
                InputSeed {
                    id: "s2".into(),
                    data: vec![2],
                    origin: "test".into(),
                },
            ])
            .await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 100,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.seeds.len(), 2);
    }

    #[tokio::test]
    async fn test_get_coverage_with_empty_oracle() {
        // Oracle with no registered branches
        let oracle = Arc::new(CoverageOracle::new());
        let service = CoordinatorService::new(oracle);

        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(snap.total_branches, 0);
        assert_eq!(snap.covered_branches, 0);
        assert!(snap.uncovered.is_empty());
    }

    #[tokio::test]
    async fn test_submit_results_with_stdout_stderr() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Verify results with non-empty stdout/stderr are accepted
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "error".into(),
                    new_branches: vec![ProtoBranchId {
                        file_id: 1,
                        line: 1,
                        col: 0,
                        direction: 0,
                    }],
                    duration_ms: 1000,
                    stdout: "some output\nwith newlines".into(),
                    stderr: "error: something went wrong".into(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 1);
    }

    #[tokio::test]
    async fn test_register_with_zero_capacity() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .register(Request::new(WorkerInfo {
                worker_id: "w1".into(),
                language: "python".into(),
                capacity: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.accepted);
    }

    #[tokio::test]
    async fn test_register_with_empty_fields() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .register(Request::new(WorkerInfo {
                worker_id: String::new(),
                language: String::new(),
                capacity: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.accepted);
        assert!(Uuid::parse_str(&resp.session_id).is_ok());
    }

    #[tokio::test]
    async fn test_heartbeat_with_high_active_seeds() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .send_heartbeat(Request::new(Heartbeat {
                worker_id: "w1".into(),
                active_seeds: u32::MAX,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.ok);
    }

    #[tokio::test]
    async fn test_heartbeat_with_empty_worker_id() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let resp = service
            .send_heartbeat(Request::new(Heartbeat {
                worker_id: String::new(),
                active_seeds: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.ok);
    }

    #[tokio::test]
    async fn test_submit_results_multiple_branches_same_result() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Single result with all 4 branches
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: (1..=4)
                        .map(|line| ProtoBranchId {
                            file_id: 1,
                            line,
                            col: 0,
                            direction: 0,
                        })
                        .collect(),
                    duration_ms: 42,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 4);
        assert!((resp.coverage_percent - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_submit_results_duplicate_branch_in_single_result() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Same branch listed twice in one result
        let branch = ProtoBranchId {
            file_id: 1,
            line: 1,
            col: 0,
            direction: 0,
        };
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![branch.clone(), branch],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        // First time counts as new, second time is already covered
        assert_eq!(resp.new_coverage_count, 1);
        assert_eq!(oracle.covered_count(), 1);
    }

    #[tokio::test]
    async fn test_get_coverage_uncovered_branches_mapped_correctly() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Cover branch at line 2 only
        service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![ProtoBranchId {
                        file_id: 1,
                        line: 2,
                        col: 0,
                        direction: 0,
                    }],
                    duration_ms: 5,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap();

        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(snap.uncovered.len(), 3);
        // All uncovered should be proto BranchIds with file_id=1
        for ub in &snap.uncovered {
            assert_eq!(ub.file_id, 1);
            assert_ne!(ub.line, 2); // line 2 is covered
        }
    }

    #[tokio::test]
    async fn test_seed_data_preserved() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let large_data: Vec<u8> = (0..=255).collect();
        service
            .enqueue_seeds(vec![InputSeed {
                id: "big".into(),
                data: large_data.clone(),
                origin: "test".into(),
            }])
            .await;

        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 1,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.seeds.len(), 1);
        assert_eq!(resp.seeds[0].data, large_data);
        assert_eq!(resp.seeds[0].id, "big");
        assert_eq!(resp.seeds[0].origin, "test");
    }

    #[tokio::test]
    async fn test_start_with_service_returns_service_and_handle() {
        // Try to bind; skip if sandboxed
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(_) => return,
        };
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let oracle = make_oracle();
        let result = CoordinatorServer::start_with_service(addr, oracle.clone()).await;
        assert!(result.is_ok());

        let (service, handle) = result.unwrap();

        // Service should work for direct calls
        let snap = service
            .get_coverage(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(snap.total_branches, 4);

        // Enqueue and drain via service
        service
            .enqueue_seeds(vec![InputSeed {
                id: "s1".into(),
                data: vec![1],
                origin: "test".into(),
            }])
            .await;

        let seeds = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(seeds.seeds.len(), 1);

        // Abort the server handle
        handle.abort();
    }

    #[tokio::test]
    async fn test_submit_results_incremental_coverage() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // First submission: cover branch 1
        let r1 = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![ProtoBranchId {
                        file_id: 1,
                        line: 1,
                        col: 0,
                        direction: 0,
                    }],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r1.new_coverage_count, 1);
        assert!((r1.coverage_percent - 25.0).abs() < 0.01);

        // Second submission: cover branches 2 and 3
        let r2 = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s2".into(),
                    status: "pass".into(),
                    new_branches: vec![
                        ProtoBranchId {
                            file_id: 1,
                            line: 2,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 1,
                            line: 3,
                            col: 0,
                            direction: 0,
                        },
                    ],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r2.new_coverage_count, 2);
        assert!((r2.coverage_percent - 75.0).abs() < 0.01);

        // Third submission: cover branch 4 (reaching 100%)
        let r3 = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s3".into(),
                    status: "pass".into(),
                    new_branches: vec![ProtoBranchId {
                        file_id: 1,
                        line: 4,
                        col: 0,
                        direction: 0,
                    }],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r3.new_coverage_count, 1);
        assert!((r3.coverage_percent - 100.0).abs() < 0.01);
        assert_eq!(oracle.covered_count(), 4);
    }

    #[tokio::test]
    async fn test_enqueue_large_batch() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let seeds: Vec<InputSeed> = (0..100)
            .map(|i| InputSeed {
                id: format!("seed-{i}"),
                data: vec![i as u8],
                origin: "bulk".into(),
            })
            .collect();

        service.enqueue_seeds(seeds).await;

        // Drain in chunks
        let r1 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 50,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r1.seeds.len(), 50);
        assert_eq!(r1.seeds[0].id, "seed-0");
        assert_eq!(r1.seeds[49].id, "seed-49");

        let r2 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 50,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(r2.seeds.len(), 50);
        assert_eq!(r2.seeds[0].id, "seed-50");
        assert_eq!(r2.seeds[49].id, "seed-99");

        // Queue empty
        let r3 = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(r3.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_proto_branch_conversion_mid_values() {
        let core = BranchId::new(500, 250, 128, 3);
        let proto = core_to_proto_branch(&core);
        let back = proto_to_core_branch(&proto);

        assert_eq!(back.file_id, 500);
        assert_eq!(back.line, 250);
        assert_eq!(back.col, 128);
        assert_eq!(back.direction, 3);
    }

    #[tokio::test]
    async fn test_get_seeds_one_at_a_time() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        service
            .enqueue_seeds(vec![
                InputSeed {
                    id: "a".into(),
                    data: vec![1],
                    origin: "t".into(),
                },
                InputSeed {
                    id: "b".into(),
                    data: vec![2],
                    origin: "t".into(),
                },
                InputSeed {
                    id: "c".into(),
                    data: vec![3],
                    origin: "t".into(),
                },
            ])
            .await;

        for expected_id in ["a", "b", "c"] {
            let resp = service
                .get_seeds(Request::new(SeedRequest {
                    worker_id: "w1".into(),
                    max_seeds: 1,
                }))
                .await
                .unwrap()
                .into_inner();
            assert_eq!(resp.seeds.len(), 1);
            assert_eq!(resp.seeds[0].id, expected_id);
        }

        // Empty now
        let resp = service
            .get_seeds(Request::new(SeedRequest {
                worker_id: "w1".into(),
                max_seeds: 1,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.seeds.is_empty());
    }

    #[tokio::test]
    async fn test_submit_results_mixed_known_and_unknown_branches() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle.clone());

        // Mix of known (file_id=1, line=1) and unknown (file_id=99, line=1)
        let resp = service
            .submit_results(Request::new(ResultBatch {
                worker_id: "w1".into(),
                results: vec![ProtoResult {
                    seed_id: "s1".into(),
                    status: "pass".into(),
                    new_branches: vec![
                        ProtoBranchId {
                            file_id: 1,
                            line: 1,
                            col: 0,
                            direction: 0,
                        },
                        ProtoBranchId {
                            file_id: 99,
                            line: 1,
                            col: 0,
                            direction: 0,
                        },
                    ],
                    duration_ms: 10,
                    stdout: String::new(),
                    stderr: String::new(),
                }],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.new_coverage_count, 2);
        assert_eq!(oracle.covered_count(), 2);
        // 4 original + 1 auto-registered
        assert_eq!(oracle.total_count(), 5);
    }

    #[tokio::test]
    async fn test_register_returns_unique_session_ids() {
        let oracle = make_oracle();
        let service = CoordinatorService::new(oracle);

        let mut session_ids = Vec::new();
        for i in 0..5 {
            let resp = service
                .register(Request::new(WorkerInfo {
                    worker_id: format!("w{i}"),
                    language: "python".into(),
                    capacity: 1,
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(resp.accepted);
            assert!(Uuid::parse_str(&resp.session_id).is_ok());
            session_ids.push(resp.session_id);
        }

        // All session IDs should be unique
        let unique_count = {
            let mut s = session_ids.clone();
            s.sort();
            s.dedup();
            s.len()
        };
        assert_eq!(unique_count, 5);
    }
}
