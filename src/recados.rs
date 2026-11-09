//! Recado de um membro pra outros: sai na hora ou fica agendado no estado, e cada destinatário recebe
//! no próprio `tick` (assim cada thread só manda o agendado pro próprio chat). Toda entrega entra no
//! mapa do reply (`estado::anota_entrega`): responder a ela manda a resposta pra quem mandou.

use crate::atendente::{trava, Atendente};
use crate::estado::{self, Entregue, RecadoAgendado};
use argos::rascunho::esc;
use argos::tarefas::fmt_when;
use argos::util::{agora, arg, arg_str, corta, from_iso, iso, Dt, Erro, R};
use serde_json::{Map, Value};

/// recado agendado que ninguém pôde receber (não pareou) some depois de tantos dias
const RECADO_DIAS: i64 = 7;

impl Atendente {
    /// `para`: lista de ids, ou "todos" (todo mundo menos quem manda). Aceita texto solto no lugar da lista
    fn destinatarios(&self, v: &Value) -> R<Vec<String>> {
        let lista: Vec<String> = match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
            _ => return Err(Erro::Valor("para: lista de ids ou \"todos\"".into())),
        };
        let eq = trava(&self.equipe);
        let mut out: Vec<String> = vec![];
        for s in lista {
            if s == "todos" {
                out.extend(eq.membros.iter().map(|m| m.slug.clone()).filter(|m| *m != self.slug));
            } else if eq.membro(&s).is_some() && s != self.slug {
                out.push(s);
            } else if s == self.slug || s == "eu" {
                return Err(Erro::Valor("recado pra si mesmo é lembrete: use delegar com para = eu".into()));
            } else {
                return Err(Erro::Valor(format!("{s:?} não é da equipe")));
            }
        }
        out.sort();
        out.dedup();
        if out.is_empty() {
            return Err(Erro::Valor("ninguém pra mandar".into()));
        }
        Ok(out)
    }

    pub fn recado(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let texto = arg_str(args, "texto")?.trim().to_string();
        if texto.is_empty() {
            return Err(Erro::Valor("texto vazio".into()));
        }
        let para = self.destinatarios(arg(args, "para")?)?;
        let quando = args.get("quando").and_then(Value::as_str).filter(|q| !q.trim().is_empty()).map(estado::le_quando).transpose()?;
        let nomes = |eu: &Atendente, v: &[String]| {
            let eq = trava(&eu.equipe);
            v.iter().map(|s| eq.primeiro_nome(s)).collect::<Vec<_>>().join(", ")
        };
        // hora que já passou (ou agora) = sai na hora
        if let Some(q) = quando.filter(|q| *q > now) {
            let id = {
                let mut eq = trava(&self.equipe);
                let id = eq.estado.proximo_id;
                eq.estado.proximo_id += 1;
                let r = RecadoAgendado { id, de: self.slug.clone(), para: para.clone(), texto, quando: iso(&q), entregue: vec![], resto: Map::new() };
                eq.estado.recados.push(r);
                eq.gravar()?;
                id
            };
            return Ok(format!("recado #{id} agendado pra {} ({})", fmt_when(&iso(&q))?, nomes(self, &para)));
        }
        let eu = trava(&self.equipe).primeiro_nome(&self.slug);
        let msg = format!("💬 <b>{}</b>: {}", esc(&eu), esc(&texto));
        let (mut foi, mut nao) = (vec![], vec![]);
        for s in &para {
            match self.envia_para(s, &msg, None)? {
                Some((chat, message_id)) => {
                    let mut eq = trava(&self.equipe);
                    estado::anota_entrega(&mut eq.estado, Entregue { chat, message_id, de: self.slug.clone(), texto: texto.clone(), em: iso(&now) }, &now);
                    eq.gravar()?;
                    foi.push(s.clone());
                }
                None => nao.push(s.clone()),
            }
        }
        let mut r = format!("entregue pra {}", if foi.is_empty() { "ninguém".into() } else { nomes(self, &foi) });
        if !nao.is_empty() {
            r += &format!("; {} ainda não entrou no Telegram comigo, não recebeu", nomes(self, &nao));
        }
        Ok(r)
    }

    /// reply a um recado entregue neste chat: a resposta vai pra quem mandou. None = não é reply a recado
    pub fn responde_recado(&mut self, message_id: i64, texto: &str) -> R<Option<String>> {
        if texto.is_empty() || texto.starts_with('/') {
            return Ok(None);
        }
        let (ent, eu, nome_de) = {
            let eq = trava(&self.equipe);
            let Some(ent) = estado::entrega(&eq.estado, self.chat_id, message_id).cloned() else { return Ok(None) };
            let nome_de = eq.primeiro_nome(&ent.de);
            (ent, eq.primeiro_nome(&self.slug), nome_de)
        };
        let msg = format!("↩️ <b>{}</b> respondeu ao seu recado \"{}\":\n{}", esc(&eu), esc(&corta(&ent.texto, 80)), esc(texto));
        Ok(Some(match self.envia_para(&ent.de, &msg, None)? {
            Some(_) => format!("Mandei pra {nome_de}."),
            None => format!("{nome_de} não está mais no Telegram comigo, não deu pra mandar."),
        }))
    }

    /// recados agendados pra mim cuja hora chegou; e a faxina dos que ninguém pôde receber
    pub fn entrega_recados(&mut self, now: &Dt) -> R<()> {
        let meus: Vec<RecadoAgendado> = {
            let mut eq = trava(&self.equipe);
            let limite = *now - chrono::Duration::days(RECADO_DIAS);
            let antes = eq.estado.recados.len();
            eq.estado.recados.retain(|r| from_iso(&r.quando).is_ok_and(|q| q >= limite));
            if eq.estado.recados.len() != antes {
                eq.gravar()?;
            }
            let slug = &self.slug;
            eq.estado.recados.iter().filter(|r| r.para.contains(slug) && !r.entregue.contains(slug) && from_iso(&r.quando).is_ok_and(|q| q <= *now)).cloned().collect()
        };
        for r in meus {
            let de = trava(&self.equipe).primeiro_nome(&r.de);
            let eu = self.slug.clone();
            let Some((chat, message_id)) = self.envia_para(&eu, &format!("💬 <b>{}</b>: {}", esc(&de), esc(&r.texto)), None)? else { continue };
            let mut eq = trava(&self.equipe);
            estado::anota_entrega(&mut eq.estado, Entregue { chat, message_id, de: r.de.clone(), texto: r.texto.clone(), em: iso(now) }, now);
            if let Some(x) = eq.estado.recados.iter_mut().find(|x| x.id == r.id) {
                x.entregue.push(eu.clone());
            }
            eq.estado.recados.retain(|x| x.para.iter().any(|p| !x.entregue.contains(p)));
            eq.gravar()?;
        }
        Ok(())
    }
}
