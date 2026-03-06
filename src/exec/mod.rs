mod config;
#[cfg(feature = "dap")]
mod dap;
#[cfg(feature = "dap")]
mod dap_client;
mod diagnostic;
mod executor;
mod host;
mod program;
mod state;
mod trace;
mod trace_event;

pub use self::{
    config::ExecutionConfig,
    diagnostic::{DiagnosticExecutor, DiagnosticExecutorFactory},
    executor::Executor,
    host::DebuggerHost,
    program::{ProgramExecutor, ProgramExecutorFactory},
    state::DebugExecutor,
    trace::{ExecutionTrace, TraceHandler},
    trace_event::TraceEvent,
};

#[cfg(feature = "dap")]
pub use self::dap::{DapConfig, DapExecutor, DapExecutorFactory};
#[cfg(feature = "dap")]
pub use self::dap_client::{DapClient, DapStopReason, SCOPE_MEMORY, SCOPE_STACK};
