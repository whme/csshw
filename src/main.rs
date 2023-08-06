#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use clap::Parser;
use csshw::spawn_console_process;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Simple SSH multiplexer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Username used to connect to the hosts
    #[clap(short, long)]
    username: Option<String>,

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
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
    let mut daemon_args: Vec<&str> = Vec::new();
    if let Some(username) = args.username.as_ref() {
        daemon_args.push("-u");
        daemon_args.push(username);
    }
    daemon_args.extend(args.hosts.iter().map(|host| -> &str { return host }));
    spawn_console_process(&format!("{PKG_NAME}-daemon.exe"), daemon_args);
}
