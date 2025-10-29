//! Unit tests for the utils mod module using mockall for Windows API mocking.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use crate::utils::{
    clear_screen_with_api, get_console_title_with_api, is_windows_10_with_api,
    read_console_input_with_api, read_keyboard_input_with_api, set_console_border_color_with_api,
    set_console_color_with_api, set_console_title_with_api, utf16_buffer_to_string, MockWindowsApi,
    KEY_EVENT,
};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Console::{
    CONSOLE_CHARACTER_ATTRIBUTES, CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD, INPUT_RECORD_0,
    MOUSE_EVENT,
};

/// Tests Windows version detection.
mod version_detection_test {
    use super::*;

    /// Tests that Windows 8.1 is correctly classified as "Windows 10 or older".
    /// Validates version parsing for major versions less than 10.
    #[test]
    fn test_is_windows_10_with_windows_8_version() {
        let mut mock_api = MockWindowsApi::new();
        mock_api
            .expect_get_os_version()
            .times(1)
            .return_const("6.3.9600".to_string());

        let result = is_windows_10_with_api(&mock_api);
        assert!(
            result,
            "Should detect Windows 6.3.9600 as Windows 10 or older (major <= 10)"
        );
    }

    /// Tests that future Windows versions are correctly classified as newer than Windows 10.
    /// Validates detection of Windows 11+ versions with major > 10.
    #[test]
    fn test_is_windows_10_with_future_version() {
        let mut mock_api = MockWindowsApi::new();
        mock_api
            .expect_get_os_version()
            .times(1)
            .return_const("11.0.25000".to_string());

        let result = is_windows_10_with_api(&mock_api);
        assert!(
            !result,
            "Should detect Windows 11.0.25000 as newer than Windows 10"
        );
    }

    /// Tests Windows 10/11 boundary detection at build 22000.
    /// Validates that build 21999 is Windows 10 and 22000+ is Windows 11.
    #[test]
    fn test_is_windows_10_boundary_cases() {
        let test_cases = vec![
            ("10.0.21999", true),
            ("10.0.22000", false),
            ("10.0.19045", true),
            ("10.0.17763", true),
        ];

        for (version, expected) in test_cases {
            let mut mock_api = MockWindowsApi::new();
            mock_api
                .expect_get_os_version()
                .times(1)
                .return_const(version.to_string());

            let result = is_windows_10_with_api(&mock_api);
            assert_eq!(
                result, expected,
                "Version {version} should return {expected}"
            );
        }
    }

    /// Tests that malformed version strings cause the function to panic.
    /// Validates error handling for unparseable version input.
    #[test]
    fn test_is_windows_10_with_malformed_version() {
        let mut mock_api = MockWindowsApi::new();
        mock_api
            .expect_get_os_version()
            .times(1)
            .return_const("invalid.version.string".to_string());

        let result = std::panic::catch_unwind(|| {
            return is_windows_10_with_api(&mock_api);
        });
        assert!(
            result.is_err(),
            "Should panic with malformed version string"
        );
    }
}

/// Tests console title management.
mod console_title_test {
    use super::*;

    /// Tests console title setting with ASCII strings.
    /// Validates proper Windows API integration and string handling.
    #[test]
    fn test_set_console_title_with_api() {
        let mut mock_api = MockWindowsApi::new();
        let test_title = "Test Console Title";

        mock_api
            .expect_set_console_title()
            .with(mockall::predicate::eq(test_title))
            .times(1)
            .returning(|_| return Ok(()));

        set_console_title_with_api(&mock_api, test_title);
    }

    /// Tests console title retrieval with UTF-16 buffer handling.
    /// Validates proper string conversion and API integration.
    #[test]
    fn test_get_console_title_with_api() {
        let mut mock_api = MockWindowsApi::new();
        let expected_title = "Current Console Title";

        let title_utf16: Vec<u16> = expected_title
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        mock_api
            .expect_get_console_title_utf16()
            .with(mockall::predicate::always())
            .times(1)
            .returning(move |buffer: &mut [u16]| {
                let copy_len = std::cmp::min(title_utf16.len(), buffer.len());
                buffer[..copy_len].copy_from_slice(&title_utf16[..copy_len]);
                return copy_len as i32;
            });

        let result = get_console_title_with_api(&mock_api);
        assert_eq!(result, expected_title);
    }

    /// Tests console title retrieval when no title is set.
    /// Validates handling of empty title buffers.
    #[test]
    fn test_get_console_title_with_empty_title() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_get_console_title_utf16()
            .with(mockall::predicate::always())
            .times(1)
            .returning(|_| return 0);

        let result = get_console_title_with_api(&mock_api);
        assert_eq!(result, "");
    }

    /// Tests console title retrieval with Unicode characters.
    /// Validates proper UTF-16 encoding and international character support.
    #[test]
    fn test_get_console_title_with_unicode() {
        let mut mock_api = MockWindowsApi::new();
        let expected_title = "Test 🦀 Rust 中文 Тест";

        let title_utf16: Vec<u16> = expected_title
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        mock_api
            .expect_get_console_title_utf16()
            .with(mockall::predicate::always())
            .times(1)
            .returning(move |buffer: &mut [u16]| {
                let copy_len = std::cmp::min(title_utf16.len(), buffer.len());
                buffer[..copy_len].copy_from_slice(&title_utf16[..copy_len]);
                return copy_len as i32;
            });

        let result = get_console_title_with_api(&mock_api);
        assert_eq!(result, expected_title);
    }

    /// Tests console title setting error handling when API calls fail.
    /// Validates that function panics appropriately on Windows API errors.
    #[test]
    fn test_set_console_title_error_handling() {
        let mut mock_api = MockWindowsApi::new();
        let test_title = "Test Title";

        mock_api
            .expect_set_console_title()
            .with(mockall::predicate::eq(test_title))
            .times(1)
            .returning(|_| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            set_console_title_with_api(&mock_api, test_title);
        });

        assert!(result.is_err(), "Should panic when set_console_title fails");
    }
}

/// Tests UTF-16 buffer conversion functionality.
mod utf16_conversion_test {
    use super::*;

    /// Tests basic UTF-16 to string conversion with null termination.
    /// Validates standard ASCII string handling.
    #[test]
    fn test_utf16_buffer_to_string_basic() {
        let test_string = "Hello World";
        let utf16_buffer: Vec<u16> = test_string
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let result = utf16_buffer_to_string(&utf16_buffer);
        assert_eq!(result, test_string);
    }

    /// Tests UTF-16 to string conversion with Unicode characters.
    /// Validates proper handling of international characters and emojis.
    #[test]
    fn test_utf16_buffer_to_string_unicode() {
        let test_string = "Test 🦀 Rust 中文 Тест";
        let utf16_buffer: Vec<u16> = test_string
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let result = utf16_buffer_to_string(&utf16_buffer);
        assert_eq!(result, test_string);
    }

    /// Tests UTF-16 to string conversion with empty buffer.
    /// Validates handling of null-only buffers.
    #[test]
    fn test_utf16_buffer_to_string_empty() {
        let utf16_buffer: Vec<u16> = vec![0];

        let result = utf16_buffer_to_string(&utf16_buffer);
        assert_eq!(result, "");
    }

    /// Tests UTF-16 to string conversion without null termination.
    /// Validates handling of buffers that lack proper null terminators.
    #[test]
    fn test_utf16_buffer_to_string_no_null_terminator() {
        let test_string = "No Null";
        let utf16_buffer: Vec<u16> = test_string.encode_utf16().collect();

        let result = utf16_buffer_to_string(&utf16_buffer);
        assert_eq!(result, test_string);
    }

    /// Tests UTF-16 to string conversion with multiple null terminators.
    /// Validates that only the first null terminator is respected.
    #[test]
    fn test_utf16_buffer_to_string_multiple_nulls() {
        let test_string = "Hello";
        let mut utf16_buffer: Vec<u16> = test_string.encode_utf16().collect();
        utf16_buffer.extend_from_slice(&[0, 0, 0]);

        let result = utf16_buffer_to_string(&utf16_buffer);
        assert_eq!(result, test_string);
    }
}

/// Test module for console color functions with proper mocking.
mod console_color_test {
    use super::*;

    /// Tests console color setting with text attributes and buffer filling.
    /// Validates proper color application across the entire console buffer.
    #[test]
    fn test_set_console_color_with_api() {
        let mut mock_api = MockWindowsApi::new();
        let test_color = CONSOLE_CHARACTER_ATTRIBUTES(0x0F);

        let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
        buffer_info.dwSize.X = 80;
        buffer_info.dwSize.Y = 25;

        mock_api
            .expect_set_console_text_attribute()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Ok(()));

        mock_api
            .expect_get_console_screen_buffer_info()
            .times(1)
            .return_const(Ok(buffer_info));

        mock_api
            .expect_fill_console_output_attribute()
            .times(25)
            .returning(|_, _, _| return Ok(80));

        set_console_color_with_api(&mock_api, test_color);
    }

    /// Tests console color setting error handling when API calls fail.
    /// Validates that function panics appropriately on Windows API errors.
    #[test]
    fn test_set_console_color_error_handling() {
        let mut mock_api = MockWindowsApi::new();
        let test_color = CONSOLE_CHARACTER_ATTRIBUTES(0x0F);

        mock_api
            .expect_set_console_text_attribute()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            set_console_color_with_api(&mock_api, test_color);
        });

        assert!(
            result.is_err(),
            "Should panic when set_console_text_attribute fails"
        );
    }
}

/// Test module for clear screen functions with proper mocking.
mod clear_screen_test {
    use super::*;

    /// Tests console screen clearing with scroll buffer operations.
    /// Validates proper screen clearing and cursor positioning to origin.
    #[test]
    fn test_clear_screen_with_api() {
        let mut mock_api = MockWindowsApi::new();

        let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
        buffer_info.dwSize.X = 80;
        buffer_info.dwSize.Y = 25;
        buffer_info.wAttributes = CONSOLE_CHARACTER_ATTRIBUTES(0x07);

        mock_api
            .expect_get_console_screen_buffer_info()
            .times(1)
            .return_const(Ok(buffer_info));

        mock_api
            .expect_scroll_console_screen_buffer()
            .times(1)
            .returning(|_, _, _| return Ok(()));

        mock_api
            .expect_set_console_cursor_position()
            .with(mockall::predicate::eq(COORD { X: 0, Y: 0 }))
            .times(1)
            .returning(|_| return Ok(()));

        clear_screen_with_api(&mock_api);
    }

    /// Tests clear screen error handling when buffer info retrieval fails.
    /// Validates that function panics appropriately on Windows API errors.
    #[test]
    fn test_clear_screen_error_handling() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_get_console_screen_buffer_info()
            .times(1)
            .returning(|| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            clear_screen_with_api(&mock_api);
        });

        assert!(
            result.is_err(),
            "Should panic when get_console_screen_buffer_info fails"
        );
    }
}

/// Test module for console border color functions with proper mocking.
mod console_border_color_with_api_test {
    use super::*;

    /// Tests console border color setting on Windows 10 (no-op behavior).
    /// Validates that function skips DWM calls on Windows 10 systems.
    #[test]
    fn test_set_console_border_color_with_api_windows_10() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.19045".to_string());

        api.expect_set_dwm_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(0);

        set_console_border_color_with_api(&api, test_color);
    }

    /// Tests console border color setting on Windows 11 with DWM integration.
    /// Validates that function properly calls DWM APIs on Windows 11+ systems.
    #[test]
    fn test_set_console_border_color_with_api_windows_11() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.22000".to_string());

        api.expect_set_dwm_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Ok(()));

        set_console_border_color_with_api(&api, test_color);
    }

    /// Tests console border color setting error handling when DWM calls fail.
    /// Validates that function panics appropriately on DWM API errors.
    #[test]
    fn test_set_console_border_color_with_api_error_handling() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.22000".to_string());

        api.expect_set_dwm_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            set_console_border_color_with_api(&api, test_color);
        });

        assert!(
            result.is_err(),
            "Should panic when set_dwm_border_color fails"
        );
    }
}

/// Test module for console input functions with proper mocking.
mod console_input_test {
    use windows::Win32::System::Console::KEY_EVENT_RECORD;

    use super::*;

    /// Tests basic console input reading with single event retrieval.
    /// Validates proper input record handling and event type detection.
    #[test]
    fn test_read_console_input_with_api() {
        let mut mock_api = MockWindowsApi::new();

        let test_record = INPUT_RECORD {
            EventType: KEY_EVENT,
            ..Default::default()
        };

        mock_api
            .expect_read_console_input()
            .with(mockall::predicate::always())
            .times(1)
            .returning(move |buffer: &mut [INPUT_RECORD]| {
                buffer[0] = test_record;
                return Ok(1);
            });

        let result = read_console_input_with_api(&mock_api);
        assert_eq!(result.EventType, KEY_EVENT);
    }

    /// Tests console input reading with retry logic when no events are available.
    /// Validates that function retries until an event is successfully retrieved.
    #[test]
    fn test_read_console_input_with_api_retry() {
        let mut mock_api = MockWindowsApi::new();

        let test_record = INPUT_RECORD {
            EventType: KEY_EVENT,
            ..Default::default()
        };

        let mut call_count = 0;
        mock_api
            .expect_read_console_input()
            .with(mockall::predicate::always())
            .times(2)
            .returning(move |buffer: &mut [INPUT_RECORD]| {
                call_count += 1;
                if call_count == 1 {
                    return Ok(0);
                } else {
                    buffer[0] = test_record;
                    return Ok(1);
                }
            });

        let result = read_console_input_with_api(&mock_api);
        assert_eq!(result.EventType, KEY_EVENT);
    }

    /// Tests keyboard input filtering with event type detection and field validation.
    /// Validates that function filters out non-key events and returns complete key data.
    #[test]
    fn test_read_keyboard_input_with_api() {
        let mut mock_api = MockWindowsApi::new();

        let non_key_record = INPUT_RECORD {
            EventType: MOUSE_EVENT as u16,
            ..Default::default()
        };
        let mut key_event_record = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(1),
            wRepeatCount: 1,
            wVirtualKeyCode: 0x41,
            wVirtualScanCode: 0x1E,
            dwControlKeyState: 0,
            ..Default::default()
        };
        key_event_record.uChar.UnicodeChar = 'A' as u16;

        let key_event_data = INPUT_RECORD_0 {
            KeyEvent: key_event_record,
        };
        let key_record = INPUT_RECORD {
            EventType: KEY_EVENT,
            Event: key_event_data,
        };

        let mut call_count = 0;
        mock_api
            .expect_read_console_input()
            .with(mockall::predicate::always())
            .times(2)
            .returning(move |buffer: &mut [INPUT_RECORD]| {
                call_count += 1;
                if call_count == 1 {
                    buffer[0] = non_key_record;
                } else {
                    buffer[0] = key_record;
                }
                return Ok(1);
            });

        let result = read_keyboard_input_with_api(&mock_api);

        let returned_key_event = unsafe { result.KeyEvent };
        assert_eq!(returned_key_event.bKeyDown, key_event_record.bKeyDown);
        assert_eq!(
            returned_key_event.wRepeatCount,
            key_event_record.wRepeatCount
        );
        assert_eq!(
            returned_key_event.wVirtualKeyCode,
            key_event_record.wVirtualKeyCode
        );
        assert_eq!(
            returned_key_event.wVirtualScanCode,
            key_event_record.wVirtualScanCode
        );
        assert_eq!(unsafe { returned_key_event.uChar.UnicodeChar }, unsafe {
            key_event_record.uChar.UnicodeChar
        });
        assert_eq!(
            returned_key_event.dwControlKeyState,
            key_event_record.dwControlKeyState
        );
    }

    /// Tests console input reading error handling when API calls fail.
    /// Validates that function panics appropriately on Windows API errors.
    #[test]
    fn test_read_console_input_error_handling() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_read_console_input()
            .with(mockall::predicate::always())
            .times(1)
            .returning(|_| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            read_console_input_with_api(&mock_api);
        });

        assert!(
            result.is_err(),
            "Should panic when read_console_input fails"
        );
    }
}

/// Additional test module for utils/mod.rs to improve coverage.
mod utils_mod_additional_test {
    use mockall::predicate::*;
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::System::Console::{
        CONSOLE_CHARACTER_ATTRIBUTES, CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD,
        INPUT_RECORD_0, KEY_EVENT_RECORD, SMALL_RECT,
    };

    use crate::utils::{
        arrange_console_with_api, get_window_title_with_api, DefaultWindowsApi, MockWindowsApi,
        KEY_EVENT,
    };

    #[test]
    fn test_arrange_console_with_api() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_arrange_console()
            .with(eq(100), eq(200), eq(800), eq(600))
            .times(1)
            .returning(|_, _, _, _| return Ok(()));

        arrange_console_with_api(&mock_api, 100, 200, 800, 600);
    }

    #[test]
    fn test_get_window_title_with_api() {
        let mock_api = MockWindowsApi::new();
        let hwnd = HWND(std::ptr::null_mut());

        // This function doesn't use the API mock for individual HWND operations
        // It uses direct Windows API calls, so we just test it doesn't panic
        let _result = get_window_title_with_api(&mock_api, &hwnd);
    }

    #[test]
    fn test_default_windows_api_creation() {
        let _api = DefaultWindowsApi;
        // Just test that it can be created without issues
    }

    #[test]
    fn test_key_event_constant() {
        assert_eq!(KEY_EVENT, 1u16);
    }

    #[test]
    fn test_read_console_input_with_api_retry_logic() {
        use crate::utils::read_console_input_with_api;

        let mut mock_api = MockWindowsApi::new();
        let input_record = INPUT_RECORD {
            EventType: 1, // KEY_EVENT
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: windows::Win32::Foundation::BOOL(1),
                    wRepeatCount: 1,
                    wVirtualKeyCode: 65, // 'A'
                    wVirtualScanCode: 30,
                    uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 65 },
                    dwControlKeyState: 0,
                },
            },
        };

        mock_api
            .expect_read_console_input()
            .times(2)
            .returning(move |buffer| {
                static mut CALL_COUNT: usize = 0;
                unsafe {
                    CALL_COUNT += 1;
                    if CALL_COUNT == 1 {
                        // First call returns 0 events read
                        return Ok(0);
                    } else {
                        // Second call returns 1 event
                        buffer[0] = input_record;
                        return Ok(1);
                    }
                }
            });

        let result = read_console_input_with_api(&mock_api);
        assert_eq!(result.EventType, 1);
    }

    #[test]
    fn test_is_windows_10_with_api_boundary_cases() {
        use crate::utils::is_windows_10_with_api;

        // Test exact boundary case - Windows 11 first build
        let mut mock_api = MockWindowsApi::new();
        mock_api
            .expect_get_os_version()
            .times(1)
            .returning(|| return "10.0.22000".to_string());

        let result = is_windows_10_with_api(&mock_api);
        assert!(!result); // Should be Windows 11

        // Test just before boundary
        let mut mock_api2 = MockWindowsApi::new();
        mock_api2
            .expect_get_os_version()
            .times(1)
            .returning(|| return "10.0.21999".to_string());

        let result2 = is_windows_10_with_api(&mock_api2);
        assert!(result2); // Should be Windows 10
    }

    #[test]
    fn test_keyboard_input_filtering() {
        use crate::utils::read_keyboard_input_with_api;

        let mut mock_api = MockWindowsApi::new();
        let key_input_record = INPUT_RECORD {
            EventType: 1, // KEY_EVENT
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: windows::Win32::Foundation::BOOL(1),
                    wRepeatCount: 1,
                    wVirtualKeyCode: 65, // 'A'
                    wVirtualScanCode: 30,
                    uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 65 },
                    dwControlKeyState: 0,
                },
            },
        };

        let non_key_input_record = INPUT_RECORD {
            EventType: 2, // MOUSE_EVENT
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD::default(),
            },
        };

        mock_api
            .expect_read_console_input()
            .times(2)
            .returning(move |buffer| {
                static mut CALL_COUNT: usize = 0;
                unsafe {
                    CALL_COUNT += 1;
                    if CALL_COUNT == 1 {
                        buffer[0] = non_key_input_record; // First call returns non-key event
                    } else {
                        buffer[0] = key_input_record; // Second call returns key event
                    }
                }
                return Ok(1);
            });

        let result = read_keyboard_input_with_api(&mock_api);
        unsafe {
            assert_eq!(result.KeyEvent.wVirtualKeyCode, 65);
        }
    }

    #[test]
    fn test_console_color_buffer_filling() {
        use crate::utils::set_console_color_with_api;

        let mut mock_api = MockWindowsApi::new();
        let color = CONSOLE_CHARACTER_ATTRIBUTES(7);
        let buffer_info = CONSOLE_SCREEN_BUFFER_INFO {
            dwSize: COORD { X: 80, Y: 25 },
            dwCursorPosition: COORD { X: 0, Y: 0 },
            wAttributes: CONSOLE_CHARACTER_ATTRIBUTES(7),
            srWindow: SMALL_RECT {
                Left: 0,
                Top: 0,
                Right: 79,
                Bottom: 24,
            },
            dwMaximumWindowSize: COORD { X: 80, Y: 25 },
        };

        mock_api
            .expect_set_console_text_attribute()
            .with(eq(color))
            .times(1)
            .returning(|_| return Ok(()));

        mock_api
            .expect_get_console_screen_buffer_info()
            .times(1)
            .returning(move || return Ok(buffer_info));

        mock_api
            .expect_fill_console_output_attribute()
            .times(25) // Once for each row
            .returning(|_, _, _| return Ok(80));

        set_console_color_with_api(&mock_api, color);
    }

    #[test]
    fn test_clear_screen_scroll_operation() {
        use crate::utils::clear_screen_with_api;

        let mut mock_api = MockWindowsApi::new();
        let buffer_info = CONSOLE_SCREEN_BUFFER_INFO {
            dwSize: COORD { X: 80, Y: 25 },
            dwCursorPosition: COORD { X: 10, Y: 5 },
            wAttributes: CONSOLE_CHARACTER_ATTRIBUTES(7),
            srWindow: SMALL_RECT {
                Left: 0,
                Top: 0,
                Right: 79,
                Bottom: 24,
            },
            dwMaximumWindowSize: COORD { X: 80, Y: 25 },
        };

        mock_api
            .expect_get_console_screen_buffer_info()
            .times(1)
            .returning(move || return Ok(buffer_info));

        mock_api
            .expect_scroll_console_screen_buffer()
            .times(1)
            .returning(|_, _, _| return Ok(()));

        mock_api
            .expect_set_console_cursor_position()
            .with(eq(COORD { X: 0, Y: 0 }))
            .times(1)
            .returning(|_| return Ok(()));

        clear_screen_with_api(&mock_api);
    }

    #[test]
    fn test_console_title_unicode_handling() {
        use crate::utils::get_console_title_with_api;

        let mut mock_api = MockWindowsApi::new();
        let test_title = "Test 🦀 Rust 中文 Тест";
        let utf16_title: Vec<u16> = test_title.encode_utf16().collect();

        mock_api
            .expect_get_console_title_utf16()
            .times(1)
            .returning(move |buffer| {
                let copy_len = std::cmp::min(utf16_title.len(), buffer.len() - 1);
                buffer[..copy_len].copy_from_slice(&utf16_title[..copy_len]);
                buffer[copy_len] = 0; // Null terminator
                return copy_len as i32;
            });

        let result = get_console_title_with_api(&mock_api);
        assert_eq!(result, test_title);
    }

    #[test]
    fn test_border_color_windows_version_detection() {
        use crate::utils::set_console_border_color_with_api;

        // Test Windows 11 behavior
        let mut mock_api = MockWindowsApi::new();
        let color = COLORREF(0x00FF0000); // Red

        mock_api
            .expect_get_os_version()
            .times(1)
            .returning(|| return "10.0.22000".to_string()); // Windows 11

        mock_api
            .expect_set_dwm_border_color()
            .with(eq(color))
            .times(1)
            .returning(|_| return Ok(()));

        set_console_border_color_with_api(&mock_api, color);

        // Test Windows 10 behavior (no DWM call)
        let mut mock_api2 = MockWindowsApi::new();
        mock_api2
            .expect_get_os_version()
            .times(1)
            .returning(|| return "10.0.19041".to_string()); // Windows 10

        // Should not call set_dwm_border_color for Windows 10
        set_console_border_color_with_api(&mock_api2, color);
    }
}
