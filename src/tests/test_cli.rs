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
            .returning(|hosts, username, _, _, debug| {
                assert_eq!(hosts, vec!["host1".to_string(), "host2".to_string()]);
                assert_eq!(username, Some("username".to_string()));
                assert!(!debug);
                return Box::pin(async {});
            });
        let args = Args {
            command: Some(Commands::Daemon {}),
            username: Some("username".to_string()),
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
            .returning(|host, username, _| {
                assert_eq!(host, "host1");
                assert_eq!(username, Some("username".to_string()));
            });
        let args = Args {
            command: Some(Commands::Client {
                host: "host1".to_string(),
            }),
            username: Some("username".to_string()),
            hosts: vec!["host1".to_string()],
            debug: false,
        };
        main(args, mock).await;
    }
}
