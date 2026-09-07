use pretty_assertions::assert_eq;
use tokio::sync::watch;

use super::StockSpawnWait;
use super::wait_classified_stock_status;
use crate::agent::AgentStatus;

async fn not_found() -> AgentStatus {
    AgentStatus::NotFound
}

#[tokio::test]
async fn wait_returns_completed_from_status_watch() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    tx.send(AgentStatus::Completed(Some("ok".into())))
        .expect("send");
    assert_eq!(
        wait.await.expect("join"),
        StockSpawnWait::Completed("ok".into())
    );
}

#[tokio::test]
async fn wait_maps_errored_and_unavailable_once() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    tx.send(AgentStatus::Errored("secret".into()))
        .expect("send");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildErrored);

    let (tx, rx) = watch::channel(AgentStatus::PendingInit);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    tx.send(AgentStatus::Shutdown).expect("send");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildUnavailable);
}

#[tokio::test]
async fn wait_cancel_returns_before_child_is_terminal() {
    let (_tx, rx) = watch::channel(AgentStatus::Running);
    let (cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    cancel_tx.send(true).expect("cancel");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::Cancelled);
}

#[tokio::test]
async fn interrupted_is_not_terminal_and_completed_still_wins() {
    let (tx, rx) = watch::channel(AgentStatus::Interrupted);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    tokio::task::yield_now().await;
    tx.send(AgentStatus::Completed(Some("ok".into())))
        .expect("send");
    assert_eq!(
        wait.await.expect("join"),
        StockSpawnWait::Completed("ok".into())
    );
}

#[tokio::test]
async fn closed_watch_rereads_status_before_classifying() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, || async {
        AgentStatus::Completed(Some("ok".into()))
    }));
    drop(tx);
    assert_eq!(
        wait.await.expect("join"),
        StockSpawnWait::Completed("ok".into())
    );
}

#[tokio::test]
async fn closed_watch_maps_missing_child_to_unavailable() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, not_found));
    drop(tx);
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildUnavailable);
}

#[tokio::test]
async fn closed_watch_does_not_invent_unavailable_for_non_final_child() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel, || async {
        AgentStatus::Interrupted
    }));
    drop(tx);
    assert_eq!(wait.await.expect("join"), StockSpawnWait::Cancelled);
}
