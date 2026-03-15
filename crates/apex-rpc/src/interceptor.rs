//! Tower interceptor layer for recording gRPC calls in tests.
//!
//! Wraps a tonic channel to capture method names and request timestamps
//! for assertion in tests. This enables verifying call ordering
//! (e.g. "register() was called before get_seeds()") and call counts
//! (e.g. "heartbeat was called 3 times").
//!
//! # Example
//!
//! ```rust,no_run
//! use apex_rpc::interceptor::{CallLog, RecordLayer};
//!
//! let log = CallLog::new();
//! let layer = RecordLayer::new(log.clone());
//! // Apply layer to a tonic Channel via tower::ServiceBuilder
//! assert!(log.is_empty());
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use http::Request;
use tower::{Layer, Service};

/// A recorded gRPC call.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// The gRPC method path, e.g. `"/apex.rpc.ApexCoordinator/Register"`.
    pub method: String,
    /// When the call was made (monotonic clock).
    pub timestamp: std::time::Instant,
}

/// Shared log of recorded gRPC calls.
///
/// Thread-safe and cheaply cloneable. Create one instance, pass clones to
/// both the [`RecordLayer`] and your test assertions.
#[derive(Debug, Clone, Default)]
pub struct CallLog {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl CallLog {
    /// Create a new empty call log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a method call. This is used internally by [`RecordService`]
    /// but is also public for unit testing the log itself.
    pub fn record(&self, method: &str) {
        let mut calls = self.calls.lock().expect("CallLog mutex poisoned");
        calls.push(RecordedCall {
            method: method.to_owned(),
            timestamp: std::time::Instant::now(),
        });
    }

    /// Get all recorded method names in call order.
    #[must_use]
    pub fn methods(&self) -> Vec<String> {
        let calls = self.calls.lock().expect("CallLog mutex poisoned");
        calls.iter().map(|c| c.method.clone()).collect()
    }

    /// Count how many times a method was called.
    #[must_use]
    pub fn count(&self, method: &str) -> usize {
        let calls = self.calls.lock().expect("CallLog mutex poisoned");
        calls.iter().filter(|c| c.method == method).count()
    }

    /// Check if `first` was called before `second`.
    ///
    /// Returns `true` if there exists a call to `first` that occurred
    /// before the earliest call to `second`. Returns `false` if either
    /// method was never called.
    #[must_use]
    pub fn called_before(&self, first: &str, second: &str) -> bool {
        let calls = self.calls.lock().expect("CallLog mutex poisoned");
        let first_idx = calls.iter().position(|c| c.method == first);
        let second_idx = calls.iter().position(|c| c.method == second);
        match (first_idx, second_idx) {
            (Some(f), Some(s)) => f < s,
            _ => false,
        }
    }

    /// Clear all recorded calls.
    pub fn clear(&self) {
        let mut calls = self.calls.lock().expect("CallLog mutex poisoned");
        calls.clear();
    }

    /// Get total number of recorded calls.
    #[must_use]
    pub fn len(&self) -> usize {
        let calls = self.calls.lock().expect("CallLog mutex poisoned");
        calls.len()
    }

    /// Returns `true` if no calls have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Tower [`Layer`] that wraps a service to record all gRPC calls.
#[derive(Clone)]
pub struct RecordLayer {
    log: CallLog,
}

impl RecordLayer {
    /// Create a new recording layer backed by the given log.
    #[must_use]
    pub fn new(log: CallLog) -> Self {
        Self { log }
    }
}

impl<S> Layer<S> for RecordLayer {
    type Service = RecordService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RecordService {
            inner,
            log: self.log.clone(),
        }
    }
}

/// Tower [`Service`] that records the HTTP/2 request URI path (which
/// corresponds to the gRPC method name) before forwarding to the inner
/// service.
#[derive(Clone)]
pub struct RecordService<S> {
    inner: S,
    log: CallLog,
}

impl<S, ReqBody> Service<Request<ReqBody>> for RecordService<S>
where
    S: Service<Request<ReqBody>>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let path = req.uri().path().to_owned();
        self.log.record(&path);
        let fut = self.inner.call(req);
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_log_records_and_retrieves() {
        let log = CallLog::new();
        log.record("/apex.rpc.ApexCoordinator/Register");
        log.record("/apex.rpc.ApexCoordinator/GetSeeds");
        assert_eq!(
            log.methods(),
            vec![
                "/apex.rpc.ApexCoordinator/Register",
                "/apex.rpc.ApexCoordinator/GetSeeds",
            ]
        );
        assert_eq!(log.count("/apex.rpc.ApexCoordinator/Register"), 1);
        assert!(log.called_before(
            "/apex.rpc.ApexCoordinator/Register",
            "/apex.rpc.ApexCoordinator/GetSeeds"
        ));
    }

    #[test]
    fn call_log_clear() {
        let log = CallLog::new();
        log.record("test");
        assert!(!log.is_empty());
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn call_log_count_multiple() {
        let log = CallLog::new();
        log.record("/Heartbeat");
        log.record("/Heartbeat");
        log.record("/Heartbeat");
        assert_eq!(log.count("/Heartbeat"), 3);
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn call_log_called_before_missing_method_returns_false() {
        let log = CallLog::new();
        log.record("/Register");
        assert!(!log.called_before("/Register", "/GetSeeds"));
        assert!(!log.called_before("/GetSeeds", "/Register"));
        assert!(!log.called_before("/A", "/B"));
    }

    #[test]
    fn call_log_called_before_wrong_order_returns_false() {
        let log = CallLog::new();
        log.record("/GetSeeds");
        log.record("/Register");
        assert!(!log.called_before("/Register", "/GetSeeds"));
        assert!(log.called_before("/GetSeeds", "/Register"));
    }

    #[test]
    fn call_log_default_is_empty() {
        let log = CallLog::default();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.methods().is_empty());
    }

    #[test]
    fn call_log_clone_shares_state() {
        let log1 = CallLog::new();
        let log2 = log1.clone();
        log1.record("/A");
        assert_eq!(log2.count("/A"), 1);
        log2.record("/B");
        assert_eq!(log1.len(), 2);
    }

    #[test]
    fn recorded_call_has_timestamp() {
        let log = CallLog::new();
        let before = std::time::Instant::now();
        log.record("/Test");
        let after = std::time::Instant::now();

        let calls = log.calls.lock().unwrap();
        assert!(calls[0].timestamp >= before);
        assert!(calls[0].timestamp <= after);
    }

    #[tokio::test]
    async fn record_service_captures_uri_path() {
        use tower::ServiceExt;

        let log = CallLog::new();
        let layer = RecordLayer::new(log.clone());

        let svc = tower::service_fn(|_req: Request<&str>| async {
            Ok::<_, std::convert::Infallible>(http::Response::new("ok"))
        });

        let mut recorded_svc = layer.layer(svc);

        let req = Request::builder()
            .uri("/apex.rpc.ApexCoordinator/Register")
            .body("")
            .unwrap();

        let _resp = recorded_svc.ready().await.unwrap().call(req).await.unwrap();

        assert_eq!(log.len(), 1);
        assert_eq!(log.methods(), vec!["/apex.rpc.ApexCoordinator/Register"]);
    }

    #[tokio::test]
    async fn record_service_captures_multiple_calls() {
        use tower::ServiceExt;

        let log = CallLog::new();
        let layer = RecordLayer::new(log.clone());

        let svc = tower::service_fn(|_req: Request<&str>| async {
            Ok::<_, std::convert::Infallible>(http::Response::new("ok"))
        });

        let mut recorded_svc = layer.layer(svc);

        for path in [
            "/apex.rpc.ApexCoordinator/Register",
            "/apex.rpc.ApexCoordinator/SendHeartbeat",
            "/apex.rpc.ApexCoordinator/GetSeeds",
            "/apex.rpc.ApexCoordinator/SendHeartbeat",
        ] {
            let req = Request::builder().uri(path).body("").unwrap();
            let _resp = recorded_svc.ready().await.unwrap().call(req).await.unwrap();
        }

        assert_eq!(log.len(), 4);
        assert_eq!(log.count("/apex.rpc.ApexCoordinator/SendHeartbeat"), 2);
        assert!(log.called_before(
            "/apex.rpc.ApexCoordinator/Register",
            "/apex.rpc.ApexCoordinator/GetSeeds"
        ));
    }

    #[tokio::test]
    async fn record_layer_is_clone() {
        let log = CallLog::new();
        let layer = RecordLayer::new(log.clone());
        let _layer2 = layer.clone();
    }
}
