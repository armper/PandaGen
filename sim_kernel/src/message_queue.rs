//! Bounded message queue for kernel channels.
//!
//! Provides deterministic FIFO ordering with explicit capacity limits.

use ipc::MessageEnvelope;
use std::collections::VecDeque;

/// Queue error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    Full,
}

/// Bounded FIFO queue for message envelopes.
#[derive(Debug, Clone)]
pub struct MessageQueue {
    capacity: usize,
    messages: VecDeque<MessageEnvelope>,
}

impl MessageQueue {
    /// Creates a queue with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            messages: VecDeque::new(),
        }
    }

    /// Returns the configured capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of queued messages.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Returns whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns remaining capacity.
    pub fn remaining_capacity(&self) -> usize {
        self.capacity.saturating_sub(self.messages.len())
    }

    /// Pushes a message onto the queue.
    pub fn push(&mut self, message: MessageEnvelope) -> Result<(), QueueError> {
        if self.messages.len() >= self.capacity {
            return Err(QueueError::Full);
        }
        self.messages.push_back(message);
        Ok(())
    }

    /// Pops the next message.
    pub fn pop(&mut self) -> Option<MessageEnvelope> {
        self.messages.pop_front()
    }

    /// Mutable access to the underlying deque (for fault injection only).
    pub fn messages_mut(&mut self) -> &mut VecDeque<MessageEnvelope> {
        &mut self.messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::ServiceId;
    use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};

    fn msg(action: &str) -> MessageEnvelope {
        MessageEnvelope::new(
            ServiceId::new(),
            action.to_string(),
            SchemaVersion::new(1, 0),
            MessagePayload::new(&action).unwrap(),
        )
    }

    #[test]
    fn test_queue_ordering() {
        let mut queue = MessageQueue::with_capacity(4);
        queue.push(msg("a")).unwrap();
        queue.push(msg("b")).unwrap();
        queue.push(msg("c")).unwrap();

        assert_eq!(queue.pop().unwrap().action, "a");
        assert_eq!(queue.pop().unwrap().action, "b");
        assert_eq!(queue.pop().unwrap().action, "c");
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_queue_capacity() {
        let mut queue = MessageQueue::with_capacity(2);
        queue.push(msg("a")).unwrap();
        queue.push(msg("b")).unwrap();
        assert_eq!(queue.push(msg("c")), Err(QueueError::Full));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.remaining_capacity(), 0);
    }
}
