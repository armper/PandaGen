#![no_std]

//! # Notification + Status Service
//!
//! Provides structured "toast" notifications and a bottom status bar for system-wide messages.
//!
//! ## Philosophy
//!
//! - **Structured, not stdout**: Notifications are typed events, not print statements
//! - **Capability-gated**: Only components with notification capability can send
//! - **Prioritized**: Notifications have severity levels
//! - **Deterministic**: All notifications are timestamped and ordered
//! - **Testable**: Notification history can be inspected
//!
//! ## Features
//!
//! - Toast notifications (temporary pop-ups)
//! - Status bar (persistent bottom bar)
//! - Editor save success/failure messages
//! - Filesystem error notifications
//! - "Caps denied" messages
//! - Background task progress (future)
//!
//! ## Example
//!
//! ```ignore
//! use services_notification::{NotificationService, Notification, NotificationLevel};
//!
//! let mut service = NotificationService::new();
//!
//! // Send a success notification
//! service.notify(Notification::success("File saved successfully"));
//!
//! // Send an error notification
//! service.notify(Notification::error("Failed to open file"));
//!
//! // Update status bar
//! service.set_status("Ready");
//!
//! // Get recent notifications
//! let recent = service.get_recent_notifications(10);
//! ```

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum number of notifications to keep in history
const MAX_NOTIFICATION_HISTORY: usize = 100;

/// Unique identifier for a notification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NotificationId(Uuid);

impl NotificationId {
    /// Creates a new notification ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a NotificationId from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for NotificationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NotificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "notif:{}", self.0)
    }
}

/// Notification severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NotificationLevel {
    /// Informational message
    Info,
    /// Success message
    Success,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

impl fmt::Display for NotificationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationLevel::Info => write!(f, "INFO"),
            NotificationLevel::Success => write!(f, "SUCCESS"),
            NotificationLevel::Warning => write!(f, "WARNING"),
            NotificationLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Notification type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationType {
    /// Toast notification (temporary pop-up)
    Toast,
    /// Status bar notification (persistent)
    Status,
}

/// A notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique notification identifier
    pub id: NotificationId,
    /// Notification level
    pub level: NotificationLevel,
    /// Notification type
    pub notification_type: NotificationType,
    /// Message text
    pub message: String,
    /// Optional source component
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Timestamp when notification was created (in nanoseconds)
    pub timestamp_ns: u64,
    /// Whether the notification has been dismissed
    pub dismissed: bool,
    /// Time-to-live for toast notifications (in nanoseconds, 0 means infinite)
    pub ttl_ns: u64,
}

impl Notification {
    /// Creates a new notification
    pub fn new(
        level: NotificationLevel,
        notification_type: NotificationType,
        message: impl Into<String>,
        timestamp_ns: u64,
    ) -> Self {
        Self {
            id: NotificationId::new(),
            level,
            notification_type,
            message: message.into(),
            source: None,
            timestamp_ns,
            dismissed: false,
            ttl_ns: 0,
        }
    }

    /// Creates an info toast notification
    pub fn info(message: impl Into<String>, timestamp_ns: u64) -> Self {
        Self::new(
            NotificationLevel::Info,
            NotificationType::Toast,
            message,
            timestamp_ns,
        )
        .with_ttl(5_000_000_000) // 5 seconds
    }

    /// Creates a success toast notification
    pub fn success(message: impl Into<String>, timestamp_ns: u64) -> Self {
        Self::new(
            NotificationLevel::Success,
            NotificationType::Toast,
            message,
            timestamp_ns,
        )
        .with_ttl(3_000_000_000) // 3 seconds
    }

    /// Creates a warning toast notification
    pub fn warning(message: impl Into<String>, timestamp_ns: u64) -> Self {
        Self::new(
            NotificationLevel::Warning,
            NotificationType::Toast,
            message,
            timestamp_ns,
        )
        .with_ttl(7_000_000_000) // 7 seconds
    }

    /// Creates an error toast notification
    pub fn error(message: impl Into<String>, timestamp_ns: u64) -> Self {
        Self::new(
            NotificationLevel::Error,
            NotificationType::Toast,
            message,
            timestamp_ns,
        )
        .with_ttl(10_000_000_000) // 10 seconds
    }

    /// Creates a status bar notification
    pub fn status(message: impl Into<String>, timestamp_ns: u64) -> Self {
        Self::new(
            NotificationLevel::Info,
            NotificationType::Status,
            message,
            timestamp_ns,
        )
    }

    /// Sets the source component
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Sets the time-to-live
    pub fn with_ttl(mut self, ttl_ns: u64) -> Self {
        self.ttl_ns = ttl_ns;
        self
    }

    /// Checks if this notification has expired based on current time
    pub fn is_expired(&self, current_time_ns: u64) -> bool {
        if self.ttl_ns == 0 {
            return false;
        }
        // Use saturating_add to avoid overflow
        let expiration_time = self.timestamp_ns.saturating_add(self.ttl_ns);
        current_time_ns >= expiration_time
    }

    /// Dismisses the notification
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }
}

/// Notification service
pub struct NotificationService {
    /// All notifications (including dismissed ones)
    notifications: VecDeque<Notification>,
    /// Current status bar message
    status: String,
    /// Current logical time (in nanoseconds)
    current_time_ns: u64,
}

impl NotificationService {
    /// Creates a new notification service
    pub fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            status: String::from("Ready"),
            current_time_ns: 0,
        }
    }

    /// Advances the logical time (for testing and determinism)
    pub fn advance_time(&mut self, delta_ns: u64) {
        self.current_time_ns += delta_ns;
        self.expire_old_notifications();
    }

    /// Sets the current time (for testing and determinism)
    pub fn set_time(&mut self, time_ns: u64) {
        self.current_time_ns = time_ns;
        self.expire_old_notifications();
    }

    /// Returns the current time
    pub fn current_time(&self) -> u64 {
        self.current_time_ns
    }

    /// Sends a notification
    pub fn notify(&mut self, notification: Notification) {
        self.notifications.push_back(notification);

        // Trim history if too large
        while self.notifications.len() > MAX_NOTIFICATION_HISTORY {
            self.notifications.pop_front();
        }
    }

    /// Updates the status bar message
    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    /// Returns the current status bar message
    pub fn get_status(&self) -> &str {
        &self.status
    }

    /// Returns active (non-dismissed, non-expired) toast notifications
    pub fn get_active_toasts(&self) -> Vec<&Notification> {
        self.notifications
            .iter()
            .filter(|n| {
                n.notification_type == NotificationType::Toast
                    && !n.dismissed
                    && !n.is_expired(self.current_time_ns)
            })
            .collect()
    }

    /// Returns all recent notifications (including dismissed ones)
    pub fn get_recent_notifications(&self, limit: usize) -> Vec<&Notification> {
        self.notifications.iter().rev().take(limit).collect()
    }

    /// Returns notifications filtered by level
    pub fn get_notifications_by_level(&self, level: NotificationLevel) -> Vec<&Notification> {
        self.notifications
            .iter()
            .filter(|n| n.level == level)
            .collect()
    }

    /// Dismisses a notification by ID
    pub fn dismiss_notification(&mut self, id: NotificationId) -> bool {
        if let Some(notification) = self.notifications.iter_mut().find(|n| n.id == id) {
            notification.dismiss();
            true
        } else {
            false
        }
    }

    /// Dismisses all active notifications
    pub fn dismiss_all(&mut self) {
        for notification in &mut self.notifications {
            notification.dismiss();
        }
    }

    /// Clears all notifications from history
    pub fn clear_all(&mut self) {
        self.notifications.clear();
    }

    /// Expires old notifications (removes expired ones)
    fn expire_old_notifications(&mut self) {
        // Only keep non-expired or status notifications
        self.notifications.retain(|n| {
            n.notification_type == NotificationType::Status || !n.is_expired(self.current_time_ns)
        });
    }

    /// Returns the total number of notifications in history
    pub fn notification_count(&self) -> usize {
        self.notifications.len()
    }
}

impl Default for NotificationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    fn test_notification_id_creation() {
        let id1 = NotificationId::new();
        let id2 = NotificationId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_notification_level_ordering() {
        assert!(NotificationLevel::Info < NotificationLevel::Success);
        assert!(NotificationLevel::Success < NotificationLevel::Warning);
        assert!(NotificationLevel::Warning < NotificationLevel::Error);
    }

    #[test]
    fn test_notification_creation() {
        let notif = Notification::info("Test message", 1000);

        assert_eq!(notif.level, NotificationLevel::Info);
        assert_eq!(notif.notification_type, NotificationType::Toast);
        assert_eq!(notif.message, "Test message");
        assert_eq!(notif.timestamp_ns, 1000);
        assert!(!notif.dismissed);
        assert!(notif.ttl_ns > 0);
    }

    #[test]
    fn test_notification_with_source() {
        let notif = Notification::info("Test", 1000).with_source("editor");
        assert_eq!(notif.source, Some("editor".to_string()));
    }

    #[test]
    fn test_notification_expiration() {
        let notif = Notification::info("Test", 1000).with_ttl(5000);

        assert!(!notif.is_expired(1000));
        assert!(!notif.is_expired(5999));
        assert!(notif.is_expired(6000));
        assert!(notif.is_expired(10000));
    }

    #[test]
    fn test_notification_no_expiration() {
        let notif = Notification::info("Test", 1000).with_ttl(0);
        assert!(!notif.is_expired(1000000));
    }

    #[test]
    fn test_notification_dismiss() {
        let mut notif = Notification::info("Test", 1000);
        assert!(!notif.dismissed);

        notif.dismiss();
        assert!(notif.dismissed);
    }

    #[test]
    fn test_service_creation() {
        let service = NotificationService::new();
        assert_eq!(service.current_time(), 0);
        assert_eq!(service.get_status(), "Ready");
        assert_eq!(service.notification_count(), 0);
    }

    #[test]
    fn test_service_notify() {
        let mut service = NotificationService::new();

        service.notify(Notification::info("Test 1", 1000));
        service.notify(Notification::success("Test 2", 2000));

        assert_eq!(service.notification_count(), 2);
    }

    #[test]
    fn test_service_set_status() {
        let mut service = NotificationService::new();

        service.set_status("Working...");
        assert_eq!(service.get_status(), "Working...");

        service.set_status("Done");
        assert_eq!(service.get_status(), "Done");
    }

    #[test]
    fn test_service_get_active_toasts() {
        let mut service = NotificationService::new();
        service.set_time(1000);

        service.notify(Notification::info("Toast 1", 1000));
        service.notify(Notification::success("Toast 2", 1000));
        service.notify(Notification::status("Status", 1000));

        let toasts = service.get_active_toasts();
        assert_eq!(toasts.len(), 2);
    }

    #[test]
    fn test_service_expire_notifications() {
        let mut service = NotificationService::new();
        service.set_time(1000);

        service.notify(Notification::info("Test", 1000).with_ttl(5000));
        assert_eq!(service.get_active_toasts().len(), 1);

        service.set_time(6001);
        assert_eq!(service.get_active_toasts().len(), 0);
    }

    #[test]
    fn test_service_dismiss_notification() {
        let mut service = NotificationService::new();
        service.set_time(1000);

        let notif = Notification::info("Test", 1000);
        let id = notif.id;
        service.notify(notif);

        assert_eq!(service.get_active_toasts().len(), 1);

        let dismissed = service.dismiss_notification(id);
        assert!(dismissed);
        assert_eq!(service.get_active_toasts().len(), 0);
    }

    #[test]
    fn test_service_dismiss_nonexistent() {
        let mut service = NotificationService::new();
        let dismissed = service.dismiss_notification(NotificationId::new());
        assert!(!dismissed);
    }

    #[test]
    fn test_service_dismiss_all() {
        let mut service = NotificationService::new();
        service.set_time(1000);

        service.notify(Notification::info("Test 1", 1000));
        service.notify(Notification::success("Test 2", 1000));

        assert_eq!(service.get_active_toasts().len(), 2);

        service.dismiss_all();
        assert_eq!(service.get_active_toasts().len(), 0);
    }

    #[test]
    fn test_service_get_recent_notifications() {
        let mut service = NotificationService::new();

        for i in 0..10 {
            service.notify(Notification::info(format!("Test {}", i), i * 1000));
        }

        let recent = service.get_recent_notifications(5);
        assert_eq!(recent.len(), 5);
        // Most recent first
        assert_eq!(recent[0].message, "Test 9");
    }

    #[test]
    fn test_service_get_notifications_by_level() {
        let mut service = NotificationService::new();

        service.notify(Notification::info("Info", 1000));
        service.notify(Notification::error("Error", 2000));
        service.notify(Notification::info("Info 2", 3000));

        let errors = service.get_notifications_by_level(NotificationLevel::Error);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Error");

        let infos = service.get_notifications_by_level(NotificationLevel::Info);
        assert_eq!(infos.len(), 2);
    }

    #[test]
    fn test_service_clear_all() {
        let mut service = NotificationService::new();

        service.notify(Notification::info("Test 1", 1000));
        service.notify(Notification::info("Test 2", 2000));

        assert_eq!(service.notification_count(), 2);

        service.clear_all();
        assert_eq!(service.notification_count(), 0);
    }

    #[test]
    fn test_service_max_history() {
        let mut service = NotificationService::new();

        // Add more than MAX_NOTIFICATION_HISTORY notifications
        for i in 0..(MAX_NOTIFICATION_HISTORY + 10) {
            service.notify(Notification::info(format!("Test {}", i), i as u64 * 1000));
        }

        // Should only keep MAX_NOTIFICATION_HISTORY
        assert_eq!(service.notification_count(), MAX_NOTIFICATION_HISTORY);
    }

    #[test]
    fn test_service_advance_time() {
        let mut service = NotificationService::new();
        service.set_time(1000);

        service.notify(Notification::info("Test", 1000).with_ttl(5000));
        assert_eq!(service.get_active_toasts().len(), 1);

        service.advance_time(3000);
        assert_eq!(service.current_time(), 4000);
        assert_eq!(service.get_active_toasts().len(), 1);

        service.advance_time(3000);
        assert_eq!(service.current_time(), 7000);
        assert_eq!(service.get_active_toasts().len(), 0);
    }
}
