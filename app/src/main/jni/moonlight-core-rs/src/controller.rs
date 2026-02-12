//! Controller type detection and identification
//!
//! Ported from controller_type.h, controller_list.h, and minisdl.c

use crate::ffi::{LI_CTYPE_NINTENDO, LI_CTYPE_PS, LI_CTYPE_UNKNOWN, LI_CTYPE_XBOX};
use crate::usb_ids::*;

/// Controller type enumeration (from Valve's controller database)
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ControllerType {
    None = -1,
    Unknown = 0,

    // Steam Controllers
    UnknownSteamController = 1,
    SteamController = 2,
    SteamControllerV2 = 3,

    // Other Controllers
    UnknownNonSteamController = 30,
    XBox360Controller = 31,
    XBoxOneController = 32,
    PS3Controller = 33,
    PS4Controller = 34,
    WiiController = 35,
    AppleController = 36,
    AndroidController = 37,
    SwitchProController = 38,
    SwitchJoyConLeft = 39,
    SwitchJoyConRight = 40,
    SwitchJoyConPair = 41,
    SwitchInputOnlyController = 42,
    MobileTouch = 43,
    XInputSwitchController = 44,
    PS5Controller = 45,
    XInputPS4Controller = 46,
}

/// Controller description entry
#[derive(Debug, Copy, Clone)]
pub struct ControllerDescription {
    pub device_id: u32,
    pub controller_type: ControllerType,
}

/// Create a controller device ID from vendor and product IDs
#[inline]
pub const fn make_controller_id(vid: u16, pid: u16) -> u32 {
    ((vid as u32) << 16) | (pid as u32)
}

/// Check if a joystick is an Xbox One Elite controller
pub fn is_joystick_xbox_one_elite(vendor_id: u16, product_id: u16) -> bool {
    if vendor_id == USB_VENDOR_MICROSOFT {
        matches!(
            product_id,
            USB_PRODUCT_XBOX_ONE_ELITE_SERIES_1
                | USB_PRODUCT_XBOX_ONE_ELITE_SERIES_2
                | USB_PRODUCT_XBOX_ONE_ELITE_SERIES_2_BLUETOOTH
                | USB_PRODUCT_XBOX_ONE_ELITE_SERIES_2_BLE
        )
    } else {
        false
    }
}

/// Check if a joystick is an Xbox Series X controller
pub fn is_joystick_xbox_series_x(vendor_id: u16, product_id: u16) -> bool {
    match vendor_id {
        USB_VENDOR_MICROSOFT => matches!(
            product_id,
            USB_PRODUCT_XBOX_SERIES_X | USB_PRODUCT_XBOX_SERIES_X_BLE
        ),
        USB_VENDOR_PDP => matches!(
            product_id,
            USB_PRODUCT_XBOX_SERIES_X_VICTRIX_GAMBIT
                | USB_PRODUCT_XBOX_SERIES_X_PDP_BLUE
                | USB_PRODUCT_XBOX_SERIES_X_PDP_AFTERGLOW
        ),
        USB_VENDOR_POWERA_ALT => {
            (product_id >= 0x2001 && product_id <= 0x201a)
                || matches!(
                    product_id,
                    USB_PRODUCT_XBOX_SERIES_X_POWERA_FUSION_PRO2
                        | USB_PRODUCT_XBOX_SERIES_X_POWERA_MOGA_XP_ULTRA
                        | USB_PRODUCT_XBOX_SERIES_X_POWERA_SPECTRA
                )
        }
        USB_VENDOR_HORI => matches!(
            product_id,
            USB_PRODUCT_HORI_FIGHTING_COMMANDER_OCTA_SERIES_X | USB_PRODUCT_HORI_HORIPAD_PRO_SERIES_X
        ),
        USB_VENDOR_RAZER => matches!(
            product_id,
            USB_PRODUCT_RAZER_WOLVERINE_V2 | USB_PRODUCT_RAZER_WOLVERINE_V2_CHROMA
        ),
        USB_VENDOR_THRUSTMASTER => product_id == USB_PRODUCT_THRUSTMASTER_ESWAPX_PRO,
        USB_VENDOR_TURTLE_BEACH => matches!(
            product_id,
            USB_PRODUCT_TURTLE_BEACH_SERIES_X_REACT_R | USB_PRODUCT_TURTLE_BEACH_SERIES_X_RECON
        ),
        USB_VENDOR_8BITDO => product_id == USB_PRODUCT_8BITDO_XBOX_CONTROLLER,
        USB_VENDOR_GAMESIR => product_id == USB_PRODUCT_GAMESIR_G7,
        _ => false,
    }
}

/// Check if a joystick is a DualSense Edge controller
pub fn is_joystick_dualsense_edge(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == USB_VENDOR_SONY && product_id == USB_PRODUCT_SONY_DS5_EDGE
}

/// Controller database (subset of the full list for common controllers)
/// This is a partial list - the full list from controller_list.h has 500+ entries
static CONTROLLERS: &[ControllerDescription] = &[
    // PS3 Controllers
    ControllerDescription { device_id: make_controller_id(0x054c, 0x0268), controller_type: ControllerType::PS3Controller },

    // PS4 Controllers
    ControllerDescription { device_id: make_controller_id(0x054c, 0x05c4), controller_type: ControllerType::PS4Controller },
    ControllerDescription { device_id: make_controller_id(0x054c, 0x09cc), controller_type: ControllerType::PS4Controller },
    ControllerDescription { device_id: make_controller_id(0x054c, 0x0ba0), controller_type: ControllerType::PS4Controller },

    // PS5 Controllers
    ControllerDescription { device_id: make_controller_id(0x054c, 0x0ce6), controller_type: ControllerType::PS5Controller },
    ControllerDescription { device_id: make_controller_id(0x054c, 0x0df2), controller_type: ControllerType::PS5Controller },

    // Xbox 360 Controllers
    ControllerDescription { device_id: make_controller_id(0x045e, 0x028e), controller_type: ControllerType::XBox360Controller },
    ControllerDescription { device_id: make_controller_id(0x045e, 0x028f), controller_type: ControllerType::XBox360Controller },
    ControllerDescription { device_id: make_controller_id(0x045e, 0x0291), controller_type: ControllerType::XBox360Controller },
    ControllerDescription { device_id: make_controller_id(0x045e, 0x0719), controller_type: ControllerType::XBox360Controller },
    ControllerDescription { device_id: make_controller_id(0x046d, 0xc21d), controller_type: ControllerType::XBox360Controller }, // Logitech F310
    ControllerDescription { device_id: make_controller_id(0x046d, 0xc21e), controller_type: ControllerType::XBox360Controller }, // Logitech F510
    ControllerDescription { device_id: make_controller_id(0x046d, 0xc21f), controller_type: ControllerType::XBox360Controller }, // Logitech F710
    ControllerDescription { device_id: make_controller_id(0x0955, 0x7210), controller_type: ControllerType::XBox360Controller }, // Nvidia Shield

    // Xbox One Controllers
    ControllerDescription { device_id: make_controller_id(0x045e, 0x02d1), controller_type: ControllerType::XBoxOneController },
    ControllerDescription { device_id: make_controller_id(0x045e, 0x02dd), controller_type: ControllerType::XBoxOneController },
    ControllerDescription { device_id: make_controller_id(0x045e, 0x02e3), controller_type: ControllerType::XBoxOneController }, // Elite Series 1
    ControllerDescription { device_id: make_controller_id(0x045e, 0x02ea), controller_type: ControllerType::XBoxOneController }, // Xbox One S
    ControllerDescription { device_id: make_controller_id(0x045e, 0x0b00), controller_type: ControllerType::XBoxOneController }, // Elite Series 2
    ControllerDescription { device_id: make_controller_id(0x045e, 0x0b12), controller_type: ControllerType::XBoxOneController }, // Xbox Series X

    // Nintendo Switch Controllers
    ControllerDescription { device_id: make_controller_id(0x057e, 0x2006), controller_type: ControllerType::SwitchJoyConLeft },
    ControllerDescription { device_id: make_controller_id(0x057e, 0x2007), controller_type: ControllerType::SwitchJoyConRight },
    ControllerDescription { device_id: make_controller_id(0x057e, 0x2009), controller_type: ControllerType::SwitchProController },
    ControllerDescription { device_id: make_controller_id(0x057e, 0x200e), controller_type: ControllerType::SwitchJoyConPair },
];

/// Guess the controller type from vendor and product IDs
/// Returns the Limelight controller type constant
pub fn guess_controller_type(vendor_id: i32, product_id: i32) -> i8 {
    let device_id = make_controller_id(vendor_id as u16, product_id as u16);

    for controller in CONTROLLERS.iter() {
        if device_id == controller.device_id {
            return match controller.controller_type {
                ControllerType::XBox360Controller | ControllerType::XBoxOneController => LI_CTYPE_XBOX,
                ControllerType::PS3Controller | ControllerType::PS4Controller | ControllerType::PS5Controller => LI_CTYPE_PS,
                ControllerType::WiiController
                | ControllerType::SwitchProController
                | ControllerType::SwitchJoyConLeft
                | ControllerType::SwitchJoyConRight
                | ControllerType::SwitchJoyConPair
                | ControllerType::SwitchInputOnlyController => LI_CTYPE_NINTENDO,
                _ => LI_CTYPE_UNKNOWN,
            };
        }
    }

    LI_CTYPE_UNKNOWN
}

/// Check if a controller has paddle buttons
pub fn guess_controller_has_paddles(vendor_id: i32, product_id: i32) -> bool {
    is_joystick_xbox_one_elite(vendor_id as u16, product_id as u16)
        || is_joystick_dualsense_edge(vendor_id as u16, product_id as u16)
}

/// Check if a controller has a share button
pub fn guess_controller_has_share_button(vendor_id: i32, product_id: i32) -> bool {
    is_joystick_xbox_series_x(vendor_id as u16, product_id as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xbox_series_x_detection() {
        assert!(is_joystick_xbox_series_x(USB_VENDOR_MICROSOFT, USB_PRODUCT_XBOX_SERIES_X));
        assert!(!is_joystick_xbox_series_x(USB_VENDOR_MICROSOFT, USB_PRODUCT_XBOX360_WIRED_CONTROLLER));
    }

    #[test]
    fn test_elite_detection() {
        assert!(is_joystick_xbox_one_elite(USB_VENDOR_MICROSOFT, USB_PRODUCT_XBOX_ONE_ELITE_SERIES_2));
        assert!(!is_joystick_xbox_one_elite(USB_VENDOR_SONY, USB_PRODUCT_SONY_DS5));
    }

    #[test]
    fn test_dualsense_edge_detection() {
        assert!(is_joystick_dualsense_edge(USB_VENDOR_SONY, USB_PRODUCT_SONY_DS5_EDGE));
        assert!(!is_joystick_dualsense_edge(USB_VENDOR_SONY, USB_PRODUCT_SONY_DS5));
    }

    #[test]
    fn test_controller_type_guessing() {
        // Xbox controller
        assert_eq!(guess_controller_type(0x045e, 0x028e), LI_CTYPE_XBOX);
        // PS5 controller
        assert_eq!(guess_controller_type(0x054c, 0x0ce6), LI_CTYPE_PS);
        // Switch Pro controller
        assert_eq!(guess_controller_type(0x057e, 0x2009), LI_CTYPE_NINTENDO);
        // Unknown controller
        assert_eq!(guess_controller_type(0x1234, 0x5678), LI_CTYPE_UNKNOWN);
    }
}

