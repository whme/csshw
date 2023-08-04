use std::{env, process::Command};

fn main() {
    let args: Vec<String> = env::args().collect();
    dbg!(&args);
    Command::new(&args[1])
        .args(&args[2..])
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
}
