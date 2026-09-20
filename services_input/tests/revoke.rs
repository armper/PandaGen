//! Revoking a capability ends that subscription. It must not sentence the
//! task to silence for the life of the service.

use core_types::TaskId;
use ipc::ChannelId;
use services_input::*;

#[test]
fn a_revoked_task_can_subscribe_again() {
    let mut service = InputService::new();
    let task = TaskId::new();

    let cap = service.subscribe_keyboard(task, ChannelId::new()).unwrap();
    service.revoke_subscription(&cap).unwrap();
    assert!(!service.is_subscription_active(&cap));

    let again = service.subscribe_keyboard(task, ChannelId::new());
    assert!(
        again.is_ok(),
        "re-subscribe after revoke refused with {:?}: the task's keyboard is \
         dead for the life of the service",
        again.err()
    );
    assert!(service.is_subscription_active(&again.unwrap()));
}

/// And the revoked records do not pile up. A subscribe/revoke cycle used to
/// leave one dead record behind every time with nothing to remove them.
#[test]
fn revoked_records_do_not_accumulate() {
    // One task, over and over. This is the case the first version of the
    // reclaim handled, so on its own it is a test tuned to the fix.
    let mut service = InputService::new();
    let task = TaskId::new();
    for _ in 0..1000 {
        let cap = service.subscribe_keyboard(task, ChannelId::new()).unwrap();
        service.revoke_subscription(&cap).unwrap();
    }
    assert!(
        service.total_subscription_count() <= 2,
        "1000 subscribe/revoke cycles on one task left {} records",
        service.total_subscription_count()
    );

    // And the ordinary case: a different task each time, each revoked and
    // never seen again. Nothing reclaimed these at all.
    let mut service = InputService::new();
    for _ in 0..1000 {
        let cap = service
            .subscribe_keyboard(TaskId::new(), ChannelId::new())
            .unwrap();
        service.revoke_subscription(&cap).unwrap();
    }
    assert!(
        service.total_subscription_count() <= 512,
        "1000 tasks that each subscribed once and were revoked left {} records",
        service.total_subscription_count()
    );
}

/// The distinction `revoke` and `unsubscribe` draw is still there: a revoked
/// subscription is inactive but remains inspectable until the task takes a
/// new one.
#[test]
fn a_revoked_subscription_is_still_inspectable() {
    let mut service = InputService::new();
    let task = TaskId::new();
    let cap = service.subscribe_keyboard(task, ChannelId::new()).unwrap();
    service.revoke_subscription(&cap).unwrap();
    assert_eq!(service.active_subscription_count(), 0);
    assert_eq!(service.total_subscription_count(), 1);
}
