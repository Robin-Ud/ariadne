//! Telegram e modelo falsos em 127.0.0.1, pros testes e o `--selftest`. O Telegram responde ok a tudo
//! (sendMessage com `message_id` crescente) e manda (método, corpo) pro canal; o modelo responde uma
//! fila de respostas prontas e manda cada pedido pro canal.

use serde_json::{json, Value};
use std::sync::mpsc::Receiver;

pub fn telegram_falso() -> (String, Receiver<(String, Value)>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut id = 0;
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
            id += 1;
            let resultado = if metodo == "sendMessage" { json!({"message_id": id}) } else { json!(true) };
            let _ = tx.send((metodo, serde_json::from_slice(&corpo).unwrap_or_default()));
            let resp = json!({"ok": true, "result": resultado}).to_string();
            write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}", resp.len()).unwrap();
        }
    });
    (url, rx)
}

/// modelo falso: cada pedido recebe a próxima resposta da fila (corpo da Messages API, status 200);
/// acabou a fila, a conexão fecha e a chamada dá erro. URL pronta pro `Claude::url`
pub fn claude_falso(respostas: Vec<Value>) -> (String, Receiver<Value>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/messages", l.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for corpo in respostas {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
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
            let mut b = vec![0; tamanho];
            r.read_exact(&mut b).unwrap();
            let _ = tx.send(serde_json::from_slice(&b).unwrap_or_default());
            let c = corpo.to_string();
            write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{c}", c.len()).unwrap();
        }
    });
    (url, rx)
}

/// Google falso: `/token` sempre dá um access token; o resto recebe a próxima resposta da fila (acabou,
/// 500) e manda ("MÉTODO /caminho?query", corpo) pro canal. A URL serve de `GoogleCalendar::base` e,
/// com `/token`, de `token_uri` no `google_client.json` do teste
pub fn google_falso(respostas: Vec<Value>) -> (String, Receiver<(String, Value)>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut fila = respostas.into_iter();
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut primeira = String::new();
            r.read_line(&mut primeira).unwrap();
            let mut tamanho = 0;
            loop {
                let mut linha = String::new();
                r.read_line(&mut linha).unwrap();
                if linha == "\r\n" || linha.is_empty() {
                    break;
                }
                if let Some(v) = linha.to_ascii_lowercase().strip_prefix("content-length:") {
                    tamanho = v.trim().parse().unwrap();
                }
            }
            let mut corpo = vec![0; tamanho];
            r.read_exact(&mut corpo).unwrap();
            let pedido: Vec<&str> = primeira.split_whitespace().take(2).collect();
            let pedido = pedido.join(" ");
            let (status, resp) = if pedido.starts_with("POST /token") {
                (200, json!({"access_token": "tok", "expires_in": 3600}))
            } else {
                let _ = tx.send((pedido, serde_json::from_slice(&corpo).unwrap_or_default()));
                fila.next().map(|v| (200, v)).unwrap_or((500, json!({"error": "fila vazia"})))
            };
            let c = resp.to_string();
            let _ = write!(s, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{c}", c.len());
        }
    });
    (url, rx)
}
