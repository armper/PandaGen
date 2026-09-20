//! The executor, driven against `SimulatedKernel` instead of a mock.
//!
//! Every other test in this crate uses an in-file `MockKernel` whose
//! `receive_message` *fabricates* a stage response, correlation id and all.
//! That models a running handler. The real kernel has no handler behind a
//! channel and no dispatch loop, so nothing had ever checked what
//! `execute_stage_once` does when the thing on the other end is real.

use core_types::ServiceId;
use kernel_api::KernelApi;
use lifecycle::CancellationToken;
use pipeline::{PayloadSchemaId, PayloadSchemaVersion, PipelineSpec, StageSpec, TypedPayload};
use services_pipeline_executor::{ExecutorError, PipelineExecutor};
use sim_kernel::SimulatedKernel;

/// A channel in `SimulatedKernel` is a single queue, so `execute_stage_once`
/// sends its request and then receives *that same request* straight back --
/// there is nobody else on the channel to consume it. The correlation check
/// catches it, which is the one piece of luck here, but the consequence
/// stands: no pipeline can complete a single stage against the real kernel,
/// and every passing test in this crate passes because its mock answers.
#[test]
fn a_stage_cannot_complete_against_the_real_kernel() {
    let mut kernel = SimulatedKernel::new();
    let handler = ServiceId::new();
    let channel = kernel.create_channel().unwrap();
    kernel.register_service(handler, channel).unwrap();

    let spec = PipelineSpec::new(
        "one-stage".to_string(),
        PayloadSchemaId::new("in"),
        PayloadSchemaId::new("out"),
    )
    .add_stage(StageSpec::new(
        "only".to_string(),
        handler,
        "stage.invoke".to_string(),
        PayloadSchemaId::new("in"),
        PayloadSchemaId::new("out"),
    ));

    let input = TypedPayload::new(
        PayloadSchemaId::new("in"),
        PayloadSchemaVersion::new(1, 0),
        b"{\"value\":1}".to_vec(),
    );

    let mut executor = PipelineExecutor::new();
    let result = executor.execute(&mut kernel, &spec, input, CancellationToken::none());

    let err = result.expect_err(
        "a stage completed against the real kernel. If the executor has since \
         gained a handler on the other end of the channel, this test should \
         assert the output instead of the failure.",
    );
    match err {
        ExecutorError::HandlerNotFound(message) => {
            assert!(
                message.contains("nothing \nis serving") || message.contains("nothing is serving"),
                "the error must say what happened, not blame a handler that \
                 does not exist: {message}"
            );
        }
        other => panic!(
            "expected the loopback to be reported as a missing handler, got: \
             {other:?}"
        ),
    }
}
