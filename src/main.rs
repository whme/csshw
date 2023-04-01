use clap::Parser;

/// Simple ssh multiplexer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
    let args = Args::parse();

    for host in args.hosts.iter() {
        println!("{:?}", host);
    }
}
