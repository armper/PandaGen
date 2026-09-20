//! The focus ring and the keyboard must agree after *every* path that moves
//! either, not only after a termination.
//!
//! Phase 332 fixed the closing path and wired `reconcile_focus` to it alone.
//! The launch path moves them apart in two ways -- a component launched
//! non-focusable never calls `focus_component`, and a focusable one whose
//! focus the policy denies has its error swallowed -- and both leave the
//! ring on the new window with the keys on the old one.

use identity::{IdentityKind, IdentityMetadata, TrustDomain};
use services_workspace_manager::{ComponentId, ComponentType, LaunchConfig, WorkspaceManager};

fn workspace() -> WorkspaceManager {
    WorkspaceManager::new(IdentityMetadata::new(
        IdentityKind::Service,
        TrustDomain::core(),
        "focus-sibling",
        0,
    ))
}

fn launch(manager: &mut WorkspaceManager, name: &str, focusable: bool) -> ComponentId {
    manager
        .launch_component(
            LaunchConfig::new(
                ComponentType::Editor,
                name,
                IdentityKind::Component,
                TrustDomain::user(),
            )
            .with_focusable(focusable),
        )
        .unwrap()
}

/// The tile drawn with the focus ring holds the component keys go to.
fn drawn_focus_matches_key_focus(manager: &WorkspaceManager, after: &str) {
    let snapshot = manager.window_layout_snapshot();
    let drawn = snapshot
        .tiles
        .iter()
        .find(|tile| tile.is_focused)
        .and_then(|tile| tile.active_component);
    let keys = manager.get_focused_component();
    assert_eq!(
        drawn, keys,
        "after {after}: the ring is on {drawn:?} but keys go to {keys:?}"
    );
}

#[test]
fn launching_a_non_focusable_component_leaves_the_ring_where_the_keys_go() {
    let mut manager = workspace();
    let first = launch(&mut manager, "first", true);
    manager.focus_component(first).unwrap();
    drawn_focus_matches_key_focus(&manager, "focusing the first component");

    let _strip = launch(&mut manager, "status-strip", false);
    drawn_focus_matches_key_focus(&manager, "launching a non-focusable component");

    let _second_strip = launch(&mut manager, "another-strip", false);
    drawn_focus_matches_key_focus(&manager, "launching a second non-focusable component");
}

#[test]
fn ordinary_launches_still_focus_the_thing_that_was_launched() {
    let mut manager = workspace();
    let first = launch(&mut manager, "first", true);
    assert_eq!(manager.get_focused_component(), Some(first));
    let second = launch(&mut manager, "second", true);
    assert_eq!(
        manager.get_focused_component(),
        Some(second),
        "the reconcile must not undo an ordinary launch's focus"
    );
    drawn_focus_matches_key_focus(&manager, "launching two focusable components");
}
