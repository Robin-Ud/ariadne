//! Resumo pro chefe, a cada `resumo_a_cada_dias` (padrão 14), no `resumo_dia` às `resumo_hora`. Sai sem
//! modelo: o que cada um concluiu, recusou ou cancelou no período (do `historico.jsonl`) e o que está
//! aberto, por pessoa, com atraso e cobranças sem resposta. Lembrete pessoal dos outros não entra.

use crate::atendente::{trava, Atendente};
use crate::estado::{self, Filtro};
use argos::rascunho::esc;
use argos::tarefas::{fmt_when, sem_resposta, Tarefa};
use argos::util::{corta, dia_semana, from_iso, iso, Dt, R};
use serde_json::{json, Value};
use std::path::Path;

/// tarefas fechadas desde `desde` (linha quebrada no histórico é pulada)
fn fechadas(historico: &Path, desde: &Dt) -> Vec<Tarefa> {
    let Ok(s) = std::fs::read_to_string(historico) else { return vec![] };
    s.lines()
        .filter_map(|l| serde_json::from_str::<Tarefa>(l).ok())
        .filter(|t| t.resto.get("fechada").and_then(Value::as_str).and_then(|f| from_iso(f).ok()).is_some_and(|f| f >= *desde))
        .collect()
}

fn status(t: &Tarefa) -> &str {
    t.resto.get("status").and_then(Value::as_str).unwrap_or_default()
}

/// o texto do resumo, visto por `chefe`; `slugs` dá a ordem das pessoas e `nome` o primeiro nome
pub fn texto(e: &estado::Estado, historico: &Path, chefe: &str, slugs: &[String], nome: impl Fn(&str) -> String, now: &Dt, dias: i64) -> String {
    let desde = *now - chrono::Duration::days(dias);
    let fechadas: Vec<Tarefa> = fechadas(historico, &desde).into_iter().filter(|t| !estado::pessoal(t) || estado::para(t) == chefe).collect();
    let abertas = estado::visiveis(e, chefe, &Filtro::Equipe);
    let mut out = format!("📊 <b>Resumo dos últimos {dias} dias</b> ({} a {})", desde.format("%d/%m"), now.format("%d/%m"));
    if fechadas.is_empty() && abertas.is_empty() {
        return out + "\n\nNada fechado e nada aberto no período.";
    }

    let por_pessoa = |de: &[&Tarefa]| -> Vec<(String, usize)> {
        slugs.iter().map(|s| (nome(s), de.iter().filter(|t| estado::para(t) == s).count())).filter(|(_, n)| *n > 0).collect()
    };
    let feitas: Vec<&Tarefa> = fechadas.iter().filter(|t| status(t) == "feita").collect();
    out += &format!("\n\n✅ <b>Concluídas: {}</b>", feitas.len());
    for (n, q) in por_pessoa(&feitas) {
        out += &format!("\n• {}: {q}", esc(&n));
    }
    let recusadas: Vec<&Tarefa> = fechadas.iter().filter(|t| status(t) == "recusada").collect();
    if !recusadas.is_empty() {
        out += &format!("\n❌ <b>Recusadas: {}</b>", recusadas.len());
        for t in &recusadas {
            let motivo = t.resto.get("motivo").and_then(Value::as_str).unwrap_or("sem motivo");
            out += &format!("\n• {}: {} ({})", esc(&nome(estado::para(t))), esc(&estado::titulo(t)), esc(motivo));
        }
    }
    let canceladas = fechadas.iter().filter(|t| status(t) == "cancelada").count();
    if canceladas > 0 {
        out += &format!("\n🚫 Canceladas: {canceladas}");
    }

    out += &format!("\n\n📋 <b>Abertas: {}</b>", abertas.len());
    for s in slugs {
        let dele: Vec<&&Tarefa> = abertas.iter().filter(|t| estado::para(t) == s).collect();
        if dele.is_empty() {
            continue;
        }
        out += &format!("\n<b>{}</b>", esc(&nome(s)));
        for t in dele {
            let mut notas = vec![];
            if !estado::pessoal(t) && estado::de(t) != chefe {
                notas.push(format!("pediu: {}", nome(estado::de(t))));
            }
            match estado::prazo(t) {
                Some(p) if p < *now => notas.push(format!("⚠️ atrasada (era {})", fmt_when(&iso(&p)).unwrap_or_default())),
                Some(p) => notas.push(format!("até {}", fmt_when(&iso(&p)).unwrap_or_default())),
                None => {}
            }
            let n = sem_resposta(t);
            if t.pendente && n >= 2 {
                notas.push(format!("{n} cobranças sem resposta"));
            }
            let notas = if notas.is_empty() { String::new() } else { format!(" · {}", notas.join(" · ")) };
            out += &format!("\n• #{} {}{}", t.id, esc(&estado::titulo(t)), esc(&notas));
        }
    }
    // a mensagem do Telegram vai até 4096
    corta(&out, 4000)
}

impl Atendente {
    /// o resumo, se sou chefe e chegou o dia, a hora e o período desde o último
    pub fn resumo(&mut self, now: &Dt) -> R<()> {
        let c = &self.config;
        if dia_semana(now) != c.resumo_dia || now.format("%H:%M").to_string() < c.resumo_hora {
            return Ok(());
        }
        let msg = {
            let eq = trava(&self.equipe);
            if !eq.membro(&self.slug).is_some_and(|m| m.chefe) {
                return Ok(());
            }
            let ultimo = eq.estado.resumos.get(&self.slug).and_then(Value::as_str).and_then(|s| from_iso(s).ok());
            // um dia de folga: o de 17:00 de hoje não espera o de 17:05 de duas semanas atrás
            if ultimo.is_some_and(|u| now.date_naive() - u.date_naive() < chrono::Duration::days(c.resumo_a_cada_dias - 1)) {
                return Ok(());
            }
            let slugs: Vec<String> = eq.membros.iter().map(|m| m.slug.clone()).collect();
            texto(&eq.estado, &estado::historico(&eq.dir), &self.slug, &slugs, |s| eq.primeiro_nome(s), now, c.resumo_a_cada_dias)
        };
        let eu = self.slug.clone();
        self.envia_para(&eu, &msg, None)?;
        let mut eq = trava(&self.equipe);
        eq.estado.resumos.insert(eu, json!(iso(now)));
        eq.gravar()
    }
}
