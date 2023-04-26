use clap::Parser;
use dissh::spawn_console_process;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Simple SSH multiplexer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Block until the session is terminted
    #[clap(short, long)]
    block: bool,

    /// Username used to connect to the hosts
    #[clap(short, long)]
    username: Option<String>,

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
    let args = Args::parse();
    let mut daemon_args: Vec<&str> = Vec::new();
    if let Some(username) = args.username.as_ref() {
        daemon_args.push("-u");
        daemon_args.push(&username);
    }
    daemon_args.extend(args.hosts.iter().map(|host| -> &str { &host }));
    spawn_console_process(&format!("{PKG_NAME}-daemon.exe"), daemon_args);
}
