use crate::config;
use argos::util::TempDir;

#[test]
fn config_vazia_pega_os_padroes() {
    let tmp = TempDir::new("ariadne_config.");
    std::fs::write(tmp.0.join("configs.json"), "{}").unwrap();
    let c = config::load_de(&tmp.0).unwrap();
    assert_eq!(c.assistente, "a Ariadne");
    assert_eq!(c.nome(), "Ariadne");
    assert_eq!(c.fuso, "America/Sao_Paulo");
    assert_eq!(c.cred_dir, tmp.0.join("credenciais").to_string_lossy());
}

#[test]
fn config_sem_arquivo_ou_fuso_de_fora_nao_sobe() {
    let tmp = TempDir::new("ariadne_config.");
    assert!(config::load_de(&tmp.0).is_err());
    std::fs::write(tmp.0.join("configs.json"), r#"{"fuso": "Asia/Tokyo"}"#).unwrap();
    let e = config::load_de(&tmp.0).unwrap_err().to_string();
    assert!(e.contains("Asia/Tokyo"), "{e}");
    std::fs::write(tmp.0.join("configs.json"), r#"{"fuso": "America/Manaus", "assistente": "a Lia", "chave_nova": 1}"#).unwrap();
    let c = config::load_de(&tmp.0).unwrap();
    assert_eq!((c.nome(), c.tz()), ("Lia", chrono_tz::Tz::America__Manaus));
}

// --- membros ---

use crate::membros;

#[test]
fn slug_sem_acento_nem_espaco() {
    assert_eq!(membros::slug_de("  João Pedro "), "joao-pedro");
    assert_eq!(membros::slug_de("Ana-Lú (sócia)"), "ana-lu-socia");
    assert_eq!(membros::slug_de("!!"), "");
    assert_eq!(membros::link("@EquipeBot", "abc"), "https://t.me/EquipeBot?start=abc");
}

#[test]
fn convite_pareia_uma_vez_e_vence() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("ariadne_membros.");
    let t0 = 1_800_000_000;
    let (m, codigo) = membros::cadastrar(&tmp.0, "Fulana de Tal", Some("fulana@exemplo.com"), t0).unwrap();
    assert_eq!((m.slug.as_str(), codigo.len()), ("fulana-de-tal", 12));
    assert!(membros::cadastrar(&tmp.0, "?!", None, t0).is_err());

    assert_eq!(membros::do_convite(&tmp.0, "errado", t0), None);
    assert_eq!(membros::do_convite(&tmp.0, "", t0), None);
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + membros::CONVITE_S + 1), None, "venceu");
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + 60).as_deref(), Some("fulana-de-tal"));
    assert!(membros::convite_pendente(&tmp.0, "fulana-de-tal", t0));

    membros::parear(&tmp.0, "fulana-de-tal", 4242).unwrap();
    let p = membros::pasta(&tmp.0).join("fulana-de-tal");
    assert_eq!(std::fs::metadata(p.join("chat_id")).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + 60), None, "consumido");
    assert!(!membros::convite_pendente(&tmp.0, "fulana-de-tal", t0));

    // recadastrar: convite novo, chat_id e e-mail ficam
    let (_, novo) = membros::cadastrar(&tmp.0, "Fulana de Tal", None, t0).unwrap();
    assert_ne!(novo, codigo);
    let v = membros::listar(&tmp.0).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!((v[0].chat_id, v[0].email.as_deref()), (Some(4242), Some("fulana@exemplo.com")));
}

#[test]
fn um_chat_e_uma_pessoa() {
    let tmp = TempDir::new("ariadne_membros.");
    membros::cadastrar(&tmp.0, "Beltrano", None, 0).unwrap();
    membros::cadastrar(&tmp.0, "Beltrano Silva", None, 0).unwrap();
    membros::parear(&tmp.0, "beltrano", 7).unwrap();
    membros::parear(&tmp.0, "beltrano-silva", 7).unwrap();
    let v = membros::listar(&tmp.0).unwrap();
    assert_eq!(v.iter().map(|m| (m.slug.as_str(), m.chat_id)).collect::<Vec<_>>(), [("beltrano", None), ("beltrano-silva", Some(7))]);
}

// --- roteador + atendentes, contra um Telegram falso ---

use crate::roteador::{self, Roteador};
use serde_json::{json, Value};
use std::sync::mpsc::Receiver;

/// Telegram falso em 127.0.0.1: responde ok a tudo e manda (método, corpo) pro canal
fn telegram_falso() -> (String, Receiver<(String, Value)>) {
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

/// o primeiro pedido que satisfaz `p` (os outros são descartados); 5 s sem ele = falha
fn espera(rx: &Receiver<(String, Value)>, p: impl Fn(&str, &Value) -> bool) -> Value {
    let fim = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let falta = fim.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(falta) {
            Ok((m, c)) if p(&m, &c) => return c,
            Ok(_) => {}
            Err(_) => panic!("pedido esperado não chegou"),
        }
    }
}

fn msg(chat: i64, tipo: &str, texto: &str) -> Value {
    json!({"update_id": 1, "message": {"message_id": 9, "from": {"id": chat}, "chat": {"id": chat, "type": tipo}, "text": texto}})
}

fn enviado_a(chat: i64, trecho: &'static str) -> impl Fn(&str, &Value) -> bool {
    move |m, c| m == "sendMessage" && c["chat_id"] == chat && c["text"].as_str().unwrap_or_default().contains(trecho)
}

#[test]
fn quem_e_codigo_do_start() {
    assert_eq!(roteador::quem(&msg(5, "private", "oi")), Some((5, true)));
    assert_eq!(roteador::quem(&msg(-5, "group", "oi")), Some((-5, false)));
    let botao = json!({"callback_query": {"id": "q", "from": {"id": 5}, "message": {"chat": {"id": 5, "type": "private"}}, "data": "x"}});
    assert_eq!(roteador::quem(&botao), Some((5, true)));
    assert_eq!(roteador::quem(&json!({"edited_message": {}})), None);
    assert_eq!(roteador::codigo_do_start(&msg(5, "private", "/start abc123")), Some("abc123"));
    assert_eq!(roteador::codigo_do_start(&msg(5, "private", "/start")), None);
    assert_eq!(roteador::codigo_do_start(&msg(5, "private", "/startabc")), None);
}

#[test]
fn link_pareia_e_cada_um_tem_o_seu_atendente() {
    let tmp = TempDir::new("ariadne_roteador.");
    std::fs::write(tmp.0.join("configs.json"), "{}").unwrap();
    let agora = argos::util::agora().timestamp();
    membros::cadastrar(&tmp.0, "Fulana Souza", None, agora).unwrap();
    membros::parear(&tmp.0, "fulana-souza", 100).unwrap();
    let (_, codigo) = membros::cadastrar(&tmp.0, "Beltrano", None, agora).unwrap();
    membros::cadastrar(&tmp.0, "Sicrano", None, agora - membros::CONVITE_S - 1).unwrap();

    let (url, rx) = telegram_falso();
    let mut tg = argos::rede::Telegram::new(argos::rede::agente(), "x");
    tg.base = url;
    let mut r = Roteador::new(tmp.0.clone(), config::load_de(&tmp.0).unwrap(), tg).unwrap();
    assert_eq!(r.atendidos(), [100]);
    espera(&rx, |m, c| m == "setMyCommands" && c["scope"] == json!({"type": "chat", "chat_id": 100}));

    r.despacha(&msg(100, "private", "/ping")).unwrap();
    espera(&rx, enviado_a(100, "tô aqui"));

    // estranho, código errado, grupo: nada
    r.despacha(&msg(200, "private", "oi")).unwrap();
    r.despacha(&msg(200, "private", "/start errado")).unwrap();
    r.despacha(&msg(-100, "group", &format!("/start {codigo}"))).unwrap();
    assert_eq!(r.atendidos(), [100]);
    assert!(!membros::pasta(&tmp.0).join("beltrano/chat_id").exists());

    // o link: pareia, sobe o atendente com o menu dele e a saudação sai no chat dele
    r.despacha(&msg(200, "private", &format!("/start {codigo}"))).unwrap();
    assert_eq!(r.atendidos(), [100, 200]);
    espera(&rx, |m, c| m == "setMyCommands" && c["scope"]["chat_id"] == 200);
    espera(&rx, enviado_a(200, "Oi, Beltrano! Aqui é a Ariadne"));
    r.despacha(&msg(200, "private", "/equipe@AlgumBot")).unwrap();
    let t = espera(&rx, enviado_a(200, "Equipe"))["text"].as_str().unwrap().to_string();
    assert!(t.contains("✅ Beltrano (você)") && t.contains("✅ Fulana Souza") && t.contains("➖ Sicrano"), "{t}");
    // o mesmo link de novo não vale pra outro chat
    r.despacha(&msg(300, "private", &format!("/start {codigo}"))).unwrap();
    assert_eq!(r.atendidos(), [100, 200]);

    // Fulana pareia de outro celular: o atendente do chat velho é solto
    let (_, novo) = membros::cadastrar(&tmp.0, "Fulana Souza", None, agora).unwrap();
    r.despacha(&msg(101, "private", &format!("/start {novo}"))).unwrap();
    assert_eq!(r.atendidos(), [101, 200]);
    espera(&rx, enviado_a(101, "Oi, Fulana!"));
    r.despacha(&msg(100, "private", "/ping")).unwrap();
    r.despacha(&msg(101, "private", "texto livre")).unwrap();
    let c = espera(&rx, |m, _| m == "sendMessage");
    assert_eq!((c["chat_id"].as_i64(), c["text"].as_str()), (Some(101), Some("Ainda não sei fazer isso: por enquanto só /ping e /equipe.")));
}
