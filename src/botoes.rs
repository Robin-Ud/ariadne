//! Os botões da tarefa entregue e da cobrança: ✅ feito, 🕒 depois, ❌ recusar (no lembrete pessoal,
//! 🗑 cancelar). Sem modelo: 🕒 e ❌ perguntam, e a próxima mensagem é a hora (`parse_hora_livre`) ou o
//! motivo. A pergunta vale por `ESPERA`; depois disso, a mensagem segue o caminho normal.

use crate::atendente::Atendente;
use crate::estado::Fecho;
use argos::rascunho::esc;
use argos::tarefas::parse_hora_livre;
use argos::util::{agora, R};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// quanto tempo a pergunta do 🕒 / ❌ fica valendo
pub const ESPERA: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Espera {
    Hora(i64),
    Motivo(i64),
}

/// teclado da tarefa; `pessoal` troca o ❌ recusar pelo 🗑 cancelar
pub fn teclado(id: i64, pessoal: bool) -> Value {
    let ultimo = if pessoal { json!({"text": "🗑 cancelar", "callback_data": format!("cx:{id}")}) } else { json!({"text": "❌ recusar", "callback_data": format!("no:{id}")}) };
    json!([[{"text": "✅ feito", "callback_data": format!("ok:{id}")}, {"text": "🕒 depois", "callback_data": format!("hr:{id}")}, ultimo]])
}

impl Atendente {
    /// toque num botão
    pub fn botao(&mut self, q: &Value) -> R<()> {
        self.tg.call("answerCallbackQuery", 30, json!({"callback_query_id": q.get("id")}))?;
        let data = q.get("data").and_then(Value::as_str).unwrap_or_default();
        self.tg.registra("eu", &format!("[botão] {data}"));
        let Some((acao, id)) = data.split_once(':').and_then(|(a, i)| Some((a, i.parse::<i64>().ok()?))) else { return Ok(()) };
        let msg_id = q.pointer("/message/message_id").and_then(Value::as_i64);
        let original = esc(q.pointer("/message/text").and_then(Value::as_str).unwrap_or_default());
        let fecho = match acao {
            "ok" => Fecho::Feita,
            "cx" => Fecho::Cancelada,
            "hr" => {
                self.esperando = Some((Espera::Hora(id), Instant::now()));
                self.send(&format!("Pra quando fica a #{id}? (\"18h\", \"amanhã 9\", \"sexta às 14\")"))?;
                return Ok(());
            }
            "no" => {
                self.esperando = Some((Espera::Motivo(id), Instant::now()));
                self.send(&format!("Por que não dá pra fazer a #{id}? O motivo vai pra quem pediu."))?;
                return Ok(());
            }
            _ => return Ok(()),
        };
        let (marca, r) = match self.fecha(id, fecho, None) {
            Ok(r) => (if fecho == Fecho::Feita { "✅" } else { "🗑" }, r),
            Err(e) => ("⚠️", e.to_string()),
        };
        // a mensagem do botão perde o teclado e ganha o resultado
        if let Some(m) = msg_id {
            self.tg.edit(self.chat_id, m, &format!("{original}\n\n{marca} <i>{}</i>", esc(&r)), None)?;
        }
        Ok(())
    }

    /// resposta à pergunta do 🕒 ou do ❌; None = não era (ou a pergunta venceu): segue o caminho normal
    pub fn responde_espera(&mut self, texto: &str) -> R<Option<String>> {
        let Some((espera, desde)) = self.esperando.take() else { return Ok(None) };
        if desde.elapsed() > ESPERA || texto.starts_with('/') {
            return Ok(None);
        }
        Ok(match espera {
            Espera::Hora(id) => {
                let now = agora();
                // não é hora (None): vai pro modelo, que entende "deixa pra quando o cliente responder"
                parse_hora_livre(texto, &now, crate::estado::HORA_PRAZO).map(|alvo| self.adiar(id, &alvo, &now).unwrap_or_else(|e| format!("⚠️ {e}")))
            }
            Espera::Motivo(id) => Some(self.fecha(id, Fecho::Recusada, Some(texto)).unwrap_or_else(|e| format!("⚠️ {e}"))),
        })
    }
}
