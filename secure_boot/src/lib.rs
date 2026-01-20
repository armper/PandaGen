//! Secure update and attested boot chain.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentMeasurement {
    pub name: String,
    pub digest: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeasurementLog {
    pub components: Vec<ComponentMeasurement>,
}

impl MeasurementLog {
    pub fn record(&mut self, name: impl Into<String>, digest: String) {
        self.components.push(ComponentMeasurement {
            name: name.into(),
            digest,
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootPolicy {
    pub required: BTreeMap<String, String>,
}

impl BootPolicy {
    pub fn new() -> Self {
        Self {
            required: BTreeMap::new(),
        }
    }

    pub fn require(mut self, name: impl Into<String>, digest: impl Into<String>) -> Self {
        self.required.insert(name.into(), digest.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    pub measurements: MeasurementLog,
    pub policy_digest: String,
}

#[derive(Debug, Error)]
pub enum BootError {
    #[error("Policy missing required component: {0}")]
    MissingComponent(String),

    #[error("Digest mismatch for {name}: expected {expected}, got {actual}")]
    DigestMismatch {
        name: String,
        expected: String,
        actual: String,
    },
}

pub struct BootVerifier {
    log: MeasurementLog,
}

impl BootVerifier {
    pub fn new() -> Self {
        Self {
            log: MeasurementLog::default(),
        }
    }

    pub fn measure_component(&mut self, name: impl Into<String>, bytes: &[u8]) {
        let digest = hash_bytes(bytes);
        self.log.record(name, digest);
    }

    pub fn verify_policy(&self, policy: &BootPolicy) -> Result<(), BootError> {
        for (name, expected) in &policy.required {
            let actual = self
                .log
                .components
                .iter()
                .find(|c| &c.name == name)
                .map(|c| c.digest.clone())
                .ok_or_else(|| BootError::MissingComponent(name.clone()))?;

            if &actual != expected {
                return Err(BootError::DigestMismatch {
                    name: name.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }

    pub fn attest(&self, policy: &BootPolicy) -> Result<AttestationReport, BootError> {
        self.verify_policy(policy)?;
        Ok(AttestationReport {
            measurements: self.log.clone(),
            policy_digest: hash_policy(policy),
        })
    }

    pub fn log(&self) -> &MeasurementLog {
        &self.log
    }
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn hash_policy(policy: &BootPolicy) -> String {
    let mut hasher = Sha256::new();
    for (name, digest) in &policy.required {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(digest.as_bytes());
        hasher.update(b";");
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_verifier_success() {
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"kernel-binary");
        verifier.measure_component("policy", b"policy");

        let policy = BootPolicy::new()
            .require("kernel", hash_bytes(b"kernel-binary"))
            .require("policy", hash_bytes(b"policy"));

        verifier.verify_policy(&policy).unwrap();
        let report = verifier.attest(&policy).unwrap();
        assert_eq!(report.measurements.components.len(), 2);
    }

    #[test]
    fn test_boot_verifier_mismatch() {
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"kernel-binary");

        let policy = BootPolicy::new().require("kernel", hash_bytes(b"other"));
        let result = verifier.verify_policy(&policy);
        assert!(matches!(result, Err(BootError::DigestMismatch { .. })));
    }
}
