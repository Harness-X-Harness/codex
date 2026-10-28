use super::*;
use pretty_assertions::assert_eq;

#[test]
fn identity_fence_retains_across_attempts_but_releases_abandoned_reservations() {
    let fence = SearchIdentityFence::default();
    fence.reserve_search("retained", /*attempt_id*/ 1).unwrap();
    fence.validate_and_record("retained", Some(1)).unwrap();
    fence.retain_search("retained", /*attempt_id*/ 1);
    fence.abandon_search("retained", /*attempt_id*/ 1);
    assert!(fence.reserve_search("retained", /*attempt_id*/ 2).is_err());
    assert!(fence.reject_retained_search("retained").is_err());
    assert!(
        fence
            .validate_and_record("retained", /*owner*/ None)
            .is_err()
    );

    fence.reserve_search("abandoned", /*attempt_id*/ 1).unwrap();
    fence.abandon_search("abandoned", /*attempt_id*/ 2);
    assert!(fence.reserve_search("abandoned", /*attempt_id*/ 2).is_err());
    fence.abandon_search("abandoned", /*attempt_id*/ 1);
    fence.reserve_search("abandoned", /*attempt_id*/ 2).unwrap();
    // A fresh turn has independent identity authority.
    SearchIdentityFence::default()
        .reserve_search("retained", /*attempt_id*/ 3)
        .unwrap();
}

#[test]
fn identity_fence_exact_bound_and_saturation_preserve_ordinary_work() {
    let fence = SearchIdentityFence::default();
    assert!(fence.reserve_search("", /*attempt_id*/ 1).is_err());
    assert!(
        fence
            .reserve_search(&"x".repeat(MAX_ITEM_ID_BYTES + 1), /*attempt_id*/ 1)
            .is_err()
    );
    for index in 0..MAX_TURN_IDENTITIES {
        let id = format!("web-{index}");
        fence.reserve_search(&id, /*attempt_id*/ 1).unwrap();
        fence.retain_search(&id, /*attempt_id*/ 1);
    }
    assert_eq!(fence.0.lock().unwrap().ids.len(), MAX_TURN_IDENTITIES);
    assert!(fence.reserve_search("overflow", /*attempt_id*/ 1).is_err());
    // Capacity exhaustion is not a new limit on ordinary canonical/local work.
    fence
        .validate_and_record("ordinary-after-full", /*owner*/ None)
        .unwrap();
    assert!(fence.0.lock().unwrap().saturated);
    assert_eq!(fence.0.lock().unwrap().ids.len(), MAX_TURN_IDENTITIES);
    assert!(
        fence
            .reserve_search("later-sampling", /*attempt_id*/ 2)
            .is_err()
    );
    assert!(fence.reject_retained_search("web-0").is_err());

    let ordinary = SearchIdentityFence::default();
    for index in 0..MAX_TURN_IDENTITIES {
        ordinary
            .validate_and_record(&format!("local-{index}"), /*owner*/ None)
            .unwrap();
    }
    ordinary
        .validate_and_record("untracked-ordinary", /*owner*/ None)
        .unwrap();
    assert!(
        ordinary
            .reserve_search("untracked-ordinary", /*attempt_id*/ 3)
            .is_err()
    );
    ordinary
        .validate_and_record("ordinary-still-works", /*owner*/ None)
        .unwrap();

    let pending = SearchIdentityFence::default();
    for index in 0..MAX_TURN_IDENTITIES {
        pending
            .reserve_search(&format!("pending-{index}"), /*attempt_id*/ 1)
            .unwrap();
    }
    pending
        .validate_and_record("untracked", /*owner*/ None)
        .unwrap();
    pending.abandon_search("pending-0", /*attempt_id*/ 1);
    assert_eq!(pending.0.lock().unwrap().ids.len(), MAX_TURN_IDENTITIES - 1);
    assert!(
        pending
            .reserve_search("untracked", /*attempt_id*/ 2)
            .is_err()
    );
}

#[test]
fn identity_fence_reservation_and_ordinary_claim_are_atomic() {
    for _ in 0..16 {
        let fence = Arc::new(SearchIdentityFence::default());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let search_fence = Arc::clone(&fence);
        let search_barrier = Arc::clone(&barrier);
        let search = std::thread::spawn(move || {
            search_barrier.wait();
            search_fence
                .reserve_search("shared", /*attempt_id*/ 1)
                .is_ok()
        });
        barrier.wait();
        let ordinary = fence.validate_and_record("shared", /*owner*/ None).is_ok();
        assert_ne!(search.join().unwrap(), ordinary);
    }
}

#[tokio::test]
async fn search_activity_same_turn_reuse_is_rejected_before_a_second_observation() {
    use crate::session::tests::make_session_and_context;
    use codex_protocol::SearchActivityKind::Web;
    use codex_protocol::SearchActivityState::Completed;
    use codex_protocol::SearchActivityState::Running;
    let (sess, turn) = make_session_and_context().await;
    let mut first = SearchActivityScope::new().unwrap();
    first
        .observe(
            &sess,
            &turn,
            /*output_index*/ 0,
            "web".into(),
            Web,
            Running,
        )
        .await
        .unwrap();
    first
        .observe(
            &sess,
            &turn,
            /*output_index*/ 0,
            "web".into(),
            Web,
            Completed,
        )
        .await
        .unwrap();
    first.retained(&turn, "web");
    assert!(
        first
            .observe(
                &sess,
                &turn,
                /*output_index*/ 1,
                "web".into(),
                Web,
                Running
            )
            .await
            .is_err()
    );
    assert!(first.pending.is_empty());
    let mut next_sampling = SearchActivityScope::new().unwrap();
    assert!(
        next_sampling
            .observe(
                &sess,
                &turn,
                /*output_index*/ 0,
                "web".into(),
                Web,
                Running
            )
            .await
            .is_err()
    );
    assert!(next_sampling.pending.is_empty());
    assert!(
        next_sampling
            .claim_canonical_identity(&turn, "web")
            .is_err()
    );
}

#[test]
fn search_activity_never_becomes_realtime_text() {
    use crate::session::turn::realtime_text_for_event;
    for state in [
        SearchActivityState::Running,
        SearchActivityState::Completed,
        SearchActivityState::Cleared,
    ] {
        let event = EventMsg::SearchActivity(SearchActivityEvent {
            attempt_id: 1,
            output_index: 2,
            item_id: "web".into(),
            kind: SearchActivityKind::Web,
            state,
        });
        assert_eq!(realtime_text_for_event(&event), None);
    }
}
