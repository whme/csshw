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

    use crate::{
        daemon::{named_pipe_server_routine, resolve_cluster_tags, Client, Clients, HWNDWrapper},
        protocol::{
            serialization::serialize_pid, ClientState, FRAMED_INPUT_RECORD_LENGTH,
            FRAMED_KEEP_ALIVE_LENGTH, FRAMED_STATE_CHANGE_LENGTH, SERIALIZED_INPUT_RECORD_0_LENGTH,
            SERIALIZED_PID_LENGTH, TAG_INPUT_RECORD, TAG_KEEP_ALIVE, TAG_STATE_CHANGE,
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
            state_tx: watch::channel(ClientState::Active).0,
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
                while received_keep_alive < SENDS {
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
        };
        let client_b = Client {
            hostname: "host-b".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 2000,
            state_tx: watch::channel(ClientState::Active).0,
        };
        let client_c = Client {
            hostname: "host-c".to_owned(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE::default(),
            process_id: 3000,
            state_tx: watch::channel(ClientState::Active).0,
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
}
