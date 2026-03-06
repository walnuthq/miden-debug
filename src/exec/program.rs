use miden_processor::{
    ExecutionError, ExecutionOptions, ExecutionOutput, FutureMaybeSend, Host, Program, StackInputs,
    advice::AdviceInputs,
};

/// A program executor extension point used by the tx-debugging helpers.
pub trait ProgramExecutor {
    fn execute<H: Host + Send>(
        self,
        program: &Program,
        host: &mut H,
    ) -> impl FutureMaybeSend<Result<ExecutionOutput, ExecutionError>>;
}

/// A factory for constructing program executors with the same input envelope.
pub trait ProgramExecutorFactory {
    type Executor: ProgramExecutor;

    fn create_executor(
        stack_inputs: StackInputs,
        advice_inputs: AdviceInputs,
        options: ExecutionOptions,
    ) -> Self::Executor;
}
