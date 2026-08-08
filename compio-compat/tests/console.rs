//! Assert that the compatibility layer reports the future it executes to
//! [`tokio-console`].
//!
//! [`tokio-console`]: https://github.com/tokio-rs/console
#![cfg(feature = "console")]

use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use compio_compat::{Adapter, RuntimeCompat};
use compio_runtime::Runtime;
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

/// What the console tells the tasks of a runtime apart by.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Task {
    kind: String,
    /// Displayed in a column of its own, when present.
    name: Option<String>,
}

/// A subscriber recording the tasks `console-subscriber` would report.
///
/// `compio-executor` asserts what the spans hold; this only has to tell which
/// of them the compatibility layer creates.
#[derive(Debug, Default, Clone)]
struct Recorder(Arc<Mutex<Vec<Task>>>);

impl Recorder {
    fn tasks(&self) -> Vec<Task> {
        self.0.lock().unwrap().clone()
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        meta.name() == "runtime.spawn"
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let mut task = Task::default();
        attrs.record(&mut TaskVisitor(&mut task));

        let mut tasks = self.0.lock().unwrap();
        tasks.push(task);
        Id::from_u64(tasks.len() as u64)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

struct TaskVisitor<'a>(&'a mut Task);

impl Visit for TaskVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "kind" => self.0.kind = value.to_owned(),
            "task.name" => self.0.name = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn Debug) {}
}

async fn test_impl<A: Adapter>() {
    let recorder = Recorder::default();
    let _guard = tracing::subscriber::set_default(recorder.clone());

    let runtime = RuntimeCompat::<A>::new(Runtime::new().unwrap()).unwrap();
    let answer = runtime
        .execute(async {
            compio_runtime::spawn(std::future::ready(())).await.unwrap();
            42
        })
        .await;
    assert_eq!(answer, 42);

    let tasks = recorder.tasks();
    let executed: Vec<_> = tasks
        .iter()
        .filter(|it| it.name.as_deref() == Some("execute"))
        .collect();
    assert_eq!(executed.len(), 1, "one task per executed future: {tasks:?}");
    assert_eq!(
        executed[0].kind, "block_on",
        "the console has no kind of its own for a future executed this way, so the name is what \
         tells it apart from one the runtime blocks on"
    );
    assert!(
        tasks.iter().any(|it| it.kind == "task"),
        "the tasks it spawns are reported alongside it: {tasks:?}"
    );
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio() {
    test_impl::<compio_compat::TokioAdapter>().await;
}

#[cfg(feature = "futures")]
#[test]
fn futures() {
    futures_executor::block_on(async {
        test_impl::<compio_compat::FuturesAdapter>().await;
    })
}
