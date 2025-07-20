//! Unit tests for the client module.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use windows::Win32::System::Console::{
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, LEFT_ALT_PRESSED, RIGHT_ALT_PRESSED, SHIFT_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;

use crate::client::{
    get_username_and_host, is_alt_shift_c_combination, is_keep_alive_packet,
    replace_argument_placeholders,
};
use crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH;
use crate::utils::config::ClientConfig;

// Test constants - consistent dummy values used throughout tests
const TEST_USERNAME: &str = "testuser";
const TEST_HOSTNAME: &str = "example.com";
const TEST_USERNAME_HOST: &str = "testuser@example.com";
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
fn test_get_username_and_host_basic_scenarios() {
    let config = create_test_client_config("/nonexistent/path".to_string());

    // Test with provided username
    let result = get_username_and_host(Some(TEST_USERNAME.to_string()), TEST_HOSTNAME, &config);
    assert_eq!(result, TEST_USERNAME_HOST);

    // Test without username and no SSH config
    let result = get_username_and_host(None, TEST_HOSTNAME, &config);
    assert_eq!(result, format!("@{TEST_HOSTNAME}"));

    // Test edge cases
    let result = get_username_and_host(Some(TEST_USERNAME.to_string()), "", &config);
    assert_eq!(result, format!("{TEST_USERNAME}@"));

    let result = get_username_and_host(None, "", &config);
    assert_eq!(result, "@");
}

#[test]
fn test_get_username_and_host_ssh_config_integration() {
    // Test that provided username always overrides SSH config
    let ssh_config_content = format!("Host {TEST_HOSTNAME}\n    User configuser\n");
    let (_temp_dir, config_path) = create_temp_ssh_config(&ssh_config_content);
    let config = create_test_client_config(config_path);

    let result = get_username_and_host(Some(TEST_USERNAME.to_string()), TEST_HOSTNAME, &config);
    assert_eq!(result, TEST_USERNAME_HOST);

    // Test SSH config parsing integration
    let result = get_username_and_host(None, TEST_HOSTNAME, &config);
    assert_eq!(result, format!("configuser@{TEST_HOSTNAME}"));

    // Test empty SSH config
    let (_temp_dir, empty_config_path) = create_temp_ssh_config("");
    let empty_config = create_test_client_config(empty_config_path);
    let result = get_username_and_host(None, TEST_HOSTNAME, &empty_config);
    assert_eq!(result, format!("@{TEST_HOSTNAME}"));
}

#[test]
fn test_get_username_and_host_special_characters() {
    let config = create_test_client_config("/nonexistent/path".to_string());

    // Test various special characters that might appear in usernames/hostnames
    let test_cases = [
        ("user.name", "sub.example.com", "user.name@sub.example.com"),
        ("user-name", "host-name", "user-name@host-name"),
        ("user_name", "host_name", "user_name@host_name"),
        ("tëst", "exämple.com", "tëst@exämple.com"), // Unicode
        (TEST_USERNAME, "host name", "testuser@host name"), // Whitespace
    ];

    for (username, hostname, expected) in test_cases {
        let result = get_username_and_host(Some(username.to_string()), hostname, &config);
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

#[test]
fn test_replace_argument_placeholders() {
    // Test single placeholder replacement
    let arguments = vec![
        "-XY".to_string(),
        TEST_PLACEHOLDER.to_string(),
        "-p".to_string(),
        "2222".to_string(),
    ];
    let result = replace_argument_placeholders(&arguments, TEST_PLACEHOLDER, TEST_USERNAME_HOST);
    let expected = vec![
        "-XY".to_string(),
        TEST_USERNAME_HOST.to_string(),
        "-p".to_string(),
        "2222".to_string(),
    ];
    assert_eq!(result, expected);

    // Test multiple placeholder occurrences
    let arguments = vec![
        TEST_PLACEHOLDER.to_string(),
        "-o".to_string(),
        format!("ProxyCommand=ssh gateway nc {} 22", TEST_PLACEHOLDER),
    ];
    let result = replace_argument_placeholders(&arguments, TEST_PLACEHOLDER, TEST_USERNAME_HOST);
    let expected = vec![
        TEST_USERNAME_HOST.to_string(),
        "-o".to_string(),
        format!("ProxyCommand=ssh gateway nc {} 22", TEST_USERNAME_HOST),
    ];
    assert_eq!(result, expected);

    // Test no placeholders
    let arguments = vec!["-v".to_string(), "-p".to_string(), "22".to_string()];
    let result = replace_argument_placeholders(&arguments, TEST_PLACEHOLDER, TEST_USERNAME_HOST);
    let expected = vec!["-v".to_string(), "-p".to_string(), "22".to_string()];
    assert_eq!(result, expected);

    // Test custom placeholder
    let arguments = vec!["-XY".to_string(), "{{CUSTOM}}".to_string()];
    let result = replace_argument_placeholders(&arguments, "{{CUSTOM}}", TEST_USERNAME_HOST);
    let expected = vec!["-XY".to_string(), TEST_USERNAME_HOST.to_string()];
    assert_eq!(result, expected);
}
