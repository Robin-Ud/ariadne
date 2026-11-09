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
    /// modelo e esforço da conversa
    pub modelo: String,
    pub esforco: String,
    /// horas entre uma cobrança e a próxima (espaça sozinho quando ninguém responde). 24 = no máximo uma por dia
    pub cobrar_a_cada_h: f64,
    /// cobrança só sai entre estas horas ("HH:MM")
    pub janela_cobranca: (String, String),
    /// tantas cobranças sem resposta → avisa quem pediu, uma vez
    pub escalar_apos: i64,
    /// a conversa com o modelo recomeça depois de tantos minutos parada
    pub conversa_reset_min: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            assistente: "a Ariadne".into(),
            fuso: "America/Sao_Paulo".into(),
            cred_dir: "credenciais".into(),
            modelo: "claude-haiku-5-5".into(),
            esforco: "medium".into(),
            cobrar_a_cada_h: 24.0,
            janela_cobranca: ("08:00".into(), "20:00".into()),
            escalar_apos: 3,
            conversa_reset_min: 30.0,
        }
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

    /// as regras de tempo das cobranças (as do Argos: espaça 1,5× até 72 h)
    pub fn cfg_cobranca(&self) -> argos::tarefas::CfgCobranca {
        argos::tarefas::CfgCobranca::new(self.cobrar_a_cada_h, self.janela_cobranca.clone())
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
    let hora = |h: &str| chrono::NaiveTime::parse_from_str(h, "%H:%M").is_ok();
    if !hora(&c.janela_cobranca.0) || !hora(&c.janela_cobranca.1) || c.janela_cobranca.0 >= c.janela_cobranca.1 {
        return Err(Erro::Msg(format!("{}: janela_cobranca {:?} (use [\"08:00\", \"20:00\"])", arquivo.display(), c.janela_cobranca)));
    }
    if c.cobrar_a_cada_h <= 0.0 || c.escalar_apos < 1 {
        return Err(Erro::Msg(format!("{}: cobrar_a_cada_h e escalar_apos têm que ser positivos", arquivo.display())));
    }
    if c.assistente.trim().is_empty() {
        c.assistente = Config::default().assistente;
    }
    if Path::new(&c.cred_dir).is_relative() {
        c.cred_dir = dir.join(&c.cred_dir).to_string_lossy().into();
    }
    Ok(c)
}
