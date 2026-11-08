//! `configs.json` da equipe: só o que é da equipe toda. Os membros ficam em `membros/<slug>/` (ver `membros.rs`).

use argos::util::{Erro, R};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// nome dela com artigo, como aparece nas mensagens ("a Ariadne")
    pub assistente: String,
    /// fuso da equipe (IANA); só os do `.cargo/config.toml` vão no binário
    pub fuso: String,
    /// pasta das credenciais; relativa = a partir da pasta da Ariadne
    pub cred_dir: String,
}

impl Default for Config {
    fn default() -> Config {
        Config { assistente: "a Ariadne".into(), fuso: "America/Sao_Paulo".into(), cred_dir: "credenciais".into() }
    }
}

impl Config {
    pub fn tz(&self) -> chrono_tz::Tz {
        self.fuso.parse().unwrap_or(chrono_tz::Tz::America__Sao_Paulo)
    }

    /// "a Ariadne" → "Ariadne"
    pub fn nome(&self) -> &str {
        let a = self.assistente.trim();
        a.split_once(' ').filter(|(art, _)| matches!(*art, "a" | "o")).map(|(_, n)| n).unwrap_or(a)
    }

    /// conteúdo de `credenciais/<nome>`, sem espaço nas pontas
    pub fn credencial(&self, nome: &str) -> R<String> {
        let p = Path::new(&self.cred_dir).join(nome);
        let s = std::fs::read_to_string(&p).map_err(|e| Erro::Msg(format!("{}: {e}", p.display())))?;
        Ok(s.trim().to_string())
    }
}

/// ARIADNE_DIR; sem ela, a pasta atual
pub fn data_dir() -> PathBuf {
    std::env::var("ARIADNE_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// `<dir>/configs.json` (tem que existir: `{}` vale e pega os padrões)
pub fn load_de(dir: &Path) -> R<Config> {
    let arquivo = dir.join("configs.json");
    let b = std::fs::read(&arquivo).map_err(|e| Erro::Msg(format!("{}: {e}", arquivo.display())))?;
    let mut c: Config = serde_json::from_slice(&b).map_err(|e| Erro::Msg(format!("{}: {e}", arquivo.display())))?;
    if c.fuso.parse::<chrono_tz::Tz>().is_err() {
        return Err(Erro::Msg(format!("{}: fuso {:?} não está no binário (ver .cargo/config.toml)", arquivo.display(), c.fuso)));
    }
    if c.assistente.trim().is_empty() {
        c.assistente = Config::default().assistente;
    }
    if Path::new(&c.cred_dir).is_relative() {
        c.cred_dir = dir.join(&c.cred_dir).to_string_lossy().into();
    }
    Ok(c)
}
