mod daemon_test {
    use std::{ffi::c_void, io};

    use tokio::{
        net::windows::named_pipe::{ClientOptions, PipeMode, ServerOptions},
        sync::broadcast,
    };
    use windows::Win32::Foundation::HWND;

    use crate::{
        daemon::{named_pipe_server_routine, resolve_cluster_tags, HWNDWrapper},
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
}
