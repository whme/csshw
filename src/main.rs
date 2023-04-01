use clap::Parser;

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
    _leader.launch_followers();
}
