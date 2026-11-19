//! Holds pull diagnostics until shutdown to reproduce replies racing a restart.
use std::{
    fs::OpenOptions,
    io::{self, BufRead, Read, Write},
};

fn send(output: &mut impl Write, id: &str, result: &str) {
    let response = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
    write!(
        output,
        "Content-Length: {}\r\n\r\n{response}",
        response.len()
    )
    .unwrap();
    output.flush().unwrap();
}

fn main() {
    let complete = std::env::args_os()
        .nth(2)
        .is_some_and(|arg| arg == "completion");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::args_os().nth(1).unwrap())
        .unwrap();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut pending = Vec::new();
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            if input.read_line(&mut header).unwrap() == 0 {
                return;
            }
            if header.trim().is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let mut message = vec![0; length.unwrap()];
        input.read_exact(&mut message).unwrap();
        let message = String::from_utf8(message).unwrap();
        writeln!(log, "{message}").unwrap();
        log.flush().unwrap();
        if message.contains("\"method\":\"exit\"") {
            return;
        }
        let Some(start) = message.find("\"id\":") else {
            continue;
        };
        let id: String = message[start + 5..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if id.is_empty() {
            continue;
        }
        if complete && message.contains("\"method\":\"initialize\"") {
            send(
                &mut output,
                &id,
                r#"{"capabilities":{"completionProvider":{},"textDocumentSync":1}}"#,
            );
        } else if complete && message.contains("\"method\":\"textDocument/completion\"") {
            send(&mut output, &id, r#"[{"label":"prefix-lsp"}]"#);
        } else if message.contains("\"method\":\"initialize\"") {
            send(
                &mut output,
                &id,
                r#"{"capabilities":{"textDocumentSync":1,"diagnosticProvider":{"interFileDependencies":false,"workspaceDiagnostics":false}}}"#,
            );
        } else if message.contains("\"method\":\"textDocument/diagnostic\"") {
            pending.push(id);
        } else if message.contains("\"method\":\"shutdown\"") {
            for id in pending.drain(..) {
                send(
                    &mut output,
                    &id,
                    r#"{"kind":"full","resultId":"retired-result","items":[{"range":{"start":{"line":0,"character":13},"end":{"line":0,"character":14}},"message":"late retired error","severity":1}]}"#,
                );
            }
            send(&mut output, &id, "null");
        } else {
            send(&mut output, &id, "null");
        }
    }
}
