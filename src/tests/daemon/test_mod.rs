mod daemon_test {
    use std::{
        ffi::c_void,
        io,
        sync::{Arc, Mutex},
    };

    use tokio::{
        net::windows::named_pipe::{ClientOptions, NamedPipeClient, PipeMode, ServerOptions},
        sync::broadcast,
    };
    use windows::Win32::Foundation::{HANDLE, HWND};

    use crate::{
        daemon::{
            named_pipe_server_routine, resolve_cluster_tags, Client, Clients, HWNDWrapper,
            PipeServerState,
        },
        serde::SERIALIZED_INPUT_RECORD_0_LENGTH,
        utils::{config::Cluster, constants::PIPE_NAME},
    };

    /// Send `pid` as a 4 byte little-endian sequence to the pipe server.
    ///
    /// Mirrors the client-side PID handshake used by [`crate::client`].
    async fn send_pid(client: &NamedPipeClient, pid: u32) -> io::Result<()> {
        let bytes = pid.to_le_bytes();
        let mut written = 0usize;
        while written < bytes.len() {
            client.writable().await?;
            match client.try_write(&bytes[written..]) {
                Ok(0) => return Err(io::Error::other("pipe closed before handshake")),
                Ok(n) => written += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        }
        return Ok(());
    }

    /// Construct a [`Clients`] collection holding a single [`Client`] whose
    /// `process_id` equals `pid`. All other fields carry sentinel values as
    /// they are unused by the pipe server routine.
    fn make_clients_with_pid(pid: u32) -> Arc<Mutex<Clients>> {
        let mut clients = Clients::new();
        clients.push(Client {
            hostname: format!("test-host-{pid}"),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: pid,
            pipe_server_state: Arc::new(Mutex::new(PipeServerState::Enabled)),
        });
        return Arc::new(Mutex::new(clients));
    }

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
        const TEST_PID: u32 = 11111;
        // Setup sender and receiver
        let (sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        // and named pipe server and client
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(PIPE_NAME)?;
        let named_pipe_client = ClientOptions::new().open(PIPE_NAME)?;
        // Build a Clients collection containing the test PID so PID correlation succeeds.
        let clients = make_clients_with_pid(TEST_PID);
        // Complete the PID handshake expected by the pipe server routine.
        send_pid(&named_pipe_client, TEST_PID).await?;
        // Spawn named pipe server routine
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
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
    async fn test_named_pipe_server_routine_sender_closes_unexpectedly(
    ) -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 22222;
        // Setup sender and receiver
        let (sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        // and named pipe server and client
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(PIPE_NAME)?;
        let named_pipe_client = ClientOptions::new().open(PIPE_NAME)?;
        // Build a Clients collection containing the test PID so PID correlation succeeds.
        let clients = make_clients_with_pid(TEST_PID);
        // Complete the PID handshake expected by the pipe server routine.
        send_pid(&named_pipe_client, TEST_PID).await?;
        // Spawn named pipe server routine
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
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

    #[tokio::test]
    async fn test_named_pipe_server_routine_pid_mismatch() -> Result<(), Box<dyn std::error::Error>>
    {
        const REGISTERED_PID: u32 = 33333;
        const SENT_PID: u32 = 44444;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(r"\\.\pipe\csshw-test-pid-mismatch-{}", std::process::id());
        // Setup sender and receiver
        let (_sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(&pipe_name)?;
        let named_pipe_client = ClientOptions::new().open(&pipe_name)?;
        // Daemon only knows about REGISTERED_PID, but the client will send SENT_PID.
        let clients = make_clients_with_pid(REGISTERED_PID);
        send_pid(&named_pipe_client, SENT_PID).await?;
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });
        // Unknown PID is unrecoverable — the routine must panic (exits the daemon in production).
        assert!(future.await.unwrap_err().is_panic());
        return Ok(());
    }

    #[tokio::test]
    async fn test_named_pipe_server_routine_client_closes_before_pid_handshake(
    ) -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 55555;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(
            r"\\.\pipe\csshw-test-client-closes-before-pid-{}",
            std::process::id()
        );
        let (_sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(&pipe_name)?;
        let named_pipe_client = ClientOptions::new().open(&pipe_name)?;
        let clients = make_clients_with_pid(TEST_PID);
        // Drop the client immediately without sending any PID bytes.
        drop(named_pipe_client);
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });
        // Pipe closed before handshake completed — the routine must panic.
        assert!(future.await.unwrap_err().is_panic());
        return Ok(());
    }

    #[test]
    #[should_panic(expected = "Duplicate client PID")]
    fn test_clients_push_duplicate_pid_panics() {
        let mut clients = Clients::new();
        let make_client = |pid: u32| {
            return Client {
                hostname: "host".to_owned(),
                window_handle: HWND(std::ptr::null_mut()),
                process_handle: HANDLE::default(),
                process_id: pid,
                pipe_server_state: Arc::new(Mutex::new(PipeServerState::Enabled)),
            };
        };
        clients.push(make_client(1000));
        clients.push(make_client(1000)); // duplicate — must panic
    }

    #[test]
    fn test_clients_push_and_lookup() {
        let mut clients = Clients::new();
        assert!(clients.is_empty());
        assert_eq!(clients.len(), 0);

        let client_a = Client {
            hostname: "host-a".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 1000,
            pipe_server_state: Arc::new(Mutex::new(PipeServerState::Enabled)),
        };
        let client_b = Client {
            hostname: "host-b".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 2000,
            pipe_server_state: Arc::new(Mutex::new(PipeServerState::Enabled)),
        };
        let client_c = Client {
            hostname: "host-c".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 3000,
            pipe_server_state: Arc::new(Mutex::new(PipeServerState::Enabled)),
        };

        clients.push(client_a);
        clients.push(client_b);
        clients.push(client_c);

        assert_eq!(clients.len(), 3);
        assert!(!clients.is_empty());
        assert_eq!(clients.get_by_pid(1000).unwrap().hostname, "host-a");
        assert_eq!(clients.get_by_pid(2000).unwrap().hostname, "host-b");
        assert_eq!(clients.get_by_pid(3000).unwrap().hostname, "host-c");
        assert!(clients.get_by_pid(9999).is_none());

        // iter preserves insertion order
        let hostnames: Vec<&str> = clients.iter().map(|c| return c.hostname.as_str()).collect();
        assert_eq!(hostnames, vec!["host-a", "host-b", "host-c"]);

        // retain rebuilds the PID index so lookups remain consistent
        clients.retain(|client| return client.process_id != 2000);
        assert_eq!(clients.len(), 2);
        assert!(clients.get_by_pid(2000).is_none());
        assert_eq!(clients.get_by_pid(1000).unwrap().hostname, "host-a");
        assert_eq!(clients.get_by_pid(3000).unwrap().hostname, "host-c");
        let hostnames_after_retain: Vec<&str> = clients
            .as_slice()
            .iter()
            .map(|c| return c.hostname.as_str())
            .collect();
        assert_eq!(hostnames_after_retain, vec!["host-a", "host-c"]);
    }
}
