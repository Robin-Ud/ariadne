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
use crate::falso::telegram_falso;
use std::sync::mpsc::Receiver;

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

/// nenhuma mensagem (sendMessage) que satisfaça `p` em `ms` milissegundos; o resto (menu, botões) passa
fn nada(rx: &Receiver<(String, Value)>, ms: u64, p: impl Fn(&Value) -> bool) {
    let fim = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while let Ok((m, c)) = rx.recv_timeout(fim.saturating_duration_since(std::time::Instant::now())) {
        assert!(!(m == "sendMessage" && p(&c)), "não devia ter saído: {c}");
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
    assert_eq!(c["chat_id"].as_i64(), Some(101));
    assert!(c["text"].as_str().unwrap().contains("Ainda não tenho a chave do modelo"), "{c}");
}

// --- conversa com o modelo (falso) ---

use crate::falso::claude_falso;

/// Fulana (chat 100) e Beltrano (chat 200) pareados, chave falsa, Telegram falso e o modelo com `respostas`
fn time_de_teste(respostas: Vec<Value>) -> (TempDir, Roteador, Receiver<(String, Value)>, Receiver<Value>) {
    let tmp = TempDir::new("ariadne_time.");
    std::fs::write(tmp.0.join("configs.json"), "{}").unwrap();
    std::fs::create_dir_all(tmp.0.join("credenciais")).unwrap();
    std::fs::write(tmp.0.join("credenciais/anthropic_key"), "sk-falsa").unwrap();
    for (nome, chat) in [("Fulana Souza", 100), ("Beltrano", 200)] {
        let (m, _) = membros::cadastrar(&tmp.0, nome, None, 0).unwrap();
        membros::parear(&tmp.0, &m.slug, chat).unwrap();
    }
    let (url, rx) = telegram_falso();
    let mut tg = argos::rede::Telegram::new(argos::rede::agente(), "x");
    tg.base = url;
    let mut r = Roteador::new(tmp.0.clone(), config::load_de(&tmp.0).unwrap(), tg).unwrap();
    let (curl, crx) = claude_falso(respostas);
    // os atendentes sobem de novo com a URL do modelo falso
    r.claude_url = Some(curl);
    r.solta_todos();
    r.recarrega().unwrap();
    (tmp, r, rx, crx)
}

fn usa(id: &str, nome: &str, input: Value) -> Value {
    json!({"stop_reason": "tool_use", "content": [{"type": "tool_use", "id": id, "name": nome, "input": input}]})
}

fn diz(texto: &str) -> Value {
    json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": texto}]})
}

#[test]
fn texto_livre_vai_pro_modelo_com_as_ferramentas() {
    let (_tmp, mut r, rx, crx) = time_de_teste(vec![usa("t1", "ver_tarefas", json!({"filtro": "minhas"})), diz("Nada aberto, Fulana.")]);
    r.despacha(&msg(100, "private", "o que tenho pra fazer?")).unwrap();
    espera(&rx, enviado_a(100, "Nada aberto, Fulana."));
    let p1 = crx.recv().unwrap();
    let system = p1["system"][0]["text"].as_str().unwrap();
    assert!(system.contains("- Fulana Souza (`fulana-souza`)") && system.contains("- Beltrano (`beltrano`)"), "{system}");
    assert!(!system.contains("falando com: Fulana"), "o que muda não vai no system (cache)");
    let turno = p1["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert!(turno.contains("[falando com: Fulana Souza (`fulana-souza`)]") && turno.ends_with("\no que tenho pra fazer?"), "{turno}");
    assert_eq!(p1["messages"][0]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(p1["tools"][0]["name"], "ver_tarefas");
    let p2 = crx.recv().unwrap();
    assert_eq!(p2["messages"][2]["content"][0], json!({"type": "tool_result", "tool_use_id": "t1", "content": "nenhuma tarefa aberta nesse filtro", "is_error": false, "cache_control": {"type": "ephemeral"}}));
}

#[test]
fn ferramenta_com_erro_volta_pro_modelo() {
    let (_tmp, mut r, rx, crx) = time_de_teste(vec![usa("t1", "ver_tarefas", json!({"filtro": "de:ninguem"})), diz("Não achei essa pessoa.")]);
    r.despacha(&msg(200, "private", "e o ninguém?")).unwrap();
    espera(&rx, enviado_a(200, "Não achei essa pessoa."));
    crx.recv().unwrap();
    let r2 = &crx.recv().unwrap()["messages"][2]["content"][0];
    assert_eq!((r2["is_error"].as_bool(), r2["content"].as_str()), (Some(true), Some("ValueError: \"ninguem\" não é da equipe")));
}

#[test]
fn config_janela_e_cobranca_validadas() {
    let tmp = TempDir::new("ariadne_config.");
    let c = {
        std::fs::write(tmp.0.join("configs.json"), "{}").unwrap();
        config::load_de(&tmp.0).unwrap()
    };
    assert_eq!((c.modelo.as_str(), c.cobrar_a_cada_h, c.escalar_apos), ("claude-haiku-5-5", 3.0, 3));
    assert_eq!(c.janela_cobranca, ("08:00".to_string(), "20:00".to_string()));
    for ruim in [r#"{"janela_cobranca": ["20:00", "08:00"]}"#, r#"{"janela_cobranca": ["8h", "20:00"]}"#, r#"{"cobrar_a_cada_h": 0}"#] {
        std::fs::write(tmp.0.join("configs.json"), ruim).unwrap();
        assert!(config::load_de(&tmp.0).is_err(), "{ruim}");
    }
}

// --- estado da equipe ---

use crate::estado::{self, Fecho, Filtro};

fn dt(s: &str) -> argos::util::Dt {
    argos::util::from_iso(s).unwrap()
}

fn cfg() -> argos::tarefas::CfgCobranca {
    config::Config::default().cfg_cobranca()
}

#[test]
fn estado_vai_e_volta_com_chave_desconhecida() {
    let tmp = TempDir::new("ariadne_estado.");
    let arq = estado::arquivo(&tmp.0);
    assert_eq!(estado::carregar(&arq).unwrap().proximo_id, 1, "sem arquivo: vazio");
    let mut e = estado::Estado::default();
    let now = dt("2026-11-09T10:00:00-03:00");
    let mut t = estado::nova_tarefa(&mut e, "fulana", "beltrano", " mandar a proposta ", None, &now, &cfg()).unwrap();
    e.tarefas[0].resto.insert("campo_futuro".into(), json!(1));
    e.resto.insert("chave_futura".into(), json!("x"));
    estado::gravar(&e, &arq).unwrap();
    let lido = estado::carregar(&arq).unwrap();
    t.resto.insert("campo_futuro".into(), json!(1));
    assert_eq!(lido.tarefas, [t]);
    assert_eq!((lido.proximo_id, &lido.resto["chave_futura"]), (2, &json!("x")));
    assert!(!arq.with_extension("json.tmp").exists());

    std::fs::write(&arq, "{quebrado").unwrap();
    assert!(estado::carregar(&arq).is_err(), "JSON quebrado não sobe");
}

#[test]
fn tarefa_sem_prazo_ja_cobra_e_com_prazo_espera_o_dia() {
    let mut e = estado::Estado::default();
    let now = dt("2026-11-09T10:00:00-03:00");
    let t = estado::nova_tarefa(&mut e, "fulana", "beltrano", "proposta", None, &now, &cfg()).unwrap();
    assert!(t.pendente);
    assert_eq!((t.texto.as_str(), estado::de(&t), estado::para(&t)), ("proposta", "fulana", "beltrano"));
    assert_eq!(t.cobrar_em.as_deref(), Some("2026-11-09T13:00:00-03:00"), "uma espera (3 h) depois");
    // 19h + 3h passa da janela (até 20h): a primeira cobrança vai pra manhã seguinte
    let t = estado::nova_tarefa(&mut e, "fulana", "beltrano", "x", None, &dt("2026-11-09T19:00:00-03:00"), &cfg()).unwrap();
    assert_eq!(t.cobrar_em.as_deref(), Some("2026-11-10T08:00:00-03:00"));

    let prazo = estado::le_quando("2026-11-13").unwrap();
    let t = estado::nova_tarefa(&mut e, "fulana", "fulana", "relatório", Some(prazo), &now, &cfg()).unwrap();
    assert!(!t.pendente && t.cobrar_em.is_none() && estado::pessoal(&t));
    assert_eq!((t.proximo.as_str(), t.id), ("2026-11-13T09:00:00-03:00", 3));
    assert_eq!(estado::prazo(&t), Some(prazo));
    // o task_tick do Argos dá o ⏰ quando o prazo chega, e antes nada
    let mut t = t;
    assert_eq!(argos::tarefas::task_tick(&mut t, &now, &cfg()).unwrap(), None);
    assert_eq!(argos::tarefas::task_tick(&mut t, &prazo, &cfg()).unwrap(), Some(argos::tarefas::Aviso::Vence));
}

#[test]
fn lembrete_pessoal_so_o_dono_ve() {
    let mut e = estado::Estado::default();
    let now = dt("2026-11-09T10:00:00-03:00");
    for (de, para, texto) in [("fulana", "beltrano", "a"), ("beltrano", "beltrano", "b"), ("sicrano", "sicrano", "c"), ("beltrano", "fulana", "d")] {
        estado::nova_tarefa(&mut e, de, para, texto, None, &now, &cfg()).unwrap();
    }
    let ver = |eu: &str, f: &str| estado::visiveis(&e, eu, &Filtro::de_texto(f).unwrap()).iter().map(|t| t.texto.clone()).collect::<Vec<_>>().join("");
    assert_eq!(ver("beltrano", "minhas"), "ab");
    assert_eq!(ver("beltrano", "pedi"), "d");
    assert_eq!(ver("beltrano", "equipe"), "abd");
    assert_eq!(ver("fulana", "equipe"), "ad");
    assert_eq!(ver("fulana", "de:beltrano"), "a");
    assert_eq!(ver("sicrano", "de:sicrano"), "c");
    assert!(Filtro::de_texto("de:").is_err() && Filtro::de_texto("todas").is_err());
}

#[test]
fn fechar_tira_do_estado_e_vai_pro_historico() {
    let tmp = TempDir::new("ariadne_estado.");
    let hist = estado::historico(&tmp.0);
    let mut e = estado::Estado::default();
    let now = dt("2026-11-09T10:00:00-03:00");
    estado::nova_tarefa(&mut e, "fulana", "beltrano", "a", None, &now, &cfg()).unwrap();
    estado::nova_tarefa(&mut e, "fulana", "beltrano", "b", None, &now, &cfg()).unwrap();
    let t = estado::fechar(&mut e, 2, Fecho::Recusada, Some(" sem tempo "), &now, &hist).unwrap();
    assert_eq!((t.resto["status"].as_str(), t.resto["motivo"].as_str()), (Some("recusada"), Some("sem tempo")));
    estado::fechar(&mut e, 1, Fecho::Feita, None, &now, &hist).unwrap();
    assert!(e.tarefas.is_empty());
    assert!(estado::fechar(&mut e, 1, Fecho::Feita, None, &now, &hist).is_err(), "já fechada");
    let linhas: Vec<Value> = std::fs::read_to_string(&hist).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(linhas.iter().map(|l| l["status"].as_str().unwrap()).collect::<Vec<_>>(), ["recusada", "feita"]);
    assert!(linhas[1].get("motivo").is_none());
}

#[test]
fn mapa_do_reply_poda_velho_e_excesso() {
    let mut e = estado::Estado::default();
    let now = dt("2026-11-09T10:00:00-03:00");
    let ent = |id: i64, em: &str| estado::Entregue { chat: 7, message_id: id, de: "fulana".into(), texto: "oi".into(), em: em.into() };
    estado::anota_entrega(&mut e, ent(1, "2026-11-01T10:00:00-03:00"), &now);
    assert!(e.respostas.is_empty(), "mais de 7 dias");
    for i in 0..(estado::RESPOSTA_MAX as i64 + 5) {
        estado::anota_entrega(&mut e, ent(i, "2026-11-09T09:00:00-03:00"), &now);
    }
    assert_eq!(e.respostas.len(), estado::RESPOSTA_MAX);
    assert!(estado::entrega(&e, 7, 4).is_none() && estado::entrega(&e, 7, 5).is_some() && estado::entrega(&e, 8, 5).is_none());
}

fn estado_de(dir: &std::path::Path) -> estado::Estado {
    estado::carregar(&estado::arquivo(dir)).unwrap()
}

/// resultado da ferramenta no último pedido ao modelo
fn resultado(p: &Value) -> String {
    let m = p["messages"].as_array().unwrap();
    m.last().unwrap()["content"][0]["content"].as_str().unwrap().to_string()
}

#[test]
fn delegar_entrega_na_hora_e_concluir_avisa_quem_pediu() {
    let (tmp, mut r, rx, crx) = time_de_teste(vec![
        usa("t1", "delegar", json!({"para": "beltrano", "texto": "mandar a <proposta>", "prazo": null})),
        diz("Mandei pro Beltrano."),
        usa("t2", "concluir", json!({"id": 1})),
        diz("Fulana já sabe."),
    ]);
    r.despacha(&msg(100, "private", "pede pro Beltrano mandar a proposta")).unwrap();
    espera(&rx, enviado_a(200, "📌 <b>Fulana</b> te pediu: mandar a &lt;proposta&gt;"));
    espera(&rx, enviado_a(100, "Mandei pro Beltrano."));
    crx.recv().unwrap();
    assert!(resultado(&crx.recv().unwrap()).starts_with("tarefa #1 criada pra Beltrano, que já recebeu; primeira cobrança"));
    let e = estado_de(&tmp.0);
    assert_eq!((e.tarefas.len(), estado::de(&e.tarefas[0]), estado::para(&e.tarefas[0]), e.tarefas[0].pendente), (1, "fulana-souza", "beltrano", true));
    let conversa = std::fs::read_to_string(membros::pasta(&tmp.0).join("beltrano/conversa.jsonl")).unwrap();
    assert!(conversa.contains("te pediu"), "a entrega vai pra conversa de quem recebe: {conversa}");

    // o Beltrano vê a tarefa no turno dele e conclui
    r.despacha(&msg(200, "private", "mandei a proposta")).unwrap();
    espera(&rx, enviado_a(100, "✅ <b>Beltrano</b> concluiu: mandar a &lt;proposta&gt;"));
    espera(&rx, enviado_a(200, "Fulana já sabe."));
    let p = crx.recv().unwrap();
    assert!(p["messages"].as_array().unwrap().last().unwrap()["content"][0]["text"].as_str().unwrap()
        .contains("[tarefas dele(a): #1 mandar a <proposta> (pediu: Fulana)]"));
    assert_eq!(resultado(&crx.recv().unwrap()), "#1 fechada; avisei Fulana");
    assert!(estado_de(&tmp.0).tarefas.is_empty());
    assert!(std::fs::read_to_string(estado::historico(&tmp.0)).unwrap().contains(r#""status":"feita""#));
}

#[test]
fn quem_faz_recusa_e_adia_quem_pediu_cancela() {
    let futuro = (argos::util::agora() + chrono::Duration::days(2)).format("%Y-%m-%dT10:00").to_string();
    let (tmp, mut r, rx, crx) = time_de_teste(vec![
        usa("t1", "delegar", json!({"para": "beltrano", "texto": "a"})),
        usa("t2", "delegar", json!({"para": "beltrano", "texto": "b"})),
        usa("t3", "concluir", json!({"id": 1})),
        diz("ok"),
        usa("t4", "recusar", json!({"id": 1, "motivo": " "})),
        usa("t5", "recusar", json!({"id": 1, "motivo": "sem tempo"})),
        usa("t6", "adiar", json!({"id": 2, "quando": futuro})),
        usa("t7", "cancelar", json!({"id": 2})),
        diz("feito"),
        usa("t8", "cancelar", json!({"id": 2})),
        diz("cancelada"),
    ]);
    r.despacha(&msg(100, "private", "pede a e b pro Beltrano")).unwrap();
    espera(&rx, enviado_a(100, "ok"));
    for _ in 0..3 {
        crx.recv().unwrap();
    }
    assert_eq!(resultado(&crx.recv().unwrap()), "ValueError: tarefa #1: só quem faz (Beltrano) conclui, adia ou recusa");

    r.despacha(&msg(200, "private", "recusa a, sem tempo; e deixa b pra depois de amanhã")).unwrap();
    espera(&rx, enviado_a(100, "❌ <b>Beltrano</b> recusou: a\nMotivo: sem tempo"));
    let adiou = espera(&rx, enviado_a(100, "🕒 <b>Beltrano</b> deixou pra"));
    assert!(adiou["text"].as_str().unwrap().ends_with(": b"));
    espera(&rx, enviado_a(200, "feito"));
    crx.recv().unwrap();
    assert_eq!(resultado(&crx.recv().unwrap()), "ValueError: recusar precisa do motivo: pergunte");
    for _ in 0..2 {
        crx.recv().unwrap();
    }
    assert_eq!(resultado(&crx.recv().unwrap()), "ValueError: tarefa #2: só quem pediu (Fulana) cancela");
    let e = estado_de(&tmp.0);
    assert_eq!(e.tarefas.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
    assert!(e.tarefas[0].cobrar_em.as_deref().unwrap().starts_with(&futuro[..13]), "{:?}", e.tarefas[0].cobrar_em);

    r.despacha(&msg(100, "private", "cancela a b")).unwrap();
    espera(&rx, enviado_a(200, "🚫 <b>Fulana</b> cancelou: b"));
    espera(&rx, enviado_a(100, "cancelada"));
    assert!(estado_de(&tmp.0).tarefas.is_empty());
}

#[test]
fn lembrete_pessoal_nao_avisa_ninguem() {
    let (tmp, mut r, rx, crx) = time_de_teste(vec![
        usa("t1", "delegar", json!({"para": "eu", "texto": "ligar pro contador", "prazo": "2099-01-05"})),
        usa("t2", "recusar", json!({"id": 1, "motivo": "x"})),
        usa("t3", "concluir", json!({"id": 1})),
        diz("pronto"),
    ]);
    r.despacha(&msg(100, "private", "me lembra de ligar pro contador")).unwrap();
    let c = espera(&rx, |m, c| m == "sendMessage" && c["text"] != "");
    assert_eq!((c["chat_id"].as_i64(), c["text"].as_str()), (Some(100), Some("pronto")), "só a resposta, pra ela mesma");
    crx.recv().unwrap();
    assert!(resultado(&crx.recv().unwrap()).starts_with("lembrete #1 criado; aviso segunda 05/01 09:00"));
    assert_eq!(resultado(&crx.recv().unwrap()), "ValueError: tarefa #1: lembrete pessoal não se recusa: cancele");
    assert_eq!(resultado(&crx.recv().unwrap()), "#1 fechada");
    assert!(estado_de(&tmp.0).tarefas.is_empty());
}

fn toque(chat: i64, message_id: i64, texto: &str, data: &str) -> Value {
    json!({"update_id": 1, "callback_query": {"id": "q", "from": {"id": chat}, "data": data,
           "message": {"message_id": message_id, "chat": {"id": chat, "type": "private"}, "text": texto}}})
}

#[test]
fn botoes_da_tarefa_sem_modelo() {
    // só o delegar passa pelo modelo; depois, a fila acaba e qualquer chamada daria 💥
    let (tmp, mut r, rx, _crx) = time_de_teste(vec![
        usa("t1", "delegar", json!({"para": "beltrano", "texto": "a"})),
        usa("t2", "delegar", json!({"para": "beltrano", "texto": "b"})),
        diz("mandei as duas"),
    ]);
    r.despacha(&msg(100, "private", "pede a e b pro Beltrano")).unwrap();
    let entrega = espera(&rx, enviado_a(200, "te pediu: a"));
    let botoes = &entrega["reply_markup"]["inline_keyboard"][0];
    assert_eq!(botoes.as_array().unwrap().iter().map(|b| b["callback_data"].as_str().unwrap()).collect::<Vec<_>>(), ["ok:1", "hr:1", "no:1"]);
    espera(&rx, enviado_a(100, "mandei as duas"));

    // ❌ → pergunta o motivo → o motivo vai pra quem pediu
    r.despacha(&toque(200, 50, "📌 Fulana te pediu: a", "no:1")).unwrap();
    espera(&rx, enviado_a(200, "Por que não dá pra fazer a #1?"));
    r.despacha(&msg(200, "private", "sem verba esse mês")).unwrap();
    espera(&rx, enviado_a(100, "❌ <b>Beltrano</b> recusou: a\nMotivo: sem verba esse mês"));
    espera(&rx, enviado_a(200, "#1 fechada; avisei Fulana"));

    // 🕒 → "amanhã 9" → a cobrança muda e quem pediu fica sabendo
    r.despacha(&toque(200, 51, "📌 Fulana te pediu: b", "hr:2")).unwrap();
    espera(&rx, enviado_a(200, "Pra quando fica a #2?"));
    r.despacha(&msg(200, "private", "amanhã 9")).unwrap();
    espera(&rx, enviado_a(100, "🕒 <b>Beltrano</b> deixou pra"));
    espera(&rx, enviado_a(200, "#2: cobro"));
    let amanha = (argos::util::agora() + chrono::Duration::days(1)).format("%Y-%m-%dT09:00").to_string();
    assert!(estado_de(&tmp.0).tarefas[0].cobrar_em.as_deref().unwrap().starts_with(&amanha));

    // ✅ → a mensagem perde os botões e mostra o resultado; tocar de novo não fecha duas vezes
    r.despacha(&toque(200, 51, "📌 Fulana te pediu: <b>", "ok:2")).unwrap();
    espera(&rx, enviado_a(100, "✅ <b>Beltrano</b> concluiu: b"));
    let ed = espera(&rx, |m, _| m == "editMessageText");
    assert_eq!((ed["message_id"].as_i64(), ed["text"].as_str()), (Some(51), Some("📌 Fulana te pediu: &lt;b&gt;\n\n✅ <i>#2 fechada; avisei Fulana</i>")));
    assert_eq!(ed["reply_markup"]["inline_keyboard"], json!([]));
    r.despacha(&toque(200, 51, "x", "ok:2")).unwrap();
    let ed = espera(&rx, |m, _| m == "editMessageText");
    assert!(ed["text"].as_str().unwrap().contains("⚠️ <i>tarefa #2 não existe ou já foi fechada"), "{ed}");
    assert!(estado_de(&tmp.0).tarefas.is_empty());
}

// --- recados ---

/// um atendente fora de thread, pra chamar `tick_em` com o relógio do teste
fn atendente_de(r: &Roteador, slug: &str, chat: i64) -> crate::atendente::Atendente {
    let mut tg = argos::rede::Telegram::new(argos::rede::agente(), "x");
    tg.base = r.tg.base.clone();
    crate::atendente::Atendente::new(tg, slug, chat, r.config.clone(), r.dir.clone(), r.equipe.clone(), None)
}

#[test]
fn recado_na_hora_vai_pra_todos_menos_quem_manda() {
    let (tmp, mut r, rx, crx) = time_de_teste(vec![
        usa("t1", "recado", json!({"para": ["eu"], "texto": "x"})),
        usa("t2", "recado", json!({"para": ["todos"], "texto": "chego 10h <amanhã>"})),
        diz("Avisei o Beltrano."),
    ]);
    r.despacha(&msg(100, "private", "avisa todo mundo que chego 10h amanhã")).unwrap();
    espera(&rx, enviado_a(200, "💬 <b>Fulana</b>: chego 10h &lt;amanhã&gt;"));
    espera(&rx, enviado_a(100, "Avisei o Beltrano."));
    crx.recv().unwrap();
    assert_eq!(resultado(&crx.recv().unwrap()), "ValueError: recado pra si mesmo é lembrete: use delegar com para = eu");
    assert_eq!(resultado(&crx.recv().unwrap()), "entregue pra Beltrano");
    let e = estado_de(&tmp.0);
    assert_eq!((e.respostas.len(), e.respostas[0].chat, e.respostas[0].de.as_str()), (1, 200, "fulana-souza"));
    assert!(e.respostas[0].message_id > 0, "o message_id que o Telegram devolveu");
    assert!(e.tarefas.is_empty() && e.recados.is_empty(), "recado não vira tarefa");
}

#[test]
fn recado_agendado_sai_no_tick_de_quem_recebe() {
    let (tmp, mut r, rx, crx) = time_de_teste(vec![
        usa("t1", "recado", json!({"para": "beltrano", "texto": "reunião 9h", "quando": "2099-01-05T08:00"})),
        diz("Agendei."),
    ]);
    r.despacha(&msg(100, "private", "amanhã 8h avisa o Beltrano da reunião")).unwrap();
    espera(&rx, enviado_a(100, "Agendei."));
    crx.recv().unwrap();
    assert_eq!(resultado(&crx.recv().unwrap()), "recado #1 agendado pra segunda 05/01 08:00 (Beltrano)");
    assert_eq!(estado_de(&tmp.0).recados.len(), 1);

    let mut fulana = atendente_de(&r, "fulana-souza", 100);
    let mut beltrano = atendente_de(&r, "beltrano", 200);
    beltrano.tick_em(&dt("2099-01-05T07:59:00-03:00")).unwrap();
    fulana.tick_em(&dt("2099-01-05T08:00:00-03:00")).unwrap();
    assert_eq!(estado_de(&tmp.0).recados.len(), 1, "cedo pro Beltrano; e o tick da Fulana não entrega o dele");
    beltrano.tick_em(&dt("2099-01-05T08:00:00-03:00")).unwrap();
    espera(&rx, enviado_a(200, "💬 <b>Fulana</b>: reunião 9h"));
    let e = estado_de(&tmp.0);
    assert!(e.recados.is_empty(), "todos receberam: sai do estado");
    assert_eq!((e.respostas.len(), e.respostas[0].de.as_str()), (1, "fulana-souza"));
    beltrano.tick_em(&dt("2099-01-05T08:01:00-03:00")).unwrap();
    nada(&rx, 200, |_| true);
}

// --- cobrança ---

#[test]
fn cobra_espacado_na_janela_e_escala_uma_vez() {
    let (tmp, r, rx, _crx) = time_de_teste(vec![]);
    let cfg = r.config.cfg_cobranca();
    {
        let mut eq = crate::atendente::trava(&r.equipe);
        estado::nova_tarefa(&mut eq.estado, "fulana-souza", "beltrano", "mandar a proposta", None, &dt("2026-11-09T10:00:00-03:00"), &cfg).unwrap();
        estado::nova_tarefa(&mut eq.estado, "beltrano", "beltrano", "pagar o boleto", Some(dt("2026-11-10T09:00:00-03:00")), &dt("2026-11-09T10:00:00-03:00"), &cfg).unwrap();
        eq.gravar().unwrap();
    }
    let mut b = atendente_de(&r, "beltrano", 200);
    let cobrar_em = || estado_de(&tmp.0).tarefas[0].cobrar_em.clone().unwrap();
    assert_eq!(cobrar_em(), "2026-11-09T13:00:00-03:00");
    b.tick_em(&dt("2026-11-09T12:59:00-03:00")).unwrap();
    nada(&rx, 200, |_| true);

    let mut horas = vec![];
    for n in 1..=4 {
        let quando = cobrar_em();
        horas.push(quando[5..16].to_string());
        b.tick_em(&dt(&quando)).unwrap();
        let c = espera(&rx, enviado_a(200, "🔔 mandar a proposta (pedido de Fulana"));
        assert_eq!(c["reply_markup"]["inline_keyboard"][0][0]["callback_data"], "ok:1");
        if n == 3 {
            espera(&rx, enviado_a(100, "⚠️ <b>Beltrano</b> não respondeu 3 cobranças de: mandar a proposta"));
        }
        if n == 4 {
            // o lembrete pessoal vence no prazo (10/11 09h), no meio das cobranças, e só pro dono
            let c = espera(&rx, enviado_a(200, "⏰ pagar o boleto"));
            assert_eq!(c["reply_markup"]["inline_keyboard"][0][2]["callback_data"], "cx:2");
        }
    }
    // 13h; +4h30 = 17h30; +6h45 cai fora da janela (até 20h) → 08h; e segue espaçando
    assert_eq!(horas[..3], ["11-09T13:00", "11-09T17:30", "11-10T08:00"]);
    nada(&rx, 300, |c| c["chat_id"] == 100);
}
