//! Membros da equipe: `membros/<slug>/membro.json` ({nome, email}), `chat_id` (0600, quando pareado) e
//! `convite` (0600, até tocar no link). `ariadne --membro "Nome"` cria; o roteador pareia.

use argos::util::{grava_secreto, Erro, R};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// quanto tempo o link do convite vale
pub const CONVITE_S: i64 = 7 * 86400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membro {
    pub nome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// nome da pasta
    #[serde(skip)]
    pub slug: String,
    /// do arquivo `chat_id`; None = ainda não tocou no link
    #[serde(skip)]
    pub chat_id: Option<i64>,
    /// chave de versão mais nova não some quando esta grava
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Convite {
    codigo: String,
    /// unix, segundos
    criado: i64,
}

pub fn pasta(dir: &Path) -> PathBuf {
    dir.join("membros")
}

/// "João Pedro" → "joao-pedro"
pub fn slug_de(nome: &str) -> String {
    let f = argos::agenda::fold(nome);
    f.split(|c: char| !c.is_ascii_alphanumeric()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// t.me com o `?start=`: o Telegram manda `/start <codigo>` sozinho quando a pessoa toca em Começar
pub fn link(bot: &str, codigo: &str) -> String {
    format!("https://t.me/{}?start={codigo}", bot.trim_start_matches('@'))
}

/// 12 caracteres hex do /dev/urandom (o link pode ser repassado; 6 dígitos é pouco)
pub fn codigo_convite() -> R<String> {
    use std::io::Read;
    let mut b = [0u8; 6];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// cria (ou atualiza) o membro e grava um convite novo; devolve o membro e o código.
/// Recadastrar quem já está pareado só troca o convite: o `chat_id` vale até alguém tocar no link novo.
pub fn cadastrar(dir: &Path, nome: &str, email: Option<&str>, agora: i64) -> R<(Membro, String)> {
    let nome = nome.trim();
    let slug = slug_de(nome);
    if slug.is_empty() {
        return Err(Erro::Valor(format!("nome sem letra nem número: {nome:?}")));
    }
    let p = pasta(dir).join(&slug);
    std::fs::create_dir_all(&p)?;
    let mut m = ler(dir, &slug).unwrap_or(Membro { nome: String::new(), email: None, slug: slug.clone(), chat_id: None, resto: Map::new() });
    m.nome = nome.to_string();
    if let Some(e) = email.map(str::trim).filter(|e| !e.is_empty()) {
        m.email = Some(e.to_string());
    }
    std::fs::write(p.join("membro.json"), serde_json::to_string_pretty(&m)? + "\n")?;
    let codigo = codigo_convite()?;
    grava_secreto(&p.join("convite"), &serde_json::to_string(&Convite { codigo: codigo.clone(), criado: agora })?)?;
    Ok((m, codigo))
}

pub fn ler(dir: &Path, slug: &str) -> R<Membro> {
    let p = pasta(dir).join(slug);
    let arq = p.join("membro.json");
    let b = std::fs::read(&arq).map_err(|e| Erro::Msg(format!("{}: {e}", arq.display())))?;
    let mut m: Membro = serde_json::from_slice(&b).map_err(|e| Erro::Msg(format!("{}: {e}", arq.display())))?;
    m.slug = slug.to_string();
    m.chat_id = std::fs::read_to_string(p.join("chat_id")).ok().and_then(|s| s.trim().parse().ok());
    Ok(m)
}

/// todos, por slug; pasta sem `membro.json` é pulada
pub fn listar(dir: &Path) -> R<Vec<Membro>> {
    let mut v = vec![];
    let Ok(rd) = std::fs::read_dir(pasta(dir)) else { return Ok(v) };
    for e in rd {
        let e = e?;
        let slug = e.file_name().to_string_lossy().to_string();
        if e.path().join("membro.json").exists() {
            v.push(ler(dir, &slug)?);
        }
    }
    v.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(v)
}

fn convite(dir: &Path, slug: &str) -> Option<Convite> {
    serde_json::from_slice(&std::fs::read(pasta(dir).join(slug).join("convite")).ok()?).ok()
}

/// tem convite ainda valendo
pub fn convite_pendente(dir: &Path, slug: &str, agora: i64) -> bool {
    convite(dir, slug).is_some_and(|c| agora - c.criado <= CONVITE_S)
}

/// slug do membro cujo convite é este código e ainda vale
pub fn do_convite(dir: &Path, codigo: &str, agora: i64) -> Option<String> {
    let codigo = codigo.trim();
    if codigo.is_empty() {
        return None;
    }
    let rd = std::fs::read_dir(pasta(dir)).ok()?;
    rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).find(|slug| {
        convite(dir, slug).is_some_and(|c| c.codigo == codigo && agora - c.criado <= CONVITE_S)
    })
}

/// grava o `chat_id` (0600) e apaga o convite. Outro membro com o mesmo chat perde o dele: um chat é uma pessoa.
pub fn parear(dir: &Path, slug: &str, chat_id: i64) -> R<()> {
    for m in listar(dir)? {
        if m.slug != slug && m.chat_id == Some(chat_id) {
            std::fs::remove_file(pasta(dir).join(&m.slug).join("chat_id"))?;
        }
    }
    let p = pasta(dir).join(slug);
    grava_secreto(&p.join("chat_id"), &chat_id.to_string())?;
    match std::fs::remove_file(p.join("convite")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
