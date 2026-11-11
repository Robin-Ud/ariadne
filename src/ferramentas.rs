//! O que cada ferramenta do modelo faz (as definições estão em `modelo.rs`). A trava da equipe vale só
//! pra ler, mudar e gravar o estado: as mensagens saem depois de soltar.
//!
//! Quem pode o quê: quem faz (`para`) conclui, adia e recusa; quem pediu (`de`) cancela. Cada um avisa
//! o outro lado, menos no lembrete pessoal (`de == para`).

use crate::atendente::{trava, Atendente};
use crate::botoes::teclado;
use crate::estado::{self, Fecho, Filtro};
use crate::modelo::linha_tarefa;
use argos::rascunho::esc;
use argos::tarefas::{fmt_when, task_move, Tarefa};
use argos::util::{agora, arg, arg_str, iso, Dt, Erro, R};
use serde_json::Value;

fn id_de(args: &Value) -> R<i64> {
    arg(args, "id")?.as_i64().ok_or_else(|| Erro::Valor("id tem que ser o número da tarefa".into()))
}

/// " (até sex 13/11 09:00)"
pub fn nota_prazo(t: &Tarefa) -> String {
    estado::prazo(t).and_then(|p| fmt_when(&iso(&p)).ok()).map(|p| format!(" (até {p})")).unwrap_or_default()
}

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
            "delegar" => self.delegar(args),
            "recado" => self.recado(args),
            "concluir" => self.fecha(id_de(args)?, Fecho::Feita, None),
            "recusar" => {
                let motivo = arg_str(args, "motivo")?;
                if motivo.trim().is_empty() {
                    return Err(Erro::Valor("recusar precisa do motivo: pergunte".into()));
                }
                self.fecha(id_de(args)?, Fecho::Recusada, Some(&motivo))
            }
            "cancelar" => self.fecha(id_de(args)?, Fecho::Cancelada, None),
            "adiar" => {
                let alvo = estado::le_quando(&arg_str(args, "quando")?)?;
                self.adiar(id_de(args)?, &alvo, &agora())
            }
            "marcar_reuniao" | "ver_agenda" | "livres_juntos" | "desmarcar_reuniao" => self.agenda(nome, args),
            "salvar_cliente" | "anotar_cliente" | "projeto_cliente" | "ver_cliente" => self.clientes(nome, args),
            _ => Err(Erro::Valor(format!("ferramenta desconhecida: {nome}"))),
        }
    }

    fn delegar(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let texto = arg_str(args, "texto")?;
        if texto.trim().is_empty() {
            return Err(Erro::Valor("texto vazio".into()));
        }
        let para = match arg_str(args, "para")?.as_str() {
            "eu" => self.slug.clone(),
            s => s.to_string(),
        };
        // opcional: o modelo pode mandar null ou nem mandar
        let prazo = args.get("prazo").and_then(Value::as_str).filter(|p| !p.trim().is_empty()).map(estado::le_quando).transpose()?;
        if prazo.is_some_and(|p| p <= now) {
            return Err(Erro::Valor("prazo no passado".into()));
        }
        let (t, eu, nome_para) = {
            let mut eq = trava(&self.equipe);
            if eq.membro(&para).is_none() {
                return Err(Erro::Valor(format!("{para:?} não é da equipe")));
            }
            let mut t = estado::nova_tarefa(&mut eq.estado, &self.slug, &para, &texto, prazo, &now, &self.config.cfg_cobranca())?;
            if para != self.slug && eq.membro(&self.slug).is_some_and(|m| m.chefe) {
                estado::marca_do_chefe(&mut t);
                if let Some(x) = eq.estado.tarefas.iter_mut().find(|x| x.id == t.id) {
                    *x = t.clone();
                }
            }
            eq.gravar()?;
            let eu = if estado::do_chefe(&t) { format!("{} (chefe)", eq.primeiro_nome(&self.slug)) } else { eq.primeiro_nome(&self.slug) };
            (t, eu, eq.primeiro_nome(&para))
        };
        let quando = |t: &Tarefa| match t.cobrar_em.as_deref() {
            Some(c) => format!("primeira cobrança {}", fmt_when(c).unwrap_or_default()),
            None => format!("aviso {}", fmt_when(&t.proximo).unwrap_or_default()),
        };
        if para == self.slug {
            return Ok(format!("lembrete #{} criado; {}", t.id, quando(&t)));
        }
        let msg = format!("📌 <b>{}</b> te pediu: {}{}", esc(&eu), esc(&estado::titulo(&t)), esc(&nota_prazo(&t)));
        Ok(match self.envia_para(&para, &msg, Some(teclado(t.id, false)))? {
            Some(_) => format!("tarefa #{} criada pra {nome_para}, que já recebeu; {}", t.id, quando(&t)),
            None => format!("tarefa #{} criada, mas {nome_para} ainda não entrou no Telegram comigo (não tocou no link do convite): não foi avisado", t.id),
        })
    }

    /// a tarefa aberta, se quem fala pode mexer nela desse jeito
    fn tarefa_pra(&self, id: i64, como: Option<Fecho>) -> R<Tarefa> {
        let eq = trava(&self.equipe);
        let t = eq.estado.tarefas.iter().find(|t| t.id == id).cloned().ok_or_else(|| Erro::Valor(format!("tarefa #{id} não existe ou já foi fechada")))?;
        let (de, para) = (estado::de(&t), estado::para(&t));
        let pode = match como {
            Some(Fecho::Cancelada) => de == self.slug,
            Some(Fecho::Recusada) => para == self.slug && de != self.slug,
            _ => para == self.slug,
        };
        if !pode {
            let porque = match como {
                Some(Fecho::Cancelada) => format!("só quem pediu ({}) cancela", eq.primeiro_nome(de)),
                Some(Fecho::Recusada) if de == self.slug => "lembrete pessoal não se recusa: cancele".into(),
                _ => format!("só quem faz ({}) conclui, adia ou recusa", eq.primeiro_nome(para)),
            };
            return Err(Erro::Valor(format!("tarefa #{id}: {porque}")));
        }
        Ok(t)
    }

    /// fecha a tarefa e avisa o outro lado
    pub(crate) fn fecha(&mut self, id: i64, como: Fecho, motivo: Option<&str>) -> R<String> {
        let now = agora();
        self.tarefa_pra(id, Some(como))?;
        let (t, eu) = {
            let mut eq = trava(&self.equipe);
            let hist = estado::historico(&eq.dir);
            let t = estado::fechar(&mut eq.estado, id, como, motivo, &now, &hist)?;
            eq.gravar()?;
            (t, eq.primeiro_nome(&self.slug))
        };
        if estado::pessoal(&t) {
            return Ok(format!("#{id} fechada"));
        }
        // concluir e recusar avisam quem pediu; cancelar avisa quem ia fazer
        let (outro, msg) = match como {
            Fecho::Feita => (estado::de(&t), format!("✅ <b>{}</b> concluiu: {}", esc(&eu), esc(&t.texto))),
            Fecho::Recusada => (estado::de(&t), format!("❌ <b>{}</b> recusou: {}\nMotivo: {}", esc(&eu), esc(&t.texto), esc(motivo.unwrap_or_default()))),
            Fecho::Cancelada => (estado::para(&t), format!("🚫 <b>{}</b> cancelou: {}", esc(&eu), esc(&t.texto))),
        };
        let outro = outro.to_string();
        let nome = trava(&self.equipe).primeiro_nome(&outro);
        Ok(match self.envia_para(&outro, &msg, None)? {
            Some(_) => format!("#{id} fechada; avisei {nome}"),
            None => format!("#{id} fechada; {nome} não está no Telegram comigo, não foi avisado"),
        })
    }

    /// quem faz empurra a próxima cobrança (ou o aviso, se ainda não venceu) e quem pediu fica sabendo
    pub fn adiar(&mut self, id: i64, alvo: &Dt, now: &Dt) -> R<String> {
        if alvo <= now {
            return Err(Erro::Valor("quando no passado".into()));
        }
        self.tarefa_pra(id, None)?;
        let (t, quando, eu) = {
            let mut eq = trava(&self.equipe);
            let cfg = self.config.cfg_cobranca();
            let t = eq.estado.tarefas.iter_mut().find(|t| t.id == id).ok_or_else(|| Erro::Valor(format!("tarefa #{id} sumiu")))?;
            let quando = task_move(t, alvo, &cfg)?;
            let t = t.clone();
            eq.gravar()?;
            (t, fmt_when(&quando)?, eq.primeiro_nome(&self.slug))
        };
        if estado::pessoal(&t) {
            return Ok(format!("#{id}: cobro {quando}"));
        }
        let de = estado::de(&t).to_string();
        self.envia_para(&de, &format!("🕒 <b>{}</b> deixou pra {quando}: {}", esc(&eu), esc(&t.texto)), None)?;
        Ok(format!("#{id}: cobro {quando}; avisei quem pediu"))
    }
}
