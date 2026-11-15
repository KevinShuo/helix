//! Standalone std-only process for build/navigation integration tests.
use std::{
    io::{self, Write},
    time::Duration,
};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let mode = &args[1];
    let path = &args[2];
    match mode.as_str() {
        "emit" => {
            print!("{path}:2:");
            io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_millis(40));
            println!("2: error: first streamed error");
            io::stdout().flush().unwrap();
            eprintln!("{path}:3:1: warning: stderr warning");
            std::thread::sleep(Duration::from_millis(1200));
            print!("{path}:4:1: error: final unterminated error");
            io::stdout().flush().unwrap();
            std::process::exit(3);
        }
        "hold" => {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["heartbeat", path])
                .spawn()
                .unwrap();
            let _ = child.wait();
        }
        "heartbeat" => {
            for i in 0..200 {
                std::fs::write(path, i.to_string()).unwrap();
                println!("heartbeat {i}");
                io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "detach" => {
            // The shell exits while a descendant still owns inherited pipes.
            let _ = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["heartbeat", path])
                .spawn()
                .unwrap();
        }
        "flood" => {
            println!("{}", "x".repeat(100_000));
            for i in 0..20_000 {
                println!("{path}:2:1: warning: warning {i}");
            }
        }
        _ => panic!("unknown fixture mode"),
    }
}
