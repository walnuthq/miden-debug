//! Headless function tracing of recorded replay snapshots.
//!
//! `miden-debug --trace <snapshot>` re-executes a recorded run (see
//! [`ReplaySnapshot`](crate::exec::ReplaySnapshot)) without any UI and prints every function the
//! program executes, in order, followed by a per-function cycle summary. The function names come
//! from the assembly-op debug info embedded in the executed MAST: segments executed by code
//! without debug info are attributed to `<unknown>`.

use std::{collections::BTreeMap, io::Write, path::Path, sync::Arc};

use miden_assembly::DefaultSourceManager;
use miden_assembly_syntax::diagnostics::{IntoDiagnostic, Report};
use miden_processor::ExecutionError;

use crate::exec::{DebugExecutor, ExecutionConfig, Executor, ReplaySnapshot};

/// The procedure name used for cycles executed by code without assembly-op debug info.
const UNKNOWN_PROCEDURE: &str = "<unknown>";

/// One function-context transition observed during execution.
#[derive(Debug)]
pub struct TraceEntry {
    /// The cycle at which execution entered this procedure context.
    pub cycle: usize,
    /// The call-frame depth at the transition (non-zero only for code compiled with frame
    /// tracing, e.g. by `midenc`; plain MASM such as the transaction kernel reports depth 0).
    pub depth: usize,
    /// The fully-qualified procedure name.
    pub procedure: String,
}

/// Per-procedure aggregate over the whole execution.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcedureTotals {
    /// Cycles spent executing ops attributed to this procedure (self time, not inclusive of
    /// procedures it transitions into).
    pub cycles: usize,
    /// The number of times execution transitioned into this procedure.
    pub entries: usize,
}

/// A chronological function trace of a single execution.
pub struct FunctionTrace {
    entries: Vec<TraceEntry>,
    totals: BTreeMap<String, ProcedureTotals>,
    total_cycles: usize,
    error: Option<ExecutionError>,
}

impl FunctionTrace {
    /// Execute `executor` to completion, recording every procedure-context transition.
    ///
    /// If execution fails, the trace collected up to the failure point is returned and the
    /// error is available via [FunctionTrace::error].
    pub fn collect(executor: &mut DebugExecutor) -> Self {
        let mut entries = Vec::new();
        let mut totals: BTreeMap<String, ProcedureTotals> = BTreeMap::new();
        let mut total_cycles = 0usize;
        let mut current: Option<String> = None;
        let mut error = None;

        while !executor.stopped {
            let previous_cycle = executor.cycle;
            match executor.step() {
                Ok(_) => {}
                Err(err) => {
                    error = Some(err);
                    break;
                }
            }

            let cycle_delta = executor.cycle.saturating_sub(previous_cycle);
            if cycle_delta == 0 {
                continue;
            }
            total_cycles += cycle_delta;

            let procedure = executor.current_proc.as_deref().unwrap_or(UNKNOWN_PROCEDURE);
            if current.as_deref() != Some(procedure) {
                current = Some(procedure.to_string());
                let depth = executor.callstack.frames().len().saturating_sub(1);
                entries.push(TraceEntry {
                    cycle: previous_cycle,
                    depth,
                    procedure: procedure.to_string(),
                });
                totals.entry(procedure.to_string()).or_default().entries += 1;
            }
            totals.entry(procedure.to_string()).or_default().cycles += cycle_delta;
        }

        Self {
            entries,
            totals,
            total_cycles,
            error,
        }
    }

    /// The chronological procedure-context transitions.
    pub fn entries(&self) -> &[TraceEntry] {
        &self.entries
    }

    /// Per-procedure aggregates, keyed by procedure name.
    pub fn totals(&self) -> &BTreeMap<String, ProcedureTotals> {
        &self.totals
    }

    /// Total cycles executed.
    pub fn total_cycles(&self) -> usize {
        self.total_cycles
    }

    /// The execution error, if the traced run failed.
    pub fn error(&self) -> Option<&ExecutionError> {
        self.error.as_ref()
    }

    /// Write the trace as human-readable text: the chronological transition log followed by a
    /// per-function summary sorted by self-cycles.
    pub fn write_text(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(
            out,
            "Function trace: {} transition(s), {} unique function(s), {} cycle(s)",
            self.entries.len(),
            self.totals.len(),
            self.total_cycles
        )?;
        writeln!(out)?;
        writeln!(out, "{:>10}  function", "cycle")?;
        for entry in &self.entries {
            writeln!(
                out,
                "{:>10}  {:indent$}{}",
                entry.cycle,
                "",
                entry.procedure,
                indent = entry.depth * 2
            )?;
        }

        writeln!(out)?;
        writeln!(out, "Functions by self-cycles:")?;
        writeln!(out, "{:>10}  {:>7}  function", "cycles", "entries")?;
        let mut by_cycles: Vec<_> = self.totals.iter().collect();
        by_cycles.sort_by(|a, b| b.1.cycles.cmp(&a.1.cycles).then_with(|| a.0.cmp(b.0)));
        for (procedure, totals) in by_cycles {
            writeln!(out, "{:>10}  {:>7}  {}", totals.cycles, totals.entries, procedure)?;
        }

        Ok(())
    }
}

/// Trace a recorded replay snapshot: re-execute it headlessly and print every function executed.
pub fn run(snapshot_path: &Path) -> Result<(), Report> {
    let snapshot = ReplaySnapshot::read_from_file(snapshot_path)
        .map_err(|err| Report::msg(format!("{err}")))?;

    eprintln!(
        "Tracing replay snapshot {} ({} event(s), {} forest(s))",
        snapshot_path.display(),
        snapshot.event_log.len(),
        snapshot.mast_forests.len()
    );

    // The snapshot carries no source files; function names come from the embedded debug info.
    let source_manager = Arc::new(DefaultSourceManager::default());
    let executor = Executor::from_config(ExecutionConfig {
        inputs: snapshot.stack_inputs,
        advice_inputs: snapshot.advice_inputs,
        options: snapshot.options,
    });
    let mut debug_executor = executor.into_debug_with_replay(
        &snapshot.program,
        source_manager,
        snapshot.mast_forests,
        snapshot.event_log.into(),
    );

    let trace = FunctionTrace::collect(&mut debug_executor);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    trace.write_text(&mut out).into_diagnostic()?;
    out.flush().into_diagnostic()?;

    if let Some(err) = trace.error() {
        return Err(Report::msg(format!(
            "execution failed at cycle {}: {err}",
            debug_executor.cycle
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use miden_assembly::DefaultSourceManager;

    use super::*;

    /// The trace records transitions between procedure contexts, with per-procedure cycle
    /// totals that add up to the executed cycle count.
    #[test]
    fn traces_procedure_transitions() {
        let source_manager = Arc::new(DefaultSourceManager::default());
        let program = miden_assembly::Assembler::new(source_manager.clone())
            .assemble_program("proc foo push.1 push.2 add drop end begin exec.foo push.3 drop end")
            .expect("failed to assemble test program");

        let executor = Executor::from_config(ExecutionConfig::default());
        let mut debug_executor = executor.into_debug(&program, source_manager);
        let trace = FunctionTrace::collect(&mut debug_executor);

        assert!(trace.error().is_none(), "execution failed: {:?}", trace.error());
        assert!(!trace.entries().is_empty(), "expected at least one transition");
        assert!(
            trace.totals().keys().any(|name| name.ends_with("foo")),
            "expected a transition into `foo`, got: {:?}",
            trace.totals().keys().collect::<Vec<_>>()
        );
        let summed: usize = trace.totals().values().map(|t| t.cycles).sum();
        assert_eq!(summed, trace.total_cycles(), "per-function cycles must sum to the total");
    }
}
