//! Unit tests for the config module.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use crate::utils::config::{
    ClientConfig, ClientConfigOpt, Cluster, Config, ConfigOpt, DaemonConfig, DaemonConfigOpt,
};

/// Test module for ClientConfig functionality.
mod client_config_test {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();

        // Test that placeholder is present in arguments
        assert!(config.arguments.contains(&config.username_host_placeholder));
    }

    #[test]
    fn test_client_config_opt_default() {
        let config_opt = ClientConfigOpt::default();
        let client_config: ClientConfig = config_opt.into();
        let default_config = ClientConfig::default();

        assert_eq!(
            client_config.ssh_config_path,
            default_config.ssh_config_path
        );
        assert_eq!(client_config.program, default_config.program);
        assert_eq!(client_config.arguments, default_config.arguments);
        assert_eq!(
            client_config.username_host_placeholder,
            default_config.username_host_placeholder
        );
    }

    #[test]
    fn test_client_config_opt_from_client_config() {
        let client_config = ClientConfig {
            ssh_config_path: "C:\\custom\\path\\config".to_string(),
            program: "openssh".to_string(),
            arguments: vec!["-v".to_string(), "{{USER_HOST}}".to_string()],
            username_host_placeholder: "{{USER_HOST}}".to_string(),
        };

        let config_opt: ClientConfigOpt = ClientConfigOpt {
            ssh_config_path: Some(client_config.ssh_config_path.clone()),
            program: Some(client_config.program.clone()),
            arguments: Some(client_config.arguments.clone()),
            username_host_placeholder: Some(client_config.username_host_placeholder.clone()),
        };
        let converted_back: ClientConfig = config_opt.into();

        assert_eq!(
            converted_back.ssh_config_path,
            client_config.ssh_config_path
        );
        assert_eq!(converted_back.program, client_config.program);
        assert_eq!(converted_back.arguments, client_config.arguments);
        assert_eq!(
            converted_back.username_host_placeholder,
            client_config.username_host_placeholder
        );
    }

    #[test]
    fn test_client_config_opt_partial_values() {
        let config_opt = ClientConfigOpt {
            ssh_config_path: Some("C:\\custom\\config".to_string()),
            program: None,
            arguments: Some(vec!["-X".to_string(), "{{HOST}}".to_string()]),
            username_host_placeholder: None,
        };

        let client_config: ClientConfig = config_opt.into();
        let default_config = ClientConfig::default();

        assert_eq!(client_config.ssh_config_path, "C:\\custom\\config");
        assert_eq!(client_config.program, default_config.program); // Should use default
        assert_eq!(client_config.arguments, vec!["-X", "{{HOST}}"]);
        assert_eq!(
            client_config.username_host_placeholder,
            default_config.username_host_placeholder
        ); // Should use default
    }

    #[test]
    fn test_client_config_custom_values() {
        let config = ClientConfig {
            ssh_config_path: "D:\\ssh\\config".to_string(),
            program: "putty".to_string(),
            arguments: vec!["-ssh".to_string(), "{{TARGET}}".to_string()],
            username_host_placeholder: "{{TARGET}}".to_string(),
        };

        assert_eq!(config.ssh_config_path, "D:\\ssh\\config");
        assert_eq!(config.program, "putty");
        assert_eq!(config.arguments, vec!["-ssh", "{{TARGET}}"]);
        assert_eq!(config.username_host_placeholder, "{{TARGET}}");
    }
}

/// Test module for DaemonConfig functionality.
mod daemon_config_test {
    use super::*;

    #[test]
    fn test_daemon_config_opt_default() {
        let config_opt = DaemonConfigOpt::default();
        let daemon_config: DaemonConfig = config_opt.into();
        let default_config = DaemonConfig::default();

        assert_eq!(daemon_config.height, default_config.height);
        assert_eq!(
            daemon_config.aspect_ratio_adjustement,
            default_config.aspect_ratio_adjustement
        );
        assert_eq!(daemon_config.console_color, default_config.console_color);
    }

    #[test]
    fn test_daemon_config_opt_from_daemon_config() {
        let daemon_config = DaemonConfig {
            height: 300,
            aspect_ratio_adjustement: 0.5,
            console_color: 15, // White on black
        };

        let config_opt: DaemonConfigOpt = DaemonConfigOpt {
            height: Some(daemon_config.height),
            aspect_ratio_adjustement: Some(daemon_config.aspect_ratio_adjustement),
            console_color: Some(daemon_config.console_color),
        };
        let converted_back: DaemonConfig = config_opt.into();

        assert_eq!(converted_back.height, daemon_config.height);
        assert_eq!(
            converted_back.aspect_ratio_adjustement,
            daemon_config.aspect_ratio_adjustement
        );
        assert_eq!(converted_back.console_color, daemon_config.console_color);
    }

    #[test]
    fn test_daemon_config_opt_partial_values() {
        let config_opt = DaemonConfigOpt {
            height: Some(150),
            aspect_ratio_adjustement: None,
            console_color: Some(112),
        };

        let daemon_config: DaemonConfig = config_opt.into();
        let default_config = DaemonConfig::default();

        assert_eq!(daemon_config.height, 150);
        assert_eq!(
            daemon_config.aspect_ratio_adjustement,
            default_config.aspect_ratio_adjustement
        ); // Should use default
        assert_eq!(daemon_config.console_color, 112);
    }
}

/// Test module for Cluster functionality.
mod cluster_test {
    use super::*;

    #[test]
    fn test_cluster_default() {
        let cluster = Cluster::default();

        assert_eq!(cluster.name, "");
        assert!(cluster.hosts.is_empty());
    }

    #[test]
    fn test_cluster_custom_values() {
        let cluster = Cluster {
            name: "production".to_string(),
            hosts: vec![
                "server1.example.com".to_string(),
                "server2.example.com".to_string(),
                "server3.example.com".to_string(),
            ],
        };

        assert_eq!(cluster.name, "production");
        assert_eq!(cluster.hosts.len(), 3);
        assert_eq!(cluster.hosts[0], "server1.example.com");
        assert_eq!(cluster.hosts[1], "server2.example.com");
        assert_eq!(cluster.hosts[2], "server3.example.com");
    }

    #[test]
    fn test_cluster_empty_hosts() {
        let cluster = Cluster {
            name: "empty-cluster".to_string(),
            hosts: vec![],
        };

        assert_eq!(cluster.name, "empty-cluster");
        assert!(cluster.hosts.is_empty());
    }

    #[test]
    fn test_cluster_single_host() {
        let cluster = Cluster {
            name: "single".to_string(),
            hosts: vec!["single-host.com".to_string()],
        };

        assert_eq!(cluster.name, "single");
        assert_eq!(cluster.hosts.len(), 1);
        assert_eq!(cluster.hosts[0], "single-host.com");
    }
}

/// Test module for main Config functionality.
mod config_test {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert_eq!(config.client, ClientConfig::default());
        assert_eq!(config.daemon, DaemonConfig::default());
        assert!(config.clusters.is_empty());
    }

    #[test]
    fn test_config_with_clusters() {
        let config = Config {
            clusters: vec![
                Cluster {
                    name: "web-servers".to_string(),
                    hosts: vec!["web1.com".to_string(), "web2.com".to_string()],
                },
                Cluster {
                    name: "db-servers".to_string(),
                    hosts: vec!["db1.com".to_string()],
                },
            ],
            client: ClientConfig::default(),
            daemon: DaemonConfig::default(),
        };

        assert_eq!(config.clusters.len(), 2);
        assert_eq!(config.clusters[0].name, "web-servers");
        assert_eq!(config.clusters[0].hosts.len(), 2);
        assert_eq!(config.clusters[1].name, "db-servers");
        assert_eq!(config.clusters[1].hosts.len(), 1);
    }

    #[test]
    fn test_config_opt_default() {
        let config_opt = ConfigOpt::default();
        let config: Config = config_opt.into();
        let default_config = Config::default();

        assert_eq!(config.clusters.len(), default_config.clusters.len());
        assert_eq!(config.client.program, default_config.client.program);
        assert_eq!(config.daemon.height, default_config.daemon.height);
    }

    #[test]
    fn test_config_opt_from_config() {
        let original_config = Config {
            clusters: vec![Cluster {
                name: "test".to_string(),
                hosts: vec!["test.com".to_string()],
            }],
            client: ClientConfig {
                ssh_config_path: "custom/path".to_string(),
                program: "custom-ssh".to_string(),
                arguments: vec!["-custom".to_string()],
                username_host_placeholder: "{{CUSTOM}}".to_string(),
            },
            daemon: DaemonConfig {
                height: 250,
                aspect_ratio_adjustement: 0.5,
                console_color: 15,
            },
        };

        let config_opt: ConfigOpt = ConfigOpt {
            clusters: Some(original_config.clusters.clone()),
            client: Some(ClientConfigOpt {
                ssh_config_path: Some(original_config.client.ssh_config_path.clone()),
                program: Some(original_config.client.program.clone()),
                arguments: Some(original_config.client.arguments.clone()),
                username_host_placeholder: Some(
                    original_config.client.username_host_placeholder.clone(),
                ),
            }),
            daemon: Some(DaemonConfigOpt {
                height: Some(original_config.daemon.height),
                aspect_ratio_adjustement: Some(original_config.daemon.aspect_ratio_adjustement),
                console_color: Some(original_config.daemon.console_color),
            }),
        };
        let converted_back: Config = config_opt.into();

        assert_eq!(
            converted_back.clusters.len(),
            original_config.clusters.len()
        );
        assert_eq!(
            converted_back.clusters[0].name,
            original_config.clusters[0].name
        );
        assert_eq!(
            converted_back.client.program,
            original_config.client.program
        );
        assert_eq!(converted_back.daemon.height, original_config.daemon.height);
    }

    #[test]
    fn test_config_opt_partial_values() {
        let config_opt = ConfigOpt {
            clusters: Some(vec![Cluster {
                name: "partial".to_string(),
                hosts: vec!["partial.com".to_string()],
            }]),
            client: None, // Should use default
            daemon: Some(DaemonConfigOpt {
                height: Some(300),
                aspect_ratio_adjustement: None, // Should use default
                console_color: Some(112),
            }),
        };

        let config: Config = config_opt.into();
        let default_client = ClientConfig::default();
        let default_daemon = DaemonConfig::default();

        assert_eq!(config.clusters.len(), 1);
        assert_eq!(config.clusters[0].name, "partial");

        // Client should be default
        assert_eq!(config.client.program, default_client.program);
        assert_eq!(
            config.client.ssh_config_path,
            default_client.ssh_config_path
        );

        // Daemon should be partially custom, partially default
        assert_eq!(config.daemon.height, 300);
        assert_eq!(
            config.daemon.aspect_ratio_adjustement,
            default_daemon.aspect_ratio_adjustement
        );
        assert_eq!(config.daemon.console_color, 112);
    }

    #[test]
    fn test_config_multiple_clusters() {
        let clusters = vec![
            Cluster {
                name: "cluster1".to_string(),
                hosts: vec!["host1.com".to_string()],
            },
            Cluster {
                name: "cluster2".to_string(),
                hosts: vec!["host2.com".to_string(), "host3.com".to_string()],
            },
            Cluster {
                name: "cluster3".to_string(),
                hosts: vec![],
            },
        ];

        let config = Config {
            clusters: clusters.clone(),
            client: ClientConfig::default(),
            daemon: DaemonConfig::default(),
        };

        assert_eq!(config.clusters.len(), 3);
        assert_eq!(config.clusters[0].hosts.len(), 1);
        assert_eq!(config.clusters[1].hosts.len(), 2);
        assert_eq!(config.clusters[2].hosts.len(), 0);
    }
}

/// Integration tests for config conversions and edge cases.
mod config_integration_test {
    use super::*;

    #[test]
    fn test_full_config_roundtrip() {
        let original = Config {
            clusters: vec![
                Cluster {
                    name: "production".to_string(),
                    hosts: vec!["prod1.com".to_string(), "prod2.com".to_string()],
                },
                Cluster {
                    name: "staging".to_string(),
                    hosts: vec!["staging.com".to_string()],
                },
            ],
            client: ClientConfig {
                ssh_config_path: "C:\\Users\\test\\.ssh\\config".to_string(),
                program: "openssh".to_string(),
                arguments: vec![
                    "-o".to_string(),
                    "StrictHostKeyChecking=no".to_string(),
                    "{{USER_HOST}}".to_string(),
                ],
                username_host_placeholder: "{{USER_HOST}}".to_string(),
            },
            daemon: DaemonConfig {
                height: 180,
                aspect_ratio_adjustement: -0.8,
                console_color: 240,
            },
        };

        // Convert to optional and back
        let config_opt: ConfigOpt = ConfigOpt {
            clusters: Some(original.clusters.clone()),
            client: Some(ClientConfigOpt {
                ssh_config_path: Some(original.client.ssh_config_path.clone()),
                program: Some(original.client.program.clone()),
                arguments: Some(original.client.arguments.clone()),
                username_host_placeholder: Some(original.client.username_host_placeholder.clone()),
            }),
            daemon: Some(DaemonConfigOpt {
                height: Some(original.daemon.height),
                aspect_ratio_adjustement: Some(original.daemon.aspect_ratio_adjustement),
                console_color: Some(original.daemon.console_color),
            }),
        };
        let roundtrip: Config = config_opt.into();

        // Verify all fields are preserved
        assert_eq!(roundtrip.clusters.len(), original.clusters.len());
        for (i, cluster) in original.clusters.iter().enumerate() {
            assert_eq!(roundtrip.clusters[i].name, cluster.name);
            assert_eq!(roundtrip.clusters[i].hosts, cluster.hosts);
        }

        assert_eq!(
            roundtrip.client.ssh_config_path,
            original.client.ssh_config_path
        );
        assert_eq!(roundtrip.client.program, original.client.program);
        assert_eq!(roundtrip.client.arguments, original.client.arguments);
        assert_eq!(
            roundtrip.client.username_host_placeholder,
            original.client.username_host_placeholder
        );

        assert_eq!(roundtrip.daemon.height, original.daemon.height);
        assert_eq!(
            roundtrip.daemon.aspect_ratio_adjustement,
            original.daemon.aspect_ratio_adjustement
        );
        assert_eq!(
            roundtrip.daemon.console_color,
            original.daemon.console_color
        );
    }

    #[test]
    fn test_config_with_special_characters() {
        let config = Config {
            clusters: vec![Cluster {
                name: "test-cluster_with.special@chars".to_string(),
                hosts: vec![
                    "host-with-dashes.example.com".to_string(),
                    "host_with_underscores.example.com".to_string(),
                    "192.168.1.100".to_string(),
                ],
            }],
            client: ClientConfig {
                ssh_config_path: "C:\\Program Files\\SSH\\config".to_string(),
                program: "ssh.exe".to_string(),
                arguments: vec![
                    "-o".to_string(),
                    "UserKnownHostsFile=C:\\temp\\known_hosts".to_string(),
                    "{{TARGET}}".to_string(),
                ],
                username_host_placeholder: "{{TARGET}}".to_string(),
            },
            daemon: DaemonConfig::default(),
        };

        // Verify special characters are preserved
        assert_eq!(config.clusters[0].name, "test-cluster_with.special@chars");
        assert!(config.clusters[0]
            .hosts
            .contains(&"192.168.1.100".to_string()));
        assert!(config.client.ssh_config_path.contains("Program Files"));
        assert!(config.client.arguments[1].contains("known_hosts"));
    }
}
