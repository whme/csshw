#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use clap::{Parser, Subcommand};
use csshw::client::main as client_main;
use csshw::daemon::main as daemon_main;
use csshw::spawn_console_process;
use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Simple SSH multiplexer
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
}

#[derive(Debug, Subcommand)]
enum Commands {
    Client {
        /// Host to connect to
        host: String,
        /// Username used to connect to the hosts
        username: String,
        /// X coordinates of the upper left corner of the console window
        /// in reference to the upper left corner of the screen
        x: i32,
        /// Y coordinates of the upper left corner of the console window
        /// in reference to the upper left corner of the screen
        y: i32,
        /// Width of the console window
        width: i32,
        /// Height of the console window
        height: i32,
    },
    Daemon {
        /// Username used to connect to the hosts
        #[clap(long, short = 'u')]
        username: Option<String>,

        /// Host(s) to connect to
        hosts: Vec<String>,
    },
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
                println!("Set current working directory to {}", exe_dir.display());
            }
        },
        Err(_) => {
            eprintln!("Failed to get executable directory");
        }
    }

    let args = Args::parse();

    match &args.command {
        Some(Commands::Client {
            host,
            username,
            x,
            y,
            width,
            height,
        }) => {
            client_main(
                host.to_owned(),
                username.to_owned(),
                x.to_owned(),
                y.to_owned(),
                width.to_owned(),
                height.to_owned(),
            )
            .await;
        }
        Some(Commands::Daemon { username, hosts }) => {
            daemon_main(hosts.to_owned(), username.clone()).await;
        }
        None => {
            let mut daemon_args: Vec<&str> = Vec::new();
            daemon_args.push("daemon");
            if let Some(username) = args.username.as_ref() {
                daemon_args.push("-u");
                daemon_args.push(username);
            }
            daemon_args.extend(args.hosts.iter().map(|host| -> &str { return host }));
            spawn_console_process(&format!("{PKG_NAME}.exe"), daemon_args);
        }
    }
}
