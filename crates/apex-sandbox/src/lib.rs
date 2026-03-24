//! Sandboxed execution for APEX — process isolation, shared-memory bitmaps,
//! and optional Firecracker microVM support.

pub mod bitmap;
pub mod firecracker;
pub mod javascript;
pub mod process;
pub mod python;
pub mod ruby;
pub mod rust_test;
pub mod sancov_rt;
pub mod shim;
pub mod shm;

pub use firecracker::FirecrackerSandbox;
pub use javascript::JavaScriptTestSandbox;
pub use process::ProcessSandbox;
pub use python::PythonTestSandbox;
pub use ruby::RubyTestSandbox;
pub use rust_test::RustTestSandbox;
