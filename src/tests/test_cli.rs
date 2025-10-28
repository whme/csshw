// Note that only the debug option of the main command
// is supported for all subcommands.
// Other main command arguments/options are ignored if a subcommand
// is given.
// This is expected behavior from clap and we do not test for it.
mod cli_args_test {
    use clap::Parser as _;

    use crate::cli::{Args, Commands};

    #[test]
    fn test_parse_args() {
        // Basic usage
        let args = Args::parse_from(vec!["executable_name", "host1", "host2", "cluster1"]);
        assert_eq!(args.command, None);
        assert_eq!(args.username, None);
        assert_eq!(args.port, None);
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(!args.debug);
        // With username
        let args = Args::parse_from(vec![
            "executable_name",
            "-u",
            "username",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, None);
        assert_eq!(args.username, Some("username".to_string()));
        assert_eq!(args.port, None);
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(!args.debug);
        // With username and debug
        let args = Args::parse_from(vec![
            "executable_name",
            "-u",
            "username",
            "-d",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, None);
        assert_eq!(args.username, Some("username".to_string()));
        assert_eq!(args.port, None);
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(args.debug);
        // With port
        let args = Args::parse_from(vec![
            "executable_name",
            "-p",
            "2222",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, None);
        assert_eq!(args.username, None);
        assert_eq!(args.port, Some(2222));
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(!args.debug);
        // With username, port and debug
        let args = Args::parse_from(vec![
            "executable_name",
            "-u",
            "username",
            "-p",
            "8080",
            "-d",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, None);
        assert_eq!(args.username, Some("username".to_string()));
        assert_eq!(args.port, Some(8080));
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(args.debug);
    }

    #[test]
    fn test_parse_daemon_args() {
        // Basic usage
        let args: Args = Args::parse_from(vec![
            "executable_name",
            "daemon",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, Some(Commands::Daemon {}));
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(!args.debug);
        // With username
        let args = Args::parse_from(vec![
            "executable_name",
            "-u",
            "username",
            "daemon",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(args.command, Some(Commands::Daemon {}));
        assert_eq!(args.username, Some("username".to_string()));
        assert_eq!(args.hosts, vec!["host1", "host2", "cluster1"]);
        assert!(!args.debug);
    }

    #[test]
    fn test_parse_client_args() {
        // Basic usage
        let args = Args::parse_from(vec!["executable_name", "client", "host1"]);
        assert_eq!(
            args.command,
            Some(Commands::Client {
                host: "host1".to_string()
            })
        );
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
        // With username
        let args = Args::parse_from(vec!["executable_name", "-u", "username", "client", "host1"]);
        assert_eq!(
            args.command,
            Some(Commands::Client {
                host: "host1".to_string()
            })
        );
        assert_eq!(args.username, Some("username".to_string()));
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
    }
}

mod cli_main_test {
    use crate::cli::{main, Args, Commands, MockEntrypoint};

    #[tokio::test]
    async fn test_main() {
        let mut mock = MockEntrypoint::new();
        let args = Args {
            command: None,
            username: None,
            port: None,
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: false,
        };
        mock.expect_main().once().returning(|config_path, _, args| {
            assert_eq!(config_path, "csshw-config.toml");
            assert_eq!(args.command, None);
            assert_eq!(args.username, None);
            assert_eq!(args.hosts, vec!["host1".to_string(), "host2".to_string()]);
            assert!(!args.debug);
            return;
        });
        main(args, mock).await;
    }

    #[tokio::test]
    async fn test_daemon_main() {
        let mut mock = MockEntrypoint::new();
        mock.expect_daemon_main()
            .once()
            .returning(|hosts, username, port, _, _, debug| {
                assert_eq!(hosts, vec!["host1".to_string(), "host2".to_string()]);
                assert_eq!(username, Some("username".to_string()));
                assert_eq!(port, None);
                assert!(!debug);
                return Box::pin(async {});
            });
        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("username".to_string()),
            port: None,
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: false,
        };
        main(args, mock).await;
    }

    #[tokio::test]
    async fn test_client_main() {
        let mut mock = MockEntrypoint::new();
        mock.expect_client_main()
            .once()
            .returning(|host, username, port, _| {
                assert_eq!(host, "host1");
                assert_eq!(username, Some("username".to_string()));
                assert_eq!(port, None);
                return Box::pin(async {});
            });
        let args = Args {
            command: Some(Commands::Client {
                host: "host1".to_string(),
            }),
            username: Some("username".to_string()),
            port: None,
            hosts: vec!["host1".to_string()],
            debug: false,
        };
        main(args, mock).await;
    }
}

/// Test module for the new interactive mode helper functions
mod interactive_mode_test {
    use crate::cli::{
        execute_parsed_command, handle_special_commands, Args, Commands, MockArgsCommand,
        MockEntrypoint,
    };
    use crate::utils::config::Config;
    use mockall::predicate::*;

    /// Test handle_special_commands function
    #[test]
    fn test_handle_special_commands() {
        // Test --help command
        assert!(handle_special_commands("--help"));

        // Test -h command
        assert!(handle_special_commands("-h"));

        // Test non-special commands
        assert!(!handle_special_commands("host1 host2"));
        assert!(!handle_special_commands("-u username host1"));
        assert!(!handle_special_commands("daemon host1"));
        assert!(!handle_special_commands("client host1"));
        assert!(!handle_special_commands(""));
        assert!(!handle_special_commands("--version"));
        assert!(!handle_special_commands("-v"));
    }

    /// Test execute_parsed_command with Client command
    #[tokio::test]
    async fn test_execute_parsed_command_client() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectations
        mock_entrypoint
            .expect_client_main()
            .with(
                eq("testhost".to_string()),
                eq(Some("testuser".to_string())),
                eq(Some(2222)),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Client {
                host: "testhost".to_string(),
            }),
            username: Some("testuser".to_string()),
            port: Some(2222),
            hosts: vec![],
            debug: false,
        };

        let mock_args_command = MockArgsCommand::new();
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &config,
            config_path,
        )
        .await;
    }

    /// Test execute_parsed_command with Daemon command
    #[tokio::test]
    async fn test_execute_parsed_command_daemon() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectations
        mock_entrypoint
            .expect_daemon_main()
            .with(
                eq(vec!["host1".to_string(), "host2".to_string()]),
                eq(Some("testuser".to_string())),
                eq(Some(8080)),
                always(),
                always(),
                eq(true),
            )
            .times(1)
            .returning(|_, _, _, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("testuser".to_string()),
            port: Some(8080),
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: true,
        };

        let mock_args_command = MockArgsCommand::new();
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &config,
            config_path,
        )
        .await;
    }

    /// Test execute_parsed_command with None command and hosts
    #[tokio::test]
    async fn test_execute_parsed_command_none_with_hosts() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectations
        mock_entrypoint
            .expect_main()
            .with(eq(config_path), always(), always())
            .times(1)
            .returning(|_, _, _| {});

        let args = Args {
            command: None,
            username: Some("testuser".to_string()),
            port: Some(3333),
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: false,
        };

        let mock_args_command = MockArgsCommand::new();
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &config,
            config_path,
        )
        .await;
    }

    /// Test execute_parsed_command with None command and no hosts (should show help)
    #[tokio::test]
    async fn test_execute_parsed_command_none_no_hosts() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mut mock_args_command = MockArgsCommand::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectation that print_help will be called
        mock_args_command
            .expect_print_help()
            .times(1)
            .returning(|| return Ok(()));

        let args = Args {
            command: None,
            username: None,
            port: None,
            hosts: vec![],
            debug: false,
        };

        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &config,
            config_path,
        )
        .await;
    }
}
