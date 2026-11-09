//! Uma thread por membro: o laço do `Bot::run` do Argos com `recv_timeout` no lugar do `getUpdates`.
//! Chegou update → `handle`; deu o tempo → `tick`. Um membro esperando o modelo não trava os outros.

use crate::config::Config;
use crate::estado::{self, Estado};
use crate::membros::{self, Membro};
use argos::rascunho::esc;
use argos::rede::{Claude, Telegram};
use argos::util::{corta, R};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// o que as threads dividem: quem é da equipe e o estado das tarefas e recados
#[derive(Debug, Default)]
pub struct Time {
    pub membros: Vec<Membro>,
    pub estado: Estado,
    /// a pasta da Ariadne (o estado vai em `dados/estado.json`)
    pub dir: PathBuf,
}

impl Time {
    /// lê o estado; os membros entram pelo `Roteador::recarrega`
    pub fn carregar(dir: &Path) -> R<Time> {
        Ok(Time { membros: vec![], estado: estado::carregar(&estado::arquivo(dir))?, dir: dir.to_path_buf() })
    }

    pub fn gravar(&self) -> R<()> {
        estado::gravar(&self.estado, &estado::arquivo(&self.dir))
    }

    pub fn membro(&self, slug: &str) -> Option<&Membro> {
        self.membros.iter().find(|m| m.slug == slug)
    }

    /// primeiro nome ("Fulana Souza" → "Fulana"); slug que não é membro volta como veio
    pub fn primeiro_nome(&self, slug: &str) -> String {
        self.membro(slug).and_then(|m| m.nome.split_whitespace().next()).unwrap_or(slug).to_string()
    }
}

/// a equipe, compartilhada entre as threads. Trava só pra ler ou mudar, nunca durante chamada de rede.
pub type Equipe = Arc<Mutex<Time>>;

/// thread que caiu com a trava na mão não derruba as outras
pub fn trava(e: &Equipe) -> MutexGuard<'_, Time> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

/// menu do "/" de cada membro (`setMyCommands` com scope do chat)
pub const COMANDOS: &[(&str, &str)] = &[("ping", "tô aí?"), ("equipe", "quem está na equipe"), ("ajuda", "o que eu faço")];

/// de quanto em quanto tempo o `tick` roda sem mensagem
const VOLTA: Duration = Duration::from_secs(30);

pub struct Atendente {
    pub tg: Telegram,
    pub slug: String,
    pub chat_id: i64,
    pub config: Config,
    pub dir: PathBuf,
    pub equipe: Equipe,
    /// None = sem `anthropic_key`: só os comandos
    pub claude: Option<Claude>,
    /// a conversa com o modelo (ver `modelo.rs`)
    pub historico: Vec<Value>,
    pub ultimo: Option<Instant>,
}

impl Atendente {
    pub fn new(tg: Telegram, slug: &str, chat_id: i64, config: Config, dir: PathBuf, equipe: Equipe, claude: Option<Claude>) -> Atendente {
        Atendente { tg, slug: slug.to_string(), chat_id, config, dir, equipe, claude, historico: vec![], ultimo: None }
    }

    pub fn laco(mut self, rx: Receiver<Value>) {
        if let Err(e) = self.menu() {
            println!("{}: setMyCommands: {e}", self.slug);
        }
        loop {
            match rx.recv_timeout(VOLTA) {
                Ok(u) => {
                    if let Err(e) = self.handle(&u) {
                        println!("{}: erro: {e:?}", self.slug);
                        let _ = self.send(&format!("💥 erro: <code>{}</code>", corta(&esc(&e.to_string()), 300)));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(e) = self.tick() {
                        println!("{}: erro nos avisos: {e:?}", self.slug);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    println!("{}: atendente encerrado", self.slug);
                    return;
                }
            }
        }
    }

    pub fn menu(&self) -> R<()> {
        let cmds: Vec<Value> = COMANDOS.iter().map(|(c, d)| json!({"command": c, "description": d})).collect();
        self.tg.call("setMyCommands", 20, json!({"commands": cmds, "scope": {"type": "chat", "chat_id": self.chat_id}}))?;
        Ok(())
    }

    fn send(&self, texto: &str) -> R<Value> {
        self.tg.send(self.chat_id, texto, None, true)
    }

    /// manda pro chat de um membro (eu também) e grava no `conversa.jsonl` dele. Devolve o `message_id`;
    /// None = ele ainda não tocou no link do convite
    pub fn envia_para(&mut self, slug: &str, texto: &str, teclado: Option<Value>) -> R<Option<i64>> {
        let chat = if slug == self.slug { Some(self.chat_id) } else { trava(&self.equipe).membro(slug).and_then(|m| m.chat_id) };
        let Some(chat) = chat else { return Ok(None) };
        let meu = self.tg.conversa.clone();
        if slug != self.slug {
            self.tg.conversa = Some(membros::pasta(&self.dir).join(slug).join("conversa.jsonl"));
        }
        let r = self.tg.send(chat, texto, teclado, true);
        self.tg.conversa = meu;
        Ok(r?.get("message_id").and_then(Value::as_i64))
    }

    /// primeiro nome deste membro
    fn nome(&self) -> String {
        trava(&self.equipe).primeiro_nome(&self.slug)
    }

    fn saudacao(&self) -> String {
        format!(
            "Oi, {}! Aqui é {}, a secretária da equipe.\n\nMe fala do jeito que falaria com uma pessoa:\n• \"pede pro Fulano mandar a proposta até sexta\"\n• \"me lembra de ligar pro contador amanhã\"\n• \"o que tenho pra fazer?\", \"o que eu pedi?\"\n\nQuem recebe uma tarefa é cobrado até concluir, adiar ou recusar, e quem pediu fica sabendo.",
            esc(&self.nome()),
            esc(&self.config.assistente)
        )
    }

    fn texto_equipe(&self) -> String {
        let agora = argos::util::agora().timestamp();
        let eq = trava(&self.equipe).membros.clone();
        let mut t = String::from("👥 <b>Equipe</b>");
        for m in &eq {
            let (icone, nota) = match m.chat_id {
                Some(_) => ("✅", ""),
                None if membros::convite_pendente(&self.dir, &m.slug, agora) => ("⏳", " (convite mandado)"),
                None => ("➖", " (sem convite: ariadne --membro)"),
            };
            let voce = if m.slug == self.slug { " (você)" } else { "" };
            t += &format!("\n{icone} {}{voce}{nota}", esc(&m.nome));
        }
        t
    }

    pub fn handle(&mut self, u: &Value) -> R<()> {
        if let Some(q) = u.get("callback_query") {
            self.tg.call("answerCallbackQuery", 30, json!({"callback_query_id": q.get("id")}))?;
            return Ok(());
        }
        let texto = u.pointer("/message/text").and_then(Value::as_str).unwrap_or_default().trim();
        self.tg.registra("eu", if texto.is_empty() { "[sem texto]" } else { texto });
        let comando = texto.split_whitespace().next().unwrap_or_default();
        let comando = comando.split('@').next().unwrap_or_default();
        let r = match comando {
            "/start" | "/ajuda" => self.saudacao(),
            "/ping" => "🏓 tô aqui".into(),
            "/equipe" => self.texto_equipe(),
            _ if comando.starts_with('/') => "Não conheço esse comando: tenho /ping, /equipe e /ajuda.".into(),
            _ if texto.is_empty() => "Por enquanto só leio texto.".into(),
            _ => self.perguntar(texto)?,
        };
        self.send(&r)?;
        Ok(())
    }

    /// avisos e cobranças (fase 1)
    pub fn tick(&mut self) -> R<()> {
        Ok(())
    }
}
