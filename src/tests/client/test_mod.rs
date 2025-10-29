//! Unit tests for the client module.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use windows::Win32::System::Console::{
    INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, LEFT_ALT_PRESSED, RIGHT_ALT_PRESSED,
    SHIFT_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;

use crate::client::{
    build_ssh_arguments, is_alt_shift_c_combination, is_keep_alive_packet,
    replace_argument_placeholders, resolve_username, write_console_input,
};
use crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH;
use crate::utils::config::ClientConfig;

// Test constants - consistent dummy values used throughout tests
const TEST_USERNAME: &str = "testuser";
const TEST_HOSTNAME: &str = "example.com";
const TEST_PLACEHOLDER: &str = "{{USERNAME_AT_HOST}}";
const TEST_SSH_PROGRAM: &str = "ssh";

/// Creates a test ClientConfig with the given SSH config path.
///
/// # Arguments
///
/// * `ssh_config_path` - Path to the SSH config file.
///
/// # Returns
///
/// A ClientConfig instance for testing.
fn create_test_client_config(ssh_config_path: String) -> ClientConfig {
    return ClientConfig {
        ssh_config_path,
        program: TEST_SSH_PROGRAM.to_string(),
        arguments: vec!["-XY".to_string(), TEST_PLACEHOLDER.to_string()],
        username_host_placeholder: TEST_PLACEHOLDER.to_string(),
    };
}

/// Creates a temporary SSH config file for testing.
///
/// # Arguments
///
/// * `content` - The content to write to the SSH config file.
///
/// # Returns
///
/// A tuple containing the temporary directory path and the path to the SSH config file.
fn create_temp_ssh_config(content: &str) -> (PathBuf, String) {
    let temp_dir = env::temp_dir().join(format!("csshw_test_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temporary directory");
    let config_path = temp_dir.join("config");
    let mut file = File::create(&config_path).expect("Failed to create SSH config file");
    file.write_all(content.as_bytes())
        .expect("Failed to write SSH config content");
    let config_path_str = config_path.to_string_lossy().to_string();
    return (temp_dir, config_path_str);
}

/// Creates a mock KEY_EVENT_RECORD for testing.
///
/// # Arguments
///
/// * `key_down` - Whether the key is pressed down.
/// * `virtual_key_code` - The virtual key code.
/// * `control_key_state` - The control key state flags.
///
/// # Returns
///
/// A KEY_EVENT_RECORD for testing.
fn create_test_key_event(
    key_down: bool,
    virtual_key_code: u16,
    control_key_state: u32,
) -> KEY_EVENT_RECORD {
    return KEY_EVENT_RECORD {
        bKeyDown: key_down.into(),
        wRepeatCount: 1,
        wVirtualKeyCode: virtual_key_code,
        wVirtualScanCode: 0,
        uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0 },
        dwControlKeyState: control_key_state,
    };
}

#[test]
fn test_resolve_username_basic_scenarios() {
    let config = create_test_client_config("/nonexistent/path".to_string());

    // Test with provided username
    let result = resolve_username(Some(TEST_USERNAME.to_string()), TEST_HOSTNAME, &config);
    assert_eq!(result, TEST_USERNAME);

    // Test without username and no SSH config
    let result = resolve_username(None, TEST_HOSTNAME, &config);
    assert_eq!(result, "");

    // Test edge cases
    let result = resolve_username(Some(TEST_USERNAME.to_string()), "", &config);
    assert_eq!(result, TEST_USERNAME);

    let result = resolve_username(None, "", &config);
    assert_eq!(result, "");
}

#[test]
fn test_resolve_username_ssh_config_integration() {
    // Test that provided username always overrides SSH config
    let ssh_config_content = format!("Host {TEST_HOSTNAME}\n    User configuser\n");
    let (_temp_dir, config_path) = create_temp_ssh_config(&ssh_config_content);
    let config = create_test_client_config(config_path);

    let result = resolve_username(Some(TEST_USERNAME.to_string()), TEST_HOSTNAME, &config);
    assert_eq!(result, TEST_USERNAME);

    // Test SSH config parsing integration
    let result = resolve_username(None, TEST_HOSTNAME, &config);
    assert_eq!(result, "configuser");

    // Test empty SSH config
    let (_temp_dir, empty_config_path) = create_temp_ssh_config("");
    let empty_config = create_test_client_config(empty_config_path);
    let result = resolve_username(None, TEST_HOSTNAME, &empty_config);
    assert_eq!(result, "");
}

#[test]
fn test_resolve_username_special_characters() {
    let config = create_test_client_config("/nonexistent/path".to_string());

    // Test various special characters that might appear in usernames/hostnames
    let test_cases = [
        ("user.name", "sub.example.com", "user.name"),
        ("user-name", "host-name", "user-name"),
        ("user_name", "host_name", "user_name"),
        ("tëst", "exämple.com", "tëst"),             // Unicode
        (TEST_USERNAME, "host name", TEST_USERNAME), // Whitespace in hostname
    ];

    for (username, hostname, expected) in test_cases {
        let result = resolve_username(Some(username.to_string()), hostname, &config);
        assert_eq!(result, expected);
    }
}

#[test]
fn test_alt_shift_c_combination_detection() {
    // Test cases: (description, key_code, control_state, expected_result)
    let test_cases = [
        (
            "Left Alt + Shift + C",
            VK_C.0,
            LEFT_ALT_PRESSED | SHIFT_PRESSED,
            true,
        ),
        (
            "Right Alt + Shift + C",
            VK_C.0,
            RIGHT_ALT_PRESSED | SHIFT_PRESSED,
            true,
        ),
        (
            "Wrong key with modifiers",
            0x41,
            LEFT_ALT_PRESSED | SHIFT_PRESSED,
            false,
        ), // 'A' key
        ("C key without Shift", VK_C.0, LEFT_ALT_PRESSED, false),
        ("C key without Alt", VK_C.0, SHIFT_PRESSED, false),
        ("C key without modifiers", VK_C.0, 0, false),
    ];

    for (description, key_code, control_state, expected) in test_cases {
        let key_event = create_test_key_event(true, key_code, control_state);
        let result = is_alt_shift_c_combination(&key_event);
        assert_eq!(result, expected, "Failed test case: {description}");
    }

    // Test that key up/down state doesn't affect detection
    let key_event_down = create_test_key_event(true, VK_C.0, LEFT_ALT_PRESSED | SHIFT_PRESSED);
    let key_event_up = create_test_key_event(false, VK_C.0, LEFT_ALT_PRESSED | SHIFT_PRESSED);
    assert_eq!(
        is_alt_shift_c_combination(&key_event_down),
        is_alt_shift_c_combination(&key_event_up)
    );
}

#[test]
fn test_keep_alive_packet_detection() {
    // Test keep-alive packet (all bytes set to u8::MAX)
    let keep_alive_packet = [u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH];
    assert!(is_keep_alive_packet(&keep_alive_packet));

    // Test non-keep-alive packets
    let test_cases = [
        vec![0u8; SERIALIZED_INPUT_RECORD_0_LENGTH], // All zeros
        {
            let mut packet = vec![0u8; SERIALIZED_INPUT_RECORD_0_LENGTH];
            packet[0] = 1;
            packet
        }, // First byte different
        {
            let mut packet = vec![u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH];
            packet[SERIALIZED_INPUT_RECORD_0_LENGTH - 1] = 0;
            packet
        }, // Last byte different
        vec![u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH - 1], // Wrong length
        vec![u8::MAX; SERIALIZED_INPUT_RECORD_0_LENGTH + 1], // Wrong length
    ];

    for (i, packet) in test_cases.iter().enumerate() {
        assert!(!is_keep_alive_packet(packet), "Failed test case {i}");
    }
}

/// Test case structure for build_ssh_arguments function.
struct SshArgumentsTestCase<'a> {
    /// Description of what this test case is testing.
    description: &'a str,
    /// Username to test.
    username: &'a str,
    /// Hostname to test.
    host: &'a str,
    /// Optional port to test.
    port: Option<u16>,
    /// Configuration to use for the test.
    config: &'a ClientConfig,
    /// Expected output arguments.
    expected_output: Vec<String>,
}

#[test]
fn test_build_ssh_arguments() {
    let config = create_test_client_config("/nonexistent/path".to_string());
    let complex_config = ClientConfig {
        ssh_config_path: "/nonexistent/path".to_string(),
        program: TEST_SSH_PROGRAM.to_string(),
        arguments: vec![
            "-v".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            TEST_PLACEHOLDER.to_string(),
            "-X".to_string(),
        ],
        username_host_placeholder: TEST_PLACEHOLDER.to_string(),
    };

    let test_cases = [
        SshArgumentsTestCase {
            description: "basic case without port",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: None,
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
            ],
        },
        SshArgumentsTestCase {
            description: "empty username and host",
            username: "",
            host: "",
            port: None,
            config: &config,
            expected_output: vec!["-XY".to_string(), "@".to_string()],
        },
        SshArgumentsTestCase {
            description: "complex arguments without port",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: None,
            config: &complex_config,
            expected_output: vec![
                "-v".to_string(),
                "-o".to_string(),
                "StrictHostKeyChecking=no".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
                "-X".to_string(),
            ],
        },
        // Cases with port
        SshArgumentsTestCase {
            description: "basic case with port 2222",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: Some(2222),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
                "-p".to_string(),
                "2222".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "standard SSH port 22",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: Some(22),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
                "-p".to_string(),
                "22".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "high port number",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: Some(65535),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
                "-p".to_string(),
                "65535".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "complex arguments with port",
            username: TEST_USERNAME,
            host: TEST_HOSTNAME,
            port: Some(8080),
            config: &complex_config,
            expected_output: vec![
                "-v".to_string(),
                "-o".to_string(),
                "StrictHostKeyChecking=no".to_string(),
                format!("{TEST_USERNAME}@{TEST_HOSTNAME}"),
                "-X".to_string(),
                "-p".to_string(),
                "8080".to_string(),
            ],
        },
        // Special characters
        SshArgumentsTestCase {
            description: "hostname with dashes and port",
            username: "user",
            host: "host-name.example.com",
            port: Some(2222),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                "user@host-name.example.com".to_string(),
                "-p".to_string(),
                "2222".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "IP address with port",
            username: "user",
            host: "192.168.1.1",
            port: Some(8080),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                "user@192.168.1.1".to_string(),
                "-p".to_string(),
                "8080".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "IPv6 address with port",
            username: "user",
            host: "[::1]",
            port: Some(2222),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                "user@[::1]".to_string(),
                "-p".to_string(),
                "2222".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "underscores in username and hostname",
            username: "test_user",
            host: "test_host",
            port: Some(9999),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                "test_user@test_host".to_string(),
                "-p".to_string(),
                "9999".to_string(),
            ],
        },
        SshArgumentsTestCase {
            description: "dots in username and hostname",
            username: "user.name",
            host: "host.name",
            port: Some(1234),
            config: &config,
            expected_output: vec![
                "-XY".to_string(),
                "user.name@host.name".to_string(),
                "-p".to_string(),
                "1234".to_string(),
            ],
        },
    ];

    for test_case in test_cases {
        let result = build_ssh_arguments(
            test_case.username,
            test_case.host,
            test_case.port,
            test_case.config,
        );
        assert_eq!(
            result, test_case.expected_output,
            "Failed test case: {}",
            test_case.description
        );
    }
}

/// Test module for argument placeholder replacement
mod placeholder_test {
    use super::*;

    #[test]
    fn test_replace_argument_placeholders_basic() {
        let arguments = vec![
            "-v".to_string(),
            "{{USER_HOST}}".to_string(),
            "-X".to_string(),
        ];
        let placeholder = "{{USER_HOST}}";
        let replacement = "user@example.com";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "-v");
        assert_eq!(result[1], "user@example.com");
        assert_eq!(result[2], "-X");
    }

    #[test]
    fn test_replace_argument_placeholders_multiple_occurrences() {
        let arguments = vec![
            "{{HOST}}".to_string(),
            "-o".to_string(),
            "ProxyCommand=ssh {{HOST}} nc %h %p".to_string(),
            "{{HOST}}".to_string(),
        ];
        let placeholder = "{{HOST}}";
        let replacement = "jumphost";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "jumphost");
        assert_eq!(result[1], "-o");
        assert_eq!(result[2], "ProxyCommand=ssh jumphost nc %h %p");
        assert_eq!(result[3], "jumphost");
    }

    #[test]
    fn test_replace_argument_placeholders_no_matches() {
        let arguments = vec!["-v".to_string(), "-X".to_string(), "user@host".to_string()];
        let placeholder = "{{NONEXISTENT}}";
        let replacement = "replacement";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        // Should return identical arguments
        assert_eq!(result, arguments);
    }

    #[test]
    fn test_replace_argument_placeholders_empty_arguments() {
        let arguments: Vec<String> = vec![];
        let placeholder = "{{HOST}}";
        let replacement = "example.com";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert!(result.is_empty());
    }

    #[test]
    fn test_replace_argument_placeholders_partial_matches() {
        let arguments = vec![
            "{{HOST".to_string(),
            "HOST}}".to_string(),
            "{{HOST}}extra".to_string(),
            "prefix{{HOST}}".to_string(),
        ];
        let placeholder = "{{HOST}}";
        let replacement = "example.com";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert_eq!(result[0], "{{HOST");
        assert_eq!(result[1], "HOST}}");
        assert_eq!(result[2], "example.comextra");
        assert_eq!(result[3], "prefixexample.com");
    }

    #[test]
    fn test_replace_argument_placeholders_special_characters() {
        let arguments = vec![
            "{{USER@HOST}}".to_string(),
            "-o".to_string(),
            "UserKnownHostsFile={{USER@HOST}}.known_hosts".to_string(),
        ];
        let placeholder = "{{USER@HOST}}";
        let replacement = "test.user@sub.example.com";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert_eq!(result[0], "test.user@sub.example.com");
        assert_eq!(result[1], "-o");
        assert_eq!(
            result[2],
            "UserKnownHostsFile=test.user@sub.example.com.known_hosts"
        );
    }

    #[test]
    fn test_replace_argument_placeholders_unicode() {
        let arguments = vec!["{{ÜSER}}".to_string(), "hëllo {{ÜSER}}".to_string()];
        let placeholder = "{{ÜSER}}";
        let replacement = "tëst@exämple.com";

        let result = replace_argument_placeholders(&arguments, placeholder, replacement);

        assert_eq!(result[0], "tëst@exämple.com");
        assert_eq!(result[1], "hëllo tëst@exämple.com");
    }
}

/// Test module for console input writing
mod console_input_test {
    use super::*;

    #[test]
    fn test_write_console_input_basic() {
        // Test that write_console_input doesn't panic with valid input
        let input_record = INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: true.into(),
                wRepeatCount: 1,
                wVirtualKeyCode: 0x41, // 'A' key
                wVirtualScanCode: 0x1E,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: 'A' as u16,
                },
                dwControlKeyState: 0,
            },
        };

        // This function writes to the console input buffer
        // We can't easily test the actual writing, but we can ensure it doesn't panic
        write_console_input(input_record);
    }

    #[test]
    fn test_write_console_input_special_keys() {
        // Test with special key combinations
        let test_cases = vec![
            (0x0D, "Enter"),     // Enter key
            (0x08, "Backspace"), // Backspace
            (0x09, "Tab"),       // Tab
            (0x1B, "Escape"),    // Escape
            (0x20, "Space"),     // Space
        ];

        for (key_code, _description) in test_cases {
            let input_record = INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: true.into(),
                    wRepeatCount: 1,
                    wVirtualKeyCode: key_code,
                    wVirtualScanCode: 0,
                    uChar: KEY_EVENT_RECORD_0 {
                        UnicodeChar: key_code,
                    },
                    dwControlKeyState: 0,
                },
            };

            // Should not panic
            write_console_input(input_record);
        }
    }

    #[test]
    fn test_write_console_input_key_up_down() {
        // Test both key down and key up events
        for key_down in [true, false] {
            let input_record = INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: key_down.into(),
                    wRepeatCount: 1,
                    wVirtualKeyCode: 0x42, // 'B' key
                    wVirtualScanCode: 0x30,
                    uChar: KEY_EVENT_RECORD_0 {
                        UnicodeChar: 'B' as u16,
                    },
                    dwControlKeyState: 0,
                },
            };

            // Should not panic
            write_console_input(input_record);
        }
    }

    #[test]
    fn test_write_console_input_with_modifiers() {
        // Test with various modifier key combinations
        let modifier_states = vec![
            0x0001, // RIGHT_ALT_PRESSED
            0x0002, // LEFT_ALT_PRESSED
            0x0004, // RIGHT_CTRL_PRESSED
            0x0008, // SHIFT_PRESSED
            0x0010, // NUMLOCK_ON
            0x0020, // SCROLLLOCK_ON
            0x0040, // CAPSLOCK_ON
            0x0080, // ENHANCED_KEY
        ];

        for modifier_state in modifier_states {
            let input_record = INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: true.into(),
                    wRepeatCount: 1,
                    wVirtualKeyCode: 0x43, // 'C' key
                    wVirtualScanCode: 0x2E,
                    uChar: KEY_EVENT_RECORD_0 {
                        UnicodeChar: 'C' as u16,
                    },
                    dwControlKeyState: modifier_state,
                },
            };

            // Should not panic
            write_console_input(input_record);
        }
    }

    #[test]
    fn test_write_console_input_unicode_characters() {
        // Test with various Unicode characters
        let unicode_chars = vec![
            'ä', 'ö', 'ü', 'ß', // German
            'α', 'β', 'γ', 'δ', // Greek
            '中', '文', // Chinese
            '🦀', '🔥', // Emojis
        ];

        for unicode_char in unicode_chars {
            let input_record = INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: true.into(),
                    wRepeatCount: 1,
                    wVirtualKeyCode: 0,
                    wVirtualScanCode: 0,
                    uChar: KEY_EVENT_RECORD_0 {
                        UnicodeChar: unicode_char as u16,
                    },
                    dwControlKeyState: 0,
                },
            };

            // Should not panic
            write_console_input(input_record);
        }
    }
}

/// Test module for SSH config integration
mod ssh_config_integration_test {
    use super::*;

    #[test]
    fn test_resolve_username_simple_ssh_config() {
        // Test with a simple SSH config that should work
        let ssh_config_content = r#"Host testhost
    User testuser
"#;
        let (_temp_dir, config_path) = create_temp_ssh_config(ssh_config_content);
        let config = ClientConfig {
            ssh_config_path: config_path,
            program: "ssh".to_string(),
            arguments: vec![],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        // Test exact host match
        let result = resolve_username(None, "testhost", &config);
        assert_eq!(result, "testuser");

        // Test non-matching host should return empty string
        let result = resolve_username(None, "nonexistent", &config);
        assert_eq!(result, "");
    }

    #[test]
    fn test_resolve_username_nonexistent_ssh_config() {
        // Test with nonexistent SSH config file
        let config = ClientConfig {
            ssh_config_path: "/nonexistent/path/config".to_string(),
            program: "ssh".to_string(),
            arguments: vec![],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        let result = resolve_username(None, "any.host", &config);
        assert_eq!(result, "");
    }

    #[test]
    fn test_resolve_username_empty_ssh_config() {
        let (_temp_dir, config_path) = create_temp_ssh_config("");
        let config = ClientConfig {
            ssh_config_path: config_path,
            program: "ssh".to_string(),
            arguments: vec![],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        let result = resolve_username(None, "any.host", &config);
        assert_eq!(result, "");
    }
}

/// Test module for edge cases and error conditions
mod edge_cases_test {
    use super::*;

    #[test]
    fn test_build_ssh_arguments_edge_cases() {
        // Test with empty config arguments
        let empty_config = ClientConfig {
            ssh_config_path: "/nonexistent".to_string(),
            program: "ssh".to_string(),
            arguments: vec![],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        let result = build_ssh_arguments("user", "host", None, &empty_config);
        assert!(result.is_empty());

        let result = build_ssh_arguments("user", "host", Some(22), &empty_config);
        assert_eq!(result, vec!["-p".to_string(), "22".to_string()]);
    }

    #[test]
    fn test_build_ssh_arguments_no_placeholder() {
        // Test config without placeholder
        let config = ClientConfig {
            ssh_config_path: "/nonexistent".to_string(),
            program: "ssh".to_string(),
            arguments: vec!["-v".to_string(), "-X".to_string()],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        let result = build_ssh_arguments("user", "host", None, &config);
        assert_eq!(result, vec!["-v".to_string(), "-X".to_string()]);

        let result = build_ssh_arguments("user", "host", Some(8080), &config);
        assert_eq!(
            result,
            vec![
                "-v".to_string(),
                "-X".to_string(),
                "-p".to_string(),
                "8080".to_string()
            ]
        );
    }

    #[test]
    fn test_build_ssh_arguments_extreme_port_values() {
        let config = ClientConfig {
            ssh_config_path: "/nonexistent".to_string(),
            program: "ssh".to_string(),
            arguments: vec!["{{HOST}}".to_string()],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        // Test minimum port
        let result = build_ssh_arguments("user", "host", Some(1), &config);
        assert_eq!(
            result,
            vec!["user@host".to_string(), "-p".to_string(), "1".to_string()]
        );

        // Test maximum port
        let result = build_ssh_arguments("user", "host", Some(65535), &config);
        assert_eq!(
            result,
            vec![
                "user@host".to_string(),
                "-p".to_string(),
                "65535".to_string()
            ]
        );
    }

    #[test]
    fn test_resolve_username_edge_cases() {
        let config = ClientConfig {
            ssh_config_path: "/nonexistent/path/that/does/not/exist".to_string(),
            program: "ssh".to_string(),
            arguments: vec![],
            username_host_placeholder: "{{HOST}}".to_string(),
        };

        // Test with very long hostname
        let long_hostname = "a".repeat(1000);
        let result = resolve_username(Some("user".to_string()), &long_hostname, &config);
        assert_eq!(result, "user");

        // Test with empty strings
        let result = resolve_username(Some("".to_string()), "", &config);
        assert_eq!(result, "");

        // Test with whitespace
        let result = resolve_username(Some(" user ".to_string()), " host ", &config);
        assert_eq!(result, " user ");
    }
}
