//! Multi-user workspace access control with delegated admin scopes.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[cfg(target_os = "none")]
fn new_uuid() -> Uuid {
    use core::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let hi = COUNTER.fetch_add(1, Ordering::Relaxed);
    let lo = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_le_bytes());
    bytes[8..].copy_from_slice(&lo.to_le_bytes());

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes)
}

#[cfg(not(target_os = "none"))]
fn new_uuid() -> Uuid {
    Uuid::new_v4()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(Uuid);

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl UserId {
    pub fn new() -> Self {
        Self(new_uuid())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub user_id: UserId,
    pub display_name: String,
    pub role: Role,
    pub scopes: HashSet<Scope>,
}

#[derive(Debug, Error)]
pub enum AccessError {
    #[error("User not found: {0:?}")]
    UserNotFound(UserId),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Access control model for workspace.
pub struct WorkspaceAccessControl {
    users: HashMap<UserId, UserRecord>,
}

impl Default for WorkspaceAccessControl {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceAccessControl {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }

    pub fn add_user(&mut self, name: impl Into<String>) -> UserId {
        let user_id = UserId::new();
        let record = UserRecord {
            user_id,
            display_name: name.into(),
            role: Role::User,
            scopes: HashSet::new(),
        };
        self.users.insert(user_id, record);
        user_id
    }

    pub fn grant_admin(&mut self, user_id: UserId) -> Result<(), AccessError> {
        let record = self
            .users
            .get_mut(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        record.role = Role::Admin;
        Ok(())
    }

    pub fn delegate_scope(
        &mut self,
        from_admin: UserId,
        to_user: UserId,
        scope: Scope,
    ) -> Result<(), AccessError> {
        let admin = self
            .users
            .get(&from_admin)
            .ok_or(AccessError::UserNotFound(from_admin))?;
        if admin.role != Role::Admin {
            return Err(AccessError::PermissionDenied(
                "Only admins can delegate scopes".to_string(),
            ));
        }
        let user = self
            .users
            .get_mut(&to_user)
            .ok_or(AccessError::UserNotFound(to_user))?;
        user.scopes.insert(scope);
        Ok(())
    }

    pub fn check_scope(&self, user_id: UserId, scope: &Scope) -> Result<(), AccessError> {
        let user = self
            .users
            .get(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        if user.role == Role::Admin || user.scopes.contains(scope) {
            Ok(())
        } else {
            Err(AccessError::PermissionDenied(format!(
                "Missing scope: {}",
                scope.0
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegated_admin_scope() {
        let mut acl = WorkspaceAccessControl::new();
        let admin = acl.add_user("admin");
        let user = acl.add_user("user");
        acl.grant_admin(admin).unwrap();

        let scope = Scope("workspace.manage".to_string());
        acl.delegate_scope(admin, user, scope.clone()).unwrap();
        acl.check_scope(user, &scope).unwrap();
    }

    #[test]
    fn test_scope_denied() {
        let mut acl = WorkspaceAccessControl::new();
        let user = acl.add_user("user");
        let scope = Scope("workspace.manage".to_string());
        let result = acl.check_scope(user, &scope);
        assert!(matches!(result, Err(AccessError::PermissionDenied(_))));
    }
}
