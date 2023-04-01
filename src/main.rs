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

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
    let args = Args::parse();
    spawn_console_process(
        &format!("{PKG_NAME}-daemon.exe"),
        args.hosts.iter().map(|host| -> &str { &host }).collect(),
    );
}
