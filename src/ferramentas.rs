//! O que cada ferramenta do modelo faz (as definições estão em `modelo.rs`). A trava da equipe vale só
//! pra ler, mudar e gravar o estado: as mensagens saem depois de soltar.

use crate::atendente::{trava, Atendente};
use crate::estado::{self, Filtro};
use crate::modelo::linha_tarefa;
use argos::util::{arg_str, Erro, R};
use serde_json::Value;

impl Atendente {
    pub fn ferramenta(&mut self, nome: &str, args: &Value) -> R<String> {
        match nome {
            "ver_tarefas" => {
                let f = Filtro::de_texto(&arg_str(args, "filtro")?)?;
                let eq = trava(&self.equipe);
                if let Filtro::De(s) = &f {
                    if eq.membro(s).is_none() {
                        return Err(Erro::Valor(format!("{s:?} não é da equipe")));
                    }
                }
                let v: Vec<String> = estado::visiveis(&eq.estado, &self.slug, &f).iter().map(|t| linha_tarefa(t, |s| eq.primeiro_nome(s), &self.slug)).collect();
                Ok(if v.is_empty() { "nenhuma tarefa aberta nesse filtro".into() } else { v.join("\n") })
            }
            _ => Err(Erro::Valor(format!("ferramenta desconhecida: {nome}"))),
        }
    }
}
