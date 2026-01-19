//! Device model and driver sandbox framework.

use core_types::Cap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(Uuid);

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DriverId(Uuid);

impl DriverId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Input,
    Display,
    Storage,
    Network,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceResource {
    IoPort { base: u16, len: u16 },
    MemoryMapped { base: u64, len: u64 },
    Interrupt { irq: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub id: DeviceId,
    pub class: DeviceClass,
    pub vendor: String,
    pub product: String,
    pub resources: Vec<DeviceResource>,
}

impl DeviceDescriptor {
    pub fn new(class: DeviceClass, vendor: impl Into<String>, product: impl Into<String>) -> Self {
        Self {
            id: DeviceId::new(),
            class,
            vendor: vendor.into(),
            product: product.into(),
            resources: Vec::new(),
        }
    }

    pub fn with_resource(mut self, resource: DeviceResource) -> Self {
        self.resources.push(resource);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverDecision {
    Allow,
    Deny { reason: String },
}

pub trait DriverPolicy: Send + Sync {
    fn evaluate(&self, driver_id: DriverId, device: &DeviceDescriptor) -> DriverDecision;
}

pub struct AllowAllDrivers;

impl DriverPolicy for AllowAllDrivers {
    fn evaluate(&self, _driver_id: DriverId, _device: &DeviceDescriptor) -> DriverDecision {
        DriverDecision::Allow
    }
}

pub struct DenyAllDrivers;

impl DriverPolicy for DenyAllDrivers {
    fn evaluate(&self, _driver_id: DriverId, _device: &DeviceDescriptor) -> DriverDecision {
        DriverDecision::Deny {
            reason: "Drivers are sandboxed and denied".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverHandleCap {
    pub driver_id: DriverId,
    pub device_id: DeviceId,
    token: u64,
}

impl DriverHandleCap {
    fn new(driver_id: DriverId, device_id: DeviceId, token: u64) -> Self {
        Self {
            driver_id,
            device_id,
            token,
        }
    }
}

#[derive(Debug, Error)]
pub enum DeviceManagerError {
    #[error("Driver not registered: {0:?}")]
    DriverNotRegistered(DriverId),

    #[error("Device not found: {0:?}")]
    DeviceNotFound(DeviceId),

    #[error("Policy denied driver: {0}")]
    PolicyDenied(String),

    #[error("Invalid driver capability")]
    InvalidCapability,
}

#[derive(Debug)]
struct DriverRecord {
    id: DriverId,
    name: String,
}

/// Device manager that provides sandboxed driver access.
pub struct DeviceManager {
    policy: Box<dyn DriverPolicy>,
    drivers: HashMap<DriverId, DriverRecord>,
    devices: HashMap<DeviceId, DeviceDescriptor>,
    tokens: HashMap<(DriverId, DeviceId), u64>,
    next_token: u64,
}

impl DeviceManager {
    pub fn new(policy: Box<dyn DriverPolicy>) -> Self {
        Self {
            policy,
            drivers: HashMap::new(),
            devices: HashMap::new(),
            tokens: HashMap::new(),
            next_token: 1,
        }
    }

    pub fn register_driver(&mut self, name: impl Into<String>) -> DriverId {
        let id = DriverId::new();
        self.drivers.insert(
            id,
            DriverRecord {
                id,
                name: name.into(),
            },
        );
        id
    }

    pub fn register_device(&mut self, descriptor: DeviceDescriptor) -> DeviceId {
        let id = descriptor.id;
        self.devices.insert(id, descriptor);
        id
    }

    pub fn attach_driver(
        &mut self,
        driver_id: DriverId,
        device_id: DeviceId,
    ) -> Result<DriverHandleCap, DeviceManagerError> {
        let driver = self
            .drivers
            .get(&driver_id)
            .ok_or(DeviceManagerError::DriverNotRegistered(driver_id))?;
        let device = self
            .devices
            .get(&device_id)
            .ok_or(DeviceManagerError::DeviceNotFound(device_id))?;

        match self.policy.evaluate(driver.id, device) {
            DriverDecision::Allow => {}
            DriverDecision::Deny { reason } => {
                return Err(DeviceManagerError::PolicyDenied(reason))
            }
        }

        let token = self.next_token;
        self.next_token += 1;
        self.tokens.insert((driver_id, device_id), token);
        Ok(DriverHandleCap::new(driver_id, device_id, token))
    }

    pub fn open_device(
        &self,
        cap: &DriverHandleCap,
    ) -> Result<Cap<DeviceDescriptor>, DeviceManagerError> {
        let token = self
            .tokens
            .get(&(cap.driver_id, cap.device_id))
            .ok_or(DeviceManagerError::InvalidCapability)?;
        if *token != cap.token {
            return Err(DeviceManagerError::InvalidCapability);
        }
        Ok(Cap::new(cap.token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_attach_and_open() {
        let mut manager = DeviceManager::new(Box::new(AllowAllDrivers));
        let driver = manager.register_driver("keyboard");
        let device = manager.register_device(
            DeviceDescriptor::new(DeviceClass::Input, "Panda", "Keyboard")
                .with_resource(DeviceResource::IoPort { base: 0x60, len: 4 }),
        );

        let cap = manager.attach_driver(driver, device).unwrap();
        let handle = manager.open_device(&cap).unwrap();
        assert_eq!(handle.id(), cap.token);
    }

    #[test]
    fn test_driver_policy_denies() {
        let mut manager = DeviceManager::new(Box::new(DenyAllDrivers));
        let driver = manager.register_driver("gpu");
        let device = manager.register_device(DeviceDescriptor::new(
            DeviceClass::Display,
            "Panda",
            "Display",
        ));

        let result = manager.attach_driver(driver, device);
        assert!(matches!(result, Err(DeviceManagerError::PolicyDenied(_))));
    }
}
