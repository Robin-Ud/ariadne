//! Um `getUpdates` só (o Telegram dá 409 pro segundo processo no mesmo bot) que entrega cada update,
//! pelo chat, pra thread do membro (`atendente.rs`). Chat desconhecido só entra com `/start <código>`.

use crate::atendente::{self, Atendente, Equipe, Time};
use crate::config::Config;
use crate::membros;
use argos::rede::{Claude, Telegram};
use argos::util::R;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

pub struct Roteador {
    pub tg: Telegram,
    pub dir: PathBuf,
    pub config: Config,
    pub equipe: Equipe,
    /// chat → fila da thread do membro
    filas: HashMap<i64, Sender<Value>>,
    /// URL do modelo (só os testes trocam)
    pub claude_url: Option<String>,
    /// URL do Google, agenda e token (só os testes trocam)
    pub google_url: Option<String>,
}

/// chat do update e se é conversa privada (message ou botão)
pub fn quem(u: &Value) -> Option<(i64, bool)> {
    let chat = u.pointer("/message/chat").or_else(|| u.pointer("/callback_query/message/chat"))?;
    Some((chat.get("id")?.as_i64()?, chat.get("type").and_then(Value::as_str) == Some("private")))
}

/// `/start <código>` (o que o link `?start=` manda)
pub fn codigo_do_start(u: &Value) -> Option<&str> {
    let t = u.pointer("/message/text")?.as_str()?.trim();
    let resto = t.strip_prefix("/start")?;
    resto.starts_with(' ').then(|| resto.trim()).filter(|c| !c.is_empty())
}

impl Roteador {
    /// lê `membros/` e sobe um atendente por membro pareado
    pub fn new(dir: PathBuf, config: Config, tg: Telegram) -> R<Roteador> {
        let time = Time::carregar(&dir)?;
        let mut r = Roteador { tg, dir, config, equipe: Arc::new(Mutex::new(time)), filas: HashMap::new(), claude_url: None, google_url: None };
        r.recarrega()?;
        Ok(r)
    }

    /// chats com atendente agora
    pub fn atendidos(&self) -> Vec<i64> {
        let mut v: Vec<i64> = self.filas.keys().copied().collect();
        v.sort();
        v
    }

    /// relê `membros/`: atendente novo pra quem pareou, e a fila de chat que saiu (pareou de outro
    /// celular, pasta apagada) é solta, o que encerra a thread dele
    pub fn recarrega(&mut self) -> R<()> {
        let lista = membros::listar(&self.dir)?;
        atendente::trava(&self.equipe).membros = lista.clone();
        self.filas.retain(|chat, _| lista.iter().any(|m| m.chat_id == Some(*chat)));
        for m in lista {
            let Some(chat) = m.chat_id else { continue };
            if self.filas.contains_key(&chat) {
                continue;
            }
            let mut tg = Telegram::new(argos::rede::agente(), "");
            tg.base = self.tg.base.clone();
            tg.file_base = self.tg.file_base.clone();
            tg.conversa = Some(membros::pasta(&self.dir).join(&m.slug).join("conversa.jsonl"));
            let mut a = Atendente::new(tg, &m.slug, chat, self.config.clone(), self.dir.clone(), self.equipe.clone(), self.claude());
            a.google_base = self.google_url.clone();
            let (tx, rx) = channel();
            std::thread::Builder::new().name(m.slug.clone()).spawn(move || a.laco(rx))?;
            println!("atendente: {} (chat {chat})", m.slug);
            self.filas.insert(chat, tx);
        }
        Ok(())
    }

    /// `--membro … --email`/`--chefe` grava no disco com o bot rodando: se `membros/` mudou desde o
    /// último `recarrega`, relê (o e-mail novo vale na próxima reunião, sem reiniciar)
    pub fn confere_membros(&mut self) -> R<()> {
        let lista = membros::listar(&self.dir)?;
        if lista != atendente::trava(&self.equipe).membros {
            println!("membros mudaram no disco: relendo");
            self.recarrega()?;
        }
        Ok(())
    }

    /// um `Claude` por atendente (a chave é relida a cada 401); sem `anthropic_key`, None
    fn claude(&self) -> Option<Claude> {
        let mut c = Claude::de_arquivo(argos::rede::agente(), &Path::new(&self.config.cred_dir).join("anthropic_key")).ok()?;
        if let Some(u) = &self.claude_url {
            c.url = u.clone();
        }
        Some(c)
    }

    /// solta todos os atendentes (as threads encerram); o próximo `recarrega` sobe de novo
    pub fn solta_todos(&mut self) {
        self.filas.clear();
    }

    pub fn despacha(&mut self, u: &Value) -> R<()> {
        let Some((chat, privado)) = quem(u) else { return Ok(()) };
        if !privado {
            println!("ignorado chat {chat} (não é privado)");
            return Ok(());
        }
        if !self.filas.contains_key(&chat) {
            let agora = argos::util::agora().timestamp();
            match codigo_do_start(u).and_then(|c| membros::do_convite(&self.dir, c, agora)) {
                Some(slug) => {
                    membros::parear(&self.dir, &slug, chat)?;
                    println!("pareado: {slug} (chat {chat})");
                    self.recarrega()?;
                }
                None => {
                    println!("ignorado chat {chat} (sem convite)");
                    return Ok(());
                }
            }
        }
        if let Some(tx) = self.filas.get(&chat) {
            if tx.send(u.clone()).is_err() {
                println!("atendente do chat {chat} caiu; sobe de novo no próximo update");
                self.filas.remove(&chat);
                self.recarrega()?;
            }
        }
        Ok(())
    }

    pub fn run(&mut self) -> ! {
        println!("{}: rodando com {} membro(s) pareado(s)", self.config.nome(), self.filas.len());
        let mut offset: Option<i64> = None;
        loop {
            if let Err(e) = self.confere_membros() {
                println!("membros: {e:?}");
            }
            let params = json!({"offset": offset, "timeout": 30, "allowed_updates": ["message", "callback_query"]});
            let updates = match self.tg.call("getUpdates", 40, params) {
                Ok(u) => u.as_array().cloned().unwrap_or_default(),
                Err(e) => {
                    println!("getUpdates: {e}");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
            };
            for u in &updates {
                offset = u.get("update_id").and_then(Value::as_i64).map(|i| i + 1).or(offset);
                if let Err(e) = self.despacha(u) {
                    println!("roteador: {e:?}");
                }
            }
        }
    }
}
