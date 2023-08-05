use std::{env, process::Command, time::Duration};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    dbg!(&args);
    let mut child = Command::new(&args[1]).args(&args[2..]).spawn().unwrap();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                // TODO: maybe differentiate between exit code 0
                // and errors. For the latter stay alive until a key is pressed
                break;
            }
            Ok(None) => (
                // child is still running
            ),
            Err(e) => panic!("{}", e),
        }
        // Sleep some time to avoid hogging 100% CPU usage.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
