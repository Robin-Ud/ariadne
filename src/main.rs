use argos::rede::{agente, Telegram};
use argos::util::R;
use ariadne::{config, membros};
use serde_json::{json, Value};
use std::path::Path;

fn load(dir: &Path) -> R<config::Config> {
    let c = config::load_de(dir)?;
    argos::util::define_fuso(c.tz());
    Ok(c)
}

fn agora_s() -> i64 {
    argos::util::agora().timestamp()
}

/// @ do bot pelo getMe (o `?start=` precisa dele); sem token ou sem rede, None
fn username(c: &config::Config) -> Option<String> {
    let tg = Telegram::new(agente(), &c.credencial("telegram_token").ok()?);
    match tg.call("getMe", 20, json!({})) {
        Ok(me) => me.get("username").and_then(Value::as_str).map(String::from),
        Err(e) => {
            println!("getMe: {e}");
            None
        }
    }
}

/// ariadne --membro "Nome" [--email x]: cria o membro e imprime o link do convite
fn cli_membro(nome: &str, email: Option<&str>) -> R<()> {
    let dir = config::data_dir();
    let c = load(&dir)?;
    let (m, codigo) = membros::cadastrar(&dir, nome, email, agora_s())?;
    let dias = membros::CONVITE_S / 86400;
    match username(&c) {
        Some(u) => println!("{} ({}): mande este link, vale {dias} dias e uma vez só:\n{}", m.nome, m.slug, membros::link(&u, &codigo)),
        None => println!("{} ({}): sem o @ do bot (falta credenciais/telegram_token?). O link é\nhttps://t.me/<bot>?start={codigo}\n(vale {dias} dias e uma vez só)", m.nome, m.slug),
    }
    if m.chat_id.is_some() {
        println!("já estava pareado: o chat atual vale até alguém tocar no link novo");
    }
    Ok(())
}

fn uso() -> ! {
    eprintln!("uso: ariadne --membro \"Nome\" [--email x]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let r: R<()> = match a.as_slice() {
        ["--membro", nome] => cli_membro(nome, None),
        ["--membro", nome, "--email", e] => cli_membro(nome, Some(e)),
        _ => uso(),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
