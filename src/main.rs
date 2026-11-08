use argos::rede::{agente, Telegram};
use argos::util::R;
use ariadne::{config, membros, roteador::Roteador};
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

/// ariadne (sem argumento): roteador + um atendente por membro
fn cli_rodar() -> R<()> {
    let dir = config::data_dir();
    let c = load(&dir)?;
    let tg = Telegram::new(agente(), &c.credencial("telegram_token")?);
    Roteador::new(dir, c, tg)?.run()
}

/// ariadne --checar-config: carrega o configs.json, testa o token (getMe, grátis) e lista os membros.
/// Rodar antes de reiniciar com binário novo.
fn cli_checar_config() -> R<()> {
    let dir = config::data_dir();
    let c = load(&dir)?;
    println!("pasta: {}\nassistente: {} · fuso: {}", dir.display(), c.assistente, c.fuso);
    let token = c.credencial("telegram_token")?;
    let me = Telegram::new(agente(), &token).call("getMe", 20, json!({}))?;
    println!("bot: @{}", me.get("username").and_then(Value::as_str).unwrap_or("?"));
    let agora = agora_s();
    let lista = membros::listar(&dir)?;
    println!("membros: {}", lista.len());
    for m in &lista {
        let estado = match m.chat_id {
            Some(_) => "pareado",
            None if membros::convite_pendente(&dir, &m.slug, agora) => "convite mandado",
            None => "sem convite (ariadne --membro)",
        };
        println!("  {} ({}): {estado}{}", m.nome, m.slug, m.email.as_deref().map(|e| format!(" · {e}")).unwrap_or_default());
    }
    if c.credencial("anthropic_key").is_err() {
        println!("anthropic_key: falta (só precisa a partir dos recados e tarefas)");
    }
    println!("ok");
    Ok(())
}

/// ariadne --selftest: 3 membros numa pasta temporária contra o Telegram falso em 127.0.0.1,
/// N updates por membro, e o pico de memória (VmHWM)
fn cli_selftest() -> R<()> {
    use argos::util::{memoria, TempDir};
    const POR_MEMBRO: usize = 200;
    let tmp = TempDir::new("ariadne_selftest.");
    std::fs::write(tmp.0.join("configs.json"), "{}")?;
    let agora = agora_s();
    let chats = [1001, 1002, 1003];
    for (i, nome) in ["Fulana Teste", "Beltrano Teste", "Sicrano Teste"].iter().enumerate() {
        let (m, _) = membros::cadastrar(&tmp.0, nome, None, agora)?;
        membros::parear(&tmp.0, &m.slug, chats[i])?;
    }
    println!("início:       {}", memoria());
    let (url, rx) = ariadne::falso::telegram_falso();
    let mut tg = Telegram::new(agente(), "0:falso");
    tg.base = url;
    let mut r = Roteador::new(tmp.0.clone(), load(&tmp.0)?, tg)?;
    println!("3 atendentes: {}", memoria());
    for n in 0..POR_MEMBRO {
        for chat in chats {
            let texto = if n % 2 == 0 { "/ping" } else { "/equipe" };
            r.despacha(&json!({"update_id": n, "message": {"message_id": n, "from": {"id": chat}, "chat": {"id": chat, "type": "private"}, "text": texto}}))?;
        }
    }
    let total = POR_MEMBRO * chats.len();
    let mut respostas = 0;
    while respostas < total {
        let (metodo, _) = rx.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| argos::util::Erro::Msg(format!("só {respostas} de {total} respostas")))?;
        respostas += usize::from(metodo == "sendMessage");
    }
    println!("{total} respostas: {}", memoria());
    Ok(())
}

fn uso() -> ! {
    eprintln!("uso: ariadne                               roda\n      ariadne --membro \"Nome\" [--email x]     cadastra e imprime o link do convite\n      ariadne --checar-config                     valida config e token, lista os membros\n      ariadne --selftest                          3 membros contra um Telegram falso, pico de memória");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let r: R<()> = match a.as_slice() {
        [] => cli_rodar(),
        ["--checar-config"] => cli_checar_config(),
        ["--selftest"] => cli_selftest(),
        ["--membro", nome] => cli_membro(nome, None),
        ["--membro", nome, "--email", e] => cli_membro(nome, Some(e)),
        _ => uso(),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
