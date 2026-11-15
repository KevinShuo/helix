//! Lightweight real child process for model lifecycle tests. No model/GPU needed.
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    time::Duration,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    let arg = |name: &str| args.windows(2).find(|pair| pair[0] == name).unwrap()[1].clone();
    let model = PathBuf::from(arg("--model"));
    let options = fs::read_to_string(&model).unwrap();
    let option = |name: &str| {
        options
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or("0")
            .parse::<u64>()
            .unwrap()
    };
    let peer = option("peer=");
    if peer != 0
        && TcpStream::connect_timeout(
            &format!("127.0.0.1:{peer}").parse().unwrap(),
            Duration::from_millis(100),
        )
        .is_ok()
    {
        eprintln!("resident process still owns its listening port");
        std::process::exit(2);
    }
    let listener = TcpListener::bind(format!("127.0.0.1:{}", arg("--port"))).unwrap();
    let mut events = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(model.with_extension("started"))
        .unwrap();
    writeln!(events, "{}", std::process::id()).unwrap();
    drop(events);
    std::thread::sleep(Duration::from_millis(option("delay=")));
    let alias = arg("--alias");
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let n = stream.read(&mut buffer).unwrap_or(0);
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let body = format!(r#"{{"data":[{{"id":"{alias}"}}]}}"#);
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    }
}
