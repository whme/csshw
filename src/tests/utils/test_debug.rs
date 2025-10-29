//! Tests for debug utilities

use crate::utils::debug::StringRepr;
use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

#[cfg(test)]
mod debug_test {
    use super::*;

    #[test]
    fn test_key_event_record_0_string_repr() {
        // Test string representation of KEY_EVENT_RECORD_0
        let key_event_record_0 = KEY_EVENT_RECORD_0 {
            UnicodeChar: 'A' as u16,
        };

        let repr = key_event_record_0.string_repr();
        assert!(repr.contains("unicode_char:"));
        assert!(repr.contains(&format!("{}", 'A' as u16)));
    }

    #[test]
    fn test_key_event_record_0_string_repr_special_chars() {
        // Test with special characters
        let test_chars = vec![
            ('\n', "newline"),
            ('\t', "tab"),
            (' ', "space"),
            ('€', "euro"),
            ('🦀', "crab emoji"),
        ];

        for (test_char, description) in test_chars {
            let key_event_record_0 = KEY_EVENT_RECORD_0 {
                UnicodeChar: test_char as u16,
            };

            let repr = key_event_record_0.string_repr();
            assert!(repr.contains("unicode_char:"), "Failed for {description}");
            assert!(
                repr.contains(&format!("{}", test_char as u16)),
                "Failed for {description}"
            );
        }
    }

    #[test]
    fn test_key_event_record_string_repr() {
        // Test string representation of KEY_EVENT_RECORD
        let key_event_record = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(1),
            wRepeatCount: 2,
            wVirtualKeyCode: 0x41, // 'A' key
            wVirtualScanCode: 0x1E,
            uChar: KEY_EVENT_RECORD_0 {
                UnicodeChar: 'A' as u16,
            },
            dwControlKeyState: 0x0008, // SHIFT_PRESSED
        };

        let repr = key_event_record.string_repr();

        // Check that all fields are represented
        assert!(repr.contains("key_down: true"));
        assert!(repr.contains("repeat_count: 2"));
        assert!(repr.contains("virtual_key_code: 0x41"));
        assert!(repr.contains("virtual_scan_code: 0x1e"));
        assert!(repr.contains("char: 0x41"));
        assert!(repr.contains("control_key_state: 8"));
    }

    #[test]
    fn test_key_event_record_string_repr_key_up() {
        // Test with key up event
        let key_event_record = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(0),
            wRepeatCount: 1,
            wVirtualKeyCode: 0x42, // 'B' key
            wVirtualScanCode: 0x30,
            uChar: KEY_EVENT_RECORD_0 {
                UnicodeChar: 'B' as u16,
            },
            dwControlKeyState: 0x0000, // No control keys
        };

        let repr = key_event_record.string_repr();

        assert!(repr.contains("key_down: false"));
        assert!(repr.contains("repeat_count: 1"));
        assert!(repr.contains("virtual_key_code: 0x42"));
        assert!(repr.contains("virtual_scan_code: 0x30"));
        assert!(repr.contains("char: 0x42"));
        assert!(repr.contains("control_key_state: 0"));
    }

    #[test]
    fn test_key_event_record_string_repr_control_keys() {
        // Test with various control key states
        let control_key_states = vec![
            (0x0001, "RIGHT_ALT_PRESSED"),
            (0x0002, "LEFT_ALT_PRESSED"),
            (0x0004, "RIGHT_CTRL_PRESSED"),
            (0x0008, "SHIFT_PRESSED"),
            (0x0010, "NUMLOCK_ON"),
            (0x0020, "SCROLLLOCK_ON"),
            (0x0040, "CAPSLOCK_ON"),
            (0x0080, "ENHANCED_KEY"),
        ];

        for (state, description) in control_key_states {
            let key_event_record = KEY_EVENT_RECORD {
                bKeyDown: windows::Win32::Foundation::BOOL(1),
                wRepeatCount: 1,
                wVirtualKeyCode: 0x43, // 'C' key
                wVirtualScanCode: 0x2E,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: 'C' as u16,
                },
                dwControlKeyState: state,
            };

            let repr = key_event_record.string_repr();
            assert!(
                repr.contains(&format!("control_key_state: {state}")),
                "Failed for {description}"
            );
        }
    }

    #[test]
    fn test_input_record_0_string_repr() {
        // Test string representation of INPUT_RECORD_0
        let input_record_0 = INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: windows::Win32::Foundation::BOOL(1),
                wRepeatCount: 3,
                wVirtualKeyCode: 0x44, // 'D' key
                wVirtualScanCode: 0x20,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: 'D' as u16,
                },
                dwControlKeyState: 0x0002, // LEFT_ALT_PRESSED
            },
        };

        let repr = input_record_0.string_repr();

        // Should contain the same information as the underlying KeyEvent
        assert!(repr.contains("key_down: true"));
        assert!(repr.contains("repeat_count: 3"));
        assert!(repr.contains("virtual_key_code: 0x44"));
        assert!(repr.contains("virtual_scan_code: 0x20"));
        assert!(repr.contains("char: 0x44"));
        assert!(repr.contains("control_key_state: 2"));
    }

    #[test]
    fn test_string_repr_format_consistency() {
        // Test that the format is consistent and parseable
        let key_event_record = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(1),
            wRepeatCount: 1,
            wVirtualKeyCode: 0x20, // Space key
            wVirtualScanCode: 0x39,
            uChar: KEY_EVENT_RECORD_0 {
                UnicodeChar: ' ' as u16,
            },
            dwControlKeyState: 0x0000,
        };

        let repr = key_event_record.string_repr();

        // Check that the format uses commas and newlines for separation
        let lines: Vec<&str> = repr.split(",\n").collect();
        assert_eq!(lines.len(), 6); // Should have 6 fields

        // Each line should contain a colon
        for line in lines {
            assert!(line.contains(":"), "Line '{line}' should contain a colon");
        }
    }

    #[test]
    fn test_string_repr_hex_formatting() {
        // Test that hex values are properly formatted
        let key_event_record = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(1),
            wRepeatCount: 1,
            wVirtualKeyCode: 0xFF, // Maximum value
            wVirtualScanCode: 0xAB,
            uChar: KEY_EVENT_RECORD_0 {
                UnicodeChar: 0x1234,
            },
            dwControlKeyState: 0x00FF,
        };

        let repr = key_event_record.string_repr();

        // Check hex formatting (should be lowercase)
        assert!(repr.contains("virtual_key_code: 0xff"));
        assert!(repr.contains("virtual_scan_code: 0xab"));
        assert!(repr.contains("char: 0x1234"));
        assert!(repr.contains("control_key_state: 255")); // This one is decimal
    }
}
