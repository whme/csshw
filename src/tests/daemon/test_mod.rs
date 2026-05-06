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
            named_pipe_server_routine, resolve_cluster_tags, toggle_pipe_server_states, Client,
            Clients, HWNDWrapper, PipeServerState,
        },
        protocol::{
            serialization::serialize_pid, FRAMED_INPUT_RECORD_LENGTH, FRAMED_KEEP_ALIVE_LENGTH,
            SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH, TAG_INPUT_RECORD,
            TAG_KEEP_ALIVE,
        },
        utils::{config::Cluster, constants::PIPE_NAME},
    };

    /// Send `pid` as a 4 byte little-endian sequence to the pipe server.
    ///
    /// Mirrors the client-side PID handshake used by [`crate::client`].
    async fn send_pid(client: &NamedPipeClient, pid: u32) -> io::Result<()> {
        let bytes = serialize_pid(pid);
        let mut written = 0usize;
        while written < SERIALIZED_PID_LENGTH {
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
            let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match named_pipe_client.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => match buf[0] {
                    TAG_KEEP_ALIVE => {
                        // Keep-alive frame: just the tag byte.
                        assert_eq!(FRAMED_KEEP_ALIVE_LENGTH, n);
                        keep_alive_received = true;
                    }
                    TAG_INPUT_RECORD => {
                        // Input-record frame: tag byte + 13-byte payload.
                        assert_eq!(FRAMED_INPUT_RECORD_LENGTH, n);
                        assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf[1..]);
                        successful_iterations += 1;
                        if successful_iterations >= 5 {
                            break;
                        }
                    }
                    other => panic!("Unexpected tag byte 0x{other:02X}"),
                },
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
            let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match named_pipe_client.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => match buf[0] {
                    TAG_KEEP_ALIVE => {
                        // Keep-alive frame: just the tag byte.
                        assert_eq!(FRAMED_KEEP_ALIVE_LENGTH, n);
                    }
                    TAG_INPUT_RECORD => {
                        // Input-record frame: tag byte + 13-byte payload.
                        assert_eq!(FRAMED_INPUT_RECORD_LENGTH, n);
                        assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf[1..]);
                        break;
                    }
                    other => panic!("Unexpected tag byte 0x{other:02X}"),
                },
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
        // Unknown PID is unrecoverable - the routine must panic (exits the daemon in production).
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
        // Pipe closed before handshake completed - the routine must panic.
        assert!(future.await.unwrap_err().is_panic());
        return Ok(());
    }

    /// Construct a [`Clients`] collection holding a single [`Client`] whose
    /// `process_id` equals `pid`, returning both the collection and the
    /// shared [`PipeServerState`] handle so the caller can mutate it.
    fn make_clients_with_pid_and_state(
        pid: u32,
    ) -> (Arc<Mutex<Clients>>, Arc<Mutex<PipeServerState>>) {
        let state = Arc::new(Mutex::new(PipeServerState::Enabled));
        let mut clients = Clients::new();
        clients.push(Client {
            hostname: format!("test-host-{pid}"),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: pid,
            pipe_server_state: Arc::clone(&state),
        });
        return (Arc::new(Mutex::new(clients)), state);
    }

    /// Verifies that when a client's [`PipeServerState`] is set to
    /// [`PipeServerState::Disabled`], the pipe server routine consumes
    /// broadcast messages but does not forward them through the pipe.
    /// Only keep-alive packets should arrive on the client side.
    #[tokio::test]
    async fn test_named_pipe_server_routine_disabled() -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 66666;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(r"\\.\pipe\csshw-test-disabled-{}", std::process::id());
        let (sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(&pipe_name)?;
        let named_pipe_client = ClientOptions::new().open(&pipe_name)?;
        let (clients, pipe_server_state) = make_clients_with_pid_and_state(TEST_PID);
        send_pid(&named_pipe_client, TEST_PID).await?;
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });

        // First, verify data flows while enabled. The pipe carries
        // tagged frames now: keep-alive frames are a single
        // `TAG_KEEP_ALIVE` byte, input-record frames are
        // `[TAG_INPUT_RECORD][13-byte payload]`.
        sender.send([2; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        let mut got_data = false;
        loop {
            named_pipe_client.readable().await?;
            let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
            match named_pipe_client.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => match buf[0] {
                    TAG_KEEP_ALIVE => {
                        assert_eq!(FRAMED_KEEP_ALIVE_LENGTH, n);
                    }
                    TAG_INPUT_RECORD => {
                        assert_eq!(FRAMED_INPUT_RECORD_LENGTH, n);
                        assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf[1..]);
                        got_data = true;
                        break;
                    }
                    other => panic!("Unexpected tag byte 0x{other:02X}"),
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
        }
        assert!(got_data);

        // Disable the client.
        *pipe_server_state.lock().unwrap() = PipeServerState::Disabled;

        // Send more data - it must NOT arrive at the client.
        const SENDS: usize = 5;
        for _ in 0..SENDS {
            sender.send([3; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        }

        // The disabled branch writes one keep-alive frame per consumed
        // broadcast record, so we expect at least `SENDS` keep-alive
        // frames to arrive. Read exactly that many and assert each is
        // a `TAG_KEEP_ALIVE` byte - any `TAG_INPUT_RECORD` frame would
        // be a leak of broadcast data and must fail the test.
        //
        // The whole loop is bounded by a `tokio::time::timeout` so a
        // regression that stops keep-alive emission surfaces as a
        // deterministic assertion instead of a hung test.
        let read_result: Result<Result<(), Box<dyn std::error::Error>>, _> =
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut received = 0;
                while received < SENDS {
                    named_pipe_client.readable().await?;
                    let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
                    match named_pipe_client.try_read(&mut buf) {
                        Ok(0) => {
                            return Err(
                                "named pipe closed before all keep-alive frames arrived".into(),
                            );
                        }
                        Ok(n) => match buf[0] {
                            TAG_KEEP_ALIVE => {
                                assert_eq!(
                                    FRAMED_KEEP_ALIVE_LENGTH, n,
                                    "Keep-alive frame must be exactly one byte"
                                );
                                received += 1;
                            }
                            TAG_INPUT_RECORD => panic!(
                                "Received input-record frame after disabling - broadcast data leaked through"
                            ),
                            other => panic!("Unexpected tag byte 0x{other:02X}"),
                        },
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
                return Ok(());
            })
            .await;
        match read_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(format!(
                    "timed out waiting for {SENDS} keep-alive frame(s) after disabling"
                )
                .into());
            }
        }

        drop(named_pipe_client);
        future.await?;
        return Ok(());
    }

    /// Verifies that when the broadcast receiver falls behind the
    /// channel's bounded buffer, the pipe server routine handles the
    /// resulting [`tokio::sync::broadcast::error::TryRecvError::Lagged`]
    /// without panicking. This is a regression guard for the previous
    /// behaviour where any `Lagged` error propagated to the catch-all
    /// `Err(err)` arm and crashed the routine.
    ///
    /// The test deliberately disables the client so the routine
    /// throttles its consumption rate, then bursts more records than
    /// the channel capacity through the sender so the first
    /// `try_recv` is guaranteed to observe `Lagged`.
    #[tokio::test]
    async fn test_named_pipe_server_routine_lagged() -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 77777;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(r"\\.\pipe\csshw-test-lagged-{}", std::process::id());
        // Capacity 2 keeps the buffer small so a modest send burst is
        // guaranteed to overflow before the routine consumes anything.
        let (sender, mut receiver) =
            broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(2);
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(&pipe_name)?;
        let named_pipe_client = ClientOptions::new().open(&pipe_name)?;
        let (clients, pipe_server_state) = make_clients_with_pid_and_state(TEST_PID);

        // Disable up front so the routine throttles consumption and
        // cannot drain the broadcast buffer before we overflow it.
        *pipe_server_state.lock().unwrap() = PipeServerState::Disabled;

        // Overflow the bounded broadcast buffer before the routine
        // begins pulling from it so the first `try_recv` observes
        // `Lagged`. 8 sends into a channel of capacity 2 leaves the
        // receiver lagging by 6 records.
        for _ in 0..8 {
            sender.send([4; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        }

        send_pid(&named_pipe_client, TEST_PID).await?;
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });

        // Read at least one keep-alive frame. If the `Lagged` arm
        // panicked (regression), the routine would never emit any
        // frame and the timeout would fire.
        let read_result: Result<Result<(), Box<dyn std::error::Error>>, _> =
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    named_pipe_client.readable().await?;
                    let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
                    match named_pipe_client.try_read(&mut buf) {
                        Ok(0) => {
                            return Err(
                                "named pipe closed before any keep-alive frame arrived".into(),
                            );
                        }
                        Ok(n) => match buf[0] {
                            TAG_KEEP_ALIVE => {
                                assert_eq!(
                                    FRAMED_KEEP_ALIVE_LENGTH, n,
                                    "Keep-alive frame must be exactly one byte"
                                );
                                return Ok(());
                            }
                            TAG_INPUT_RECORD => panic!(
                                "Received input-record frame while disabled - broadcast data leaked through after Lagged"
                            ),
                            other => panic!("Unexpected tag byte 0x{other:02X}"),
                        },
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
            })
            .await;
        match read_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(
                    "timed out waiting for keep-alive frame after Lagged - routine likely panicked"
                        .into(),
                );
            }
        }

        // Closing the client makes the routine's next pipe write fail
        // and exits the loop cleanly. The join handle resolving
        // without an error confirms the `Lagged` path did not panic.
        drop(named_pipe_client);
        future.await?;
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
        clients.push(make_client(1000)); // duplicate - must panic
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
        let hostnames_after_retain: Vec<&str> =
            clients.iter().map(|c| return c.hostname.as_str()).collect();
        assert_eq!(hostnames_after_retain, vec!["host-a", "host-c"]);
    }

    /// Builds a [`Client`] with the given PID and initial [`PipeServerState`].
    fn make_client_with_state(pid: u32, state: PipeServerState) -> Client {
        return Client {
            hostname: format!("host-{pid}"),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: pid,
            pipe_server_state: Arc::new(Mutex::new(state)),
        };
    }

    /// Verifies that [`toggle_pipe_server_states`] flips each client's
    /// [`PipeServerState`] independently and is its own inverse over two
    /// invocations.
    #[test]
    fn test_toggle_pipe_server_states_flips_each_client_independently() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, PipeServerState::Enabled));
        clients.push(make_client_with_state(2, PipeServerState::Disabled));
        clients.push(make_client_with_state(3, PipeServerState::Enabled));
        clients.push(make_client_with_state(4, PipeServerState::Disabled));

        let snapshot = |c: &Clients| -> Vec<PipeServerState> {
            return c
                .iter()
                .map(|client| return *client.pipe_server_state.lock().unwrap())
                .collect();
        };

        let initial = snapshot(&clients);
        assert_eq!(
            initial,
            vec![
                PipeServerState::Enabled,
                PipeServerState::Disabled,
                PipeServerState::Enabled,
                PipeServerState::Disabled,
            ]
        );

        // First press of `t`: every client flips.
        toggle_pipe_server_states(&clients);
        assert_eq!(
            snapshot(&clients),
            vec![
                PipeServerState::Disabled,
                PipeServerState::Enabled,
                PipeServerState::Disabled,
                PipeServerState::Enabled,
            ]
        );

        // Second press of `t`: every client flips back to its initial state.
        toggle_pipe_server_states(&clients);
        assert_eq!(snapshot(&clients), initial);
    }
}
