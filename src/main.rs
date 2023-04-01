use clap::Parser;
use std::{thread, time};

mod leader;

/// Simple SSH multiplexer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short = 'd', long, default_value = "ubuntu")]
    wsl_distro: String,

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
    let args = Args::parse();
    let _leader = leader::Leader {
        hosts: args.hosts,
        wsl_distro: args.wsl_distro,
    };
    unsafe {
        _leader.test_window();
    }
    println!("Test 1 two 3");
    thread::sleep(time::Duration::from_millis(10000));
}
