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

    #[test]
    fn test_core_service_ids_stable() {
        assert_eq!(console_service_id(), ServiceId::from_u128(CONSOLE_SERVICE_ID));
        assert_eq!(command_service_id(), ServiceId::from_u128(COMMAND_SERVICE_ID));
        assert_eq!(timer_service_id(), ServiceId::from_u128(TIMER_SERVICE_ID));
        assert_eq!(input_service_id(), ServiceId::from_u128(INPUT_SERVICE_ID));
    }
}
