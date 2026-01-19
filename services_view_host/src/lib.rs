//! # View Host Service
//!
//! This crate implements the view host service for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Views, not streams**: Components publish structured views, not byte streams
//! - **Capability-based**: Publishing and subscribing require capabilities
//! - **Immutable frames**: View frames are immutable; updates replace by revision
//! - **Monotonic revisions**: Revisions must strictly increase
//! - **Host-managed**: The host decides layout and presentation
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A graphics system
//! - A terminal emulator
//! - A compositor
//! - A full UI toolkit

use core_types::TaskId;
use ipc::ChannelId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use view_types::{ViewFrame, ViewId, ViewKind};

/// View handle capability
///
/// Represents the right to publish frames to a specific view.
/// When dropped or revoked, no more frames can be published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewHandleCap {
    /// View ID
    pub view_id: ViewId,
    /// Task that owns this handle
    pub task_id: TaskId,
    /// Channel for publishing
    pub channel: ChannelId,
    /// Secret token for verification
    token: u64,
}

impl ViewHandleCap {
    /// Creates a new view handle capability
    fn new(view_id: ViewId, task_id: TaskId, channel: ChannelId, token: u64) -> Self {
        Self {
            view_id,
            task_id,
            channel,
            token,
        }
    }
}

/// View subscription capability
///
/// Represents the right to receive updates for a specific view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewSubscriptionCap {
    /// View ID
    pub view_id: ViewId,
    /// Task that owns this subscription
    pub task_id: TaskId,
    /// Channel for receiving updates
    pub channel: ChannelId,
    /// Secret token for verification
    token: u64,
}

impl ViewSubscriptionCap {
    /// Creates a new view subscription capability
    fn new(view_id: ViewId, task_id: TaskId, channel: ChannelId, token: u64) -> Self {
        Self {
            view_id,
            task_id,
            channel,
            token,
        }
    }
}

/// View host service error types
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ViewHostError {
    #[error("View not found: {0}")]
    ViewNotFound(ViewId),

    #[error("View already exists: {0}")]
    ViewAlreadyExists(ViewId),

    #[error("Invalid capability")]
    InvalidCapability,

    #[error("Unauthorized access to view: {0}")]
    Unauthorized(ViewId),

    #[error("Revision not monotonic: expected > {expected}, got {actual}")]
    RevisionNotMonotonic { expected: u64, actual: u64 },

    #[error("View ID mismatch: expected {expected}, got {actual}")]
    ViewIdMismatch { expected: ViewId, actual: ViewId },

    #[error("No frames published yet for view: {0}")]
    NoFrames(ViewId),
}

/// View record - internal state for a view
#[derive(Debug, Clone)]
struct ViewRecord {
    /// View ID
    view_id: ViewId,
    /// View kind
    kind: ViewKind,
    /// Owner task ID
    owner_task_id: TaskId,
    /// Secret token for handle verification
    handle_token: u64,
    /// Latest published frame (if any)
    latest_frame: Option<ViewFrame>,
    /// Active subscriptions
    subscriptions: Vec<ViewSubscriptionCap>,
}

impl ViewRecord {
    fn new(view_id: ViewId, kind: ViewKind, owner_task_id: TaskId, handle_token: u64) -> Self {
        Self {
            view_id,
            kind,
            owner_task_id,
            handle_token,
            latest_frame: None,
            subscriptions: Vec::new(),
        }
    }

    /// Verifies a handle capability
    fn verify_handle(&self, handle: &ViewHandleCap) -> Result<(), ViewHostError> {
        if handle.view_id != self.view_id {
            return Err(ViewHostError::ViewIdMismatch {
                expected: self.view_id,
                actual: handle.view_id,
            });
        }
        if handle.task_id != self.owner_task_id || handle.token != self.handle_token {
            return Err(ViewHostError::Unauthorized(self.view_id));
        }
        Ok(())
    }

    /// Verifies revision is monotonic
    fn verify_revision(&self, new_revision: u64) -> Result<(), ViewHostError> {
        if let Some(latest) = &self.latest_frame {
            if new_revision <= latest.revision {
                return Err(ViewHostError::RevisionNotMonotonic {
                    expected: latest.revision,
                    actual: new_revision,
                });
            }
        }
        Ok(())
    }
}

/// View host service
///
/// Manages view creation, publishing, and subscriptions.
pub struct ViewHost {
    /// Next token for capability verification
    next_token: u64,
    /// Views by ID
    views: HashMap<ViewId, ViewRecord>,
    /// Next subscription token
    next_subscription_token: u64,
}

impl ViewHost {
    /// Creates a new view host service
    pub fn new() -> Self {
        Self {
            next_token: 1,
            views: HashMap::new(),
            next_subscription_token: 1000,
        }
    }

    /// Creates a new view
    ///
    /// Returns a handle capability that can be used to publish frames.
    pub fn create_view(
        &mut self,
        kind: ViewKind,
        title: Option<String>,
        task_id: TaskId,
        channel: ChannelId,
    ) -> Result<ViewHandleCap, ViewHostError> {
        let view_id = ViewId::new();

        // Generate token for this handle
        let token = self.next_token;
        self.next_token += 1;

        // Create view record
        let mut record = ViewRecord::new(view_id, kind, task_id, token);

        // If title provided, create initial empty frame
        if let Some(title) = title {
            let initial_content = match kind {
                ViewKind::TextBuffer => view_types::ViewContent::empty_text_buffer(),
                ViewKind::StatusLine => view_types::ViewContent::status_line(""),
                ViewKind::Panel => view_types::ViewContent::panel(""),
            };
            let initial_frame =
                ViewFrame::new(view_id, kind, 0, initial_content, 0).with_title(title);
            record.latest_frame = Some(initial_frame);
        }

        self.views.insert(view_id, record);

        Ok(ViewHandleCap::new(view_id, task_id, channel, token))
    }

    /// Publishes a view frame
    ///
    /// Requires a valid handle capability.
    /// Enforces monotonic revision ordering.
    pub fn publish_frame(
        &mut self,
        handle: &ViewHandleCap,
        frame: ViewFrame,
    ) -> Result<(), ViewHostError> {
        let record = self
            .views
            .get_mut(&handle.view_id)
            .ok_or(ViewHostError::ViewNotFound(handle.view_id))?;

        // Verify handle
        record.verify_handle(handle)?;

        // Verify view ID matches
        if frame.view_id != handle.view_id {
            return Err(ViewHostError::ViewIdMismatch {
                expected: handle.view_id,
                actual: frame.view_id,
            });
        }

        // Verify revision is monotonic
        record.verify_revision(frame.revision)?;

        // Update latest frame
        record.latest_frame = Some(frame);

        Ok(())
    }

    /// Subscribes to a view
    ///
    /// Returns a subscription capability that can be used to receive updates.
    pub fn subscribe(
        &mut self,
        view_id: ViewId,
        task_id: TaskId,
        channel: ChannelId,
    ) -> Result<ViewSubscriptionCap, ViewHostError> {
        let record = self
            .views
            .get_mut(&view_id)
            .ok_or(ViewHostError::ViewNotFound(view_id))?;

        // Generate subscription token
        let token = self.next_subscription_token;
        self.next_subscription_token += 1;

        let subscription = ViewSubscriptionCap::new(view_id, task_id, channel, token);
        record.subscriptions.push(subscription);

        Ok(subscription)
    }

    /// Gets the latest frame for a view
    ///
    /// Returns None if no frames have been published yet.
    pub fn get_latest(&self, view_id: ViewId) -> Result<Option<ViewFrame>, ViewHostError> {
        let record = self
            .views
            .get(&view_id)
            .ok_or(ViewHostError::ViewNotFound(view_id))?;

        Ok(record.latest_frame.clone())
    }

    /// Removes a view
    ///
    /// Requires a valid handle capability.
    pub fn remove_view(&mut self, handle: &ViewHandleCap) -> Result<(), ViewHostError> {
        let record = self
            .views
            .get(&handle.view_id)
            .ok_or(ViewHostError::ViewNotFound(handle.view_id))?;

        // Verify handle
        record.verify_handle(handle)?;

        self.views.remove(&handle.view_id);
        Ok(())
    }

    /// Lists all view IDs
    pub fn list_views(&self) -> Vec<ViewId> {
        self.views.keys().copied().collect()
    }

    /// Gets view metadata (kind, owner, etc.) without the frame content
    pub fn get_view_info(&self, view_id: ViewId) -> Result<(ViewKind, TaskId), ViewHostError> {
        let record = self
            .views
            .get(&view_id)
            .ok_or(ViewHostError::ViewNotFound(view_id))?;

        Ok((record.kind, record.owner_task_id))
    }
}

impl Default for ViewHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use view_types::ViewContent;

    fn create_test_task_id() -> TaskId {
        TaskId::new()
    }

    fn create_test_channel() -> ChannelId {
        ChannelId::new()
    }

    #[test]
    fn test_create_view() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(
                ViewKind::TextBuffer,
                Some("Test".to_string()),
                task_id,
                channel,
            )
            .unwrap();

        assert_eq!(handle.task_id, task_id);
        assert_eq!(host.list_views().len(), 1);
    }

    #[test]
    fn test_publish_frame() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        let content = ViewContent::text_buffer(vec!["Hello".to_string()]);
        let frame = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content, 1000);

        let result = host.publish_frame(&handle, frame.clone());
        assert!(result.is_ok());

        // Verify frame was stored
        let latest = host.get_latest(handle.view_id).unwrap();
        assert_eq!(latest, Some(frame));
    }

    #[test]
    fn test_publish_frame_monotonic_revision() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        // Publish frame with revision 1
        let content1 = ViewContent::text_buffer(vec!["First".to_string()]);
        let frame1 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content1, 1000);
        host.publish_frame(&handle, frame1).unwrap();

        // Publish frame with revision 2 (OK)
        let content2 = ViewContent::text_buffer(vec!["Second".to_string()]);
        let frame2 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 2, content2, 2000);
        assert!(host.publish_frame(&handle, frame2).is_ok());

        // Try to publish frame with revision 1 (should fail)
        let content3 = ViewContent::text_buffer(vec!["Third".to_string()]);
        let frame3 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content3, 3000);
        let result = host.publish_frame(&handle, frame3);
        assert_eq!(
            result,
            Err(ViewHostError::RevisionNotMonotonic {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn test_publish_frame_unauthorized() {
        let mut host = ViewHost::new();
        let task_id1 = create_test_task_id();
        let task_id2 = create_test_task_id();
        let channel = create_test_channel();

        let handle1 = host
            .create_view(ViewKind::TextBuffer, None, task_id1, channel)
            .unwrap();

        // Create a fake handle for different task
        let fake_handle = ViewHandleCap::new(handle1.view_id, task_id2, channel, 999);

        let content = ViewContent::text_buffer(vec!["Hello".to_string()]);
        let frame = ViewFrame::new(handle1.view_id, ViewKind::TextBuffer, 1, content, 1000);

        let result = host.publish_frame(&fake_handle, frame);
        assert_eq!(result, Err(ViewHostError::Unauthorized(handle1.view_id)));
    }

    #[test]
    fn test_subscribe_to_view() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        let subscriber_task = create_test_task_id();
        let subscriber_channel = create_test_channel();

        let subscription = host
            .subscribe(handle.view_id, subscriber_task, subscriber_channel)
            .unwrap();

        assert_eq!(subscription.view_id, handle.view_id);
        assert_eq!(subscription.task_id, subscriber_task);
    }

    #[test]
    fn test_subscribe_nonexistent_view() {
        let mut host = ViewHost::new();
        let view_id = ViewId::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let result = host.subscribe(view_id, task_id, channel);
        assert_eq!(result, Err(ViewHostError::ViewNotFound(view_id)));
    }

    #[test]
    fn test_get_latest_no_frames() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        let latest = host.get_latest(handle.view_id).unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_get_latest_with_frames() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        // Publish multiple frames
        let content1 = ViewContent::text_buffer(vec!["First".to_string()]);
        let frame1 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 1, content1, 1000);
        host.publish_frame(&handle, frame1).unwrap();

        let content2 = ViewContent::text_buffer(vec!["Second".to_string()]);
        let frame2 = ViewFrame::new(handle.view_id, ViewKind::TextBuffer, 2, content2, 2000);
        host.publish_frame(&handle, frame2.clone()).unwrap();

        // Should get latest
        let latest = host.get_latest(handle.view_id).unwrap();
        assert_eq!(latest, Some(frame2));
    }

    #[test]
    fn test_remove_view() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        assert_eq!(host.list_views().len(), 1);

        host.remove_view(&handle).unwrap();
        assert_eq!(host.list_views().len(), 0);
    }

    #[test]
    fn test_remove_view_unauthorized() {
        let mut host = ViewHost::new();
        let task_id1 = create_test_task_id();
        let task_id2 = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id1, channel)
            .unwrap();

        // Create fake handle for different task
        let fake_handle = ViewHandleCap::new(handle.view_id, task_id2, channel, 999);

        let result = host.remove_view(&fake_handle);
        assert_eq!(result, Err(ViewHostError::Unauthorized(handle.view_id)));
    }

    #[test]
    fn test_list_views() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        assert_eq!(host.list_views().len(), 0);

        let _handle1 = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();
        let _handle2 = host
            .create_view(ViewKind::StatusLine, None, task_id, channel)
            .unwrap();

        assert_eq!(host.list_views().len(), 2);
    }

    #[test]
    fn test_get_view_info() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        let (kind, owner) = host.get_view_info(handle.view_id).unwrap();
        assert_eq!(kind, ViewKind::TextBuffer);
        assert_eq!(owner, task_id);
    }

    #[test]
    fn test_view_id_mismatch() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        // Create frame with wrong view ID
        let wrong_view_id = ViewId::new();
        let content = ViewContent::text_buffer(vec!["Hello".to_string()]);
        let frame = ViewFrame::new(wrong_view_id, ViewKind::TextBuffer, 1, content, 1000);

        let result = host.publish_frame(&handle, frame);
        assert!(matches!(result, Err(ViewHostError::ViewIdMismatch { .. })));
    }

    #[test]
    fn test_create_view_with_title() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(
                ViewKind::TextBuffer,
                Some("My View".to_string()),
                task_id,
                channel,
            )
            .unwrap();

        // Should have initial frame with title
        let latest = host.get_latest(handle.view_id).unwrap();
        assert!(latest.is_some());
        let frame = latest.unwrap();
        assert_eq!(frame.title, Some("My View".to_string()));
        assert_eq!(frame.revision, 0);
    }

    #[test]
    fn test_multiple_subscriptions() {
        let mut host = ViewHost::new();
        let task_id = create_test_task_id();
        let channel = create_test_channel();

        let handle = host
            .create_view(ViewKind::TextBuffer, None, task_id, channel)
            .unwrap();

        // Multiple tasks can subscribe
        let sub1 = host
            .subscribe(handle.view_id, create_test_task_id(), create_test_channel())
            .unwrap();
        let sub2 = host
            .subscribe(handle.view_id, create_test_task_id(), create_test_channel())
            .unwrap();

        assert_eq!(sub1.view_id, handle.view_id);
        assert_eq!(sub2.view_id, handle.view_id);
        assert_ne!(sub1.task_id, sub2.task_id);
    }
}
