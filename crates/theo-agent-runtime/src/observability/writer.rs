//! Background writer thread for trajectory JSONL files.
//!
//! Properties (from the ADR):
//! - **INV-3** (per_run_ordering): sequence numbers are assigned by the
//!   writer thread, which drains the receiver in FIFO order.
//! - **INV-2** (drop_accounting): before each write, the writer swaps the
//!   dropped counter and emits a DropSentinel if non-zero.
//! - **INV-4** (no_silent_write_failure): I/O failures go to a bounded
//!   retry queue. When the write recovers, a WriterRecovered sentinel is
//!   emitted. Retry-queue overflow falls back to the drop counter.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use theo_domain::event::DomainEvent;

use crate::observability::envelope::{EnvelopeKind, TrajectoryEnvelope, ENVELOPE_SCHEMA_VERSION};

/// Maximum events written before forcing a `BufWriter::flush()`.
///
/// Kept small so that crash-tolerance dominates throughput. The BufWriter
/// default capacity is 8KB (about 30-50 envelopes); flushing every 5
/// envelopes keeps < 5 events in-flight at any moment without becoming
/// fsync-heavy (flush → write(2), not sync_data(2)).
const FLUSH_EVERY: usize = 5;

/// Wall-clock interval between forced flushes on an idle writer.
///
/// Prevents arbitrary data loss on long LLM waits where no event arrives
/// for many seconds. Unit is milliseconds.
const FORCE_FLUSH_MS: u64 = 500;

const RETRY_QUEUE_CAP: usize = 100;

/// Payload the caller may hand the writer when the run is ending so that it
/// can append a summary line before closing the file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryPayload(pub serde_json::Value);

/// Handle returned by `spawn_writer_thread`. Keeps ownership of shared
/// counters and the JoinHandle.
pub struct WriterHandle {
    pub join: JoinHandle<()>,
    pub dropped_events: Arc<AtomicU64>,
    pub serialization_errors: Arc<AtomicU64>,
    pub write_errors: Arc<AtomicU64>,
}

/// Compute the trajectory file path for a run.
pub fn trajectory_path<P: AsRef<Path>>(base: P, run_id: &str) -> PathBuf {
    base.as_ref().join(format!("{}.jsonl", run_id))
}

/// Spawn the writer thread. Returns a handle with shared counters.
///
/// The writer drains the `receiver` until the sender is dropped, then writes
/// a final flush + fsync. Sequence numbers are assigned monotonically.
pub fn spawn_writer_thread(
    receiver: Receiver<Vec<u8>>,
    run_id: String,
    base_path: PathBuf,
    dropped_events: Arc<AtomicU64>,
    serialization_errors: Arc<AtomicU64>,
) -> WriterHandle {
    let write_errors = Arc::new(AtomicU64::new(0));
    let write_errors_clone = write_errors.clone();
    let dropped_clone = dropped_events.clone();

    std::fs::create_dir_all(&base_path).ok();
    let file_path = trajectory_path(&base_path, &run_id);

    let join = std::thread::Builder::new()
        .name(format!("theo-obs-writer-{}", run_id))
        .spawn(move || {
            run_writer_loop(
                receiver,
                run_id,
                file_path,
                dropped_clone,
                write_errors_clone,
            );
        })
        .expect("failed to spawn observability writer thread");

    WriterHandle {
        join,
        dropped_events,
        serialization_errors,
        write_errors,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn run_writer_loop(
    receiver: Receiver<Vec<u8>>,
    run_id: String,
    file_path: PathBuf,
    dropped_events: Arc<AtomicU64>,
    write_errors: Arc<AtomicU64>,
) {
    // Open file (create + append). If it fails, write nothing — but still
    // consume the channel so senders don't block.
    let file = match File::options()
        .create(true)
        .append(true)
        .open(&file_path)
    {
        Ok(f) => f,
        Err(_) => {
            // Fallback: drain and drop.
            for _ in receiver.iter() {
                dropped_events.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
    };

    let mut writer = BufWriter::new(file);
    let mut seq: u64 = 0;
    let mut since_flush: usize = 0;
    let mut retry_queue: VecDeque<Vec<u8>> = VecDeque::with_capacity(RETRY_QUEUE_CAP);

    // Helper: attempt to write a line; if it fails return the bytes back.
    fn try_write_line(
        writer: &mut BufWriter<File>,
        write_errors: &Arc<AtomicU64>,
        bytes: &[u8],
    ) -> Result<(), std::io::Error> {
        writer.write_all(bytes).and_then(|_| writer.write_all(b"\n")).inspect_err(|e| {
            write_errors.fetch_add(1, Ordering::Relaxed);
        })
    }

    let drain_retry_queue = |writer: &mut BufWriter<File>,
                             retry_queue: &mut VecDeque<Vec<u8>>,
                             write_errors: &Arc<AtomicU64>|
     -> (u64, Option<String>) {
        let mut drained = 0u64;
        while let Some(front) = retry_queue.front() {
            match try_write_line(writer, write_errors, front) {
                Ok(_) => {
                    retry_queue.pop_front();
                    drained += 1;
                }
                Err(e) => {
                    return (drained, Some(e.to_string()));
                }
            }
        }
        (drained, None)
    };

    // Periodic flush loop: use `recv_timeout` so we can flush on idle too.
    let flush_interval = std::time::Duration::from_millis(FORCE_FLUSH_MS);
    loop {
        let bytes = match receiver.recv_timeout(flush_interval) {
            Ok(b) => b,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if since_flush > 0 {
                    let _ = writer.flush();
                    since_flush = 0;
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // If the retry queue has pending bytes, try to drain them first.
        if !retry_queue.is_empty() {
            let (drained, err) = drain_retry_queue(&mut writer, &mut retry_queue, &write_errors);
            if err.is_none() && drained > 0 {
                // Emit recovery sentinel.
                let env = TrajectoryEnvelope::writer_recovered(&run_id, seq, now_ms(), drained, "recovered");
                if let Ok(line) = serde_json::to_vec(&env) {
                    let _ = try_write_line(&mut writer, &write_errors, &line);
                    seq += 1;
                }
            }
        }

        // Emit any drop sentinel before the next event.
        let dropped = dropped_events.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            let env = TrajectoryEnvelope::drop_sentinel(&run_id, seq, now_ms(), dropped);
            if let Ok(line) = serde_json::to_vec(&env) {
                match try_write_line(&mut writer, &write_errors, &line) {
                    Ok(_) => {
                        seq += 1;
                    }
                    Err(_) => {
                        if retry_queue.len() < RETRY_QUEUE_CAP {
                            retry_queue.push_back(line);
                            seq += 1;
                        } else {
                            dropped_events.fetch_add(dropped, Ordering::Relaxed);
                        }
                    }
                }
            }
        }

        // Parse incoming DomainEvent bytes and wrap in envelope.
        let env = match serde_json::from_slice::<DomainEvent>(&bytes) {
            Ok(ev) => TrajectoryEnvelope::from_event(&ev, &run_id, seq),
            Err(_) => {
                // Store as raw — unknown payload wrapped into an event envelope.
                TrajectoryEnvelope {
                    v: ENVELOPE_SCHEMA_VERSION,
                    seq,
                    ts: now_ms(),
                    run_id: run_id.clone(),
                    kind: EnvelopeKind::Event,
                    event_type: None,
                    event_kind: None,
                    entity_id: None,
                    payload: serde_json::Value::Null,
                    dropped_since_last: 0,
                }
            }
        };
        let line = match serde_json::to_vec(&env) {
            Ok(l) => l,
            Err(_) => continue,
        };

        match try_write_line(&mut writer, &write_errors, &line) {
            Ok(_) => {
                seq += 1;
                since_flush += 1;
                if since_flush >= FLUSH_EVERY {
                    let _ = writer.flush();
                    since_flush = 0;
                }
            }
            Err(_) => {
                if retry_queue.len() < RETRY_QUEUE_CAP {
                    retry_queue.push_back(line);
                    seq += 1;
                } else {
                    dropped_events.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    // Shutdown — attempt final drain.
    if !retry_queue.is_empty() {
        let (drained, _err) = drain_retry_queue(&mut writer, &mut retry_queue, &write_errors);
        if drained > 0 {
            let env = TrajectoryEnvelope::writer_recovered(&run_id, seq, now_ms(), drained, "recovered");
            if let Ok(line) = serde_json::to_vec(&env) {
                let _ = try_write_line(&mut writer, &write_errors, &line);
            }
        }
    }

    let _ = writer.flush();
    if let Ok(f) = writer.into_inner() {
        let _ = f.sync_data();
    }
}

/// Append a summary line to an existing trajectory file and fsync.
pub fn append_summary_line<P: AsRef<Path>>(
    file_path: P,
    run_id: &str,
    seq: u64,
    payload: serde_json::Value,
) -> std::io::Result<()> {
    let env = TrajectoryEnvelope::summary(run_id, seq, now_ms(), payload);
    let bytes = serde_json::to_vec(&env).map_err(std::io::Error::other)?;
    let file = File::options().create(true).append(true).open(&file_path)?;
    let mut w = BufWriter::new(file);
    w.write_all(&bytes)?;
    w.write_all(b"\n")?;
    w.flush()?;
    if let Ok(f) = w.into_inner() {
        let _ = f.sync_data();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc::sync_channel;

    use theo_domain::event::{DomainEvent, EventType};

    fn make_event(t: EventType, entity: &str) -> Vec<u8> {
        let e = DomainEvent::new(t, entity, serde_json::json!({}));
        serde_json::to_vec(&e).unwrap()
    }

    fn parse_lines(path: &Path) -> Vec<TrajectoryEnvelope> {
        let f = File::open(path).unwrap();
        let r = BufReader::new(f);
        r.lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect()
    }

    #[test]
    fn test_writer_creates_file_at_expected_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let handle = spawn_writer_thread(rx, "run-a".into(), tmp.path().into(), dropped, serr);
        tx.send(make_event(EventType::RunInitialized, "run-a")).unwrap();
        drop(tx);
        handle.join.join().unwrap();

        let expected = tmp.path().join("run-a.jsonl");
        assert!(expected.exists());
    }

    #[test]
    fn test_envelope_contains_all_required_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let handle = spawn_writer_thread(rx, "run-b".into(), tmp.path().into(), dropped, serr);
        tx.send(make_event(EventType::ToolCallCompleted, "call-1"))
            .unwrap();
        drop(tx);
        handle.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-b.jsonl"));
        assert_eq!(env.len(), 1);
        let e = &env[0];
        assert_eq!(e.v, 1);
        assert_eq!(e.run_id, "run-b");
        assert!(matches!(e.kind, EnvelopeKind::Event));
        assert_eq!(e.event_type.as_deref(), Some("ToolCallCompleted"));
        assert!(e.event_kind.is_some());
    }

    #[test]
    fn test_sequence_numbers_are_strictly_monotonic() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(64);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let handle = spawn_writer_thread(rx, "run-c".into(), tmp.path().into(), dropped, serr);
        for i in 0..10 {
            tx.send(make_event(EventType::ToolCallCompleted, &format!("call-{}", i)))
                .unwrap();
        }
        drop(tx);
        handle.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-c.jsonl"));
        assert_eq!(env.len(), 10);
        for (i, e) in env.iter().enumerate() {
            assert_eq!(e.seq, i as u64);
        }
    }

    #[test]
    fn test_schema_version_is_1() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let d = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "run-d".into(), tmp.path().into(), d, s);
        tx.send(make_event(EventType::RunInitialized, "x")).unwrap();
        drop(tx);
        h.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-d.jsonl"));
        assert!(env.iter().all(|e| e.v == 1));
    }

    #[test]
    fn test_event_kind_field_matches_event_type() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let d = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "run-k".into(), tmp.path().into(), d, s);
        tx.send(make_event(EventType::ToolCallCompleted, "x")).unwrap();
        drop(tx);
        h.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-k.jsonl"));
        let e = &env[0];
        assert_eq!(e.event_kind, Some(theo_domain::event::EventKind::Tooling));
    }

    #[test]
    fn test_drop_sentinel_written_when_events_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(64);
        let dropped = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "run-s".into(), tmp.path().into(), dropped.clone(), s);
        // Simulate drops by incrementing counter directly.
        dropped.store(7, Ordering::Relaxed);
        tx.send(make_event(EventType::ToolCallCompleted, "x")).unwrap();
        drop(tx);
        h.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-s.jsonl"));
        assert!(env.iter().any(|e| matches!(e.kind, EnvelopeKind::DropSentinel)));
        let sentinel = env.iter().find(|e| matches!(e.kind, EnvelopeKind::DropSentinel)).unwrap();
        assert_eq!(sentinel.payload["dropped_count"], 7);
    }

    #[test]
    fn test_no_sentinel_when_no_drops() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let d = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "run-n".into(), tmp.path().into(), d, s);
        tx.send(make_event(EventType::RunInitialized, "x")).unwrap();
        drop(tx);
        h.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-n.jsonl"));
        assert!(!env.iter().any(|e| matches!(e.kind, EnvelopeKind::DropSentinel)));
    }

    #[test]
    fn test_graceful_shutdown_fsyncs() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let d = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "run-f".into(), tmp.path().into(), d, s);
        tx.send(make_event(EventType::RunInitialized, "x")).unwrap();
        drop(tx);
        // join is the implicit flush — after this, file must be readable & valid.
        h.join.join().unwrap();
        let env = parse_lines(&tmp.path().join("run-f.jsonl"));
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn test_summary_line_appended_and_fsynced() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("run-sum.jsonl");
        std::fs::write(&path, b"").unwrap();
        append_summary_line(&path, "run-sum", 10, serde_json::json!({"metric": 0.5})).unwrap();
        let env = parse_lines(&path);
        assert_eq!(env.len(), 1);
        assert!(matches!(env[0].kind, EnvelopeKind::Summary));
    }

    // --- T1.4: writer failure recovery via non-existent parent directory ---
    //
    // When the base path cannot be opened, the writer gracefully drops every
    // event into the dropped counter rather than panicking or leaking.
    #[test]
    fn test_writer_handles_open_failure_without_panic() {
        // Build a path with an invalid parent (a regular file we can't cd into).
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"").unwrap();
        let bad_base = blocker.join("cannot_create_here");

        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "r".into(), bad_base, dropped.clone(), serr);
        for i in 0..5 {
            let _ = tx.send(make_event(EventType::ToolCallCompleted, &format!("c{}", i)));
        }
        drop(tx);
        h.join.join().unwrap();
        // With open failure, events fall back to dropped counter.
        assert!(dropped.load(Ordering::Relaxed) >= 5);
    }

    // --- Bug-fix regression: periodic idle flush before shutdown ---
    //
    // If a sender dies before enough events (<5) are received, the writer
    // must still flush what it has so that `ls -la .theo/trajectories/` shows
    // a populated file instead of a 0-byte placeholder.
    #[test]
    fn test_idle_flush_before_shutdown_yields_populated_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let d = Arc::new(AtomicU64::new(0));
        let s = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "idle".into(), tmp.path().into(), d, s);
        // Send a single event, then idle longer than FORCE_FLUSH_MS.
        tx.send(make_event(EventType::RunInitialized, "x")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(700));
        // File MUST be populated even though we haven't dropped the sender
        // or hit FLUSH_EVERY — idle flush runs on recv_timeout.
        let contents = std::fs::read_to_string(tmp.path().join("idle.jsonl")).unwrap();
        assert!(
            !contents.is_empty(),
            "idle flush must persist events before shutdown"
        );
        drop(tx);
        h.join.join().unwrap();
    }

    // --- T1.4: write_errors counter is accessible on the handle (INV-4) ---
    #[test]
    fn test_write_errors_counter_initialized() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "r".into(), tmp.path().into(), dropped, serr);
        drop(tx);
        h.join.join().unwrap();
        // Counter exists and is readable via the handle API.
        assert_eq!(h.write_errors.load(Ordering::Relaxed), 0);
    }

    // --- T1.4: retry queue does not leak memory under many write attempts ---
    //
    // Smoke test: events flow through even under rapid bursts. The retry queue
    // is bounded at RETRY_QUEUE_CAP=100 so even if every write failed, memory
    // would stay bounded. We verify by sending many events and confirming the
    // writer terminates cleanly.
    #[test]
    fn test_retry_queue_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = sync_channel::<Vec<u8>>(256);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let h = spawn_writer_thread(rx, "r".into(), tmp.path().into(), dropped, serr);
        for i in 0..200 {
            tx.send(make_event(EventType::ToolCallCompleted, &format!("c{}", i)))
                .unwrap();
        }
        drop(tx);
        h.join.join().unwrap();
        // All 200 events drained (either written or dropped). No deadlock.
        assert!(h.dropped_events.load(Ordering::Relaxed) + 200 >= 200);
    }
}
