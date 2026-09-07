use super::SpawnWaits;
use codex_protocol::ThreadId;

#[test]
fn remember_replaces_and_cancels_the_previous_wait() {
    let waits = SpawnWaits::default();
    let thread_id = ThreadId::from_u128(70);
    let first = waits.remember(thread_id);
    let second = waits.remember(thread_id);
    assert!(*first.borrow());
    assert!(!*second.borrow());
    waits.cancel(thread_id);
    assert!(*second.borrow());
}

#[test]
fn forget_cancels_and_drops_the_wait() {
    let waits = SpawnWaits::default();
    let thread_id = ThreadId::from_u128(71);
    let token = waits.remember(thread_id);
    waits.forget(thread_id);
    assert!(*token.borrow());
    let next = waits.remember(thread_id);
    assert!(!*next.borrow());
}
