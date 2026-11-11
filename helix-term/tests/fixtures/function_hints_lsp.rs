//! Deterministic stdio LSP fixture, compiled by the function-hints integration test.
//! Uses only std so the test does not require Python, Node, or an installed LSP.
use std::{
    fs::OpenOptions,
    io::{self, BufRead, Read, Write},
};

fn main() {
    let log_path = std::env::args_os().nth(1).unwrap();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .unwrap();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
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
        let Some(id_start) = message.find("\"id\":") else {
            continue;
        };
        let id: String = message[id_start + 5..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if id.is_empty() {
            continue;
        }
        let result = if message.contains("\"method\":\"initialize\"") {
            assert!(message.contains("\"hierarchicalDocumentSymbolSupport\":true"));
            r#"{"capabilities":{"documentSymbolProvider":true,"referencesProvider":true,"textDocumentSync":1}}"#.to_owned()
        } else if message.contains("\"method\":\"textDocument/documentSymbol\"") {
            r#"[{"name":"hello","kind":12,"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":12}},"selectionRange":{"start":{"line":0,"character":4},"end":{"line":0,"character":9}}}]"#.to_owned()
        } else if message.contains("\"method\":\"textDocument/references\"") {
            assert!(message.contains("\"includeDeclaration\":false"));
            assert!(message.contains("\"position\":{\"character\":4,\"line\":0}"));
            let uri_start = message.find("\"uri\":\"").unwrap() + 7;
            let uri_end = uri_start + message[uri_start..].find('"').unwrap();
            let uri = &message[uri_start..uri_end];
            // Include a buggy server's declaration and duplicate. The client must
            // still display two distinct usages, including calls in this file.
            let locations: Vec<_> = [(0, 4), (3, 0), (3, 0), (4, 0)].into_iter().map(|(line, col)| {
                format!(r#"{{"uri":"{uri}","range":{{"start":{{"line":{line},"character":{col}}},"end":{{"line":{line},"character":{}}}}}}}"#, col + 5)
            }).collect();
            format!("[{}]", locations.join(","))
        } else {
            "null".to_owned()
        };
        let response = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
        write!(
            output,
            "Content-Length: {}\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        output.flush().unwrap();
    }
}
