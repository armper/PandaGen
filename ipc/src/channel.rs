//! Channel abstraction for message passing

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a communication channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(Uuid);

impl ChannelId {
    /// Creates a new random channel ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a channel ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Channel({})", self.0)
    }
}

/// Represents one end of a bidirectional channel
///
/// Unlike Unix file descriptors, channel ends are explicitly typed
/// and cannot be confused with other resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelEnd {
    /// Sending end of the channel
    Sender(ChannelId),
    /// Receiving end of the channel
    Receiver(ChannelId),
}

impl ChannelEnd {
    /// Returns the channel ID
    pub fn channel_id(&self) -> ChannelId {
        match self {
            ChannelEnd::Sender(id) | ChannelEnd::Receiver(id) => *id,
        }
    }

    /// Checks if this is the sender end
    pub fn is_sender(&self) -> bool {
        matches!(self, ChannelEnd::Sender(_))
    }

    /// Checks if this is the receiver end
    pub fn is_receiver(&self) -> bool {
        matches!(self, ChannelEnd::Receiver(_))
    }
}

impl fmt::Display for ChannelEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelEnd::Sender(id) => write!(f, "Sender({})", id),
            ChannelEnd::Receiver(id) => write!(f, "Receiver({})", id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_id_creation() {
        let id1 = ChannelId::new();
        let id2 = ChannelId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_channel_end_sender() {
        let id = ChannelId::new();
        let end = ChannelEnd::Sender(id);
        assert!(end.is_sender());
        assert!(!end.is_receiver());
        assert_eq!(end.channel_id(), id);
    }

    #[test]
    fn test_channel_end_receiver() {
        let id = ChannelId::new();
        let end = ChannelEnd::Receiver(id);
        assert!(!end.is_sender());
        assert!(end.is_receiver());
        assert_eq!(end.channel_id(), id);
    }

    #[test]
    fn test_channel_end_equality() {
        let id = ChannelId::new();
        let sender1 = ChannelEnd::Sender(id);
        let sender2 = ChannelEnd::Sender(id);
        let receiver = ChannelEnd::Receiver(id);

        assert_eq!(sender1, sender2);
        assert_ne!(sender1, receiver);
    }
}
