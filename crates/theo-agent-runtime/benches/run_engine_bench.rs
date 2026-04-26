//! REMEDIATION_PLAN T7.4 — Baseline benchmarks for `theo-agent-runtime`.
//!
//! These `criterion` benches lock in a first set of performance numbers
//! for the 4 hot paths called out in the plan. They're deliberately
//! scoped to operations we can drive without a live LLM client:
//!
//! - `event_bus_publish` (T6.1) — log-bounded publish + listener fan-out
//! - `tool_call_dispatch_throughput` (T6.5) — enqueue + dispatch loop
//!   under the refactored lock discipline
//! - `record_session_exit_large_log` (T6.2) — `events_range` / snapshot
//!   on a 10k-event log (proxy for the record-session-exit hot path)
//! - `streaming_delta_batching` (T6.4) — end-to-end `publish` cost of
//!   streaming deltas at different batch sizes
//!
//! Run with `cargo bench -p theo-agent-runtime`.
//! CI may opt out with `--no-default-features`.

use std::path::PathBuf;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::TaskId;
use theo_domain::tool::ToolContext;
use theo_tooling::registry::create_default_registry;

use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::tool_call_manager::ToolCallManager;

// ────────────────────────────────────────────────────────────────────
// T6.1 — EventBus publish throughput
// ────────────────────────────────────────────────────────────────────

fn bench_event_bus_publish(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_bus_publish");

    // Baseline: empty bus (no listeners) — measures the log-bounded
    // insert + clone path.
    group.bench_function("no_listeners", |b| {
        let bus = EventBus::new();
        b.iter(|| {
            bus.publish(black_box(DomainEvent::new(
                EventType::RunInitialized,
                "bench",
                serde_json::json!({}),
            )));
        });
    });

    // With a single lightweight listener — exercises the fan-out loop.
    group.bench_function("one_listener", |b| {
        let bus = EventBus::new();
        let listener = Arc::new(RecordingListener::default());
        bus.subscribe(listener);
        b.iter(|| {
            bus.publish(black_box(DomainEvent::new(
                EventType::RunInitialized,
                "bench",
                serde_json::json!({}),
            )));
        });
    });

    group.finish();
}

// ────────────────────────────────────────────────────────────────────
// T6.5 — ToolCallManager dispatch throughput
//
// Exercises the 3-lock path (entry + exit on `records`, insert on
// `results`) under 1-thread load. The 100-way parallel path is covered
// by `tests/resilience_t7_2.rs::dispatch_under_mutex_contention_100_parallel`.
// ────────────────────────────────────────────────────────────────────

fn bench_tool_call_dispatch_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let mut group = c.benchmark_group("tool_call_dispatch_throughput");
    group.throughput(Throughput::Elements(1));

    // Single enqueue + dispatch per iteration. We use a tool that
    // fails fast (`read` of a non-existent file) to keep each
    // iteration sub-millisecond — the goal is manager overhead, not
    // tool cost.
    group.bench_function("enqueue_dispatch_read_fail", |b| {
        let bus = Arc::new(EventBus::new());
        let manager = ToolCallManager::new(bus);
        let registry = create_default_registry();
        let ctx = ToolContext::test_context(PathBuf::from("/tmp"));

        b.iter(|| {
            rt.block_on(async {
                let call_id = manager.enqueue(
                    TaskId::new("bench"),
                    "read".into(),
                    serde_json::json!({"filePath": "/nonexistent/bench"}),
                );
                let _ = manager
                    .dispatch_and_execute(&call_id, &registry, &ctx)
                    .await;
            });
        });
    });

    group.finish();
}

// ────────────────────────────────────────────────────────────────────
// T6.2 — Large-log query cost (record_session_exit proxy).
//
// `record_session_exit` walks the bus log to aggregate events.
// Benchmark the `events()` snapshot + `events_range(offset, limit)`
// pagination on a pre-populated 10k-event bus.
// ────────────────────────────────────────────────────────────────────

fn bench_record_session_exit_large_log(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_log_query");

    // Prime once, benchmark the read paths repeatedly.
    let bus = EventBus::new();
    for i in 0..10_000 {
        bus.publish(DomainEvent::new(
            EventType::ToolCallQueued,
            &format!("call-{i}"),
            serde_json::json!({}),
        ));
    }

    group.bench_function("events_full_snapshot_10k", |b| {
        b.iter(|| {
            let snap = bus.events();
            black_box(snap.len())
        });
    });

    // Paginated read — a 1000-event window is typical for UI display.
    group.bench_function("events_range_1000_window", |b| {
        b.iter(|| {
            let slice = bus.events_range(5000, 1000);
            black_box(slice.len())
        });
    });

    group.finish();
}

// ────────────────────────────────────────────────────────────────────
// T6.4 — Streaming delta batching.
//
// The streaming path issues many small `ContentDelta` events per LLM
// chunk. Benchmark the bus at several batch sizes so T6.4's eventual
// optimization has a ground truth to beat.
// ────────────────────────────────────────────────────────────────────

fn bench_streaming_delta_batching(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_delta");

    for &batch in &[1usize, 10, 100, 1000] {
        group.throughput(Throughput::Elements(batch as u64));
        group.bench_with_input(
            BenchmarkId::new("publish_batch", batch),
            &batch,
            |b, &batch| {
                let bus = EventBus::new();
                b.iter(|| {
                    for i in 0..batch {
                        bus.publish(DomainEvent::new(
                            EventType::ContentDelta,
                            "stream-bench",
                            serde_json::json!({ "text": format!("chunk-{i}") }),
                        ));
                    }
                });
            },
        );
    }

    group.finish();
}

// ────────────────────────────────────────────────────────────────────
// Test fixtures
// ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct RecordingListener {
    received: std::sync::atomic::AtomicU64,
}

impl theo_agent_runtime::event_bus::EventListener for RecordingListener {
    fn on_event(&self, _event: &DomainEvent) {
        self.received
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

criterion_group!(
    benches,
    bench_event_bus_publish,
    bench_tool_call_dispatch_throughput,
    bench_record_session_exit_large_log,
    bench_streaming_delta_batching,
);
criterion_main!(benches);
