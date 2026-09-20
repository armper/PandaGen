//! Multi-user workspace access control with delegated admin scopes.

use serde::{Deserialize, Serialize};
// `BTreeSet` rather than `HashSet` for the serialized field: serde
// implements the ordered collections with `alloc` alone, while the hashed
// ones need `std`. Declaring that feature here turned it on for the whole
// workspace and broke the no_std kernel build. The ordering is a bonus --
// two defects this session came from handing HashMap order to a caller.
use std::collections::{BTreeSet, HashMap};
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Scope(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub user_id: UserId,
    pub display_name: String,
    pub role: Role,
    pub scopes: BTreeSet<Scope>,
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
            scopes: BTreeSet::new(),
        };
        self.users.insert(user_id, record);
        user_id
    }

    /// Create the first administrator.
    ///
    /// Only possible while there is none: after that, promotion goes through
    /// `grant_admin`, which requires an existing admin. `grant_admin` used
    /// to take no `from_admin` at all, so `delegate_scope`'s admin check was
    /// bypassed by one prior call -- any user could make itself an admin and
    /// then delegate any scope to anyone.
    pub fn bootstrap_admin(&mut self, user_id: UserId) -> Result<(), AccessError> {
        if self.users.values().any(|user| user.role == Role::Admin) {
            return Err(AccessError::PermissionDenied(
                "an administrator already exists; use grant_admin".to_string(),
            ));
        }
        let record = self
            .users
            .get_mut(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        record.role = Role::Admin;
        Ok(())
    }

    /// Promote a user. Only an administrator may.
    pub fn grant_admin(&mut self, from_admin: UserId, user_id: UserId) -> Result<(), AccessError> {
        self.require_admin(from_admin)?;
        let record = self
            .users
            .get_mut(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        record.role = Role::Admin;
        Ok(())
    }

    /// Demote an administrator. The last one cannot be demoted, or the
    /// directory would have no way back to an administrator at all.
    pub fn revoke_admin(&mut self, from_admin: UserId, user_id: UserId) -> Result<(), AccessError> {
        self.require_admin(from_admin)?;
        let admins = self
            .users
            .values()
            .filter(|user| user.role == Role::Admin)
            .count();
        if admins <= 1 {
            return Err(AccessError::PermissionDenied(
                "cannot demote the last administrator".to_string(),
            ));
        }
        let record = self
            .users
            .get_mut(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        record.role = Role::User;
        Ok(())
    }

    /// Take a scope back. There used to be no way to: a delegated scope was
    /// permanent for the life of the directory.
    pub fn revoke_scope(
        &mut self,
        from_admin: UserId,
        from_user: UserId,
        scope: &Scope,
    ) -> Result<(), AccessError> {
        self.require_admin(from_admin)?;
        let user = self
            .users
            .get_mut(&from_user)
            .ok_or(AccessError::UserNotFound(from_user))?;
        user.scopes.remove(scope);
        Ok(())
    }

    fn require_admin(&self, user_id: UserId) -> Result<(), AccessError> {
        let user = self
            .users
            .get(&user_id)
            .ok_or(AccessError::UserNotFound(user_id))?;
        if user.role != Role::Admin {
            return Err(AccessError::PermissionDenied(
                "only administrators may do this".to_string(),
            ));
        }
        Ok(())
    }

    pub fn delegate_scope(
        &mut self,
        from_admin: UserId,
        to_user: UserId,
        scope: Scope,
    ) -> Result<(), AccessError> {
        self.require_admin(from_admin)?;
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
        acl.bootstrap_admin(admin).unwrap();

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

#[cfg(test)]
mod escalation_tests {
    use super::*;

    #[test]
    fn a_user_cannot_make_itself_an_administrator() {
        // `grant_admin` took no `from_admin` and checked nothing, so
        // `delegate_scope`'s admin check was bypassed by one prior call: any
        // user could promote itself and then delegate any scope to anyone.
        let mut acl = WorkspaceAccessControl::new();
        let admin = acl.add_user("admin");
        let attacker = acl.add_user("attacker");
        acl.bootstrap_admin(admin).unwrap();

        assert!(
            acl.grant_admin(attacker, attacker).is_err(),
            "a plain user promoted itself to administrator"
        );
        assert!(acl
            .delegate_scope(attacker, attacker, Scope("everything".to_string()))
            .is_err());

        // And the bootstrap door is shut once there is an administrator.
        assert!(
            acl.bootstrap_admin(attacker).is_err(),
            "the bootstrap path stayed open after the first administrator"
        );
    }

    #[test]
    fn a_delegated_scope_can_be_taken_back() {
        // There was no revoke at all: a scope, once delegated, was permanent
        // for the life of the directory, and so was a role.
        let mut acl = WorkspaceAccessControl::new();
        let admin = acl.add_user("admin");
        let user = acl.add_user("user");
        acl.bootstrap_admin(admin).unwrap();

        let scope = Scope("deploy".to_string());
        acl.delegate_scope(admin, user, scope.clone()).unwrap();
        assert!(acl.check_scope(user, &scope).is_ok());

        acl.revoke_scope(admin, user, &scope).unwrap();
        assert!(acl.check_scope(user, &scope).is_err());

        // A user may not revoke its own way around the admin check.
        acl.delegate_scope(admin, user, scope.clone()).unwrap();
        assert!(acl.revoke_scope(user, admin, &scope).is_err());
    }

    #[test]
    fn the_last_administrator_cannot_be_demoted() {
        let mut acl = WorkspaceAccessControl::new();
        let admin = acl.add_user("admin");
        acl.bootstrap_admin(admin).unwrap();
        assert!(
            acl.revoke_admin(admin, admin).is_err(),
            "the directory was left with no administrator and no way back"
        );

        let second = acl.add_user("second");
        acl.grant_admin(admin, second).unwrap();
        acl.revoke_admin(admin, second).unwrap();
        assert!(acl
            .check_scope(second, &Scope("anything".to_string()))
            .is_err());
    }
}
