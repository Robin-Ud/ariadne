//! A conversa com o modelo: persona, ferramentas e o laço (o mesmo do `agenda_ask` do Argos, sobre
//! `Claude::create`). Cada atendente tem o próprio histórico, na memória.
//!
//! Cache: tools + system são iguais pra todos os membros (quem fala vai na mensagem, não no system),
//! e só mudam quando entra gente. O que muda a cada mensagem (hora, tarefas) vai no turno.

use crate::atendente::{trava, Atendente};
use crate::config::Config;
use crate::estado::{self, Filtro};
use crate::membros::Membro;
use argos::agenda::{carimbo, log_uso};
use argos::rede::{blocos, model_params};
use argos::tarefas::{fmt_when, sem_resposta, Tarefa};
use argos::util::R;
use serde_json::{json, Value};
use std::time::Instant;

/// as respostas dela são curtas; 8000 sobra pra raciocínio + várias chamadas de ferramenta
const MAX_TOKENS: u32 = 8000;
const VOLTAS: usize = 10;
/// histórico maior que isto recomeça (como no Argos)
const MAX_HISTORICO: usize = 40;

/// a persona e a equipe. Não muda de mensagem pra mensagem (fica em cache)
pub fn system(config: &Config, membros: &[Membro]) -> String {
    let nome = config.nome();
    let equipe: Vec<String> = membros.iter().map(|m| format!("- {} (`{}`){}", m.nome, m.slug, if m.chefe { ", o chefe" } else { "" })).collect();
    format!(
        "Você é {nome}, a secretária de uma equipe pequena, no Telegram. Cada membro fala com você no privado, \
e você leva tarefas e recados de um pro outro.\n\n\
Equipe (nome e o id que vai nas ferramentas):\n{}\n\n\
Jeito:\n\
- Curta e direta, em português do Brasil. Chama a pessoa pelo primeiro nome e diz o que fez (\"Mandei pro Fulano, ele já recebeu\").\n\
- Você é uma pessoa só pra equipe toda: sabe o que cada um pediu e quem está esperando o quê.\n\
- Lembra e avisa do que está parado, mas não decide prioridade por ninguém.\n\
- Não invente tarefa, prazo nem membro. Se faltar o quem ou o quê, pergunte.\n\
- Cada mensagem começa com [agora: …], [falando com: …] e as tarefas abertas de quem fala: use isso, não pergunte de novo.\n\
- A resposta vai como HTML do Telegram: <b>, <i>, <code>; nada de Markdown.\n\n\
Tarefas:\n\
- \"Pede pro Fulano…\", \"o Fulano precisa…\" → delegar com para = o id dele. \"Me lembra de…\" → delegar com para = eu (lembrete pessoal).\n\
- Sem prazo, a tarefa já começa a ser cobrada (espaçado, só em horário de trabalho). Com prazo, o aviso sai no prazo.\n\
- Prazo e quando: AAAA-MM-DD (vale às 09:00) ou AAAA-MM-DDTHH:MM, calculado pelo [agora] e pelos próximos dias da mensagem.\n\
- Quem faz conclui, adia ou recusa (recusar pede o motivo: pergunte se não veio). Quem pediu cancela. Use o #id que vem nas tarefas.\n\
- Cada ação já avisa o outro lado sozinha: não diga que vai avisar, diga que avisou (o resultado da ferramenta conta).\n\n\
Recados:\n\
- \"Avisa o Fulano que…\", \"fala pra todo mundo que…\" → recado (não é tarefa: ninguém é cobrado). Com hora (\"amanhã às 8 avisa…\") → quando.\n\
- O texto do recado vai como quem mandou falaria pro outro, em primeira pessoa (\"chego 10h amanhã\"), sem \"a Fulana disse que\": a mensagem já sai com o nome dela.\n\
- Quem recebe pode responder (reply) ao recado, e a resposta volta pra quem mandou.\n",
        equipe.join("\n")
    )
}

/// `para` das ferramentas: os ids dos membros, mais `eu`
fn enum_para(membros: &[Membro]) -> Value {
    let mut v: Vec<Value> = membros.iter().map(|m| json!(m.slug)).collect();
    v.push(json!("eu"));
    Value::Array(v)
}

/// `para` do recado: os ids dos membros, mais `todos`
fn enum_todos(membros: &[Membro]) -> Value {
    let mut v: Vec<Value> = membros.iter().map(|m| json!(m.slug)).collect();
    v.push(json!("todos"));
    Value::Array(v)
}

pub fn ferramentas(membros: &[Membro]) -> Value {
    let id = json!({"type": "integer", "description": "o #id da tarefa"});
    let filtro = "minhas (o que quem fala tem que fazer), pedi (o que pediu pros outros), equipe (tudo da equipe, menos lembrete pessoal dos outros) ou de:<id> (o que aquele membro tem que fazer)";
    json!([
        {"name": "ver_tarefas", "description": "Lista as tarefas abertas, com id, quem pediu, quem faz, prazo e cobranças sem resposta.",
         "input_schema": {"type": "object", "properties": {
             "filtro": {"type": "string", "description": filtro}},
          "required": ["filtro"]}},
        {"name": "delegar", "description": "Cria uma tarefa pra um membro (ele recebe na hora) ou um lembrete pessoal (para = eu).",
         "input_schema": {"type": "object", "properties": {
             "para": {"type": "string", "enum": enum_para(membros), "description": "id de quem faz; eu = quem está falando"},
             "texto": {"type": "string", "description": "o que fazer, curto e no infinitivo, como quem pediu falaria"},
             "prazo": {"type": ["string", "null"], "description": "AAAA-MM-DD ou AAAA-MM-DDTHH:MM; null = sem prazo (já cobra)"}},
          "required": ["para", "texto"]}},
        {"name": "recado", "description": "Manda um recado pra um ou mais membros, na hora ou agendado. Não cria tarefa.",
         "input_schema": {"type": "object", "properties": {
             "para": {"type": "array", "items": {"type": "string", "enum": enum_todos(membros)}, "description": "ids de quem recebe; [\"todos\"] = a equipe toda menos quem manda"},
             "texto": {"type": "string"},
             "quando": {"type": ["string", "null"], "description": "AAAA-MM-DDTHH:MM pra agendar; null = agora"}},
          "required": ["para", "texto"]}},
        {"name": "concluir", "description": "Quem faz marca a tarefa como feita; quem pediu é avisado.",
         "input_schema": {"type": "object", "properties": {"id": id}, "required": ["id"]}},
        {"name": "adiar", "description": "Quem faz deixa a tarefa pra depois: a próxima cobrança (ou o aviso, se ainda não venceu) vai pro quando; quem pediu é avisado.",
         "input_schema": {"type": "object", "properties": {"id": id, "quando": {"type": "string", "description": "AAAA-MM-DD ou AAAA-MM-DDTHH:MM"}}, "required": ["id", "quando"]}},
        {"name": "recusar", "description": "Quem faz recusa a tarefa, com o motivo; quem pediu recebe o motivo.",
         "input_schema": {"type": "object", "properties": {"id": id, "motivo": {"type": "string"}}, "required": ["id", "motivo"]}},
        {"name": "cancelar", "description": "Quem pediu cancela a tarefa (ou o próprio lembrete); quem ia fazer é avisado.",
         "input_schema": {"type": "object", "properties": {"id": id}, "required": ["id"]}},
    ])
}

/// cache_control no último bloco da última mensagem (só na cópia que vai pro request)
fn marca_cache(m: Option<&mut Value>) {
    let Some(m) = m else { return };
    if let Some(last) = m.get_mut("content").and_then(Value::as_array_mut).and_then(|a| a.last_mut()) {
        last["cache_control"] = json!({"type": "ephemeral"});
    }
}

/// "#3 mandar a proposta (pediu: Fulana · até sex 13/11 09:00 · 2 cobranças sem resposta)"
pub fn linha_tarefa(t: &Tarefa, nome: impl Fn(&str) -> String, eu: &str) -> String {
    let mut notas = vec![];
    if !estado::pessoal(t) {
        if estado::de(t) != eu {
            notas.push(format!("pediu: {}", nome(estado::de(t))));
        }
        if estado::para(t) != eu {
            notas.push(format!("faz: {}", nome(estado::para(t))));
        }
    } else {
        notas.push("lembrete pessoal".into());
    }
    if estado::do_chefe(t) {
        notas.push("⭐ pedido do chefe".into());
    }
    if let Some(p) = estado::prazo(t) {
        notas.push(format!("até {}", fmt_when(&argos::util::iso(&p)).unwrap_or_default()));
    }
    let n = sem_resposta(t);
    if t.pendente && n > 0 {
        notas.push(format!("{n} cobrança{} sem resposta", if n == 1 { "" } else { "s" }));
    }
    format!("#{} {} ({})", t.id, t.texto, notas.join(" · "))
}

impl Atendente {
    /// a mensagem do usuário com hora, quem fala e as tarefas abertas dele
    fn turno(&self, texto: &str) -> Value {
        let now = argos::util::agora();
        let eq = trava(&self.equipe);
        let nome = |s: &str| eq.primeiro_nome(s);
        let lista = |f: Filtro| estado::visiveis(&eq.estado, &self.slug, &f).iter().map(|t| linha_tarefa(t, nome, &self.slug)).collect::<Vec<_>>();
        let (minhas, pedi) = (lista(Filtro::Minhas), lista(Filtro::Pedi));
        let quem = eq.membro(&self.slug).map(|m| m.nome.clone()).unwrap_or_else(|| self.slug.clone());
        let mut ctx = format!("{}\n[falando com: {quem} (`{}`)]", carimbo(&now), self.slug);
        if !minhas.is_empty() {
            ctx += &format!("\n[tarefas dele(a): {}]", minhas.join("; "));
        }
        if !pedi.is_empty() {
            ctx += &format!("\n[pediu pros outros: {}]", pedi.join("; "));
        }
        json!({"role": "user", "content": [{"type": "text", "text": format!("{ctx}\n{texto}")}]})
    }

    /// texto livre → modelo, com as ferramentas. Erro no meio zera o histórico (não fica tool_use sem resultado)
    pub fn perguntar(&mut self, texto: &str) -> R<String> {
        if self.claude.is_none() {
            return Ok("Ainda não tenho a chave do modelo (credenciais/anthropic_key): por enquanto só /ping e /equipe.".into());
        }
        let parado = self.ultimo.map(|t| t.elapsed().as_secs_f64() > self.config.conversa_reset_min * 60.0).unwrap_or(true);
        if parado || self.historico.len() > MAX_HISTORICO {
            self.historico.clear();
        }
        self.ultimo = Some(Instant::now());
        let _ = self.tg.call("sendChatAction", 10, json!({"chat_id": self.chat_id, "action": "typing"}));
        let turno = self.turno(texto);
        self.historico.push(turno);
        let r = self.laco_modelo();
        if r.is_err() {
            self.historico.clear();
        }
        r
    }

    fn laco_modelo(&mut self) -> R<String> {
        let (system, tools) = {
            let eq = trava(&self.equipe);
            (system(&self.config, &eq.membros), ferramentas(&eq.membros))
        };
        let (base, beta) = model_params(&self.config.modelo, &self.config.esforco);
        for _ in 0..VOLTAS {
            let mut body = base.clone();
            body["max_tokens"] = json!(MAX_TOKENS);
            body["system"] = json!([{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}]);
            body["tools"] = tools.clone();
            let mut msgs = self.historico.clone();
            marca_cache(msgs.last_mut());
            body["messages"] = Value::Array(msgs);
            let response = self.claude.as_ref().expect("checado no perguntar").create(&body, beta)?;
            drop(body);
            log_uso(&response);
            let stop = response.get("stop_reason").and_then(Value::as_str).unwrap_or("");
            if stop == "refusal" {
                self.historico.clear();
                return Ok("O modelo recusou esse pedido. 🤷".into());
            }
            // cortada: a ferramenta do meio não roda; a mensagem pela metade não fica no histórico
            if stop == "max_tokens" {
                self.historico.clear();
                return Ok("✂️ A resposta passou do limite e foi cortada. Pede em partes menores?".into());
            }
            self.historico.push(json!({"role": "assistant", "content": response.get("content").cloned().unwrap_or(json!([]))}));
            let blocks = blocos(&response);
            if stop != "tool_use" {
                let t = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n");
                let t = t.trim();
                return Ok(if t.is_empty() { "👍".into() } else { t.to_string() });
            }
            let mut results = vec![];
            for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")) {
                let nome = b.get("name").and_then(Value::as_str).unwrap_or("");
                let input = b.get("input").cloned().unwrap_or(json!({}));
                let (content, is_error) = match self.ferramenta(nome, &input) {
                    Ok(c) => (c, false),
                    Err(e) => (format!("{}: {e}", e.tipo()), true),
                };
                results.push(json!({"type": "tool_result", "tool_use_id": b.get("id").cloned().unwrap_or(Value::Null),
                                    "content": content, "is_error": is_error}));
            }
            self.historico.push(json!({"role": "user", "content": results}));
        }
        self.historico.clear();
        Ok("Me perdi nas ferramentas (10 voltas). Tenta de novo com outras palavras?".into())
    }
}
