use pretty_assertions::assert_eq;
use tokio::sync::watch;

use super::StockSpawnWait;
use super::classify_stock_status;
use super::wait_classified_stock_status;
use crate::agent::AgentStatus;

#[test]
fn classify_stock_status_matches_host_spawn_contract() {
    assert_eq!(
        classify_stock_status(AgentStatus::Completed(Some("ok".into()))),
        Some(StockSpawnWait::Completed("ok".into()))
    );
    assert_eq!(
        classify_stock_status(AgentStatus::Completed(None)),
        Some(StockSpawnWait::Completed(String::new()))
    );
    assert_eq!(
        classify_stock_status(AgentStatus::Errored("secret".into())),
        Some(StockSpawnWait::ChildErrored)
    );
    assert_eq!(
        classify_stock_status(AgentStatus::Shutdown),
        Some(StockSpawnWait::ChildUnavailable)
    );
    assert_eq!(
        classify_stock_status(AgentStatus::NotFound),
        Some(StockSpawnWait::ChildUnavailable)
    );
    assert_eq!(classify_stock_status(AgentStatus::PendingInit), None);
    assert_eq!(classify_stock_status(AgentStatus::Running), None);
    assert_eq!(classify_stock_status(AgentStatus::Interrupted), None);
}

#[tokio::test]
async fn wait_returns_completed_from_status_watch() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
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
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
    tx.send(AgentStatus::Errored("secret".into()))
        .expect("send");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildErrored);

    let (tx, rx) = watch::channel(AgentStatus::PendingInit);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
    tx.send(AgentStatus::Shutdown).expect("send");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildUnavailable);
}

#[tokio::test]
async fn wait_cancel_returns_before_child_is_terminal() {
    let (_tx, rx) = watch::channel(AgentStatus::Running);
    let (cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
    cancel_tx.send(true).expect("cancel");
    assert_eq!(wait.await.expect("join"), StockSpawnWait::Cancelled);
}

#[tokio::test]
async fn interrupted_is_not_terminal_and_completed_still_wins() {
    let (tx, rx) = watch::channel(AgentStatus::Interrupted);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
    tokio::task::yield_now().await;
    tx.send(AgentStatus::Completed(Some("ok".into())))
        .expect("send");
    assert_eq!(
        wait.await.expect("join"),
        StockSpawnWait::Completed("ok".into())
    );
}

#[tokio::test]
async fn closed_watch_without_final_status_is_unavailable() {
    let (tx, rx) = watch::channel(AgentStatus::Running);
    let (_cancel_tx, cancel) = watch::channel(false);
    let wait = tokio::spawn(wait_classified_stock_status(rx, cancel));
    drop(tx);
    assert_eq!(wait.await.expect("join"), StockSpawnWait::ChildUnavailable);
}
