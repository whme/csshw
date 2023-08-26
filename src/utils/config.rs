use serde_derive::{Deserialize, Serialize};
use std::env;

const DEFAULT_USERNAME_HOST_PLACEHOLDER: &str = "{{USERNAME_AT_HOST}}";

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub clusters: Vec<Cluster>,
    pub client: ClientConfig,
    pub daemon: DaemonConfig,
}

impl From<Config> for ConfigOpt {
    fn from(val: Config) -> Self {
        return ConfigOpt {
            clusters: Some(val.clusters),
            client: Some(val.client.into()),
            daemon: Some(val.daemon.into()),
        };
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct ConfigOpt {
    pub clusters: Option<Vec<Cluster>>,
    pub client: Option<ClientConfigOpt>,
    pub daemon: Option<DaemonConfigOpt>,
}

impl From<ConfigOpt> for Config {
    fn from(val: ConfigOpt) -> Self {
        return Config {
            clusters: val.clusters.unwrap_or_default(),
            client: val.client.unwrap_or(ClientConfigOpt::default()).into(),
            daemon: val.daemon.unwrap_or(DaemonConfigOpt::default()).into(),
        };
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Cluster {
    pub name: String,
    pub hosts: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ClientConfig {
    /// Full path to the SSH config.
    /// e.g. `'C:\Users\<username>\.ssh\config'`
    pub ssh_config_path: String,
    /// Name of the program used to establish the SSH connection.
    /// e.g. `'ssh'`
    pub program: String,
    /// List of arguments provided to the program.
    /// Must include the `username_host_placeholder`.
    /// e.g. `['-XY' '{{USERNAME_AT_HOST}}']`
    pub arguments: Vec<String>,
    /// Placeholder string used to inject `<user>@<host>` into the list of arguments.
    /// e.g. `'{{USERNAME_AT_HOST}}'`
    pub username_host_placeholder: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        return ClientConfig {
            ssh_config_path: format!("{}\\.ssh\\config", env::var("USERPROFILE").unwrap()),
            program: "ssh".to_string(),
            arguments: vec![
                "-XY".to_string(),
                DEFAULT_USERNAME_HOST_PLACEHOLDER.to_string(),
            ],
            username_host_placeholder: DEFAULT_USERNAME_HOST_PLACEHOLDER.to_string(),
        };
    }
}

impl From<ClientConfig> for ClientConfigOpt {
    fn from(val: ClientConfig) -> Self {
        return ClientConfigOpt {
            ssh_config_path: Some(val.ssh_config_path),
            program: Some(val.program),
            arguments: Some(val.arguments),
            username_host_placeholder: Some(val.username_host_placeholder),
        };
    }
}

#[derive(Serialize, Deserialize)]
pub struct ClientConfigOpt {
    pub ssh_config_path: Option<String>,
    pub program: Option<String>,
    pub arguments: Option<Vec<String>>,
    pub username_host_placeholder: Option<String>,
}

impl Default for ClientConfigOpt {
    fn default() -> Self {
        return ClientConfig::default().into();
    }
}

impl From<ClientConfigOpt> for ClientConfig {
    fn from(val: ClientConfigOpt) -> Self {
        let _default = ClientConfig::default();
        return ClientConfig {
            ssh_config_path: val.ssh_config_path.unwrap_or(_default.ssh_config_path),
            program: val.program.unwrap_or(_default.program),
            arguments: val.arguments.unwrap_or(_default.arguments),
            username_host_placeholder: val
                .username_host_placeholder
                .unwrap_or(_default.username_host_placeholder),
        };
    }
}

#[derive(Serialize, Deserialize)]
pub struct DaemonConfig {
    pub height: i32,
    pub aspect_ratio_adjustement: f64,
}

impl From<DaemonConfig> for DaemonConfigOpt {
    fn from(val: DaemonConfig) -> Self {
        return DaemonConfigOpt {
            height: Some(val.height),
            aspect_ratio_adjustement: Some(val.aspect_ratio_adjustement),
        };
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        return DaemonConfig {
            height: 200,
            aspect_ratio_adjustement: -1f64,
        };
    }
}

#[derive(Serialize, Deserialize)]
pub struct DaemonConfigOpt {
    pub height: Option<i32>,
    pub aspect_ratio_adjustement: Option<f64>,
}

impl Default for DaemonConfigOpt {
    fn default() -> Self {
        return DaemonConfig::default().into();
    }
}

impl From<DaemonConfigOpt> for DaemonConfig {
    fn from(val: DaemonConfigOpt) -> Self {
        let _default = DaemonConfig::default();
        return DaemonConfig {
            height: val.height.unwrap_or(_default.height),
            aspect_ratio_adjustement: val
                .aspect_ratio_adjustement
                .unwrap_or(_default.aspect_ratio_adjustement),
        };
    }
}
