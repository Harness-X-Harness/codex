use super::SpawnWaits;
use codex_protocol::ThreadId;

#[test]
fn remember_replaces_and_cancels_the_previous_wait() {
    let waits = SpawnWaits::default();
    let thread_id = ThreadId::from_u128(70);
    let first = waits.remember(thread_id);
    let second = waits.remember(thread_id);
    assert!(*first.rx.borrow());
    assert!(!*second.rx.borrow());
    waits.cancel(thread_id);
    assert!(*second.rx.borrow());
}

#[test]
fn forget_cancels_and_drops_only_that_wait() {
    let waits = SpawnWaits::default();
    let thread_id = ThreadId::from_u128(71);
    let token = waits.remember(thread_id);
    waits.forget(&token);
    assert!(*token.rx.borrow());
    let next = waits.remember(thread_id);
    assert!(!*next.rx.borrow());
}

#[test]
fn forget_does_not_drop_a_later_wait() {
    let waits = SpawnWaits::default();
    let thread_id = ThreadId::from_u128(72);
    let first = waits.remember(thread_id);
    let second = waits.remember(thread_id);
    waits.forget(&first);
    assert!(!*second.rx.borrow());
    waits.cancel(thread_id);
    assert!(*second.rx.borrow());
}
