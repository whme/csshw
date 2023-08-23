use serde_derive::{Deserialize, Serialize};
use std::env;

const DEFAULT_USERNAME_HOST_PLACEHOLDER: &str = "{{USERNAME_AT_HOST}}";

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub clusters: Vec<Cluster>,
    pub client: ClientConfig,
    pub daemon: DaemonConfig,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Cluster {
    pub name: String,
    pub hosts: Vec<String>,
}

/// If not present the default config will be written to the default
/// configuration place, under windows this is `%AppData%`
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

#[derive(Serialize, Deserialize, Default)]
pub struct DaemonConfig {}
