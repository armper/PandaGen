//! Measured boot: a log of component digests, checked against a policy.
//!
//! **This is not verified boot and it is not attestation.** There is no
//! signature here and no root of trust: `verify_policy` compares digests
//! against a `BootPolicy` *the caller supplies*, so it establishes only that
//! the caller got what the caller expected. An attacker who supplies the
//! policy is comparing the measurements against their own expectations.
//! Real attestation requires a key this machine does not hold and a
//! quote this crate does not produce.
//!
//! Nothing in the workspace calls any of this. `kernel_bootstrap`, `boot/`
//! and `xtask` measure nothing and verify nothing, so the shipped image has
//! no boot chain at all -- the name of this crate describes an intention,
//! not a property of the system. That is recorded in GAUNTLET.md as open
//! work rather than papered over here.
//!
//! What it does do honestly: it refuses a policy that does not account for
//! everything measured, which is the difference between "the components I
//! named are as I expected" and "the machine booted what I expected".

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

impl Default for BootPolicy {
    fn default() -> Self {
        Self::new()
    }
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
    /// Digest of what was measured. Unsigned, so it is a summary and not
    /// evidence.
    pub measurements_digest: String,
    /// Digest of the policy that was applied -- the caller's own, so it
    /// identifies the policy rather than attesting to anything.
    pub policy_digest: String,
}

#[derive(Debug, Error)]
pub enum BootError {
    #[error("Policy missing required component: {0}")]
    MissingComponent(String),

    #[error("Component {0} was measured but the policy does not account for it")]
    Unaccounted(String),

    #[error("Component {0} was measured more than once")]
    Duplicate(String),

    #[error("The policy requires nothing, so it accepts any measurement")]
    EmptyPolicy,

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

impl Default for BootVerifier {
    fn default() -> Self {
        Self::new()
    }
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

    /// Check every measurement against the policy.
    ///
    /// The loop used to run over `policy.required` only, so anything
    /// measured but not named was unconstrained -- and an empty policy
    /// returned `Ok` for a fully tampered log. A policy that does not
    /// account for what was measured has not verified the boot; it has
    /// verified a subset of the caller's choosing.
    pub fn verify_policy(&self, policy: &BootPolicy) -> Result<(), BootError> {
        if policy.required.is_empty() {
            return Err(BootError::EmptyPolicy);
        }
        let mut seen = BTreeMap::new();
        for measured in &self.log.components {
            if seen.insert(measured.name.clone(), ()).is_some() {
                return Err(BootError::Duplicate(measured.name.clone()));
            }
            if !policy.required.contains_key(&measured.name) {
                return Err(BootError::Unaccounted(measured.name.clone()));
            }
        }
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

    /// A report of what was measured, once it matches the policy.
    ///
    /// The `policy_digest` is a hash of the policy the *caller* passed, so
    /// it certifies the caller's own expectations and is worth nothing as
    /// evidence to anyone else. It is kept because it usefully identifies
    /// *which* policy was applied; `measurements_digest` is the part that
    /// describes the machine.
    pub fn attest(&self, policy: &BootPolicy) -> Result<AttestationReport, BootError> {
        self.verify_policy(policy)?;
        Ok(AttestationReport {
            measurements: self.log.clone(),
            measurements_digest: hash_measurements(&self.log),
            policy_digest: hash_policy(policy),
        })
    }

    pub fn log(&self) -> &MeasurementLog {
        &self.log
    }
}

/// Digest of a measurement log, in the order it was recorded.
pub fn hash_measurements(log: &MeasurementLog) -> String {
    let mut hasher = Sha256::new();
    for component in &log.components {
        hasher.update(component.name.as_bytes());
        hasher.update(b"=");
        hasher.update(component.digest.as_bytes());
        hasher.update(b";");
    }
    hex::encode(hasher.finalize())
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

#[cfg(test)]
mod accounting_tests {
    use super::*;

    #[test]
    fn an_empty_policy_does_not_accept_a_tampered_boot() {
        // The loop ran over `policy.required` only, so an empty policy
        // returned Ok for any measurement at all -- a fully tampered log
        // passed, and `attest` then reported it as verified.
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"TAMPERED");
        assert!(matches!(
            verifier.verify_policy(&BootPolicy::new()),
            Err(BootError::EmptyPolicy)
        ));
        assert!(verifier.attest(&BootPolicy::new()).is_err());
    }

    #[test]
    fn a_component_the_policy_does_not_name_is_refused() {
        // Anything measured but not named was unconstrained, so a policy
        // that mentioned the kernel said nothing about the five other things
        // that were loaded.
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"good");
        verifier.measure_component("smuggled", b"whatever");

        let policy = BootPolicy::new().require("kernel", hash_bytes(b"good"));
        assert!(matches!(
            verifier.verify_policy(&policy),
            Err(BootError::Unaccounted(name)) if name == "smuggled"
        ));

        // Accounting for it is enough to pass, which is the point: the
        // policy must describe the whole boot.
        let policy = policy.require("smuggled", hash_bytes(b"whatever"));
        assert!(verifier.verify_policy(&policy).is_ok());
    }

    #[test]
    fn a_required_component_that_was_not_measured_is_refused() {
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"good");
        let policy = BootPolicy::new()
            .require("kernel", hash_bytes(b"good"))
            .require("initrd", hash_bytes(b"absent"));
        assert!(matches!(
            verifier.verify_policy(&policy),
            Err(BootError::MissingComponent(name)) if name == "initrd"
        ));
    }

    #[test]
    fn a_component_measured_twice_is_refused() {
        // Otherwise a second measurement of the same name can sit behind the
        // one the policy matched.
        let mut verifier = BootVerifier::new();
        verifier.measure_component("kernel", b"good");
        verifier.measure_component("kernel", b"TAMPERED");
        let policy = BootPolicy::new().require("kernel", hash_bytes(b"good"));
        assert!(matches!(
            verifier.verify_policy(&policy),
            Err(BootError::Duplicate(_))
        ));
    }
}
