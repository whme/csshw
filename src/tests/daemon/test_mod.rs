mod daemon_test {
    use std::{
        ffi::c_void,
        io,
        sync::{Arc, Mutex},
    };

    use tokio::{
        net::windows::named_pipe::{ClientOptions, NamedPipeClient, PipeMode, ServerOptions},
        sync::{broadcast, watch},
    };
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::Console::{
        CAPSLOCK_ON, ENHANCED_KEY, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, LEFT_ALT_PRESSED,
        LEFT_CTRL_PRESSED, NUMLOCK_ON, RIGHT_ALT_PRESSED, RIGHT_CTRL_PRESSED, SCROLLLOCK_ON,
        SHIFT_PRESSED,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VIRTUAL_KEY, VK_C, VK_D, VK_DOWN, VK_E, VK_H, VK_J, VK_K, VK_L, VK_LEFT, VK_N, VK_R,
        VK_RIGHT, VK_T, VK_UP, VK_X,
    };

    use crate::{
        daemon::{
            classify_control_mode_key, classify_enable_disable_submenu_key, expand_hosts,
            named_pipe_server_routine, resolve_cluster_tags, Client, Clients, ControlModeAction,
            ControlModeState, Daemon, EnableDisableSubmenuAction, HWNDWrapper, NavigationDirection,
        },
        protocol::{
            serialization::serialize_pid, ClientState, FRAMED_HIGHLIGHT_LENGTH,
            FRAMED_INPUT_RECORD_LENGTH, FRAMED_KEEP_ALIVE_LENGTH, FRAMED_STATE_CHANGE_LENGTH,
            SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH, TAG_HIGHLIGHT,
            TAG_INPUT_RECORD, TAG_KEEP_ALIVE, TAG_STATE_CHANGE,
        },
        utils::{
            config::{Cluster, DaemonConfig},
            constants::PIPE_NAME,
        },
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
            state_tx: watch::channel(ClientState::Active).0,
            highlight_tx: watch::channel(false).0,
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

    #[test]
    fn test_expand_hosts_brace_only() {
        let hosts: Vec<&str> = vec!["host{1..3}.local"];
        let clusters: Vec<Cluster> = vec![];
        assert_eq!(
            expand_hosts(hosts, &clusters),
            vec!["host1.local", "host2.local", "host3.local"]
        );
    }

    #[test]
    fn test_expand_hosts_cluster_with_brace_member() {
        let hosts: Vec<&str> = vec!["clusterA"];
        let clusters: Vec<Cluster> = vec![Cluster {
            name: "clusterA".to_string(),
            hosts: vec!["box{1..2}.local".to_string()],
        }];
        assert_eq!(
            expand_hosts(hosts, &clusters),
            vec!["box1.local", "box2.local"]
        );
    }

    #[test]
    fn test_expand_hosts_mixed_cluster_tag_and_brace() {
        let hosts: Vec<&str> = vec!["clusterA", "edge{1..2}.local"];
        let clusters: Vec<Cluster> = vec![Cluster {
            name: "clusterA".to_string(),
            hosts: vec!["a".to_string(), "b".to_string()],
        }];
        assert_eq!(
            expand_hosts(hosts, &clusters),
            vec!["a", "b", "edge1.local", "edge2.local"]
        );
    }

    #[test]
    fn test_expand_hosts_plain_hostnames_unchanged() {
        let hosts: Vec<&str> = vec!["a.local", "b.local"];
        let clusters: Vec<Cluster> = vec![];
        assert_eq!(expand_hosts(hosts, &clusters), vec!["a.local", "b.local"]);
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

        // Push 5 input records up front; once the routine drains them the
        // broadcast channel goes idle and the 5 ms keep-alive branch of the
        // select! starts firing, letting us assert both behaviours in one
        // loop.
        const TARGET_INPUT_FRAMES: usize = 5;
        for _ in 0..TARGET_INPUT_FRAMES {
            sender.send([2; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        }
        let mut keep_alive_received = false;
        let mut successful_iterations = 0;
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
                        keep_alive_received = true;
                    }
                    TAG_INPUT_RECORD => {
                        // Input-record frame: tag byte + 13-byte payload.
                        assert_eq!(FRAMED_INPUT_RECORD_LENGTH, n);
                        assert_eq!([2; SERIALIZED_INPUT_RECORD_0_LENGTH], buf[1..]);
                        successful_iterations += 1;
                    }
                    TAG_STATE_CHANGE => {
                        // Initial state push emitted right after the routine
                        // subscribes; the default state is `Active`. Drain it
                        // and keep reading so the rest of the assertions still
                        // observe the input-record and keep-alive frames.
                        assert_eq!(FRAMED_STATE_CHANGE_LENGTH, n);
                        assert_eq!(buf[1], ClientState::Active as u8);
                    }
                    TAG_HIGHLIGHT => {
                        // Initial highlight push emitted right after the
                        // state push; the default highlight is `false`.
                        assert_eq!(FRAMED_HIGHLIGHT_LENGTH, n);
                        assert_eq!(buf[1], 0u8);
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
            if keep_alive_received && successful_iterations >= TARGET_INPUT_FRAMES {
                break;
            }
        }
        assert!(keep_alive_received);
        assert!(successful_iterations >= TARGET_INPUT_FRAMES);
        // Drop the client, closing the pipe.
        drop(named_pipe_client);
        // We expect the routine to exit gracefully.
        future.await?;
        return Ok(());
    }

    #[tokio::test]
    async fn test_named_pipe_server_routine_forwards_state_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 66666;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(r"\\.\pipe\csshw-test-state-change-{}", std::process::id());
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
        // Grab the watch sender so we can later trigger a state change push.
        let state_tx = clients
            .lock()
            .unwrap()
            .get_by_pid(TEST_PID)
            .unwrap()
            .state_tx
            .clone();
        send_pid(&named_pipe_client, TEST_PID).await?;
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });
        // The routine emits the current authoritative state right after
        // subscribing, so the very first frame on the pipe must be the
        // initial `TAG_STATE_CHANGE(Active)` push. Receiving it also
        // proves the subscribe has happened and any subsequent
        // `state_tx.send` will be observed.
        loop {
            named_pipe_client.readable().await?;
            let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
            match named_pipe_client.try_read(&mut buf) {
                Ok(0) => return Err("pipe closed before initial state push".into()),
                Ok(n) => match buf[0] {
                    TAG_STATE_CHANGE => {
                        assert_eq!(FRAMED_STATE_CHANGE_LENGTH, n);
                        assert_eq!(buf[1], ClientState::Active as u8);
                        break;
                    }
                    other => {
                        panic!("Expected initial TAG_STATE_CHANGE, got tag byte 0x{other:02X}")
                    }
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
        }
        // Push a real state transition through the watch sender; the routine
        // must write a tagged state-change frame to the pipe.
        state_tx.send(ClientState::Disabled)?;
        let mut state_change_seen = false;
        loop {
            named_pipe_client.readable().await?;
            let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
            match named_pipe_client.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => match buf[0] {
                    TAG_STATE_CHANGE => {
                        assert_eq!(FRAMED_STATE_CHANGE_LENGTH, n);
                        assert_eq!(buf[1], ClientState::Disabled as u8);
                        state_change_seen = true;
                        break;
                    }
                    TAG_KEEP_ALIVE => {
                        assert_eq!(FRAMED_KEEP_ALIVE_LENGTH, n);
                        // Keep-alives may interleave with the state change; keep waiting.
                    }
                    TAG_HIGHLIGHT => {
                        // Initial highlight push following the initial state
                        // push; drain it and keep waiting for the state
                        // transition.
                        assert_eq!(FRAMED_HIGHLIGHT_LENGTH, n);
                        assert_eq!(buf[1], 0u8);
                    }
                    other => panic!("Unexpected tag byte 0x{other:02X}"),
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
        }
        assert!(state_change_seen);
        drop(named_pipe_client);
        future.await?;
        return Ok(());
    }

    /// Verifies that the pipe server routine emits a `TAG_STATE_CHANGE`
    /// frame carrying the current authoritative state immediately after
    /// the PID handshake, even when no transition fires after subscribe.
    ///
    /// `Daemon::set_client_state` may run in the brief window between
    /// `Client` construction and the routine's `state_tx.subscribe()`
    /// call. In that case `state_rx.changed()` would never fire for the
    /// pre-existing value, leaving the client stuck on its default
    /// `ClientState::Active` even though the daemon already gates
    /// forwarding on the new value. The fix - and what this test
    /// asserts - is that the routine pushes the snapshot from
    /// `state_rx.borrow_and_update()` as its very first frame.
    #[tokio::test]
    async fn test_named_pipe_server_routine_sends_initial_state_after_subscribe(
    ) -> Result<(), Box<dyn std::error::Error>> {
        const TEST_PID: u32 = 88888;
        // Use a per-test unique pipe name so parallel test runs don't collide
        // on the global PIPE_NAME.
        let pipe_name = format!(r"\\.\pipe\csshw-test-initial-state-{}", std::process::id());
        let (_sender, mut receiver) = broadcast::channel::<[u8; SERIALIZED_INPUT_RECORD_0_LENGTH]>(
            SERIALIZED_INPUT_RECORD_0_LENGTH,
        );
        let named_pipe_server = ServerOptions::new()
            .access_inbound(true)
            .access_outbound(true)
            .pipe_mode(PipeMode::Message)
            .create(&pipe_name)?;
        let named_pipe_client = ClientOptions::new().open(&pipe_name)?;
        let (clients, state_tx) = make_clients_with_pid_and_state(TEST_PID);

        // Pre-disable BEFORE the routine subscribes. `send_replace`
        // updates the stored value even when there are no receivers,
        // which is exactly the production race this test guards against:
        // `Daemon::set_client_state` mutating the watch before the
        // pipe-server task has had a chance to call `subscribe`. A
        // regular `send` would error with no receivers.
        state_tx.send_replace(ClientState::Disabled);

        send_pid(&named_pipe_client, TEST_PID).await?;
        let future = tokio::spawn(async move {
            named_pipe_server_routine(named_pipe_server, &mut receiver, clients).await;
        });

        // The very first non-keep-alive frame on the pipe must be the
        // initial `TAG_STATE_CHANGE(Disabled)` push. Keep-alive frames
        // may legally interleave because the routine spawns its keep-
        // alive timer through the same select; tolerate them but reject
        // any other tag.
        let read_result: Result<Result<(), Box<dyn std::error::Error>>, _> =
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    named_pipe_client.readable().await?;
                    let mut buf = [0u8; FRAMED_INPUT_RECORD_LENGTH];
                    match named_pipe_client.try_read(&mut buf) {
                        Ok(0) => {
                            return Err("pipe closed before initial state frame arrived".into());
                        }
                        Ok(n) => match buf[0] {
                            TAG_STATE_CHANGE => {
                                assert_eq!(
                                    FRAMED_STATE_CHANGE_LENGTH, n,
                                    "State-change frame must be exactly two bytes"
                                );
                                assert_eq!(
                                    buf[1],
                                    ClientState::Disabled as u8,
                                    "Initial state push must reflect the value set before subscribe"
                                );
                                return Ok(());
                            }
                            TAG_KEEP_ALIVE => {
                                // The initial state push happens before the
                                // select loop, so a keep-alive arriving first
                                // would mean the routine skipped the push.
                                return Err("received keep-alive before initial state frame".into());
                            }
                            TAG_INPUT_RECORD => {
                                return Err(
                                    "received input record before initial state frame".into()
                                );
                            }
                            other => {
                                return Err(format!("Unexpected tag byte 0x{other:02X}").into());
                            }
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
                return Err("timed out waiting for initial state frame after subscribe".into());
            }
        }

        drop(named_pipe_client);
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
                    TAG_STATE_CHANGE => {
                        // Initial state push emitted right after subscribe.
                        // Drain it and continue reading.
                        assert_eq!(FRAMED_STATE_CHANGE_LENGTH, n);
                        assert_eq!(buf[1], ClientState::Active as u8);
                    }
                    TAG_HIGHLIGHT => {
                        // Initial highlight push following the initial
                        // state push. Drain and keep reading.
                        assert_eq!(FRAMED_HIGHLIGHT_LENGTH, n);
                        assert_eq!(buf[1], 0u8);
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
    /// shared [`watch::Sender`] handle so the caller can drive [`ClientState`]
    /// transitions.
    fn make_clients_with_pid_and_state(
        pid: u32,
    ) -> (Arc<Mutex<Clients>>, watch::Sender<ClientState>) {
        let state_tx = watch::channel(ClientState::Active).0;
        let mut clients = Clients::new();
        clients.push(Client {
            hostname: format!("test-host-{pid}"),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: pid,
            state_tx: state_tx.clone(),
            highlight_tx: watch::channel(false).0,
        });
        return (Arc::new(Mutex::new(clients)), state_tx);
    }

    /// Verifies that when a client's [`ClientState`] is set to
    /// [`ClientState::Disabled`], the pipe server routine consumes
    /// broadcast messages but does not forward them through the pipe.
    /// Only keep-alive packets (and the [`TAG_STATE_CHANGE`] frame
    /// announcing the transition itself) should arrive on the client
    /// side.
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
        let (clients, state_tx) = make_clients_with_pid_and_state(TEST_PID);
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
                    TAG_STATE_CHANGE => {
                        // Initial state push emitted right after subscribe.
                        // The default state is `Active`. Drain it and keep
                        // reading so the assertion still observes the
                        // input-record frame.
                        assert_eq!(FRAMED_STATE_CHANGE_LENGTH, n);
                        assert_eq!(buf[1], ClientState::Active as u8);
                    }
                    TAG_HIGHLIGHT => {
                        // Initial highlight push following the initial
                        // state push. Drain and keep reading.
                        assert_eq!(FRAMED_HIGHLIGHT_LENGTH, n);
                        assert_eq!(buf[1], 0u8);
                    }
                    other => panic!("Unexpected tag byte 0x{other:02X}"),
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
        }
        assert!(got_data);

        // Disable the client. The routine emits a `TAG_STATE_CHANGE`
        // frame as soon as it observes this transition.
        state_tx.send(ClientState::Disabled).unwrap();

        // Send more data - it must NOT arrive at the client.
        const SENDS: usize = 5;
        for _ in 0..SENDS {
            sender.send([3; SERIALIZED_INPUT_RECORD_0_LENGTH])?;
        }

        // While disabled, the only frames that may arrive are the
        // single `TAG_STATE_CHANGE(Disabled)` announcement and any
        // number of `TAG_KEEP_ALIVE` frames. We require at least
        // `SENDS` keep-alive frames to confirm the routine keeps
        // running while suppressed; any `TAG_INPUT_RECORD` would be
        // a leak of broadcast data and must fail the test.
        //
        // The whole loop is bounded by a `tokio::time::timeout` so a
        // regression that stops keep-alive emission surfaces as a
        // deterministic assertion instead of a hung test.
        let read_result: Result<Result<(), Box<dyn std::error::Error>>, _> =
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut received_keep_alive = 0;
                let mut saw_state_change = false;
                while received_keep_alive < SENDS || !saw_state_change {
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
                                received_keep_alive += 1;
                            }
                            TAG_STATE_CHANGE => {
                                assert!(!saw_state_change, "Disabled transition was announced more than once");
                                assert_eq!(
                                    FRAMED_STATE_CHANGE_LENGTH, n,
                                    "State-change frame must be exactly two bytes"
                                );
                                assert_eq!(
                                    buf[1],
                                    ClientState::Disabled as u8,
                                    "State-change announcement must carry Disabled"
                                );
                                saw_state_change = true;
                            }
                            TAG_HIGHLIGHT => {
                                // Steady-state highlight is `false` while the
                                // submenu is not driving navigation, so a
                                // highlight frame may arrive once at startup.
                                assert_eq!(FRAMED_HIGHLIGHT_LENGTH, n);
                                assert_eq!(buf[1], 0u8);
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
                assert!(saw_state_change, "Disabled transition must be announced");
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
    /// resulting [`tokio::sync::broadcast::error::RecvError::Lagged`]
    /// without panicking. This is a regression guard for the previous
    /// behaviour where any `Lagged` error propagated to the catch-all
    /// `Err(err)` arm and crashed the routine.
    ///
    /// The test deliberately disables the client so the routine
    /// throttles its consumption rate, then bursts more records than
    /// the channel capacity through the sender so the next `recv`
    /// is guaranteed to observe `Lagged`.
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
        let (clients, state_tx) = make_clients_with_pid_and_state(TEST_PID);

        // Disable up front so the routine throttles consumption and
        // cannot drain the broadcast buffer before we overflow it. Use
        // `send_replace` because the routine has not yet subscribed to
        // the watch channel, so a regular `send` would error with no
        // receivers; `send_replace` updates the stored value either
        // way and the routine reads it through `borrow_and_update` on
        // its first iteration.
        state_tx.send_replace(ClientState::Disabled);

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
                            TAG_STATE_CHANGE => {
                                // The initial Disabled transition may surface
                                // here; drain it and continue reading so the
                                // test still observes a keep-alive frame.
                                assert_eq!(
                                    FRAMED_STATE_CHANGE_LENGTH, n,
                                    "State-change frame must be exactly two bytes"
                                );
                                continue;
                            }
                            TAG_HIGHLIGHT => {
                                // Initial highlight push following the state
                                // push; drain it and keep waiting for the
                                // first keep-alive frame.
                                assert_eq!(
                                    FRAMED_HIGHLIGHT_LENGTH, n,
                                    "Highlight frame must be exactly two bytes"
                                );
                                continue;
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
                state_tx: watch::channel(ClientState::Active).0,
                highlight_tx: watch::channel(false).0,
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
            state_tx: watch::channel(ClientState::Active).0,
            highlight_tx: watch::channel(false).0,
        };
        let client_b = Client {
            hostname: "host-b".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 2000,
            state_tx: watch::channel(ClientState::Active).0,
            highlight_tx: watch::channel(false).0,
        };
        let client_c = Client {
            hostname: "host-c".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 3000,
            state_tx: watch::channel(ClientState::Active).0,
            highlight_tx: watch::channel(false).0,
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

    /// Builds a [`Client`] with the given PID and initial [`ClientState`].
    fn make_client_with_state(pid: u32, state: ClientState) -> Client {
        return Client {
            hostname: format!("host-{pid}"),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: pid,
            state_tx: watch::channel(state).0,
            highlight_tx: watch::channel(false).0,
        };
    }

    /// Verifies that the `[t]oggle enabled` control-mode handler flips each
    /// client's [`ClientState`] independently and is its own inverse over
    /// two invocations.
    ///
    /// Mirrors the snapshot-then-flip logic in
    /// [`crate::daemon::Daemon::handle_control_mode`]'s `VK_T` arm so the
    /// per-client toggle behaviour is exercised without standing up a full
    /// [`crate::daemon::Daemon`].
    #[test]
    fn test_toggle_flips_each_client_independently() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        clients.push(make_client_with_state(3, ClientState::Active));
        clients.push(make_client_with_state(4, ClientState::Disabled));

        let snapshot = |c: &Clients| -> Vec<ClientState> {
            return c
                .iter()
                .map(|client| return *client.state_tx.borrow())
                .collect();
        };

        let initial = snapshot(&clients);
        assert_eq!(
            initial,
            vec![
                ClientState::Active,
                ClientState::Disabled,
                ClientState::Active,
                ClientState::Disabled,
            ]
        );

        // Press `t` once: snapshot every state, then flip each.
        let toggle = |c: &Clients| {
            let flips: Vec<ClientState> = c
                .iter()
                .map(|client| {
                    return match *client.state_tx.borrow() {
                        ClientState::Active => ClientState::Disabled,
                        ClientState::Disabled => ClientState::Active,
                    };
                })
                .collect();
            // `send_replace` succeeds even when no task has subscribed;
            // tests don't spin up the pipe-server routine that would
            // normally hold the receiver.
            for (client, flipped) in c.iter().zip(flips) {
                client.state_tx.send_replace(flipped);
            }
        };

        toggle(&clients);
        assert_eq!(
            snapshot(&clients),
            vec![
                ClientState::Disabled,
                ClientState::Active,
                ClientState::Disabled,
                ClientState::Active,
            ]
        );

        // Press `t` again: every client flips back to its initial state.
        toggle(&clients);
        assert_eq!(snapshot(&clients), initial);
    }

    /// Collects every client's [`ClientState`] in insertion order.
    fn snapshot_states(clients: &Clients) -> Vec<ClientState> {
        return clients
            .iter()
            .map(|client| return *client.state_tx.borrow())
            .collect();
    }

    /// Builds a [`KEY_EVENT_RECORD`] for a key-down press with no
    /// active modifier bits, mirroring the matcher used by the
    /// submenu's `[e]`/`[d]`/`[t]` arms.
    fn submenu_key_event(virtual_key: VIRTUAL_KEY) -> KEY_EVENT_RECORD {
        return submenu_key_event_with_state(virtual_key, 0);
    }

    /// Same as [`submenu_key_event`] but with a caller-supplied
    /// `dwControlKeyState`. Used by the GH #196 regression to drive
    /// the submenu with lock-state bits engaged.
    fn submenu_key_event_with_state(
        virtual_key: VIRTUAL_KEY,
        control_key_state: u32,
    ) -> KEY_EVENT_RECORD {
        return KEY_EVENT_RECORD {
            bKeyDown: true.into(),
            wRepeatCount: 1,
            wVirtualKeyCode: virtual_key.0,
            wVirtualScanCode: 0,
            uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0 },
            dwControlKeyState: control_key_state,
        };
    }

    /// Builds a fresh [`MockWindowsApi`] with no expectations set.
    ///
    /// Suitable for submenu dispatch tests that exercise only the
    /// `[e]`/`[d]`/`[t]` arms (which never touch the Windows API).
    /// Tests that drive the `Navigate` arm must additionally stub the
    /// console calls performed by [`crate::utils::windows::clear_screen`]
    /// (see `mock_with_clear_screen`).
    fn mock_no_calls() -> crate::utils::windows::MockWindowsApi {
        return crate::utils::windows::MockWindowsApi::new();
    }

    /// Builds a [`MockWindowsApi`] that satisfies the console calls
    /// [`crate::utils::windows::clear_screen`] performs when the
    /// submenu renderer redraws after a navigation keystroke.
    ///
    /// Mirrors the mock setup used by
    /// `test_esc_in_active_state_is_consumed_and_resets_to_inactive`.
    fn mock_with_clear_screen() -> crate::utils::windows::MockWindowsApi {
        use windows::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, COORD};
        let mut mock = crate::utils::windows::MockWindowsApi::new();
        mock.expect_get_console_screen_buffer_info().returning(|| {
            return Ok(CONSOLE_SCREEN_BUFFER_INFO {
                dwSize: COORD { X: 80, Y: 25 },
                ..Default::default()
            });
        });
        mock.expect_scroll_console_screen_buffer()
            .returning(|_, _, _| return Ok(()));
        mock.expect_set_console_cursor_position()
            .returning(|_| return Ok(()));
        return mock;
    }

    /// Verifies that `VK_E` in the enable/disable submenu enables
    /// only the currently selected client (index 0 here) and keeps
    /// the submenu open so the user can chain further
    /// enable/disable/toggle actions across clients without
    /// re-entering the submenu.
    #[test]
    fn test_submenu_e_enables_only_selected_client_and_stays_open() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Disabled));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        clients.push(make_client_with_state(3, ClientState::Disabled));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_E),
        );

        assert_eq!(
            snapshot_states(&clients.lock().unwrap()),
            vec![
                ClientState::Active,
                ClientState::Disabled,
                ClientState::Disabled,
            ]
        );
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Verifies that `VK_D` in the enable/disable submenu disables
    /// only the currently selected client (index 0 here) and keeps
    /// the submenu open so the user can chain further
    /// enable/disable/toggle actions across clients without
    /// re-entering the submenu.
    #[test]
    fn test_submenu_d_disables_only_selected_client_and_stays_open() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.push(make_client_with_state(3, ClientState::Active));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_D),
        );

        assert_eq!(
            snapshot_states(&clients.lock().unwrap()),
            vec![
                ClientState::Disabled,
                ClientState::Active,
                ClientState::Active,
            ]
        );
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Verifies that `VK_T` in the enable/disable submenu flips only
    /// the currently selected client's state, keeps the submenu open
    /// after the flip, and is its own inverse over two consecutive
    /// presses without needing to re-enter the submenu in between.
    #[test]
    fn test_submenu_t_toggles_only_selected_client_and_stays_open() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        clients.push(make_client_with_state(3, ClientState::Active));
        let clients = Mutex::new(clients);

        let initial = snapshot_states(&clients.lock().unwrap());

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_T),
        );
        assert_eq!(
            snapshot_states(&clients.lock().unwrap()),
            vec![
                ClientState::Disabled,
                ClientState::Disabled,
                ClientState::Active,
            ]
        );
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );

        // Second press without re-entering the submenu: the submenu
        // must still be open from the first press, and the toggle is
        // its own inverse.
        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_T),
        );
        assert_eq!(snapshot_states(&clients.lock().unwrap()), initial);
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Verifies that an unrecognised key in the enable/disable
    /// submenu leaves every client state unchanged and keeps the
    /// submenu open for the next press.
    #[test]
    fn test_submenu_ignores_unmapped_key() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        let clients = Mutex::new(clients);

        let initial = snapshot_states(&clients.lock().unwrap());

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_X),
        );

        assert_eq!(snapshot_states(&clients.lock().unwrap()), initial);
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Verifies that pressing `VK_E` with no clients tracked is a
    /// no-op for the client list (and does not panic) while leaving
    /// the submenu open for the next press.
    #[test]
    fn test_submenu_no_panic_with_empty_clients() {
        let clients = Mutex::new(Clients::new());

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        // Submenu entry on an empty cluster leaves the selection at
        // `None`; the dispatch must not panic.
        daemon.submenu_selected_index = None;

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_E),
        );

        assert!(clients.lock().unwrap().iter().next().is_none());
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Regression for GH #196: control-mode dispatch must ignore
    /// lock toggles (`CAPSLOCK_ON`, `NUMLOCK_ON`, `SCROLLLOCK_ON`)
    /// and the `ENHANCED_KEY` flag when matching `(VK_*, 0)` arms.
    /// Those bits live in `dwControlKeyState` alongside the real
    /// modifier bits (Ctrl/Alt/Shift), and an enabled CapsLock
    /// previously made the entire field non-zero, silently skipping
    /// every action.
    ///
    /// Conversely, any real modifier bit must survive the masking
    /// so combos like Shift+R do not collapse into the plain-R arm.
    #[test]
    fn test_control_mode_classifiers_ignore_lock_state_and_enhanced_key() {
        let benign_states = [
            0,
            CAPSLOCK_ON,
            NUMLOCK_ON,
            SCROLLLOCK_ON,
            ENHANCED_KEY,
            CAPSLOCK_ON | NUMLOCK_ON | SCROLLLOCK_ON | ENHANCED_KEY,
        ];
        let main_expected = [
            (VK_R, ControlModeAction::Retile),
            (VK_E, ControlModeAction::OpenEnableDisableSubmenu),
            (VK_T, ControlModeAction::ToggleEnabled),
            (VK_N, ControlModeAction::EnableAll),
            (VK_C, ControlModeAction::CreateWindows),
            (VK_H, ControlModeAction::CopyHostnames),
        ];
        let submenu_expected = [
            (VK_E, EnableDisableSubmenuAction::Enable),
            (VK_D, EnableDisableSubmenuAction::Disable),
            (VK_T, EnableDisableSubmenuAction::Toggle),
            (
                VK_UP,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Up),
            ),
            (
                VK_K,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Up),
            ),
            (
                VK_DOWN,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Down),
            ),
            (
                VK_J,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Down),
            ),
            (
                VK_LEFT,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Left),
            ),
            (
                VK_H,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Left),
            ),
            (
                VK_RIGHT,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Right),
            ),
            (
                VK_L,
                EnableDisableSubmenuAction::Navigate(NavigationDirection::Right),
            ),
        ];

        for state in benign_states {
            for (vk, action) in &main_expected {
                assert_eq!(
                    &classify_control_mode_key(*vk, state),
                    action,
                    "main menu: VK {vk:?} with state 0x{state:08X} must classify as {action:?}",
                );
            }
            for (vk, action) in &submenu_expected {
                assert_eq!(
                    &classify_enable_disable_submenu_key(*vk, state),
                    action,
                    "submenu: VK {vk:?} with state 0x{state:08X} must classify as {action:?}",
                );
            }
        }

        let modifier_states = [
            LEFT_CTRL_PRESSED,
            RIGHT_CTRL_PRESSED,
            LEFT_ALT_PRESSED,
            RIGHT_ALT_PRESSED,
            SHIFT_PRESSED,
            LEFT_CTRL_PRESSED | CAPSLOCK_ON,
            SHIFT_PRESSED | NUMLOCK_ON | ENHANCED_KEY,
        ];
        for state in modifier_states {
            for (vk, _) in &main_expected {
                assert_eq!(
                    classify_control_mode_key(*vk, state),
                    ControlModeAction::NoOp,
                    "main menu: VK {vk:?} with modifier state 0x{state:08X} must NOT fire the plain-key arm",
                );
            }
            for (vk, _) in &submenu_expected {
                assert_eq!(
                    classify_enable_disable_submenu_key(*vk, state),
                    EnableDisableSubmenuAction::NoOp,
                    "submenu: VK {vk:?} with modifier state 0x{state:08X} must NOT fire the plain-key arm",
                );
            }
        }
    }

    /// End-to-end regression for GH #196 at the dispatch level: when
    /// CapsLock (or any other lock toggle) is engaged, pressing
    /// `[e]` in the enable/disable submenu must still enable the
    /// first client. Before the [`MODIFIER_MASK`][1] fix, the
    /// non-zero `dwControlKeyState` would skip the `(VK_E, 0)` arm
    /// and the press would silently do nothing.
    ///
    /// [1]: crate::daemon
    #[test]
    fn test_submenu_dispatch_ignores_lock_state_bits() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Disabled));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event_with_state(VK_E, CAPSLOCK_ON | NUMLOCK_ON | ENHANCED_KEY),
        );

        assert_eq!(
            snapshot_states(&clients.lock().unwrap()),
            vec![ClientState::Active, ClientState::Disabled],
            "VK_E with lock-state bits set must still enable the selected client",
        );
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Regression test for #197: when control mode is `Active` and the
    /// user presses `Esc`, `control_mode_is_active` must report that the
    /// keystroke was consumed (`true`) so that `handle_input_record`
    /// suppresses the broadcast. Before the fix it returned `false`,
    /// which leaked the `Esc` to every connected client.
    #[test]
    fn test_esc_in_active_state_is_consumed_and_resets_to_inactive() {
        use crate::utils::windows::MockWindowsApi;
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD_0};
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;

        // Arrange: a daemon already in `Active` control mode and a mock
        // that stubs the console calls `quit_control_mode` makes via
        // `print_instructions` -> `clear_screen`.
        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon = Daemon::for_test(&config, &clusters, ControlModeState::Active);

        let mut mock = MockWindowsApi::new();
        mock.expect_get_console_screen_buffer_info().returning(|| {
            return Ok(CONSOLE_SCREEN_BUFFER_INFO {
                dwSize: COORD { X: 80, Y: 25 },
                ..Default::default()
            });
        });
        mock.expect_scroll_console_screen_buffer()
            .returning(|_, _, _| return Ok(()));
        mock.expect_set_console_cursor_position()
            .returning(|_| return Ok(()));

        let esc_input = INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: BOOL(1),
                wRepeatCount: 1,
                wVirtualKeyCode: VK_ESCAPE.0,
                wVirtualScanCode: 0,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0 },
                dwControlKeyState: 0,
            },
        };

        // Act
        let clients: Arc<Mutex<Clients>> = Arc::new(Mutex::new(Clients::new()));
        let consumed = daemon.control_mode_is_active(&mock, &clients, esc_input);

        // Assert: the `Esc` is reported as owned by control mode (so the
        // caller will skip forwarding it) and the state machine is back
        // to `Inactive`.
        assert!(consumed);
        assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
    }

    /// Verifies that `quit_control_mode` clears
    /// `submenu_selected_index` so the selection cursor never
    /// outlives a control-mode session.
    #[test]
    fn test_quit_control_mode_clears_submenu_selection() {
        use crate::utils::windows::MockWindowsApi;
        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(2);

        let mut mock = MockWindowsApi::new();
        mock.expect_get_console_screen_buffer_info().returning(|| {
            use windows::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, COORD};
            return Ok(CONSOLE_SCREEN_BUFFER_INFO {
                dwSize: COORD { X: 80, Y: 25 },
                ..Default::default()
            });
        });
        mock.expect_scroll_console_screen_buffer()
            .returning(|_, _, _| return Ok(()));
        mock.expect_set_console_cursor_position()
            .returning(|_| return Ok(()));

        daemon.quit_control_mode(&mock);

        assert_eq!(daemon.control_mode_state, ControlModeState::Inactive);
        assert_eq!(daemon.submenu_selected_index, None);
    }

    /// Verifies that `move_submenu_selection` advances the cursor by
    /// one position on `Down`/`Right` and clamps at `len - 1`.
    #[test]
    fn test_move_submenu_selection_down_right_clamp_at_last() {
        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.move_submenu_selection(NavigationDirection::Down, 3);
        assert_eq!(daemon.submenu_selected_index, Some(1));
        daemon.move_submenu_selection(NavigationDirection::Right, 3);
        assert_eq!(daemon.submenu_selected_index, Some(2));
        // Clamp at the last client - no wrap.
        daemon.move_submenu_selection(NavigationDirection::Down, 3);
        assert_eq!(daemon.submenu_selected_index, Some(2));
        daemon.move_submenu_selection(NavigationDirection::Right, 3);
        assert_eq!(daemon.submenu_selected_index, Some(2));
    }

    /// Verifies that `move_submenu_selection` steps back by one on
    /// `Up`/`Left` and clamps at `0` rather than wrapping.
    #[test]
    fn test_move_submenu_selection_up_left_clamp_at_zero() {
        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(2);

        daemon.move_submenu_selection(NavigationDirection::Up, 3);
        assert_eq!(daemon.submenu_selected_index, Some(1));
        daemon.move_submenu_selection(NavigationDirection::Left, 3);
        assert_eq!(daemon.submenu_selected_index, Some(0));
        // Clamp at zero - no wrap.
        daemon.move_submenu_selection(NavigationDirection::Up, 3);
        assert_eq!(daemon.submenu_selected_index, Some(0));
        daemon.move_submenu_selection(NavigationDirection::Left, 3);
        assert_eq!(daemon.submenu_selected_index, Some(0));
    }

    /// Verifies that `move_submenu_selection` is a no-op when the
    /// cluster is empty: the selection is cleared and stays `None`
    /// across every direction.
    #[test]
    fn test_move_submenu_selection_empty_clients_stays_none() {
        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = None;

        for direction in [
            NavigationDirection::Up,
            NavigationDirection::Down,
            NavigationDirection::Left,
            NavigationDirection::Right,
        ] {
            daemon.move_submenu_selection(direction, 0);
            assert_eq!(daemon.submenu_selected_index, None);
        }
    }

    /// Verifies that the dispatch arm for `Navigate(Down)` calls
    /// through `move_submenu_selection` and triggers a re-render
    /// (which performs the console calls stubbed on the mock).
    #[test]
    fn test_submenu_navigate_down_advances_selection_via_dispatch() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.push(make_client_with_state(3, ClientState::Active));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_with_clear_screen(),
            &clients,
            submenu_key_event(VK_DOWN),
        );

        assert_eq!(daemon.submenu_selected_index, Some(1));
        assert_eq!(
            daemon.control_mode_state,
            ControlModeState::EnableDisableSubmenu
        );
    }

    /// Verifies that `VK_E` targets the *selected* client rather
    /// than always the first one - the regression the navigation
    /// feature is designed to enable.
    #[test]
    fn test_submenu_e_targets_non_zero_selected_index() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Disabled));
        clients.push(make_client_with_state(2, ClientState::Disabled));
        clients.push(make_client_with_state(3, ClientState::Disabled));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(1);

        daemon.handle_enable_disable_submenu_key(
            &mock_no_calls(),
            &clients,
            submenu_key_event(VK_E),
        );

        assert_eq!(
            snapshot_states(&clients.lock().unwrap()),
            vec![
                ClientState::Disabled,
                ClientState::Active,
                ClientState::Disabled,
            ],
            "VK_E with selection at index 1 must enable only client 1",
        );
    }

    /// Collects every client's `highlight_tx` value in insertion order.
    fn snapshot_highlights(clients: &Clients) -> Vec<bool> {
        return clients
            .iter()
            .map(|client| return *client.highlight_tx.borrow())
            .collect();
    }

    /// Verifies that opening the enable/disable submenu pushes
    /// `highlight_tx = true` on the first client and leaves the
    /// others cleared - the visual signal that drives the new
    /// per-client highlight color.
    #[test]
    fn test_open_enable_disable_submenu_highlights_first_client() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.push(make_client_with_state(3, ClientState::Active));
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let daemon = Daemon::for_test(&config, &clusters, ControlModeState::Active);

        let clients_guard = clients.lock().unwrap();
        daemon.apply_submenu_highlight(&clients_guard, None, Some(0));

        assert_eq!(
            snapshot_highlights(&clients_guard),
            vec![true, false, false],
            "opening the submenu must highlight only the first client",
        );
    }

    /// Verifies that the `Navigate(Down)` dispatch arm moves the
    /// per-client highlight from the previously selected index to
    /// the new one.
    #[test]
    fn test_submenu_navigate_moves_highlight() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.push(make_client_with_state(3, ClientState::Active));
        // Start with client 0 highlighted, matching the state just
        // after `OpenEnableDisableSubmenu`.
        clients.first().unwrap().highlight_tx.send_replace(true);
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let mut daemon =
            Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);
        daemon.submenu_selected_index = Some(0);

        daemon.handle_enable_disable_submenu_key(
            &mock_with_clear_screen(),
            &clients,
            submenu_key_event(VK_DOWN),
        );

        assert_eq!(daemon.submenu_selected_index, Some(1));
        assert_eq!(
            snapshot_highlights(&clients.lock().unwrap()),
            vec![false, true, false],
            "Navigate(Down) must clear the old highlight and set the new one",
        );
    }

    /// Verifies that `apply_submenu_highlight(.., Some(idx), None)` -
    /// the path the `Esc` arm in `control_mode_is_active` takes when
    /// leaving the submenu - clears the highlight on every client.
    #[test]
    fn test_submenu_esc_clears_highlight() {
        let mut clients = Clients::new();
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.push(make_client_with_state(3, ClientState::Active));
        clients.get(1).unwrap().highlight_tx.send_replace(true);
        let clients = Mutex::new(clients);

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let daemon = Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);

        let clients_guard = clients.lock().unwrap();
        daemon.apply_submenu_highlight(&clients_guard, Some(1), None);

        assert_eq!(
            snapshot_highlights(&clients_guard),
            vec![false, false, false],
            "Esc must clear the highlight on the previously-selected client",
        );
    }

    /// Regression test: when an exited client is retained-out
    /// mid-submenu, the same numeric index now points at a
    /// different client. The new occupant of the selected index
    /// must still receive `highlight_tx = true` even though the
    /// index value did not change.
    #[test]
    fn test_apply_submenu_highlight_handles_index_reuse_after_retain() {
        let mut clients = Clients::new();
        // Two clients, the first is highlighted (state right after
        // `OpenEnableDisableSubmenu`).
        clients.push(make_client_with_state(1, ClientState::Active));
        clients.push(make_client_with_state(2, ClientState::Active));
        clients.first().unwrap().highlight_tx.send_replace(true);

        // The background monitor would call `retain` to remove the
        // exited client; do the same here.
        clients.retain(|client| return client.process_id != 1);
        assert_eq!(clients.len(), 1);
        // The surviving client (PID 2) was never highlighted: its
        // `highlight_tx` is still `false`.
        assert!(!*clients.first().unwrap().highlight_tx.borrow());

        let config = DaemonConfig::default();
        let clusters: Vec<Cluster> = Vec::new();
        let daemon = Daemon::for_test(&config, &clusters, ControlModeState::EnableDisableSubmenu);

        // `move_submenu_selection(Down, 1)` clamps back to `Some(0)`,
        // so `previous == next == Some(0)`. The old code's early
        // return would skip the `send_replace`; the fix must
        // re-assert `true` on the new occupant of index 0.
        daemon.apply_submenu_highlight(&clients, Some(0), Some(0));

        assert_eq!(
            snapshot_highlights(&clients),
            vec![true],
            "after retain reuses an index the surviving client must be highlighted",
        );
    }
}
