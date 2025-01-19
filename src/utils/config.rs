//! Client and Daemon configuration structs.

use serde_derive::{Deserialize, Serialize};
use std::env;
use windows::Win32::System::Console::{
    BACKGROUND_INTENSITY, BACKGROUND_RED, FOREGROUND_BLUE, FOREGROUND_GREEN, FOREGROUND_INTENSITY,
    FOREGROUND_RED,
};

/// Placeholder for the `<username>@<host>` argument to the chosen SSH program.
const DEFAULT_USERNAME_HOST_PLACEHOLDER: &str = "{{USERNAME_AT_HOST}}";

/// Representation of the project configuration.
///
/// Includes subcommand specific configurations for `client` and `daemon` subcommands
/// as well es the cluster tags.
#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    /// List of cluster tags.
    ///
    /// Includes the name of the cluster tag and a list of hostnames.
    pub clusters: Vec<Cluster>,
    /// Configuration relevant for the `client` subcommand.
    pub client: ClientConfig,
    /// Configuration relevant for the `daemon` subcommand.
    pub daemon: DaemonConfig,
}

/// Representation of the project configuration
/// where everything is optional.
///
/// Used to handle cases where only some or none of the configurations are present.
/// Enables backwards compatiblity with configuration files written by older versions.
#[derive(Serialize, Deserialize, Default)]
pub struct ConfigOpt {
    #[allow(missing_docs)]
    pub clusters: Option<Vec<Cluster>>,
    #[allow(missing_docs)]
    pub client: Option<ClientConfigOpt>,
    #[allow(missing_docs)]
    pub daemon: Option<DaemonConfigOpt>,
}

impl From<ConfigOpt> for Config {
    /// Unwraps the existing configuration values or applies the default.
    fn from(val: ConfigOpt) -> Self {
        return Config {
            clusters: val.clusters.unwrap_or_default(),
            client: val.client.unwrap_or_default().into(),
            daemon: val.daemon.unwrap_or_default().into(),
        };
    }
}

impl From<Config> for ConfigOpt {
    /// Wraps all configuration values as options.
    fn from(val: Config) -> Self {
        return ConfigOpt {
            clusters: Some(val.clusters),
            client: Some(val.client.into()),
            daemon: Some(val.daemon.into()),
        };
    }
}

/// Representation of a cluster tag.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Cluster {
    /// Name of the cluster tag, used to identify it.
    pub name: String,
    /// List of hostnames the cluster tag is an alias for.
    pub hosts: Vec<String>,
}

/// Representation of the `client` subcommand configurations.
#[derive(Serialize, Deserialize)]
pub struct ClientConfig {
    /// Full path to the SSH config.
    ///
    /// # Example
    ///
    /// `'C:\Users\<username>\.ssh\config'`
    pub ssh_config_path: String,
    /// Name of the program used to establish the SSH connection.
    /// # Example
    ///
    /// `'ssh'`
    pub program: String,
    /// List of arguments provided to the program.
    ///
    /// Must include the `username_host_placeholder`.
    ///
    /// # Example
    ///
    /// `['-XY', '{{USERNAME_AT_HOST}}']`
    pub arguments: Vec<String>,
    /// Placeholder string used to inject `<user>@<host>` into the list of arguments.
    ///
    /// # Example
    ///
    /// `'{{USERNAME_AT_HOST}}'`
    pub username_host_placeholder: String,
}

impl Default for ClientConfig {
    /// Returns a sensible default `ClientConfig`.
    ///
    /// # Returns
    ///
    /// `ClientConfig` with the following values:
    /// * `ssh_config_path`             - `%USERPROFILE%\.ssh\config`
    /// * `program`                     - `ssh`
    /// * `arguments`                   - `-XY {{USERNAME_AT_HOST}}`
    /// * `usernamt_host_placeholder`   - `{{USERNAME_AT_HOST}}`
    ///
    /// Note: %USERPROFILE% actually is resolved by us, so the actual value
    ///       is whatever the environment variable at runtime points to.
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

/// Representation of the `client` subcommand configurations
/// where everything is optional.
#[derive(Serialize, Deserialize)]
pub struct ClientConfigOpt {
    #[allow(missing_docs)]
    pub ssh_config_path: Option<String>,
    #[allow(missing_docs)]
    pub program: Option<String>,
    #[allow(missing_docs)]
    pub arguments: Option<Vec<String>>,
    #[allow(missing_docs)]
    pub username_host_placeholder: Option<String>,
}

impl Default for ClientConfigOpt {
    fn default() -> Self {
        return ClientConfig::default().into();
    }
}

impl From<ClientConfigOpt> for ClientConfig {
    /// Unwraps the existing configuration values or applies the default.
    fn from(val: ClientConfigOpt) -> Self {
        let default = ClientConfig::default();
        return ClientConfig {
            ssh_config_path: val.ssh_config_path.unwrap_or(default.ssh_config_path),
            program: val.program.unwrap_or(default.program),
            arguments: val.arguments.unwrap_or(default.arguments),
            username_host_placeholder: val
                .username_host_placeholder
                .unwrap_or(default.username_host_placeholder),
        };
    }
}

impl From<ClientConfig> for ClientConfigOpt {
    /// Wraps all configuration values as options.
    fn from(val: ClientConfig) -> Self {
        return ClientConfigOpt {
            ssh_config_path: Some(val.ssh_config_path),
            program: Some(val.program),
            arguments: Some(val.arguments),
            username_host_placeholder: Some(val.username_host_placeholder),
        };
    }
}

/// Representation of the `daemon` subcommand configurations.
#[derive(Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Height in pixel of the daemon console window.
    ///
    /// Note: we are [DPI Unaware][1] which means the number of pixels
    ///       represents the `logical` scale, not the physical.
    ///
    /// [1]: https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows#dpi-unaware
    pub height: i32,
    /// Controls how the client console windows make use of the available screen space.
    ///
    /// * `> 0.0` - Aims for vertical rectangle shape.
    ///             The larger the value, the more exaggerated the "verticality".
    ///             Eventually the windows will all be columns.
    /// * `= 0.0` - Aims for square shape.
    /// * `< 0.0` - Aims for horizontal rectangle shape.
    ///             The smaller the value, the more exaggerated the "horizontality".
    ///             Eventually the windows will all be rows.
    ///             `-1.0` is the sweetspot for mostly preserving a 16:9 ratio.
    pub aspect_ratio_adjustement: f64,
    /// Controls back- and foreground colors of the daemon console window.
    ///
    /// All [standard windows color combinations][1] are available:
    ///
    /// FOREGROUND_BLUE:        1   \
    /// FOREGROUND_GREEN:       2   \
    /// FOREGROUND_RED:         4   \
    /// FOREGROUND_INTENSITY:   8   \
    /// BACKGROUND_BLUE:        16  \
    /// BACKGROUND_GREEN:       32  \
    /// BACKGROUND_RED:         64  \
    /// BACKGROUND_INTENSITY:   128 \
    ///
    /// # Example
    ///
    /// White font on red background: 8 + 4 + 2 + 1 + 128 + 64 = `207`
    ///
    /// [1]: https://learn.microsoft.com/en-us/windows/console/console-screen-buffers#character-attributes
    pub console_color: u16,
}

impl Default for DaemonConfig {
    /// Returns a sensible default `DaemonConfig`.
    ///
    /// # Returns
    ///
    /// `DaemonConfig` with the following values:
    /// * `height`                      - `200`
    /// * `aspect_ratio_adjustment`    - `-1.0`
    /// * `console_color`               - `207`
    fn default() -> Self {
        return DaemonConfig {
            height: 200,
            aspect_ratio_adjustement: -1f64,
            console_color: (FOREGROUND_INTENSITY
                | FOREGROUND_RED
                | FOREGROUND_GREEN
                | FOREGROUND_BLUE
                | BACKGROUND_INTENSITY
                | BACKGROUND_RED)
                .0,
        };
    }
}

/// Representation of the `daemon` subcommand configurations
/// where everything is optional.
#[derive(Serialize, Deserialize)]
pub struct DaemonConfigOpt {
    #[allow(missing_docs)]
    pub height: Option<i32>,
    #[allow(missing_docs)]
    pub aspect_ratio_adjustement: Option<f64>,
    #[allow(missing_docs)]
    pub console_color: Option<u16>,
}

impl Default for DaemonConfigOpt {
    fn default() -> Self {
        return DaemonConfig::default().into();
    }
}

impl From<DaemonConfigOpt> for DaemonConfig {
    /// Unwraps the existing configuration values or applies the default.
    fn from(val: DaemonConfigOpt) -> Self {
        let default = DaemonConfig::default();
        return DaemonConfig {
            height: val.height.unwrap_or(default.height),
            aspect_ratio_adjustement: val
                .aspect_ratio_adjustement
                .unwrap_or(default.aspect_ratio_adjustement),
            console_color: val.console_color.unwrap_or(default.console_color),
        };
    }
}

impl From<DaemonConfig> for DaemonConfigOpt {
    /// Wraps all configuration values as options.
    fn from(val: DaemonConfig) -> Self {
        return DaemonConfigOpt {
            height: Some(val.height),
            aspect_ratio_adjustement: Some(val.aspect_ratio_adjustement),
            console_color: Some(val.console_color),
        };
    }
}
