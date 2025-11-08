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
    use crate::cli::{
        main, Args, Commands, MockArgsCommand, MockConfigManager, MockEntrypoint, MockEnvironment,
        MockInput, MockLoggerInitializer, MockOutput,
    };
    use crate::utils::config::ConfigOpt;
    use crate::utils::windows::MockWindowsApi;

    /// Test parameters for parametrized main function tests
    struct MainTestParams {
        /// Arguments to pass to main function
        args: Args,
        /// Whether launched from GUI
        launched_from_gui: bool,
        /// Expected behavior verification function
        verify_fn:
            fn(&mut MockEntrypoint, &mut MockWindowsApi, &mut MockOutput, &mut MockArgsCommand),
    }

    /// Helper function to set up common Windows API mocks
    fn setup_common_windows_api_mocks(
        mock_windows_api: &mut MockWindowsApi,
        mock_output: &mut MockOutput,
        launched_from_gui: bool,
    ) {
        // Mock the is_launched_from_gui call
        mock_windows_api.expect_get_stdout_handle().returning(|| {
            return Ok(windows::Win32::Foundation::HANDLE(
                std::ptr::dangling_mut::<std::ffi::c_void>(),
            ));
        });
        mock_windows_api
            .expect_get_console_screen_buffer_info_with_handle()
            .returning(move |_| {
                if launched_from_gui {
                    // Return error to simulate GUI launch
                    return Err(windows::core::Error::from(
                        windows::Win32::Foundation::E_FAIL,
                    ));
                } else {
                    // Return success to simulate console launch
                    return Ok(
                        windows::Win32::System::Console::CONSOLE_SCREEN_BUFFER_INFO {
                            dwSize: windows::Win32::System::Console::COORD { X: 80, Y: 25 },
                            dwCursorPosition: windows::Win32::System::Console::COORD {
                                X: 10,
                                Y: 5,
                            },
                            wAttributes:
                                windows::Win32::System::Console::CONSOLE_CHARACTER_ATTRIBUTES(0),
                            srWindow: windows::Win32::System::Console::SMALL_RECT {
                                Left: 0,
                                Top: 0,
                                Right: 79,
                                Bottom: 24,
                            },
                            dwMaximumWindowSize: windows::Win32::System::Console::COORD {
                                X: 80,
                                Y: 25,
                            },
                        },
                    );
                }
            });
        // Mock the set_process_dpi_awareness call
        mock_windows_api
            .expect_set_process_dpi_awareness()
            .returning(|_| {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_FAIL,
                ));
            });

        // Expect DPI awareness error message to be written
        mock_output
            .expect_eprintln()
            .with(mockall::predicate::str::starts_with(
                "Failed to set DPI awareness programatically:",
            ))
            .times(1)
            .returning(|_| {});
    }

    /// Helper function to set up common Environment mocks
    fn setup_common_environment_mocks(mock_environment: &mut MockEnvironment) {
        mock_environment.expect_current_exe().returning(|| {
            return Ok(std::path::PathBuf::from("C:\\test\\csshw.exe"));
        });
        mock_environment
            .expect_set_current_dir()
            .returning(|_| return Ok(()));
    }

    /// Helper function to set up common ConfigManager mocks
    fn setup_common_config_manager_mocks(mock_config_manager: &mut MockConfigManager) {
        mock_config_manager
            .expect_load_config()
            .with(mockall::predicate::eq("csshw-config.toml"))
            .returning(|_| return Ok(ConfigOpt::default()));
    }

    /// Parametrized test for main function covering all branches
    #[tokio::test]
    async fn test_main_parametrized() {
        let test_cases = vec![
            // main_with_hosts_success
            MainTestParams {
                args: Args {
                    command: None,
                    username: None,
                    port: None,
                    hosts: vec!["host1".to_string(), "host2".to_string()],
                    debug: false,
                },
                launched_from_gui: false,
                verify_fn: |mock, mock_windows_api, _mock_output, _mock_args_command| {
                    // Mock the create_process_with_args call that will be made by the main method
                    mock_windows_api
                        .expect_create_process_with_args()
                        .with(
                            mockall::predicate::eq("csshw.exe"),
                            mockall::predicate::eq(vec![
                                "daemon".to_string(),
                                "host1".to_string(),
                                "host2".to_string(),
                            ]),
                        )
                        .returning(|_, _| {
                            return Some(windows::Win32::System::Threading::PROCESS_INFORMATION {
                                hProcess: windows::Win32::Foundation::HANDLE(
                                    std::ptr::dangling_mut::<std::ffi::c_void>(),
                                ),
                                hThread: windows::Win32::Foundation::HANDLE(
                                    std::ptr::dangling_mut::<std::ffi::c_void>(),
                                ),
                                dwProcessId: 1234,
                                dwThreadId: 5678,
                            });
                        });

                    // Mock the get_window_handle_for_process call
                    mock_windows_api
                        .expect_get_window_handle_for_process()
                        .with(mockall::predicate::eq(1234u32))
                        .returning(|_| {
                            return windows::Win32::Foundation::HWND(std::ptr::dangling_mut::<
                                std::ffi::c_void,
                            >(
                            ));
                        });

                    mock.expect_main().once().returning(
                        |_: &MockWindowsApi, _: &MockConfigManager, config_path, _, args| {
                            assert_eq!(config_path, "csshw-config.toml");
                            assert_eq!(args.command, None);
                            assert_eq!(args.username, None);
                            assert_eq!(args.hosts, vec!["host1".to_string(), "host2".to_string()]);
                            assert!(!args.debug);
                            return;
                        },
                    );
                },
            },
            // main_with_empty_hosts_console_launch
            MainTestParams {
                args: Args {
                    command: None,
                    username: None,
                    port: None,
                    hosts: vec![],
                    debug: false,
                },
                launched_from_gui: false,
                verify_fn: |_mock, _mock_windows_api, _mock_output, mock_args_command| {
                    // Set up mock for help printing in empty hosts cases
                    mock_args_command
                        .expect_print_help()
                        .times(1)
                        .returning(|| return Ok(()));
                },
            },
            // main_with_empty_hosts_gui_launch
            MainTestParams {
                args: Args {
                    command: None,
                    username: None,
                    port: None,
                    hosts: vec![],
                    debug: false,
                },
                launched_from_gui: true,
                verify_fn: |_mock, _mock_windows_api, _mock_output, mock_args_command| {
                    // Set up mock for help printing in empty hosts cases
                    mock_args_command
                        .expect_print_help()
                        .times(1)
                        .returning(|| return Ok(()));
                },
            },
        ];

        for test_case in test_cases {
            let mut mock = MockEntrypoint::new();
            let mut mock_windows_api = MockWindowsApi::new();
            let mut mock_output = MockOutput::new();
            let mut mock_input = MockInput::new();
            let mut mock_environment = MockEnvironment::new();
            let mut mock_args_command = MockArgsCommand::new();
            let mock_logger_initializer = MockLoggerInitializer::new();
            let mut mock_config_manager = MockConfigManager::new();

            // Set up common mocks
            setup_common_windows_api_mocks(
                &mut mock_windows_api,
                &mut mock_output,
                test_case.launched_from_gui,
            );
            setup_common_environment_mocks(&mut mock_environment);
            setup_common_config_manager_mocks(&mut mock_config_manager);

            // Set up test-specific expectations
            (test_case.verify_fn)(
                &mut mock,
                &mut mock_windows_api,
                &mut mock_output,
                &mut mock_args_command,
            );

            main(
                &mock_windows_api,
                test_case.args,
                mock,
                &mut mock_output,
                &mut mock_input,
                &mock_environment,
                &mock_args_command,
                &mock_logger_initializer,
                &mock_config_manager,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn test_daemon_main() {
        let mut mock = MockEntrypoint::new();
        let mut mock_windows_api = MockWindowsApi::new();
        let mut mock_output = MockOutput::new();
        let mut mock_input = MockInput::new();
        let mut mock_environment = MockEnvironment::new();
        let mut mock_config_manager = MockConfigManager::new();

        // Set up Windows API mocks without the DPI error expectation
        mock_windows_api.expect_get_stdout_handle().returning(|| {
            return Ok(windows::Win32::Foundation::HANDLE(
                std::ptr::dangling_mut::<std::ffi::c_void>(),
            ));
        });
        mock_windows_api
            .expect_get_console_screen_buffer_info_with_handle()
            .returning(|_| {
                return Ok(
                    windows::Win32::System::Console::CONSOLE_SCREEN_BUFFER_INFO {
                        dwSize: windows::Win32::System::Console::COORD { X: 80, Y: 25 },
                        dwCursorPosition: windows::Win32::System::Console::COORD { X: 10, Y: 5 },
                        wAttributes: windows::Win32::System::Console::CONSOLE_CHARACTER_ATTRIBUTES(
                            0,
                        ),
                        srWindow: windows::Win32::System::Console::SMALL_RECT {
                            Left: 0,
                            Top: 0,
                            Right: 79,
                            Bottom: 24,
                        },
                        dwMaximumWindowSize: windows::Win32::System::Console::COORD {
                            X: 80,
                            Y: 25,
                        },
                    },
                );
            });

        // Mock the set_process_dpi_awareness call to succeed for this test
        mock_windows_api
            .expect_set_process_dpi_awareness()
            .returning(|_| return Ok(()));

        // Set up environment mocks
        setup_common_environment_mocks(&mut mock_environment);
        setup_common_config_manager_mocks(&mut mock_config_manager);

        mock.expect_daemon_main().once().returning(
            |_: &MockWindowsApi, hosts, username, port, _, _, debug| {
                assert_eq!(hosts, vec!["host1".to_string(), "host2".to_string()]);
                assert_eq!(username, Some("username".to_string()));
                assert_eq!(port, None);
                assert!(!debug);
                return Box::pin(async {});
            },
        );
        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("username".to_string()),
            port: None,
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: false,
        };
        let mock_args_command = MockArgsCommand::new();
        let mock_logger_initializer = MockLoggerInitializer::new();
        main(
            &mock_windows_api,
            args,
            mock,
            &mut mock_output,
            &mut mock_input,
            &mock_environment,
            &mock_args_command,
            &mock_logger_initializer,
            &mock_config_manager,
        )
        .await;
    }

    #[tokio::test]
    async fn test_client_main() {
        let mut mock = MockEntrypoint::new();
        let mut mock_windows_api = MockWindowsApi::new();
        let mut mock_output = MockOutput::new();
        let mut mock_input = MockInput::new();
        let mut mock_environment = MockEnvironment::new();
        let mut mock_config_manager = MockConfigManager::new();

        // Set up Windows API mocks without the DPI error expectation
        mock_windows_api.expect_get_stdout_handle().returning(|| {
            return Ok(windows::Win32::Foundation::HANDLE(
                std::ptr::dangling_mut::<std::ffi::c_void>(),
            ));
        });
        mock_windows_api
            .expect_get_console_screen_buffer_info_with_handle()
            .returning(|_| {
                return Ok(
                    windows::Win32::System::Console::CONSOLE_SCREEN_BUFFER_INFO {
                        dwSize: windows::Win32::System::Console::COORD { X: 80, Y: 25 },
                        dwCursorPosition: windows::Win32::System::Console::COORD { X: 10, Y: 5 },
                        wAttributes: windows::Win32::System::Console::CONSOLE_CHARACTER_ATTRIBUTES(
                            0,
                        ),
                        srWindow: windows::Win32::System::Console::SMALL_RECT {
                            Left: 0,
                            Top: 0,
                            Right: 79,
                            Bottom: 24,
                        },
                        dwMaximumWindowSize: windows::Win32::System::Console::COORD {
                            X: 80,
                            Y: 25,
                        },
                    },
                );
            });

        // Mock the set_process_dpi_awareness call to succeed for this test
        mock_windows_api
            .expect_set_process_dpi_awareness()
            .returning(|_| return Ok(()));

        // Set up environment mocks
        setup_common_environment_mocks(&mut mock_environment);
        setup_common_config_manager_mocks(&mut mock_config_manager);

        mock.expect_client_main::<MockWindowsApi>()
            .once()
            .returning(|_, host, username, port, _| {
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
        let mock_args_command = MockArgsCommand::new();
        let mock_logger_initializer = MockLoggerInitializer::new();
        main(
            &mock_windows_api,
            args,
            mock,
            &mut mock_output,
            &mut mock_input,
            &mock_environment,
            &mock_args_command,
            &mock_logger_initializer,
            &mock_config_manager,
        )
        .await;
    }
}

/// Test module for the new interactive mode helper functions
mod interactive_mode_test {
    use crate::cli::{
        execute_parsed_command, handle_special_commands, Args, Commands, MockArgsCommand,
        MockConfigManager, MockEntrypoint, MockLoggerInitializer,
    };
    use crate::utils::config::Config;
    use crate::utils::windows::MockWindowsApi;
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
        let mock_windows_api = MockWindowsApi::new();
        let config = Config::default();
        let config_path = "test-config.toml";

        // Set up expectations
        mock_entrypoint
            .expect_client_main::<MockWindowsApi>()
            .with(
                always(),
                eq("testhost".to_string()),
                eq(Some("testuser".to_string())),
                eq(Some(2222)),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _, _| return Box::pin(async {}));

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
            &mock_windows_api,
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &MockConfigManager::new(),
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
                always(),
                eq(vec!["host1".to_string(), "host2".to_string()]),
                eq(Some("testuser".to_string())),
                eq(Some(8080)),
                always(),
                always(),
                eq(true),
            )
            .times(1)
            .returning(|_: &MockWindowsApi, _, _, _, _, _, _| return Box::pin(async {}));

        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("testuser".to_string()),
            port: Some(8080),
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: true,
        };

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            &MockWindowsApi::new(),
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &MockConfigManager::new(),
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
            .with(always(), always(), eq(config_path), always(), always())
            .times(1)
            .returning(|_: &MockWindowsApi, _: &MockConfigManager, _, _, _| {});

        let args = Args {
            command: None,
            username: Some("testuser".to_string()),
            port: Some(3333),
            hosts: vec!["host1".to_string(), "host2".to_string()],
            debug: false,
        };

        // Call the actual execute_parsed_command function with mocked dependencies
        execute_parsed_command(
            &MockWindowsApi::new(),
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &MockConfigManager::new(),
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
            &MockWindowsApi::new(),
            args,
            &mut mock_entrypoint,
            &mock_args_command,
            &mock_logger_initializer,
            &MockConfigManager::new(),
            &config,
            config_path,
        )
        .await;
    }
}
