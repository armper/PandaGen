//! One dead sink must not silence the others.
//!
//! This is C4 (Phase 314) in a second crate: the same `for .. { sink.send()? }`
//! that took the whole remote UI down for every viewer because one viewer
//! closed its window.

use developer_sdk::{DebuggerError, DebuggerHost, TraceEvent, TraceSink};
use std::cell::RefCell;
use std::rc::Rc;

struct DeadSink;

impl TraceSink for DeadSink {
    fn send(&mut self, _event: TraceEvent) -> Result<(), DebuggerError> {
        Err(DebuggerError::Encode("peer gone".to_string()))
    }
}

struct LiveSink(Rc<RefCell<usize>>);

impl TraceSink for LiveSink {
    fn send(&mut self, _event: TraceEvent) -> Result<(), DebuggerError> {
        *self.0.borrow_mut() += 1;
        Ok(())
    }
}

fn event(n: u64) -> TraceEvent {
    TraceEvent {
        timestamp_ns: n,
        category: "test".to_string(),
        message: format!("event {n}"),
    }
}

#[test]
fn one_dead_sink_does_not_silence_every_sink_behind_it() {
    let delivered = Rc::new(RefCell::new(0usize));
    let mut host = DebuggerHost::new();
    // Registration order decided whether you got any tracing at all.
    host.add_sink(Box::new(DeadSink));
    host.add_sink(Box::new(LiveSink(delivered.clone())));

    for n in 0..10 {
        let _ = host.publish(event(n));
    }

    assert_eq!(
        *delivered.borrow(),
        10,
        "the live sink received {} of 10 events",
        delivered.borrow()
    );
    assert_eq!(
        host.sink_count(),
        1,
        "the dead sink must be dropped, not retried for ever"
    );
}
