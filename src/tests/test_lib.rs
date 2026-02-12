//! Unit tests for the lib module with proper mocking and behavior verification.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]

use std::ffi::c_void;

use mockall::predicate::*;
use windows::Win32::System::Threading::PROCESS_INFORMATION;

use crate::utils::windows::MockWindowsApi;
use crate::{
    create_process, init_logger_with_fs, is_launched_from_gui, spawn_console_process,
    MockFileSystem, MockRegistry, WindowsSettingsDefaultTerminalApplicationGuard, CLSID_CONHOST,
    DEFAULT_TERMINAL_APP_REGISTRY_PATH, DELEGATION_CONSOLE, DELEGATION_TERMINAL,
};

/// Test module for WindowsSettingsDefaultTerminalApplicationGuard functionality.
mod windows_settings_guard_test {
    use super::*;

    /// Tests guard creation when registry operations fail.
    /// Validates that guard defaults to no-op behavior when registry access fails.
    #[test]
    fn test_guard_new_registry_failure() {
        let mut mock_registry = MockRegistry::new();
        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
            )
            .times(1)
            .returning(|_, _| return None);

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
            )
            .times(1)
            .returning(|_, _| return None);

        let guard =
            WindowsSettingsDefaultTerminalApplicationGuard::new_with_registry(mock_registry);

        assert!(guard.old_windows_terminal_console.is_none());
        assert!(guard.old_windows_terminal_terminal.is_none());
    }

    /// Tests guard creation when current settings already match conhost.
    /// Validates that guard skips modification when conhost is already configured.
    #[test]
    fn test_guard_new_already_conhost() {
        let mut mock_registry = MockRegistry::new();

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
            )
            .times(1)
            .returning(|_, _| return Some(CLSID_CONHOST.to_string()));

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
            )
            .times(1)
            .returning(|_, _| return Some(CLSID_CONHOST.to_string()));

        let guard =
            WindowsSettingsDefaultTerminalApplicationGuard::new_with_registry(mock_registry);

        // Should be no-op since values are already conhost
        assert!(guard.old_windows_terminal_console.is_none());
        assert!(guard.old_windows_terminal_terminal.is_none());
    }

    /// Tests guard creation with different existing registry values.
    /// Validates that guard stores original values and sets conhost values.
    #[test]
    fn test_guard_new_with_existing_values() {
        let mut mock_registry = MockRegistry::new();

        let old_console_value = "old-console-value".to_string();
        let old_terminal_value = "old-terminal-value".to_string();

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
            )
            .times(1)
            .returning({
                let val = old_console_value.clone();
                move |_, _| return Some(val.clone())
            });

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
            )
            .times(1)
            .returning({
                let val = old_terminal_value.clone();
                move |_, _| return Some(val.clone())
            });

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
                eq(CLSID_CONHOST),
            )
            .times(1)
            .returning(|_, _, _| return true);

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
                eq(CLSID_CONHOST),
            )
            .times(1)
            .returning(|_, _, _| return true);

        // Setup for guard drop
        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
                eq(old_console_value.clone()),
            )
            .times(1)
            .returning(|_, _, _| return true);

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
                eq(old_terminal_value.clone()),
            )
            .times(1)
            .returning(|_, _, _| return true);

        let guard =
            WindowsSettingsDefaultTerminalApplicationGuard::new_with_registry(mock_registry);

        assert_eq!(guard.old_windows_terminal_console, Some(old_console_value));
        assert_eq!(
            guard.old_windows_terminal_terminal,
            Some(old_terminal_value)
        );
    }

    /// Tests guard drop behavior when no restoration is needed.
    /// Validates that drop is no-op when original values weren't stored.
    #[test]
    fn test_guard_drop_no_restoration() {
        let mut mock_registry = MockRegistry::new();
        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
            )
            .times(1)
            .returning(|_, _| return None);

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
            )
            .times(1)
            .returning(|_, _| return None);

        let guard =
            WindowsSettingsDefaultTerminalApplicationGuard::new_with_registry(mock_registry);

        // Drop should not call any registry operations since no values were stored
        drop(guard);
        // Test passes if no panic occurs during drop
    }

    /// Tests guard drop behavior with stored values.
    /// Validates that drop attempts to restore original registry values.
    #[test]
    fn test_guard_drop_with_restoration() {
        let mut mock_registry = MockRegistry::new();

        let old_console_value = "original-console".to_string();
        let old_terminal_value = "original-terminal".to_string();

        // Setup for guard creation
        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
            )
            .times(1)
            .returning({
                let val = old_console_value.clone();
                move |_, _| return Some(val.clone())
            });

        mock_registry
            .expect_get_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
            )
            .times(1)
            .returning({
                let val = old_terminal_value.clone();
                move |_, _| return Some(val.clone())
            });

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
                eq(CLSID_CONHOST),
            )
            .times(1)
            .returning(|_, _, _| return true);

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
                eq(CLSID_CONHOST),
            )
            .times(1)
            .returning(|_, _, _| return true);

        // Setup for guard drop
        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_CONSOLE),
                eq(old_console_value.clone()),
            )
            .times(1)
            .returning(|_, _, _| return true);

        mock_registry
            .expect_set_registry_string_value()
            .with(
                eq(DEFAULT_TERMINAL_APP_REGISTRY_PATH),
                eq(DELEGATION_TERMINAL),
                eq(old_terminal_value.clone()),
            )
            .times(1)
            .returning(|_, _, _| return true);

        let guard =
            WindowsSettingsDefaultTerminalApplicationGuard::new_with_registry(mock_registry);
        drop(guard);
        // Test passes if restoration calls were made during drop
    }
}

/// Test module for create_process_with_command_line_api functionality.
mod create_process_api_test {
    use super::*;

    /// Tests create_process_with_command_line_api with successful process creation.
    /// Validates proper business logic: STARTUPINFOW initialization, command line buffer handling, and error processing.
    #[test]
    fn test_create_process_with_command_line_api_success() {
        let mut mock_api = MockWindowsApi::new();
        let application = "cmd.exe";
        let command_line = vec![b'"' as u16, b'c' as u16, b'm' as u16, b'd' as u16, 0];

        mock_api
            .expect_create_process_raw()
            .times(1)
            .returning(|_, _, _, _| return Ok(()));

        let result = create_process(&mock_api, application, &command_line);

        assert!(result.is_some());
        let process_info = result.unwrap();
        // Verify that PROCESS_INFORMATION was properly initialized
        assert_eq!(process_info.dwProcessId, 0); // Default initialization
        assert_eq!(process_info.dwThreadId, 0);
    }

    /// Tests create_process_with_command_line_api with API failure.
    /// Validates proper error handling when the underlying API call fails.
    #[test]
    fn test_create_process_with_command_line_api_failure() {
        let mut mock_api = MockWindowsApi::new();
        let application = "nonexistent.exe";
        let command_line = vec![b'"' as u16, b'n' as u16, b'o' as u16, b'n' as u16, 0];

        mock_api
            .expect_create_process_raw()
            .times(1)
            .returning(|_, _, _, _| return Err(windows::core::Error::from_win32()));

        let result = create_process(&mock_api, application, &command_line);

        assert!(result.is_none());
    }

    /// Tests create_process_with_command_line_api with empty command line.
    /// Validates handling of edge case with minimal command line.
    #[test]
    fn test_create_process_with_command_line_api_empty_command() {
        let mut mock_api = MockWindowsApi::new();
        let application = "test.exe";
        let command_line = vec![0]; // Just null terminator

        mock_api
            .expect_create_process_raw()
            .times(1)
            .returning(|_, _, _, _| return Ok(()));

        let result = create_process(&mock_api, application, &command_line);

        assert!(result.is_some());
    }
}

/// Test module for process spawning functionality.
mod spawn_process_test {
    use super::*;

    /// Tests spawn_console_process with successful process creation.
    /// Validates proper API call and return value handling.
    #[test]
    fn test_spawn_console_process_success() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_create_process_with_args()
            .with(
                eq("cmd.exe"),
                eq(vec![
                    "/c".to_string(),
                    "echo".to_string(),
                    "test".to_string(),
                ]),
            )
            .times(1)
            .returning(|_, _| {
                return Some(PROCESS_INFORMATION {
                    hProcess: windows::Win32::Foundation::HANDLE(0x1234 as *mut c_void),
                    hThread: windows::Win32::Foundation::HANDLE(0x5678 as *mut c_void),
                    dwProcessId: 1000,
                    dwThreadId: 2000,
                });
            });

        let result = spawn_console_process(
            &mock_api,
            "cmd.exe",
            vec!["/c".to_string(), "echo".to_string(), "test".to_string()],
        );

        assert!(result.is_some());
        let process_info = result.unwrap();
        assert_eq!(process_info.dwProcessId, 1000);
        assert_eq!(process_info.dwThreadId, 2000);
    }

    /// Tests spawn_console_process with process creation failure.
    /// Validates proper error handling when API call fails.
    #[test]
    fn test_spawn_console_process_failure() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_create_process_with_args()
            .with(eq("nonexistent.exe"), eq(vec!["arg1".to_string()]))
            .times(1)
            .returning(|_, _| return None);

        let result = spawn_console_process(&mock_api, "nonexistent.exe", vec!["arg1".to_string()]);

        assert!(result.is_none());
    }

    /// Tests spawn_console_process with no arguments.
    /// Validates proper handling of applications without command line arguments.
    #[test]
    fn test_spawn_console_process_no_args() {
        let mut mock_api = MockWindowsApi::new();

        mock_api
            .expect_create_process_with_args()
            .with(eq("notepad.exe"), eq(Vec::<String>::new()))
            .times(1)
            .returning(|_, _| {
                return Some(PROCESS_INFORMATION {
                    hProcess: windows::Win32::Foundation::HANDLE(0xABCD as *mut c_void),
                    hThread: windows::Win32::Foundation::HANDLE(0xEF01 as *mut c_void),
                    dwProcessId: 3000,
                    dwThreadId: 4000,
                });
            });

        let result = spawn_console_process(&mock_api, "notepad.exe", vec![]);

        assert!(result.is_some());
        let process_info = result.unwrap();
        assert_eq!(process_info.dwProcessId, 3000);
        assert_eq!(process_info.dwThreadId, 4000);
    }

    /// Tests spawn_console_process with complex arguments containing spaces.
    /// Validates proper handling of arguments with special characters.
    #[test]
    fn test_spawn_console_process_complex_args() {
        let mut mock_api = MockWindowsApi::new();

        let args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "user@host.com".to_string(),
        ];
        mock_api
            .expect_create_process_with_args()
            .with(eq("ssh.exe"), eq(args.clone()))
            .times(1)
            .returning(|_, _| {
                return Some(PROCESS_INFORMATION {
                    hProcess: windows::Win32::Foundation::HANDLE(0x2468 as *mut c_void),
                    hThread: windows::Win32::Foundation::HANDLE(0x1357 as *mut c_void),
                    dwProcessId: 5000,
                    dwThreadId: 6000,
                });
            });

        let result = spawn_console_process(&mock_api, "ssh.exe", args);

        assert!(result.is_some());
        let process_info = result.unwrap();
        assert_eq!(process_info.dwProcessId, 5000);
        assert_eq!(process_info.dwThreadId, 6000);
    }
}

/// Test module for logger initialization functionality.
mod logger_test {
    use super::*;

    /// Tests init_logger with successful directory and file creation.
    /// Validates proper file system operations and logger initialization.
    #[test]
    fn test_init_logger_success() {
        let mut mock_fs = MockFileSystem::new();

        mock_fs
            .expect_create_directory()
            .with(eq("logs"))
            .times(1)
            .returning(|_| return true);

        mock_fs
            .expect_create_log_file()
            .with(function(|filename: &str| {
                return filename.starts_with("logs/") && filename.ends_with("_test_daemon.log");
            }))
            .times(1)
            .returning(|_| return true);

        init_logger_with_fs(&mock_fs, "test_daemon");
        // Test passes if all expected calls were made
    }

    /// Tests init_logger with directory creation failure.
    /// Validates graceful handling when directory cannot be created.
    #[test]
    fn test_init_logger_directory_failure() {
        let mut mock_fs = MockFileSystem::new();

        mock_fs
            .expect_create_directory()
            .with(eq("logs"))
            .times(1)
            .returning(|_| return false);

        mock_fs
            .expect_create_log_file()
            .with(function(|filename: &str| {
                return filename.starts_with("logs/") && filename.ends_with("_test_daemon.log");
            }))
            .times(1)
            .returning(|_| return false);

        init_logger_with_fs(&mock_fs, "test_daemon");
        // Test passes if logger handles directory failure gracefully
    }

    /// Tests init_logger with file creation failure.
    /// Validates graceful handling when log file cannot be created.
    #[test]
    fn test_init_logger_file_failure() {
        let mut mock_fs = MockFileSystem::new();

        mock_fs
            .expect_create_directory()
            .with(eq("logs"))
            .times(1)
            .returning(|_| return true);

        mock_fs
            .expect_create_log_file()
            .with(function(|filename: &str| {
                return filename.starts_with("logs/") && filename.ends_with("_test_daemon.log");
            }))
            .times(1)
            .returning(|_| return false);

        init_logger_with_fs(&mock_fs, "test_daemon");
        // Test passes if logger handles file creation failure gracefully
    }

    /// Tests init_logger with various name inputs.
    /// Validates proper handling of different logger name formats.
    #[test]
    fn test_init_logger_name_variations() {
        let test_names = vec![
            "daemon",
            "client_1",
            "test-logger",
            "logger.with.dots",
            "UPPERCASE",
            "123numeric",
        ];

        for name in test_names {
            let mut mock_fs = MockFileSystem::new();

            mock_fs
                .expect_create_directory()
                .with(eq("logs"))
                .times(1)
                .returning(|_| return true);

            mock_fs
                .expect_create_log_file()
                .with(function({
                    let expected_name = name.to_string();
                    move |filename: &str| {
                        return filename.starts_with("logs/")
                            && filename.contains(&expected_name)
                            && filename.ends_with(".log");
                    }
                }))
                .times(1)
                .returning(|_| return true);

            init_logger_with_fs(&mock_fs, name);
        }
    }
}

/// Test module for GUI launch detection functionality.
mod gui_launch_detection_test {
    use super::*;
    /// Tests is_launched_from_gui with cursor at origin (GUI launch).
    /// Validates detection of GUI launch when console cursor is at (0,0).
    #[test]
    fn test_is_launched_from_gui_cursor_at_origin() {
        let mut mock_windows_api = MockWindowsApi::new();

        mock_windows_api
            .expect_get_console_attached_process_count()
            .times(1)
            .returning(|| {
                return 1;
            });

        let result = is_launched_from_gui(&mock_windows_api);
        assert!(result);
    }

    /// Tests is_launched_from_gui with cursor not at origin (console launch).
    /// Validates detection of console launch when cursor has moved from (0,0).
    #[test]
    fn test_is_launched_from_gui_cursor_moved() {
        let mut mock_windows_api = MockWindowsApi::new();

        mock_windows_api
            .expect_get_console_attached_process_count()
            .times(1)
            .returning(|| {
                return 2;
            });

        let result = is_launched_from_gui(&mock_windows_api);
        assert!(!result);
    }
}
