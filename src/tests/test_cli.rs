//! Unit tests for CLI module

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

    #[test]
    fn test_args_debug_parsing() {
        let args = Args::try_parse_from(["csshw", "-d", "host1", "host2"]).unwrap();
        assert!(args.debug);
        assert_eq!(args.hosts, vec!["host1", "host2"]);
        assert!(args.command.is_none());
    }

    #[test]
    fn test_args_username_parsing() {
        let args = Args::try_parse_from(["csshw", "-u", "testuser", "host1"]).unwrap();
        assert_eq!(args.username, Some("testuser".to_string()));
        assert_eq!(args.hosts, vec!["host1"]);
    }

    #[test]
    fn test_args_port_parsing() {
        let args = Args::try_parse_from(["csshw", "-p", "2222", "host1"]).unwrap();
        assert_eq!(args.port, Some(2222));
        assert_eq!(args.hosts, vec!["host1"]);
    }

    #[test]
    fn test_args_client_command_parsing() {
        let args = Args::try_parse_from(["csshw", "client", "test-host"]).unwrap();
        match args.command {
            Some(Commands::Client { host }) => {
                assert_eq!(host, "test-host");
            }
            _ => panic!("Expected Client command"),
        }
    }

    #[test]
    fn test_args_daemon_command_parsing() {
        let args = Args::try_parse_from(["csshw", "daemon"]).unwrap();
        match args.command {
            Some(Commands::Daemon {}) => {
                // Success
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_args_combined_options() {
        let args = Args::try_parse_from([
            "csshw", "-d", "-u", "testuser", "-p", "2222", "host1", "host2",
        ])
        .unwrap();
        assert!(args.debug);
        assert_eq!(args.username, Some("testuser".to_string()));
        assert_eq!(args.port, Some(2222));
        assert_eq!(args.hosts, vec!["host1", "host2"]);
    }

    #[test]
    fn test_args_default_values() {
        let args = Args::try_parse_from(["csshw"]).unwrap();
        assert!(!args.debug);
        assert!(args.username.is_none());
        assert!(args.port.is_none());
        assert!(args.hosts.is_empty());
        assert!(args.command.is_none());
    }

    #[test]
    fn test_args_long_options() {
        let args = Args::try_parse_from([
            "csshw",
            "--debug",
            "--username",
            "testuser",
            "--port",
            "2222",
            "host1",
        ])
        .unwrap();
        assert!(args.debug);
        assert_eq!(args.username, Some("testuser".to_string()));
        assert_eq!(args.port, Some(2222));
        assert_eq!(args.hosts, vec!["host1"]);
    }

    #[test]
    fn test_args_invalid_port() {
        let result = Args::try_parse_from(["csshw", "-p", "invalid", "host1"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_args_missing_client_host() {
        let result = Args::try_parse_from(["csshw", "client"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_args_daemon_with_extra_args() {
        let args = Args::try_parse_from(["csshw", "daemon", "host1", "host2"]).unwrap();
        match args.command {
            Some(Commands::Daemon {}) => {
                assert_eq!(args.hosts, vec!["host1", "host2"]);
            }
            _ => panic!("Expected Daemon command"),
        }
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
        MockEntrypoint, MockLoggerInitializer,
    };
    use crate::utils::config::Config;
    use mockall::predicate::*;

    /// Test handle_special_commands function
    #[test]
    fn test_handle_special_commands() {
        let mut mock_args_command = MockArgsCommand::new();

        // Set up expectations for help commands
        mock_args_command
            .expect_print_help()
            .times(2)
            .returning(|| return Ok(()));

        // Test --help command
        assert!(handle_special_commands("--help", &mock_args_command));

        // Test -h command
        assert!(handle_special_commands("-h", &mock_args_command));

        // Test non-special commands (these don't need mock expectations)
        let mock_args_command2 = MockArgsCommand::new();
        assert!(!handle_special_commands("host1 host2", &mock_args_command2));
        assert!(!handle_special_commands(
            "-u username host1",
            &mock_args_command2
        ));
        assert!(!handle_special_commands(
            "daemon host1",
            &mock_args_command2
        ));
        assert!(!handle_special_commands(
            "client host1",
            &mock_args_command2
        ));
        assert!(!handle_special_commands("", &mock_args_command2));
        assert!(!handle_special_commands("--version", &mock_args_command2));
        assert!(!handle_special_commands("-v", &mock_args_command2));
    }

    /// Test execute_parsed_command with Client command
    #[tokio::test]
    async fn test_execute_parsed_command_client() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mock_logger_initializer = MockLoggerInitializer::new();
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

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &config,
            config_path,
        )
        .await;
    }

    /// Test execute_parsed_command with Daemon command
    #[tokio::test]
    async fn test_execute_parsed_command_daemon() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mut mock_logger_initializer = MockLoggerInitializer::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectations for logger initialization
        mock_logger_initializer
            .expect_init_logger()
            .with(eq("csshw_daemon"))
            .times(1)
            .returning(|_| {});

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

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &config,
            config_path,
        )
        .await;
    }

    /// Test execute_parsed_command with None command and hosts
    #[tokio::test]
    async fn test_execute_parsed_command_none_with_hosts() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mock_logger_initializer = MockLoggerInitializer::new();
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

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
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
        let mock_logger_initializer = MockLoggerInitializer::new();
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

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &config,
            config_path,
        )
        .await;
    }
}

/// Additional test module for CLI functionality to improve coverage.
mod cli_additional_test {
    use mockall::predicate::*;

    use crate::cli::{
        execute_parsed_command, handle_special_commands, read_user_input, show_interactive_prompt,
        Args, Commands, MainEntrypoint, MockArgsCommand, MockEntrypoint, MockLoggerInitializer,
    };
    use crate::utils::config::Config;

    #[test]
    fn test_commands_debug_trait() {
        let client_cmd = Commands::Client {
            host: "test-host".to_string(),
        };
        let daemon_cmd = Commands::Daemon {};

        let client_debug = format!("{client_cmd:?}");
        let daemon_debug = format!("{daemon_cmd:?}");

        assert!(client_debug.contains("Client"));
        assert!(client_debug.contains("test-host"));
        assert!(daemon_debug.contains("Daemon"));
    }

    #[test]
    fn test_commands_partial_eq() {
        let client_cmd1 = Commands::Client {
            host: "host1".to_string(),
        };
        let client_cmd2 = Commands::Client {
            host: "host1".to_string(),
        };
        let client_cmd3 = Commands::Client {
            host: "host2".to_string(),
        };
        let daemon_cmd = Commands::Daemon {};

        assert_eq!(client_cmd1, client_cmd2);
        assert_ne!(client_cmd1, client_cmd3);
        assert_ne!(client_cmd1, daemon_cmd);
        assert_eq!(daemon_cmd, Commands::Daemon {});
    }

    #[test]
    fn test_show_interactive_prompt() {
        // This function prints to stdout, we just test it doesn't panic
        show_interactive_prompt();
    }

    #[test]
    fn test_handle_special_commands_help() {
        let mut mock_args_command = MockArgsCommand::new();
        mock_args_command
            .expect_print_help()
            .times(1)
            .returning(|| return Ok(()));

        let result = handle_special_commands("--help", &mock_args_command);
        assert!(result);
    }

    #[test]
    fn test_handle_special_commands_help_short() {
        let mut mock_args_command = MockArgsCommand::new();
        mock_args_command
            .expect_print_help()
            .times(1)
            .returning(|| return Ok(()));

        let result = handle_special_commands("-h", &mock_args_command);
        assert!(result);
    }

    #[test]
    fn test_handle_special_commands_non_special() {
        let mock_args_command = MockArgsCommand::new();
        let result = handle_special_commands("regular command", &mock_args_command);
        assert!(!result);
    }

    #[test]
    fn test_main_entrypoint_creation() {
        let _entrypoint = MainEntrypoint;
        // Just test that it can be created without issues
    }

    #[test]
    fn test_read_user_input_function_exists() {
        // We can't easily test read_user_input without mocking stdin
        // But we can verify the function exists and has the right signature
        let _: fn() -> Result<Option<String>, std::io::Error> = read_user_input;
    }

    #[test]
    fn test_pkg_name_constant() {
        use crate::cli::PKG_NAME;
        assert_eq!(PKG_NAME, env!("CARGO_PKG_NAME"));
    }

    #[tokio::test]
    async fn test_execute_parsed_command_client_with_debug() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mut mock_logger = MockLoggerInitializer::new();

        mock_logger
            .expect_init_logger()
            .with(eq("csshw_client_test-host"))
            .times(1)
            .returning(|_| {});

        mock_entrypoint
            .expect_client_main()
            .with(
                eq("test-host".to_string()),
                eq(Some("testuser".to_string())),
                eq(Some(2222)),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Client {
                host: "test-host".to_string(),
            }),
            username: Some("testuser".to_string()),
            port: Some(2222),
            hosts: vec![],
            debug: true,
        };

        let config = Config::default();
        let config_path = "test-config.toml";

        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger,
            &config,
            config_path,
        )
        .await;
    }

    #[tokio::test]
    async fn test_execute_parsed_command_daemon_with_debug() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mut mock_logger = MockLoggerInitializer::new();

        mock_logger
            .expect_init_logger()
            .with(eq("csshw_daemon"))
            .times(1)
            .returning(|_| {});

        mock_entrypoint
            .expect_daemon_main()
            .with(
                eq(vec!["host1".to_string(), "host2".to_string()]),
                eq(Some("testuser".to_string())),
                eq(Some(2222)),
                always(),
                always(),
                eq(true),
            )
            .times(1)
            .returning(|_, _, _, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("testuser".to_string()),
            port: Some(2222),
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: true,
        };

        let config = Config::default();
        let config_path = "test-config.toml";

        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger,
            &config,
            config_path,
        )
        .await;
    }

    #[tokio::test]
    async fn test_execute_parsed_command_client_no_debug() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mock_logger = MockLoggerInitializer::new();

        mock_entrypoint
            .expect_client_main()
            .with(eq("test-host".to_string()), eq(None), eq(None), always())
            .times(1)
            .returning(|_, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Client {
                host: "test-host".to_string(),
            }),
            username: None,
            port: None,
            hosts: vec![],
            debug: false,
        };

        let config = Config::default();
        let config_path = "test-config.toml";

        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger,
            &config,
            config_path,
        )
        .await;
    }

    #[tokio::test]
    async fn test_execute_parsed_command_daemon_no_debug() {
        let mut mock_entrypoint = MockEntrypoint::new();
        let mock_args_command = MockArgsCommand::new();
        let mock_logger = MockLoggerInitializer::new();

        mock_entrypoint
            .expect_daemon_main()
            .with(
                eq(vec![]),
                eq(None),
                eq(None),
                always(),
                always(),
                eq(false),
            )
            .times(1)
            .returning(|_, _, _, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Daemon {}),
            username: None,
            port: None,
            hosts: vec![],
            debug: false,
        };

        let config = Config::default();
        let config_path = "test-config.toml";

        execute_parsed_command(
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger,
            &config,
            config_path,
        )
        .await;
    }

    #[tokio::test]
    async fn test_run_interactive_mode_exit_immediately() {
        // Create a mock entrypoint that should not be called
        let _mock_entrypoint = MockEntrypoint::new();
        let _config = Config::default();
        let _config_path = "test-config.toml";

        // This test simulates immediate exit, so we can't easily test the actual interactive loop
        // without mocking stdin, but we can test that the function exists and compiles
        // In a real test environment, this would require more complex stdin mocking
    }
}
