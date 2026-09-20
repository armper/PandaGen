//! Stable service identifiers for core system services.

use crate::ServiceId;

const CONSOLE_SERVICE_ID: u128 = 0x2b2f_8f83_4d77_4d6f_9b9f_0d8a_5c2a_9e11u128;
const COMMAND_SERVICE_ID: u128 = 0x3c1a_1d5e_2f14_4a4a_8e9c_7b3c_19f0_7a22u128;
const TIMER_SERVICE_ID: u128 = 0x5d8b_2af1_7d2a_4a97_9c4d_2e4b_1c7e_6b33u128;
const INPUT_SERVICE_ID: u128 = 0x91a7_2f0e_c9c3_4d8a_8e76_0e8c_9f0a_2d4bu128;

/// Stable service ID for the console service.
pub fn console_service_id() -> ServiceId {
    ServiceId::from_u128(CONSOLE_SERVICE_ID)
}

/// Stable service ID for the command service.
pub fn command_service_id() -> ServiceId {
    ServiceId::from_u128(COMMAND_SERVICE_ID)
}

/// Stable service ID for the timer service.
pub fn timer_service_id() -> ServiceId {
    ServiceId::from_u128(TIMER_SERVICE_ID)
}

/// Stable service ID for the input service.
pub fn input_service_id() -> ServiceId {
    ServiceId::from_u128(INPUT_SERVICE_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These four numbers are the wire identity of the core services --
    /// `input_service_id()` is stamped into every envelope `services_input`
    /// builds -- so the one test named for their stability has to pin them
    /// to something other than themselves.
    ///
    /// It used to read `assert_eq!(console_service_id(),
    /// ServiceId::from_u128(CONSOLE_SERVICE_ID))`, which restates the
    /// function body. Changing a constant to `0xdead_beef...` left the whole
    /// crate's suite green: a tautology in the shape of a guarantee.
    #[test]
    fn test_core_service_ids_stable() {
        assert_eq!(
            console_service_id().as_uuid().as_u128(),
            0x2b2f_8f83_4d77_4d6f_9b9f_0d8a_5c2a_9e11u128,
            "the console service's identity changed"
        );
        assert_eq!(
            command_service_id().as_uuid().as_u128(),
            0x3c1a_1d5e_2f14_4a4a_8e9c_7b3c_19f0_7a22u128,
            "the command service's identity changed"
        );
        assert_eq!(
            timer_service_id().as_uuid().as_u128(),
            0x5d8b_2af1_7d2a_4a97_9c4d_2e4b_1c7e_6b33u128,
            "the timer service's identity changed"
        );
        assert_eq!(
            input_service_id().as_uuid().as_u128(),
            0x91a7_2f0e_c9c3_4d8a_8e76_0e8c_9f0a_2d4bu128,
            "the input service's identity changed"
        );
    }

    /// And they are four different services.
    #[test]
    fn the_core_service_ids_are_distinct() {
        let ids = [
            console_service_id(),
            command_service_id(),
            timer_service_id(),
            input_service_id(),
        ];
        for (i, a) in ids.iter().enumerate() {
            for b in ids.iter().skip(i + 1) {
                assert_ne!(a, b, "two core services share an identity");
            }
        }
    }
}
