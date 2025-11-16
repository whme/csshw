//! Unit tests for the utils windows module using mockall for Windows API mocking.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use crate::utils::windows::{
    clear_screen, is_windows_10, read_console_input, read_keyboard_input, set_console_border_color,
    set_console_color, utf16_buffer_to_string, MockWindowsApi, KEY_EVENT,
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

        let result = is_windows_10(&mock_api);
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

        let result = is_windows_10(&mock_api);
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

            let result = is_windows_10(&mock_api);
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
            return is_windows_10(&mock_api);
        });
        assert!(
            result.is_err(),
            "Should panic with malformed version string"
        );
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
    fn test_set_console_color() {
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

        set_console_color(&mock_api, test_color);
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
            set_console_color(&mock_api, test_color);
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
    fn test_clear_screen() {
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

        clear_screen(&mock_api);
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
            clear_screen(&mock_api);
        });

        assert!(
            result.is_err(),
            "Should panic when get_console_screen_buffer_info fails"
        );
    }
}

/// Test module for console border color functions with proper mocking.
mod console_border_color_test {
    use super::*;

    /// Tests console border color setting on Windows 10 (no-op behavior).
    /// Validates that function skips DWM calls on Windows 10 systems.
    #[test]
    fn test_set_console_border_color_windows_10() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.19045".to_string());

        api.expect_set_console_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(0);

        set_console_border_color(&api, test_color);
    }

    /// Tests console border color setting on Windows 11 with DWM integration.
    /// Validates that function properly calls DWM APIs on Windows 11+ systems.
    #[test]
    fn test_set_console_border_color_windows_11() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.22000".to_string());

        api.expect_set_console_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Ok(()));

        set_console_border_color(&api, test_color);
    }

    /// Tests console border color setting error handling when DWM calls fail.
    /// Validates that function panics appropriately on DWM API errors.
    #[test]
    fn test_set_console_border_color_error_handling() {
        let mut api = MockWindowsApi::new();
        let test_color = COLORREF(0x00FF0000);

        api.expect_get_os_version()
            .times(1)
            .return_const("10.0.22000".to_string());

        api.expect_set_console_border_color()
            .with(mockall::predicate::eq(test_color))
            .times(1)
            .returning(|_| return Err(windows::core::Error::from_win32()));

        let result = std::panic::catch_unwind(|| {
            set_console_border_color(&api, test_color);
        });

        assert!(
            result.is_err(),
            "Should panic when set_console_border_color fails"
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
    fn test_read_console_input() {
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

        let result = read_console_input(&mock_api);
        assert_eq!(result.EventType, KEY_EVENT);
    }

    /// Tests console input reading with retry logic when no events are available.
    /// Validates that function retries until an event is successfully retrieved.
    #[test]
    fn test_read_console_input_retry() {
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

        let result = read_console_input(&mock_api);
        assert_eq!(result.EventType, KEY_EVENT);
    }

    /// Tests keyboard input filtering with event type detection and field validation.
    /// Validates that function filters out non-key events and returns complete key data.
    #[test]
    fn test_read_keyboard_input() {
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

        let result = read_keyboard_input(&mock_api);

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
            read_console_input(&mock_api);
        });

        assert!(
            result.is_err(),
            "Should panic when read_console_input fails"
        );
    }
}

/// Test module for command line building functionality.
mod command_line_test {
    use crate::utils::windows::build_command_line;

    /// Tests build_command_line with simple application and arguments.
    /// Validates proper UTF-16 encoding and quoting.
    #[test]
    fn test_build_command_line_simple() {
        let application = "cmd.exe";
        let args = vec!["arg1".to_string(), "arg2".to_string()];

        let result = build_command_line(application, &args);

        // Also make sure its null terminated
        assert_eq!(
            result,
            vec![
                34, 99, 109, 100, 46, 101, 120, 101, 34, 32, 34, 97, 114, 103, 49, 34, 32, 34, 97,
                114, 103, 50, 34, 0
            ]
        );
    }

    /// Tests build_command_line with no arguments.
    /// Validates proper handling of applications without arguments.
    #[test]
    fn test_build_command_line_no_args() {
        let application = "notepad.exe";
        let args: Vec<String> = vec![];

        let result = build_command_line(application, &args);

        assert_eq!(
            result,
            vec![34, 110, 111, 116, 101, 112, 97, 100, 46, 101, 120, 101, 34, 0]
        );
    }

    /// Tests build_command_line with arguments containing spaces.
    /// Validates proper quoting of complex arguments.
    #[test]
    fn test_build_command_line_spaces() {
        let application = "program.exe";
        let args = vec!["arg with spaces".to_string(), "another arg".to_string()];

        let result = build_command_line(application, &args);

        assert_eq!(
            result,
            vec![
                34, 112, 114, 111, 103, 114, 97, 109, 46, 101, 120, 101, 34, 32, 34, 97, 114, 103,
                32, 119, 105, 116, 104, 32, 115, 112, 97, 99, 101, 115, 34, 32, 34, 97, 110, 111,
                116, 104, 101, 114, 32, 97, 114, 103, 34, 0
            ]
        );
    }
}

/// Tests actually calling Windows APIs using DefaultWindowsApi.
mod default_windows_api_tests {

    mod create_process_with_args_test {
        use windows::Win32::{
            Foundation::{GetLastError, STILL_ACTIVE},
            System::Threading::TerminateProcess,
        };

        use crate::utils::windows::{DefaultWindowsApi, WindowsApi};

        /// Tests create_process_with_args with valid application and arguments.
        /// Validates that the process creation function is called with correct parameters.
        /// Note: This test actually creates a process.
        #[test]
        fn test_create_process_with_args() {
            let windows_api = DefaultWindowsApi;
            let application = r"C:\Windows\System32\timeout.exe";
            let args = vec!["30".to_string()];
            let process_info = match windows_api.create_process_with_args(application, args) {
                None => panic!("Failed to create process: {:?}", unsafe { GetLastError() }),
                Some(process_info) => process_info,
            };
            assert!(
                windows_api.get_exit_code(process_info.hProcess).unwrap() == STILL_ACTIVE.0 as u32
            );
            unsafe { TerminateProcess(process_info.hProcess, 0) }
                .expect("Failed to terminate process");
            assert!(windows_api.get_exit_code(process_info.hProcess).unwrap() == 0);
        }
    }

    mod get_os_version_test {
        use crate::utils::windows::{DefaultWindowsApi, WindowsApi};

        /// Tests getting OS version.
        /// Validates that the OS version string is non-empty.
        #[test]
        fn test_get_os_version() {
            let windows_api = DefaultWindowsApi;

            let os_version = windows_api.get_os_version();

            assert!(
                !os_version.is_empty(),
                "OS version string should not be empty"
            );
        }
    }

    mod console_mode_test {
        use windows::Win32::System::Console::ENABLE_PROCESSED_INPUT;

        use crate::utils::windows::get_console_input_buffer;
        use crate::utils::windows::{DefaultWindowsApi, WindowsApi};

        /// Tests setting and getting console mode.
        /// Validates that console mode can be set and retrieved correctly.
        #[test]
        fn test_set_and_get_console_mode() {
            let windows_api = DefaultWindowsApi;
            let orig_mode = windows_api
                .get_console_mode(get_console_input_buffer())
                .unwrap();
            let new_mode = windows::Win32::System::Console::CONSOLE_MODE(
                orig_mode.0 ^ ENABLE_PROCESSED_INPUT.0,
            );
            let _ = windows_api.set_console_mode(get_console_input_buffer(), new_mode);
            let updated_mode = windows_api
                .get_console_mode(get_console_input_buffer())
                .unwrap();
            assert_eq!(updated_mode, new_mode);
            let _ = windows_api.set_console_mode(get_console_input_buffer(), orig_mode);
        }
    }

    mod window_handle_tests {
        use windows::Win32::{
            System::Threading::PROCESS_QUERY_INFORMATION,
            UI::WindowsAndMessaging::GetWindowThreadProcessId,
        };

        use crate::utils::windows::{arrange_console, DefaultWindowsApi, WindowsApi};

        struct TestProcessGuard {
            process_info: windows::Win32::System::Threading::PROCESS_INFORMATION,
        }

        impl Drop for TestProcessGuard {
            fn drop(&mut self) {
                match unsafe {
                    windows::Win32::System::Threading::TerminateProcess(
                        self.process_info.hProcess,
                        0,
                    )
                } {
                    Ok(_) => {}
                    Err(err) => {
                        if err.code().0 == 0x80070005u32 as i32 {
                            eprintln!("Access denied when terminating process, that's okay");
                        } else {
                            panic!("Failed to terminate process: {}", err);
                        }
                    }
                }
            }
        }

        fn get_test_window_handle(
            windows_api: &DefaultWindowsApi,
        ) -> (windows::Win32::Foundation::HWND, TestProcessGuard) {
            let application = r"C:\Windows\System32\timeout.exe";
            let args = vec!["30".to_string()];
            let process_info = windows_api
                .create_process_with_args(application, args)
                .expect("Failed to create process");
            let hwnd = windows_api.get_window_handle_for_process(process_info.dwProcessId);
            return (hwnd, TestProcessGuard { process_info });
        }

        mod console_title_test {
            use crate::utils::windows::{
                get_console_title,
                test_mod::default_windows_api_tests::window_handle_tests::get_test_window_handle,
                DefaultWindowsApi, WindowsApi,
            };

            /// Tests setting and getting console title.
            /// Validates that the console title is correctly set and retrieved.
            #[test]
            fn test_set_and_get_console_title() {
                let windows_api = DefaultWindowsApi;
                let (handle, _guard) = get_test_window_handle(&windows_api);
                let test_title = "Test Console Title";

                windows_api
                    .set_console_title(test_title, Some(handle))
                    .expect("Failed to set console title");

                let set_title = get_console_title(&windows_api);
                assert_eq!(set_title, test_title);
            }
        }

        /// Tests getting console window handle.
        /// Validates that the console window handle is non-null.
        #[test]
        fn test_get_console_window_handle() {
            let windows_api = DefaultWindowsApi;

            let window_handle = windows_api.get_console_window();

            assert!(
                !window_handle.is_invalid(),
                "Console window handle should not be null"
            );
            assert!(windows_api.is_window(window_handle));
        }

        /// Tests arrange_console.
        /// Validates that arranging the console works.
        #[test]
        fn test_arrange_console() {
            let windows_api = DefaultWindowsApi;

            let original_placement = windows_api
                .get_window_placement(windows_api.get_console_window())
                .expect("Failed to get window placement");
            let x = original_placement.rcNormalPosition.left;
            let y = original_placement.rcNormalPosition.top;
            let width = original_placement.rcNormalPosition.right
                - original_placement.rcNormalPosition.left;
            let height = original_placement.rcNormalPosition.bottom
                - original_placement.rcNormalPosition.top;
            arrange_console(&windows_api, x, y, width, height);
            let arranged_placement = windows_api
                .get_window_placement(windows_api.get_console_window())
                .expect("Failed to get window placement after arrange");
            assert_eq!(arranged_placement.rcNormalPosition.left, x);
            assert_eq!(arranged_placement.rcNormalPosition.top, y);
            assert_eq!(arranged_placement.rcNormalPosition.right, x + width);
            assert_eq!(arranged_placement.rcNormalPosition.bottom, y + height);
        }

        /// Tests getting foreground window handle.
        /// Validates that the foreground window handle is non-null.
        #[test]
        fn test_get_foreground_window_handle() {
            let windows_api = DefaultWindowsApi;

            let window_handle = windows_api.get_foreground_window();

            assert!(
                !window_handle.is_invalid(),
                "Foreground window handle should not be null"
            );
        }

        /// Tests setting foreground window.
        /// Validates that setting the foreground window does not return an error.
        #[test]
        fn test_set_foreground_window() {
            let windows_api = DefaultWindowsApi;

            let (window_handle, _guard) = get_test_window_handle(&windows_api);
            windows_api
                .set_foreground_window(window_handle)
                .expect("Failed to set foreground window");
        }

        /// Tests moving a window.
        /// Validates that moving a window works.
        #[test]
        fn test_move_window() {
            let windows_api = DefaultWindowsApi;

            let (window_handle, _guard) = get_test_window_handle(&windows_api);

            let original_placement = windows_api
                .get_window_placement(window_handle)
                .expect("Failed to get window placement");

            windows_api
                .move_window(
                    window_handle,
                    original_placement.rcNormalPosition.left + 1,
                    original_placement.rcNormalPosition.top + 1,
                    original_placement.rcNormalPosition.right
                        - original_placement.rcNormalPosition.left
                        - 1,
                    original_placement.rcNormalPosition.bottom
                        - original_placement.rcNormalPosition.top
                        - 1,
                    true,
                )
                .expect("Failed to move window");

            let moved_placement = windows_api
                .get_window_placement(window_handle)
                .expect("Failed to get window placement after move");
            assert_eq!(
                moved_placement.rcNormalPosition.left,
                original_placement.rcNormalPosition.left + 1
            );
            assert_eq!(
                moved_placement.rcNormalPosition.top,
                original_placement.rcNormalPosition.top + 1
            );
            assert_eq!(
                moved_placement.rcNormalPosition.right,
                original_placement.rcNormalPosition.right
            );
            assert_eq!(
                moved_placement.rcNormalPosition.bottom,
                original_placement.rcNormalPosition.bottom
            );

            windows_api
                .move_window(
                    window_handle,
                    original_placement.rcNormalPosition.left,
                    original_placement.rcNormalPosition.top,
                    original_placement.rcNormalPosition.right
                        - original_placement.rcNormalPosition.left,
                    original_placement.rcNormalPosition.bottom
                        - original_placement.rcNormalPosition.top,
                    true,
                )
                .expect("Failed to move window back to original position");
        }

        mod com_ui_automation {
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

            use crate::utils::windows::{
                test_mod::default_windows_api_tests::window_handle_tests::get_test_window_handle,
                DefaultWindowsApi, WindowsApi,
            };

            /// Tests show and focus window.
            /// Validates that showing and focusing a window works.
            #[test]
            fn test_show_and_focus_window() {
                let windows_api = DefaultWindowsApi;
                let (window_handle, _guard) = get_test_window_handle(&windows_api);

                windows_api
                    .initialize_com_library(windows::Win32::System::Com::COINIT_MULTITHREADED)
                    .expect("Failed to initialize COM library");
                windows_api
                    .focus_window_with_automation(window_handle)
                    .expect("Failed to focus window");
                windows_api
                    .show_window(window_handle, SW_SHOW)
                    .expect("Failed to show window");
            }
        }

        /// Tests getting a process handle to an existing process
        #[test]
        fn test_open_process() {
            let windows_api = DefaultWindowsApi;
            let (window_handle, _guard) = get_test_window_handle(&windows_api);

            let mut process_id: u32 = 0;
            unsafe { GetWindowThreadProcessId(window_handle, Some(&mut process_id)) };
            let process_handle =
                windows_api.open_process(PROCESS_QUERY_INFORMATION.0, false, process_id);
            assert!(process_handle.is_ok());
        }
    }

    mod get_system_metrics_test {
        use windows::Win32::UI::WindowsAndMessaging::SM_CXFIXEDFRAME;

        use crate::utils::windows::{DefaultWindowsApi, WindowsApi};

        /// Tests getting system metrics.
        /// Validates that system metrics retrieval works.
        #[test]
        fn test_get_system_metrics() {
            let windows_api = DefaultWindowsApi;

            let metrics = windows_api.get_system_metrics(SM_CXFIXEDFRAME);
            assert!(metrics > 0, "System metrics should be greater than zero");
        }
    }
}
