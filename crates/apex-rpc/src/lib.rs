//! gRPC distributed coordination for APEX — coordinator/worker architecture
//! for parallel coverage exploration across multiple machines.

pub mod coordinator;
pub mod interceptor;
pub mod worker;

pub mod proto {
    tonic::include_proto!("apex.rpc");
}

pub use coordinator::CoordinatorServer;
pub use worker::WorkerClient;
