//! Estado da equipe em `dados/estado.json`: tarefas abertas, recados agendados e o mapa do reply.
//! Fica sob a trava da `Equipe` (ver `atendente.rs`); grava com tmp + rename.
//!
//! A tarefa é a `argos::tarefas::Tarefa` (as regras de tempo da cobrança vêm de lá), com `de`, `para`,
//! `criada` e `prazo` no `resto`. Tarefa fechada (feita, recusada, cancelada) sai daqui e vira uma
//! linha em `dados/historico.jsonl`: o `task_tick` só vê tarefa aberta.

use argos::tarefas::{encaixe, espera, CfgCobranca, Tarefa};
use argos::util::{from_iso, iso, so_data, Dt, Erro, R};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// hora do vencimento quando o prazo é só um dia ("até sexta")
pub const HORA_PRAZO: &str = "09:00";
/// o reply a um recado vale por tantos dias, e o mapa guarda no máximo tantas entregas
pub const RESPOSTA_DIAS: i64 = 7;
pub const RESPOSTA_MAX: usize = 200;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Estado {
    #[serde(default)]
    pub tarefas: Vec<Tarefa>,
    #[serde(default)]
    pub recados: Vec<RecadoAgendado>,
    #[serde(default)]
    pub respostas: Vec<Entregue>,
    /// id da próxima tarefa ou recado (o mesmo contador)
    #[serde(default = "um")]
    pub proximo_id: i64,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

fn um() -> i64 {
    1
}

impl Default for Estado {
    fn default() -> Estado {
        Estado { tarefas: vec![], recados: vec![], respostas: vec![], proximo_id: 1, resto: Map::new() }
    }
}

/// recado com hora marcada: cada destinatário recebe no próprio `tick` e entra em `entregue`
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecadoAgendado {
    pub id: i64,
    pub de: String,
    pub para: Vec<String>,
    pub texto: String,
    pub quando: String,
    #[serde(default)]
    pub entregue: Vec<String>,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

/// recado entregue num chat: responder (reply) a esta mensagem manda a resposta pra `de`
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entregue {
    pub chat: i64,
    pub message_id: i64,
    pub de: String,
    pub texto: String,
    pub em: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fecho {
    Feita,
    Recusada,
    Cancelada,
}

impl Fecho {
    fn nome(self) -> &'static str {
        match self {
            Fecho::Feita => "feita",
            Fecho::Recusada => "recusada",
            Fecho::Cancelada => "cancelada",
        }
    }
}

pub fn arquivo(dir: &Path) -> PathBuf {
    dir.join("dados").join("estado.json")
}

pub fn historico(dir: &Path) -> PathBuf {
    dir.join("dados").join("historico.jsonl")
}

/// sem arquivo = equipe nova. JSON quebrado não sobe: melhor parado do que gravar por cima das tarefas
pub fn carregar(path: &Path) -> R<Estado> {
    let Ok(b) = std::fs::read(path) else { return Ok(Estado::default()) };
    serde_json::from_slice(&b).map_err(|e| Erro::Msg(format!("{}: {e}", path.display())))
}

pub fn gravar(e: &Estado, path: &Path) -> R<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(e)? + "\n")?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn campo<'a>(t: &'a Tarefa, k: &str) -> &'a str {
    t.resto.get(k).and_then(Value::as_str).unwrap_or_default()
}

/// quem pediu
pub fn de(t: &Tarefa) -> &str {
    campo(t, "de")
}

/// quem faz
pub fn para(t: &Tarefa) -> &str {
    campo(t, "para")
}

/// lembrete pessoal: só o dono vê, e ninguém recebe aviso de volta
pub fn pessoal(t: &Tarefa) -> bool {
    de(t) == para(t)
}

pub fn prazo(t: &Tarefa) -> Option<Dt> {
    t.resto.get("prazo").and_then(Value::as_str).and_then(|s| from_iso(s).ok())
}

/// "2026-11-13" ou "2026-11-13T15:00": só o dia → às `HORA_PRAZO`
pub fn le_quando(s: &str) -> R<Dt> {
    let s = s.trim();
    if so_data(s) {
        from_iso(&format!("{s}T{HORA_PRAZO}"))
    } else {
        from_iso(s)
    }
}

/// Tarefa nova de `de` pra `para`. Sem prazo, já fica cobrando (a primeira cobrança depois de uma
/// `espera`; a entrega é a mensagem de agora). Com prazo, o `task_tick` dá o ⏰ quando ele chegar.
pub fn nova_tarefa(e: &mut Estado, de: &str, para: &str, texto: &str, prazo: Option<Dt>, now: &Dt, cfg: &CfgCobranca) -> R<Tarefa> {
    let quando = prazo.unwrap_or(*now);
    let mut resto = Map::new();
    resto.insert("de".into(), json!(de));
    resto.insert("para".into(), json!(para));
    resto.insert("criada".into(), json!(iso(now)));
    if let Some(p) = &prazo {
        resto.insert("prazo".into(), json!(iso(p)));
    }
    let mut t = Tarefa {
        id: e.proximo_id,
        texto: texto.trim().to_string(),
        proximo: iso(&quando),
        hora: quando.format("%H:%M").to_string(),
        repetir_dias: None,
        desde_feito: false,
        pendente: false,
        cobrar_em: None,
        cobrancas: 0,
        resto,
    };
    if prazo.is_none() {
        t.pendente = true;
        t.resto.insert("sem_resposta".into(), json!(0));
        t.resto.insert("pendente_desde".into(), json!(iso(now)));
        t.cobrar_em = Some(iso(&encaixe(&t, &(*now + espera(&t, cfg)), cfg)?));
    }
    e.proximo_id += 1;
    e.tarefas.push(t.clone());
    Ok(t)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filtro {
    /// as que eu faço
    Minhas,
    /// as que eu pedi pra alguém
    Pedi,
    /// todas, menos os lembretes pessoais dos outros
    Equipe,
    /// as que este membro faz (lembrete pessoal dele só se for eu)
    De(String),
}

impl Filtro {
    pub fn de_texto(s: &str) -> R<Filtro> {
        Ok(match s.trim() {
            "minhas" | "" => Filtro::Minhas,
            "pedi" => Filtro::Pedi,
            "equipe" => Filtro::Equipe,
            o => match o.strip_prefix("de:") {
                Some(slug) if !slug.trim().is_empty() => Filtro::De(slug.trim().to_string()),
                _ => return Err(Erro::Valor(format!("filtro {o:?}: use minhas, pedi, equipe ou de:<membro>"))),
            },
        })
    }
}

/// tarefas abertas que `eu` pode ver por este filtro, em ordem de id
pub fn visiveis<'a>(e: &'a Estado, eu: &str, f: &Filtro) -> Vec<&'a Tarefa> {
    e.tarefas
        .iter()
        .filter(|t| !pessoal(t) || para(t) == eu)
        .filter(|t| match f {
            Filtro::Minhas => para(t) == eu,
            Filtro::Pedi => de(t) == eu && !pessoal(t),
            Filtro::Equipe => true,
            Filtro::De(s) => para(t) == s,
        })
        .collect()
}

/// tira a tarefa do estado e anexa no histórico, com o status e o motivo
pub fn fechar(e: &mut Estado, id: i64, como: Fecho, motivo: Option<&str>, now: &Dt, historico: &Path) -> R<Tarefa> {
    let pos = e.tarefas.iter().position(|t| t.id == id).ok_or_else(|| Erro::Valor(format!("tarefa {id} não existe (ou já foi fechada)")))?;
    let mut t = e.tarefas.remove(pos);
    t.resto.insert("status".into(), json!(como.nome()));
    t.resto.insert("fechada".into(), json!(iso(now)));
    if let Some(m) = motivo.map(str::trim).filter(|m| !m.is_empty()) {
        t.resto.insert("motivo".into(), json!(m));
    }
    if let Some(d) = historico.parent() {
        std::fs::create_dir_all(d)?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(historico)?;
    writeln!(f, "{}", serde_json::to_string(&t)?)?;
    Ok(t)
}

/// entrega nova no mapa do reply; poda as velhas e o excesso
pub fn anota_entrega(e: &mut Estado, ent: Entregue, now: &Dt) {
    e.respostas.push(ent);
    let limite = *now - chrono::Duration::days(RESPOSTA_DIAS);
    e.respostas.retain(|r| from_iso(&r.em).is_ok_and(|em| em >= limite));
    let sobra = e.respostas.len().saturating_sub(RESPOSTA_MAX);
    e.respostas.drain(..sobra);
}

/// o recado entregue nesta mensagem deste chat, se ainda vale
pub fn entrega(e: &Estado, chat: i64, message_id: i64) -> Option<&Entregue> {
    e.respostas.iter().find(|r| r.chat == chat && r.message_id == message_id)
}
