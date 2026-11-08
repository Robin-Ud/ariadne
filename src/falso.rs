//! Telegram falso em 127.0.0.1, pros testes e o `--selftest`: responde ok a tudo (sendMessage com um
//! `message_id`) e manda (método, corpo) pro canal.

use serde_json::{json, Value};
use std::sync::mpsc::Receiver;

pub fn telegram_falso() -> (String, Receiver<(String, Value)>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut primeira = String::new();
            r.read_line(&mut primeira).unwrap();
            let mut tamanho = 0;
            loop {
                let mut linha = String::new();
                r.read_line(&mut linha).unwrap();
                if linha == "\r\n" {
                    break;
                }
                if let Some(v) = linha.to_ascii_lowercase().strip_prefix("content-length:") {
                    tamanho = v.trim().parse().unwrap();
                }
            }
            let mut corpo = vec![0; tamanho];
            r.read_exact(&mut corpo).unwrap();
            let metodo = primeira.split_whitespace().nth(1).unwrap_or_default().rsplit('/').next().unwrap().to_string();
            let resultado = if metodo == "sendMessage" { json!({"message_id": 1}) } else { json!(true) };
            let _ = tx.send((metodo, serde_json::from_slice(&corpo).unwrap_or_default()));
            let resp = json!({"ok": true, "result": resultado}).to_string();
            write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}", resp.len()).unwrap();
        }
    });
    (url, rx)
}
