#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]

use clap::{ArgAction, Parser, Subcommand};
use csshw::client::main as client_main;
use csshw::daemon::main as daemon_main;
use csshw::utils::config::{Cluster, Config, ConfigOpt};
use csshw::{
    get_concole_window_handle, init_logger, spawn_console_process,
    WindowsSettingsDefaultTerminalApplicationGuard,
};
use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Cluster SSH tool for Windows inspired by csshX
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Option<Commands>,
    /// Username used to connect to the hosts
    #[clap(short, long)]
    username: Option<String>,
    /// Hosts to connect to
    #[clap(required = false)]
    hosts: Vec<String>,
    /// Enable extensive logging
    #[clap(short, long, action=ArgAction::SetTrue)]
    debug: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Client {
        /// Host to connect to
        host: String,
        /// Username used to connect to the hosts
        username: String,
    },
    Daemon {
        /// Username used to connect to the hosts
        #[clap(long, short = 'u')]
        username: Option<String>,

        /// Host(s) to connect to
        hosts: Vec<String>,
    },
}

fn resolve_cluster_tags<'a>(hosts: Vec<&'a str>, clusters: &'a Vec<Cluster>) -> Vec<&'a str> {
    let mut resolved_hosts: Vec<&str> = Vec::new();
    let mut is_cluster_tag: bool;
    for host in hosts {
        is_cluster_tag = false;
        for cluster in clusters {
            if host == cluster.name {
                is_cluster_tag = true;
                resolved_hosts.extend(resolve_cluster_tags(
                    cluster.hosts.iter().map(|host| return &**host).collect(),
                    clusters,
                ));
                break;
            }
        }
        if !is_cluster_tag {
            resolved_hosts.push(host);
        }
    }
    return resolved_hosts;
}

#[tokio::main]
async fn main() {
    unsafe {
        LoadImageW(
            GetModuleHandleW(None).unwrap(),
            PCWSTR(1 as _), // Value must match the `nameID` in the .rc script
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )
        .unwrap()
    };

    match std::env::current_exe() {
        Ok(path) => match path.parent() {
            None => {
                eprintln!("Failed to get executable path parent working directory");
            }
            Some(exe_dir) => {
                std::env::set_current_dir(exe_dir)
                    .expect("Failed to change current working directory");
            }
        },
        Err(_) => {
            eprintln!("Failed to get executable directory");
        }
    }

    let args = Args::parse();

    let config_path = format!("{PKG_NAME}-config.toml");
    let config_on_disk: ConfigOpt = confy::load_path(&config_path).unwrap();
    let config: Config = config_on_disk.into();

    match &args.command {
        Some(Commands::Client { host, username }) => {
            if args.debug {
                init_logger(&format!("csshw_client_{host}"));
            }
            client_main(host.to_owned(), username.to_owned(), &config.client).await;
        }
        Some(Commands::Daemon { username, hosts }) => {
            if args.debug {
                init_logger("csshw_daemon");
            }
            daemon_main(
                hosts.to_owned(),
                username.clone(),
                &config.daemon,
                args.debug,
            )
            .await;
        }
        None => {
            confy::store_path(&config_path, &config).unwrap();

            let mut daemon_args: Vec<&str> = Vec::new();
            if args.debug {
                daemon_args.push("-d");
            }
            daemon_args.push("daemon");
            if let Some(username) = args.username.as_ref() {
                daemon_args.push("-u");
                daemon_args.push(username);
            }
            daemon_args.extend(resolve_cluster_tags(
                args.hosts.iter().map(|host| return &**host).collect(),
                &config.clusters,
            ));
            let _guard = WindowsSettingsDefaultTerminalApplicationGuard::new();
            // We must wait for the window to actually launch before dropping the _guard as we might otherwise
            // reset the configuration before the window was launched
            let _ = get_concole_window_handle(
                spawn_console_process(&format!("{PKG_NAME}.exe"), daemon_args).dwProcessId,
            );
        }
    }
}
