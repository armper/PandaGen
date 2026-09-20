//! What a `ViewSubscriptionCap` is for.

use core_types::TaskId;
use ipc::ChannelId;
use services_view_host::{ViewHost, ViewHostError};
use view_types::{ViewContent, ViewFrame, ViewKind};

fn published_view(host: &mut ViewHost, owner: TaskId, text: &str) -> view_types::ViewId {
    let handle = host
        .create_view(ViewKind::TextBuffer, None, owner, ChannelId::new())
        .unwrap();
    host.publish_frame(
        &handle,
        ViewFrame::new(
            handle.view_id,
            ViewKind::TextBuffer,
            1,
            ViewContent::text_buffer(vec![text.to_string()]),
            1,
        ),
    )
    .unwrap();
    handle.view_id
}

/// The capability was minted, pushed onto a list and read by nothing, so
/// the header's promise that subscribing required a capability rested on
/// nothing at all. `read_subscribed` is what makes it mean something.
#[test]
fn a_subscribed_read_needs_a_live_subscription() {
    let mut host = ViewHost::new();
    let owner = TaskId::new();
    let view = published_view(&mut host, owner, "my private notes");

    let reader = TaskId::new();
    let subscription = host.subscribe(view, reader, ChannelId::new()).unwrap();
    let frame = host
        .read_subscribed(&subscription)
        .unwrap()
        .expect("a live subscription reads the frame");
    match frame.content {
        ViewContent::TextBuffer { ref lines } => assert_eq!(lines[0], "my private notes"),
        ref other => panic!("unexpected content: {other:?}"),
    }

    // Once ended, it reads nothing.
    host.unsubscribe(&subscription).unwrap();
    assert!(
        matches!(
            host.read_subscribed(&subscription),
            Err(ViewHostError::InvalidSubscription(_))
        ),
        "a revoked subscription still read the view"
    );
    assert!(
        host.unsubscribe(&subscription).is_err(),
        "unsubscribing twice is not two unsubscribes"
    );
}

/// `subscribe` pushed unconditionally, with no dedup, no ceiling and no way
/// to release. A task could hold a hundred thousand subscriptions to one
/// view for the life of the host.
#[test]
fn subscriptions_are_deduplicated_and_bounded() {
    let mut host = ViewHost::new();
    let owner = TaskId::new();
    let view = published_view(&mut host, owner, "content");

    let reader = TaskId::new();
    let channel = ChannelId::new();
    let first = host.subscribe(view, reader, channel).unwrap();
    for _ in 0..1000 {
        let again = host.subscribe(view, reader, channel).unwrap();
        assert_eq!(
            again, first,
            "the same task and channel got a second subscription"
        );
    }

    let mut refused = false;
    for _ in 0..(ViewHost::MAX_SUBSCRIPTIONS_PER_VIEW * 2) {
        if host
            .subscribe(view, TaskId::new(), ChannelId::new())
            .is_err()
        {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "one view accepted an unbounded number of subscribers"
    );
}

/// `list_views` iterated a `HashMap`, so its order was whatever that map's
/// per-instance random seed produced -- E9's defect, which showed up as `ls`
/// output that changed for no reason between one host and the next.
///
/// Listing one host twice proves nothing: a single map iterates consistently
/// within a process. The property that actually holds is that the listing is
/// *ordered*, which two hosts can then agree on.
#[test]
fn listing_views_is_ordered() {
    let mut host = ViewHost::new();
    let owner = TaskId::new();
    for _ in 0..32 {
        published_view(&mut host, owner, "content");
    }

    let listed = host.list_views();
    assert_eq!(listed.len(), 32);
    let mut sorted = listed.clone();
    sorted.sort_by_key(|id| id.as_uuid());
    assert_eq!(
        listed, sorted,
        "the listing is in the hash map's order, which differs from one host \
         to the next and from one run to the next"
    );
}
