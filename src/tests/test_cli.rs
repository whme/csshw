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
        let args = Args::parse_from(vec![
            "executable_name",
            "daemon",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(
            args.command,
            Some(Commands::Daemon {
                username: None,
                hosts: vec![
                    "host1".to_string(),
                    "host2".to_string(),
                    "cluster1".to_string()
                ]
            })
        );
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
        // With username
        let args = Args::parse_from(vec![
            "executable_name",
            "daemon",
            "-u",
            "username",
            "host1",
            "host2",
            "cluster1",
        ]);
        assert_eq!(
            args.command,
            Some(Commands::Daemon {
                username: Some("username".to_string()),
                hosts: vec![
                    "host1".to_string(),
                    "host2".to_string(),
                    "cluster1".to_string()
                ]
            })
        );
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
    }

    #[test]
    fn test_parse_client_args() {
        // Basic usage
        let args = Args::parse_from(vec!["executable_name", "client", "host1"]);
        assert_eq!(
            args.command,
            Some(Commands::Client {
                username: None,
                host: "host1".to_string()
            })
        );
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
        // With username
        let args = Args::parse_from(vec!["executable_name", "client", "-u", "username", "host1"]);
        assert_eq!(
            args.command,
            Some(Commands::Client {
                username: Some("username".to_string()),
                host: "host1".to_string()
            })
        );
        assert_eq!(args.username, None);
        assert_eq!(args.hosts, Vec::<String>::new());
        assert!(!args.debug);
    }
}
