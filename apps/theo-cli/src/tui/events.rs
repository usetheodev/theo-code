//! Event task — bridges async broadcast channel to TUI message loop.
//!
//! Batches events every 16ms (~60fps) to prevent tearing during token bursts.
//! Handles RecvError::Lagged explicitly with Msg::EventsLost.

use std::time::Duration;

use tokio::sync::{broadcast, mpsc};

use theo_domain::event::DomainEvent;

use super::app::Msg;

/// Drain broadcast events and forward as batched Msg to the render loop.
///
/// Runs until the broadcast sender is dropped (EventBus deallocated).
pub async fn event_loop(
    mut rx: broadcast::Receiver<DomainEvent>,
    tx: mpsc::Sender<Msg>,
) {
    let mut batch = Vec::new();
    let mut interval = tokio::time::interval(Duration::from_millis(16));
    // Don't deliver "burst" ticks — just skip missed intervals
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        batch.push(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        if tx.send(Msg::EventsLost(n)).await.is_err() {
                            break; // render loop closed
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // EventBus dropped — flush remaining and exit
                        if !batch.is_empty() {
                            let _ = tx.send(Msg::DomainEventBatch(std::mem::take(&mut batch))).await;
                        }
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    let events = std::mem::take(&mut batch);
                    if tx.send(Msg::DomainEventBatch(events)).await.is_err() {
                        break; // render loop closed
                    }
                }
            }
        }
    }
}
