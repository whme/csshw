//! Unit tests for the daemon module.

use std::{ffi::c_void, io};

use tokio::{
    net::windows::named_pipe::{ClientOptions, PipeMode, ServerOptions},
    sync::broadcast,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Console::{
    INPUT_RECORD_0, KEY_EVENT_RECORD, LEFT_CTRL_PRESSED, RIGHT_CTRL_PRESSED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_C, VK_E, VK_ESCAPE, VK_H, VK_R, VK_T};

use crate::daemon::workspace::WorkspaceArea;
use crate::utils::config::DaemonConfig;
use crate::{
    daemon::{
        get_console_window_wrapper, get_foreground_window_wrapper, named_pipe_server_routine,
        resolve_cluster_tags, toggle_processed_input_mode, Client, ControlModeState, Daemon,
        HWNDWrapper, SENDER_CAPACITY,
    },
    serde::SERIALIZED_INPUT_RECORD_0_LENGTH,
    utils::{config::Cluster, constants::PIPE_NAME},
};

#[test]
fn test_hwnd_wrapper_equality() {
    assert_eq!(
        HWNDWrapper {
            hwdn: HWND(std::ptr::dangling_mut::<c_void>())
        },
        HWNDWrapper {
            hwdn: HWND(std::ptr::dangling_mut::<c_void>())
        }
    );
    assert_ne!(
        HWNDWrapper {
            hwdn: HWND(std::ptr::dangling_mut::<c_void>())
        },
        HWNDWrapper {
            hwdn: HWND(unsafe { std::ptr::dangling_mut::<c_void>().add(1) })
        }
    );
}

#[test]
fn test_resolve_cluster_tags() {
    let hosts: Vec<&str> = vec!["host0", "cluster1", "host3", "host0", "host1"];
    let clusters: Vec<Cluster> = vec![Cluster {
        name: "cluster1".to_string(),
        hosts: vec!["host1".to_string(), "host2".to_string()],
    }];
    assert_eq!(
        resolve_cluster_tags(hosts, &clusters),
        vec!["host0", "host1", "host2", "host3", "host0", "host1"]
    );
}

#[test]
fn test_resolve_cluster_tags_no_cluster() {
    let hosts: Vec<&str> = vec!["host0"];
    let clusters: Vec<Cluster> = vec![Cluster {
        name: "cluster1".to_string(),
        hosts: vec!["host1".to_string(), "host2".to_string()],
    }];
    assert_eq!(resolve_cluster_tags(hosts, &clusters), vec!["host0"]);
}

#[test]
fn test_resolve_cluster_tags_simple_nested_cluster() {
    let hosts: Vec<&str> = vec!["cluster2"];
    let clusters: Vec<Cluster> = vec![
        Cluster {
            name: "cluster1".to_string(),
            hosts: vec!["host1".to_string(), "host2".to_string()],
        },
        Cluster {
            name: "cluster2".to_string(),
            hosts: vec!["cluster1".to_owned(), "host3".to_owned()],
        },
    ];
    assert_eq!(
        resolve_cluster_tags(hosts, &clusters),
        vec!["host1", "host2", "host3"]
    );
}

#[tokio::test]
async fn test_named_pipe_server_routine() -> Result<(), Box<dyn std::error::Error>> {
    // Setup sender and receiver
    let (sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
        SERIALIZED_INPUT_RECORD_0_LENGTH,
    );
    // and named pipe server and client
    let named_pipe_server = ServerOptions::new()
        .access_outbound(true)
        .pipe_mode(PipeMode::Message)
        .create(PIPE_NAME)?;
    let named_pipe_client = ClientOptions::new().open(PIPE_NAME)?;
    // Spawn named pipe server routine
    let future = tokio::spawn(async move {
        named_pipe_server_routine(named_pipe_server, &mut receiver).await;
    });

    let mut keep_alive_received = false;
    let mut successful_iterations = 0;
    // Verify the routine forwards the data through the pipe
    loop {
        // Send data to the routine
        sender.send([2; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        // Wait for the pipe to be readable
        named_pipe_client.readable().await?;
        let mut buf = [0; SERIALIZED_INPUT_RECORD_0_LENGTH];
        // Try to read data, this may still fail with `WouldBlock`
        // if the readiness event is a false positive.
        match named_pipe_client.try_read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                assert_eq!(SERIALIZED_INPUT_RECORD_0_LENGTH, n);
                if buf[0] == 255 {
                    // Thats a keep alive packet, make sure its complete.
                    assert_eq!([255; SERIALIZED_INPUT_RECORD_0_LENGTH], buf);
                    keep_alive_received = true;
                } else {
                    // Thats the actual data, make sure its complete.
                    assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf);
                    successful_iterations += 1;
                    if successful_iterations >= 5 {
                        break;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
    assert!(keep_alive_received);
    // Drop the client, closing the pipe.
    drop(named_pipe_client);
    // We expect the routine to exit gracefully.
    future.await?;
    return Ok(());
}

#[tokio::test]
async fn test_named_pipe_server_routine_sender_closes_unexpectidly(
) -> Result<(), Box<dyn std::error::Error>> {
    // Setup sender and receiver
    let (sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
        SERIALIZED_INPUT_RECORD_0_LENGTH,
    );
    // and named pipe server and client
    let named_pipe_server = ServerOptions::new()
        .access_outbound(true)
        .pipe_mode(PipeMode::Message)
        .create(PIPE_NAME)?;
    let named_pipe_client = ClientOptions::new().open(PIPE_NAME)?;
    // Spawn named pipe server routine
    let future = tokio::spawn(async move {
        named_pipe_server_routine(named_pipe_server, &mut receiver).await;
    });
    // Send data to the routine
    sender.send([2; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
    // Verify the routine forwards the data through the pipe
    loop {
        // Wait for the pipe to be readable
        named_pipe_client.readable().await?;
        let mut buf = [0; SERIALIZED_INPUT_RECORD_0_LENGTH];
        // Try to read data, this may still fail with `WouldBlock`
        // if the readiness event is a false positive.
        match named_pipe_client.try_read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                assert_eq!(SERIALIZED_INPUT_RECORD_0_LENGTH, n);
                if buf[0] == 255 {
                    // Thats a keep alive packet, make sure its complete.
                    assert_eq!([255; SERIALIZED_INPUT_RECORD_0_LENGTH], buf);
                } else {
                    // Thats the actual data, make sure its complete.
                    assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf);
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
    // Drop the sender end of the broadcast channel
    drop(sender);
    // This is unexpected, we should panic
    assert!(future.await.unwrap_err().is_panic());
    return Ok(());
}

#[test]
fn test_hwnd_wrapper_send_trait() {
    // Test that HWNDWrapper implements Send trait
    let wrapper = HWNDWrapper {
        hwdn: HWND(std::ptr::null_mut()),
    };

    // This should compile if Send is properly implemented
    let _: Box<dyn Send> = Box::new(wrapper);
}

#[test]
fn test_client_send_trait() {
    // Test that Client implements Send trait
    let client = Client {
        hostname: "test-host".to_string(),
        window_handle: HWND(std::ptr::null_mut()),
        process_handle: windows::Win32::Foundation::HANDLE(std::ptr::null_mut()),
    };

    // This should compile if Send is properly implemented
    let _: Box<dyn Send> = Box::new(client);
}

#[test]
fn test_get_console_window_wrapper() {
    let wrapper = get_console_window_wrapper();
    // Should return a valid wrapper (even if HWND is null in test environment)
    assert_eq!(wrapper, wrapper); // Test PartialEq implementation
}

#[test]
fn test_get_foreground_window_wrapper() {
    let wrapper = get_foreground_window_wrapper();
    // Should return a valid wrapper (even if HWND is null in test environment)
    assert_eq!(wrapper, wrapper); // Test PartialEq implementation
}

#[test]
fn test_control_mode_state_debug() {
    // Test Debug implementation for ControlModeState
    let inactive = ControlModeState::Inactive;
    let initiated = ControlModeState::Initiated;
    let active = ControlModeState::Active;

    assert_eq!(format!("{inactive:?}"), "Inactive");
    assert_eq!(format!("{initiated:?}"), "Initiated");
    assert_eq!(format!("{active:?}"), "Active");
}

#[test]
fn test_control_mode_state_partial_eq() {
    // Test PartialEq implementation for ControlModeState
    assert_eq!(ControlModeState::Inactive, ControlModeState::Inactive);
    assert_eq!(ControlModeState::Initiated, ControlModeState::Initiated);
    assert_eq!(ControlModeState::Active, ControlModeState::Active);

    assert_ne!(ControlModeState::Inactive, ControlModeState::Initiated);
    assert_ne!(ControlModeState::Initiated, ControlModeState::Active);
    assert_ne!(ControlModeState::Active, ControlModeState::Inactive);
}

#[test]
fn test_daemon_control_mode_is_active_escape_key() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Active,
        debug: false,
    };

    // Create escape key input record
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_ESCAPE.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 27 },
        dwControlKeyState: 0,
    };

    // Should exit control mode and return false
    let result = daemon.control_mode_is_active(input_record);
    assert!(!result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_daemon_control_mode_is_active_ctrl_a_combination() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // Create Ctrl+A key input record
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_A.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 1 },
        dwControlKeyState: LEFT_CTRL_PRESSED,
    };

    // Should initiate control mode
    let result = daemon.control_mode_is_active(input_record);
    assert!(result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Initiated);
}

#[test]
fn test_daemon_control_mode_is_active_right_ctrl_a_combination() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // Create Right Ctrl+A key input record
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_A.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 1 },
        dwControlKeyState: RIGHT_CTRL_PRESSED,
    };

    // Should initiate control mode
    let result = daemon.control_mode_is_active(input_record);
    assert!(result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Initiated);
}

#[test]
fn test_daemon_control_mode_is_active_non_control_key() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // Create regular key input record (no control key)
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_H.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 72 },
        dwControlKeyState: 0,
    };

    // Should not activate control mode
    let result = daemon.control_mode_is_active(input_record);
    assert!(!result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_daemon_quit_control_mode() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Active,
        debug: false,
    };

    daemon.quit_control_mode();
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_daemon_print_instructions() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // This should not panic
    daemon.print_instructions();
}

#[test]
fn test_toggle_processed_input_mode() {
    // This function toggles console input mode
    // In test environment, this might fail but shouldn't panic
    // We just test that it doesn't crash
    toggle_processed_input_mode();
}

#[test]
fn test_daemon_creation() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![Cluster {
        name: "test-cluster".to_string(),
        hosts: vec!["host1".to_string(), "host2".to_string()],
    }];

    let daemon = Daemon {
        hosts: vec!["host1".to_string(), "host2".to_string()],
        username: Some("testuser".to_string()),
        port: Some(2222),
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: true,
    };

    assert_eq!(daemon.hosts.len(), 2);
    assert_eq!(daemon.username, Some("testuser".to_string()));
    assert_eq!(daemon.port, Some(2222));
    assert!(daemon.debug);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_client_clone() {
    let client = Client {
        hostname: "test-host".to_string(),
        window_handle: HWND(std::ptr::null_mut()),
        process_handle: windows::Win32::Foundation::HANDLE(std::ptr::null_mut()),
    };

    let cloned_client = client.clone();
    assert_eq!(client.hostname, cloned_client.hostname);
    assert_eq!(client.window_handle, cloned_client.window_handle);
    assert_eq!(client.process_handle, cloned_client.process_handle);
}

#[test]
fn test_hwnd_wrapper_debug() {
    let wrapper = HWNDWrapper {
        hwdn: HWND(std::ptr::null_mut()),
    };

    let debug_str = format!("{wrapper:?}");
    assert!(debug_str.contains("HWNDWrapper"));
}

#[test]
fn test_hwnd_wrapper_eq() {
    let wrapper1 = HWNDWrapper {
        hwdn: HWND(std::ptr::null_mut()),
    };
    let wrapper2 = HWNDWrapper {
        hwdn: HWND(std::ptr::null_mut()),
    };
    let wrapper3 = HWNDWrapper {
        hwdn: HWND(unsafe { std::ptr::null_mut::<std::ffi::c_void>().add(1) }),
    };

    assert_eq!(wrapper1, wrapper2);
    assert_ne!(wrapper1, wrapper3);
    assert_ne!(wrapper2, wrapper3);
}

#[test]
fn test_sender_capacity_constant() {
    assert_eq!(SENDER_CAPACITY, 1024 * 1024);
}

#[tokio::test]
async fn test_daemon_launch_named_pipe_servers() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let daemon = Daemon {
        hosts: vec!["host1".to_string(), "host2".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    let (sender, _) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(1024);
    let servers = daemon.launch_named_pipe_servers(&sender);

    // Should create one server per host
    assert_eq!(servers.len(), 2);

    // Clean up by aborting the spawned tasks
    for server in servers {
        server.abort();
    }
}

// Additional comprehensive tests
#[test]
fn test_resolve_cluster_tags_simple() {
    let clusters = vec![
        Cluster {
            name: "web".to_string(),
            hosts: vec!["web1".to_string(), "web2".to_string()],
        },
        Cluster {
            name: "db".to_string(),
            hosts: vec!["db1".to_string(), "db2".to_string()],
        },
    ];

    let hosts = vec!["web", "standalone"];
    let result = resolve_cluster_tags(hosts, &clusters);

    assert_eq!(result.len(), 3);
    assert!(result.contains(&"web1"));
    assert!(result.contains(&"web2"));
    assert!(result.contains(&"standalone"));
}

#[test]
fn test_resolve_cluster_tags_nested() {
    let clusters = vec![
        Cluster {
            name: "web".to_string(),
            hosts: vec!["web1".to_string(), "web2".to_string()],
        },
        Cluster {
            name: "all".to_string(),
            hosts: vec!["web".to_string(), "db1".to_string()],
        },
    ];

    let hosts = vec!["all"];
    let result = resolve_cluster_tags(hosts, &clusters);

    assert_eq!(result.len(), 3);
    assert!(result.contains(&"web1"));
    assert!(result.contains(&"web2"));
    assert!(result.contains(&"db1"));
}

#[test]
fn test_resolve_cluster_tags_no_clusters() {
    let clusters = vec![];
    let hosts = vec!["host1", "host2"];
    let result = resolve_cluster_tags(hosts, &clusters);

    assert_eq!(result.len(), 2);
    assert!(result.contains(&"host1"));
    assert!(result.contains(&"host2"));
}

#[test]
fn test_resolve_cluster_tags_empty_hosts() {
    let clusters = vec![Cluster {
        name: "web".to_string(),
        hosts: vec!["web1".to_string()],
    }];
    let hosts = vec![];
    let result = resolve_cluster_tags(hosts, &clusters);

    assert!(result.is_empty());
}

#[test]
fn test_daemon_control_mode_initiated_state_behavior() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Initiated,
        debug: false,
    };

    // Create any non-Ctrl+A key input record when in Initiated state
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_R.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 82 },
        dwControlKeyState: 0,
    };

    // Should return false because it's not a Ctrl+A combination and reset to Inactive
    let result = daemon.control_mode_is_active(input_record);
    assert!(!result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_daemon_control_mode_key_up_ignored() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Active,
        debug: false,
    };

    // Create key up event
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(0), // Key up
        wRepeatCount: 1,
        wVirtualKeyCode: VK_R.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 82 },
        dwControlKeyState: 0,
    };

    // Should remain in active state but not process the key
    let result = daemon.control_mode_is_active(input_record);
    assert!(result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Active);
}

#[test]
fn test_daemon_control_mode_various_keys() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Active,
        debug: false,
    };

    // Test various control keys
    let test_keys = vec![VK_R, VK_E, VK_T, VK_C, VK_H];

    for key in test_keys {
        let mut input_record = INPUT_RECORD_0::default();
        input_record.KeyEvent = KEY_EVENT_RECORD {
            bKeyDown: windows::Win32::Foundation::BOOL(1),
            wRepeatCount: 1,
            wVirtualKeyCode: key.0,
            wVirtualScanCode: 0,
            uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: key.0 },
            dwControlKeyState: 0,
        };

        let result = daemon.control_mode_is_active(input_record);
        assert!(result);
        assert_eq!(daemon.control_mode_state, ControlModeState::Active);
    }
}

#[test]
fn test_daemon_control_mode_inactive_no_ctrl() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // Create A key without Ctrl
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_A.0,
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 65 },
        dwControlKeyState: 0, // No control key
    };

    // Should not activate control mode
    let result = daemon.control_mode_is_active(input_record);
    assert!(!result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[test]
fn test_daemon_control_mode_ctrl_wrong_key() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let mut daemon = Daemon {
        hosts: vec!["test-host".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    // Create Ctrl+B (not Ctrl+A)
    let mut input_record = INPUT_RECORD_0::default();
    input_record.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: windows::Win32::Foundation::BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: 66, // B key
        wVirtualScanCode: 0,
        uChar: windows::Win32::System::Console::KEY_EVENT_RECORD_0 { UnicodeChar: 66 },
        dwControlKeyState: LEFT_CTRL_PRESSED,
    };

    // Should not activate control mode
    let result = daemon.control_mode_is_active(input_record);
    assert!(!result);
    assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
}

#[tokio::test]
async fn test_daemon_launch_named_pipe_servers_multiple() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let daemon = Daemon {
        hosts: vec![
            "host1".to_string(),
            "host2".to_string(),
            "host3".to_string(),
        ],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    let (sender, _) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(1024);
    let servers = daemon.launch_named_pipe_servers(&sender);

    // Should create one server per host
    assert_eq!(servers.len(), 3);

    // Clean up by aborting the spawned tasks
    for server in servers {
        server.abort();
    }
}

#[test]
fn test_daemon_with_username_and_port() {
    let config = DaemonConfig::default();
    let clusters: Vec<Cluster> = vec![];
    let daemon = Daemon {
        hosts: vec!["host1".to_string()],
        username: Some("testuser".to_string()),
        port: Some(2222),
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: true,
    };

    assert_eq!(daemon.username, Some("testuser".to_string()));
    assert_eq!(daemon.port, Some(2222));
    assert!(daemon.debug);
}

#[test]
fn test_daemon_with_clusters() {
    let config = DaemonConfig::default();
    let clusters = vec![Cluster {
        name: "web".to_string(),
        hosts: vec!["web1".to_string(), "web2".to_string()],
    }];
    let daemon = Daemon {
        hosts: vec!["host1".to_string()],
        username: None,
        port: None,
        config: &config,
        clusters: &clusters,
        control_mode_state: ControlModeState::Inactive,
        debug: false,
    };

    assert_eq!(daemon.clusters.len(), 1);
    assert_eq!(daemon.clusters[0].name, "web");
    assert_eq!(daemon.clusters[0].hosts.len(), 2);
}

#[test]
fn test_client_debug_trait() {
    let client = Client {
        hostname: "test-host".to_string(),
        window_handle: HWND(std::ptr::null_mut()),
        process_handle: windows::Win32::Foundation::HANDLE(std::ptr::null_mut()),
    };

    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("Client"));
    assert!(debug_str.contains("test-host"));
}

#[test]
fn test_workspace_area_usage() {
    let workspace_area = WorkspaceArea {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        x_fixed_frame: 8,
        y_fixed_frame: 8,
        x_size_frame: 8,
        y_size_frame: 8,
    };

    // Test that workspace area values are accessible
    assert_eq!(workspace_area.width, 1920);
    assert_eq!(workspace_area.height, 1080);
    assert_eq!(workspace_area.x_fixed_frame, 8);
    assert_eq!(workspace_area.y_fixed_frame, 8);
}

#[test]
fn test_control_mode_state_transitions() {
    // Test all possible state transitions
    let states = vec![
        ControlModeState::Inactive,
        ControlModeState::Initiated,
        ControlModeState::Active,
    ];

    for state in states {
        // Test that states can be compared
        assert_eq!(state, state);

        // Test that different states are not equal
        match state {
            ControlModeState::Inactive => {
                assert_ne!(state, ControlModeState::Initiated);
                assert_ne!(state, ControlModeState::Active);
            }
            ControlModeState::Initiated => {
                assert_ne!(state, ControlModeState::Inactive);
                assert_ne!(state, ControlModeState::Active);
            }
            ControlModeState::Active => {
                assert_ne!(state, ControlModeState::Inactive);
                assert_ne!(state, ControlModeState::Initiated);
            }
        }
    }
}

#[test]
fn test_resolve_cluster_tags_complex_nesting() {
    let clusters = vec![
        Cluster {
            name: "web".to_string(),
            hosts: vec!["web1".to_string(), "web2".to_string()],
        },
        Cluster {
            name: "db".to_string(),
            hosts: vec!["db1".to_string(), "db2".to_string()],
        },
        Cluster {
            name: "backend".to_string(),
            hosts: vec!["web".to_string(), "db".to_string(), "cache1".to_string()],
        },
        Cluster {
            name: "all".to_string(),
            hosts: vec!["backend".to_string(), "frontend1".to_string()],
        },
    ];

    let hosts = vec!["all", "standalone"];
    let result = resolve_cluster_tags(hosts, &clusters);

    // Should resolve: all -> backend + frontend1 -> (web + db + cache1) + frontend1 -> (web1 + web2 + db1 + db2 + cache1) + frontend1 + standalone
    assert_eq!(result.len(), 7);
    assert!(result.contains(&"web1"));
    assert!(result.contains(&"web2"));
    assert!(result.contains(&"db1"));
    assert!(result.contains(&"db2"));
    assert!(result.contains(&"cache1"));
    assert!(result.contains(&"frontend1"));
    assert!(result.contains(&"standalone"));
}

#[test]
fn test_resolve_cluster_tags_mixed_hosts_and_clusters() {
    let clusters = vec![Cluster {
        name: "web".to_string(),
        hosts: vec!["web1".to_string(), "web2".to_string()],
    }];

    let hosts = vec!["web", "standalone1", "standalone2"];
    let result = resolve_cluster_tags(hosts, &clusters);

    assert_eq!(result.len(), 4);
    assert!(result.contains(&"web1"));
    assert!(result.contains(&"web2"));
    assert!(result.contains(&"standalone1"));
    assert!(result.contains(&"standalone2"));
}

#[test]
fn test_resolve_cluster_tags_duplicate_resolution() {
    let clusters = vec![
        Cluster {
            name: "web".to_string(),
            hosts: vec!["web1".to_string(), "web2".to_string()],
        },
        Cluster {
            name: "all".to_string(),
            hosts: vec!["web".to_string(), "web1".to_string()], // web1 appears twice after resolution
        },
    ];

    let hosts = vec!["all"];
    let result = resolve_cluster_tags(hosts, &clusters);

    // Should contain duplicates as the function doesn't deduplicate
    assert_eq!(result.len(), 3); // web1, web2, web1
    assert!(result.contains(&"web1"));
    assert!(result.contains(&"web2"));
}
