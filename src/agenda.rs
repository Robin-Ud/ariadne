//! Agenda da equipe, na conta Google dela (fase 2).
//!
//! Reunião = evento na agenda dela com os membros em `attendees` (o Google manda o convite) e quem
//! marcou e quem vai no `extendedProperties.private` (`ariadne_de`, `ariadne_quem`): assim ela acha as
//! reuniões de cada um, até de quem não tem e-mail. O livre/ocupado de cada um vem do freeBusy (a pessoa
//! compartilha "ver apenas livre/ocupado" com a conta dela). Todo mundo também recebe o aviso aqui; quem
//! não tem e-mail recebe o botão "📅 Salvar na agenda". Sem o `--google-login`, marcar manda só o botão.

use crate::atendente::{trava, Atendente};
use argos::agenda::{agenda_line, horarios_livres, linha_livre, linhas_livres, parse_janela};
use argos::gcal::{link_salvar, GoogleCalendar};
use argos::rascunho::esc;
use argos::util::{agora, arg_str, from_iso, so_data, Dt, Erro, R};
use chrono::Datelike;
use serde_json::{json, Value};

/// escopos do `--google-login` além do `calendar.events`: o livre/ocupado das agendas que ela enxerga
pub const ESCOPOS_EXTRA: [&str; 2] = ["https://www.googleapis.com/auth/calendar.events.freebusy", "https://www.googleapis.com/auth/calendar.freebusy"];
const SEM_GOOGLE: &str = "a agenda ainda não está ligada (falta o ariadne --google-login no servidor)";
/// dias de horário livre que vão pro modelo
const MAX_DIAS_LIVRES: usize = 5;

/// quem vai (ids), gravado no evento
fn quem_do_evento(e: &Value) -> Vec<String> {
    e.pointer("/extendedProperties/private/ariadne_quem").and_then(Value::as_str).unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// quem marcou (id); vazio = evento que não foi ela que marcou
fn de_do_evento(e: &Value) -> &str {
    e.pointer("/extendedProperties/private/ariadne_de").and_then(Value::as_str).unwrap_or_default()
}

/// "2026-11-12" → 00:00 daquele dia (ou `fim`: 00:00 do dia seguinte); com hora, a hora
fn le_dia(s: &str, fim: bool) -> R<Dt> {
    let s = s.trim();
    if so_data(s) {
        let d = from_iso(&format!("{s}T00:00"))?;
        return Ok(if fim { d + chrono::Duration::days(1) } else { d });
    }
    from_iso(s)
}

fn opcional<'a>(args: &'a Value, k: &str) -> &'a str {
    args.get(k).and_then(Value::as_str).map(str::trim).unwrap_or_default()
}

fn status(e: &Value, email: &str) -> &'static str {
    let r = e.get("attendees").and_then(Value::as_array).and_then(|a| {
        a.iter().find(|x| x.get("email").and_then(Value::as_str).is_some_and(|m| m.eq_ignore_ascii_case(email)))
    });
    match r.and_then(|x| x.get("responseStatus")).and_then(Value::as_str) {
        Some("accepted") => " ✅",
        Some("declined") => " ❌ recusou",
        Some("tentative") => " ❔ talvez",
        Some(_) => " ⏳ não respondeu",
        None => "",
    }
}

/// os intervalos (a, b) que caem dentro de um dos eventos
fn sobrepoe(a: &Dt, b: &Dt, eventos: &[Value]) -> bool {
    eventos.iter().any(|e| {
        let ini = e.pointer("/start/dateTime").and_then(Value::as_str).and_then(|s| from_iso(s).ok());
        let fim = e.pointer("/end/dateTime").and_then(Value::as_str).and_then(|s| from_iso(s).ok());
        matches!((ini, fim), (Some(i), Some(f)) if i < *b && f > *a)
    })
}

/// (slug, primeiro nome, e-mail) de cada um
type Pessoa = (String, String, Option<String>);

impl Atendente {
    /// a agenda da conta dela; sem login, Err (o login pode vir com ela rodando: tenta de novo a cada uso)
    pub fn google(&mut self) -> R<&mut GoogleCalendar> {
        if self.google.is_none() {
            let mut g = GoogleCalendar::new(argos::rede::agente(), std::path::Path::new(&self.config.cred_dir)).map_err(|_| Erro::Valor(SEM_GOOGLE.into()))?;
            if let Some(b) = &self.google_base {
                g.base = b.clone();
            }
            self.google = Some(g);
        }
        Ok(self.google.as_mut().expect("criado acima"))
    }

    pub fn agenda(&mut self, nome: &str, args: &Value) -> R<String> {
        match nome {
            "marcar_reuniao" => self.marcar_reuniao(args),
            "ver_agenda" => self.ver_agenda(args),
            "livres_juntos" => self.livres_juntos(args),
            "desmarcar_reuniao" => self.desmarcar_reuniao(args),
            _ => Err(Erro::Valor(format!("ferramenta desconhecida: {nome}"))),
        }
    }

    /// `quem` (ids ou "todos") mais quem fala, na ordem da equipe
    fn participantes(&self, args: &Value) -> R<Vec<Pessoa>> {
        let eq = trava(&self.equipe);
        let pedidos: Vec<String> = match args.get("quem") {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => vec![],
        };
        let mut v = vec![self.slug.clone()];
        for p in pedidos {
            match p.as_str() {
                "todos" => v.extend(eq.membros.iter().map(|m| m.slug.clone())),
                "eu" => {}
                s if eq.membro(s).is_none() => return Err(Erro::Valor(format!("{s:?} não é da equipe"))),
                s => v.push(s.to_string()),
            }
        }
        Ok(eq.membros.iter().filter(|m| v.contains(&m.slug)).map(|m| (m.slug.clone(), eq.primeiro_nome(&m.slug), m.email.clone())).collect())
    }

    fn marcar_reuniao(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let titulo = arg_str(args, "titulo")?.trim().to_string();
        if titulo.is_empty() {
            return Err(Erro::Valor("titulo vazio".into()));
        }
        let inicio = arg_str(args, "inicio")?;
        if so_data(inicio.trim()) {
            return Err(Erro::Valor("inicio precisa da hora (AAAA-MM-DDTHH:MM): pergunte ou use livres_juntos".into()));
        }
        let ini = from_iso(inicio.trim())?;
        if ini <= now {
            return Err(Erro::Valor("inicio no passado".into()));
        }
        let min = args.get("duracao_min").and_then(Value::as_i64).unwrap_or(self.config.reuniao_min);
        if !(5..=12 * 60).contains(&min) {
            return Err(Erro::Valor("duracao_min entre 5 e 720".into()));
        }
        let fim = ini + chrono::Duration::minutes(min);
        let (local, descricao) = (opcional(args, "local").to_string(), opcional(args, "descricao").to_string());
        let forcar = args.get("forcar").and_then(Value::as_bool).unwrap_or(false);
        let pessoas = self.participantes(args)?;
        let eu = trava(&self.equipe).primeiro_nome(&self.slug);
        let com_google = self.google().is_ok();

        // alguém ocupado? (só quem tem e-mail e compartilhou o livre/ocupado)
        let emails: Vec<String> = pessoas.iter().filter_map(|p| p.2.clone()).collect();
        if com_google && !forcar && !emails.is_empty() {
            let ocupados: Vec<String> = self
                .google()?
                .free_busy(&emails, &ini, &fim)?
                .into_iter()
                .filter(|(_, r)| r.as_ref().is_ok_and(|v| !v.is_empty()))
                .filter_map(|(m, _)| pessoas.iter().find(|p| p.2.as_deref() == Some(m.as_str())).map(|p| p.1.clone()))
                .collect();
            if !ocupados.is_empty() {
                return Ok(format!(
                    "NÃO marquei: {} já tem compromisso nesse horário. Ofereça outro (livres_juntos) ou, se quiserem assim mesmo, chame de novo com forcar = true",
                    ocupados.join(", ")
                ));
            }
        }

        let mut ev = None;
        if com_google {
            let fuso = argos::util::fuso().name().to_string();
            let assinatura = format!("Marcada por {eu} pela {}.", self.config.nome());
            let mut body = json!({
                "summary": titulo,
                "start": {"dateTime": argos::util::iso(&ini), "timeZone": fuso},
                "end": {"dateTime": argos::util::iso(&fim), "timeZone": fuso},
                "description": if descricao.is_empty() { assinatura } else { format!("{descricao}\n\n{assinatura}") },
                "attendees": emails.iter().map(|e| json!({"email": e})).collect::<Vec<_>>(),
                "guestsCanModify": false,
                "extendedProperties": {"private": {"ariadne_de": self.slug, "ariadne_quem": pessoas.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(",")}},
            });
            if !local.is_empty() {
                body["location"] = json!(local);
            }
            ev = Some(self.google()?.create_convidando(&body)?);
        }

        let quando = linha_livre(&ini.naive_local(), &fim.naive_local());
        let link = link_salvar(&titulo, &ini, &fim, &descricao, &local);
        let (mut avisei, mut fora) = (vec![], vec![]);
        for (slug, nome, email) in &pessoas {
            let convite = ev.is_some() && email.is_some();
            // quem marcou fica sabendo pela resposta; o cartão só vai pra ele se não tem convite
            if slug == &self.slug && convite {
                continue;
            }
            let cab = if slug == &self.slug { "📅 Marcado".to_string() } else { format!("📅 <b>{}</b> marcou", esc(&eu)) };
            let mut msg = format!("{cab}: <b>{}</b>\n{}", esc(&titulo), esc(&quando));
            if !local.is_empty() {
                msg += &format!(" · {}", esc(&local));
            }
            let outros: Vec<&str> = pessoas.iter().filter(|p| &p.0 != slug).map(|p| p.1.as_str()).collect();
            if !outros.is_empty() {
                msg += &format!("\ncom {}", esc(&outros.join(", ")));
            }
            if convite {
                msg += "\n<i>O convite do Google chegou no seu e-mail.</i>";
            }
            let botao = (!convite).then(|| json!([[{"text": "📅 Salvar na agenda", "url": link}]]));
            match self.envia_para(slug, &msg, botao)? {
                Some(_) if slug != &self.slug => avisei.push(nome.clone()),
                Some(_) => {}
                None => fora.push(nome.clone()),
            }
        }

        let mut r = match &ev {
            Some(e) => format!("marcada: {quando} (id {})", e.get("id").and_then(Value::as_str).unwrap_or("?")),
            None => format!("{SEM_GOOGLE}: não criei evento; mandei o botão \"Salvar na agenda\" pra cada um ({quando})"),
        };
        if ev.is_some() {
            let convidados: Vec<&str> = pessoas.iter().filter(|p| p.2.is_some()).map(|p| p.1.as_str()).collect();
            let sem: Vec<&str> = pessoas.iter().filter(|p| p.2.is_none()).map(|p| p.1.as_str()).collect();
            if !convidados.is_empty() {
                r += &format!("; convite do Google pra {}", convidados.join(", "));
            }
            if !sem.is_empty() {
                r += &format!("; sem e-mail cadastrado (recebeu só o botão Salvar na agenda): {}", sem.join(", "));
            }
        }
        if !avisei.is_empty() {
            r += &format!("; avisei aqui: {}", avisei.join(", "));
        }
        if !fora.is_empty() {
            r += &format!("; não está no Telegram comigo: {}", fora.join(", "));
        }
        Ok(r)
    }

    fn ver_agenda(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let alvo = match opcional(args, "quem") {
            "" | "eu" => self.slug.clone(),
            s => s.to_string(),
        };
        let (nome, email, nomes) = {
            let eq = trava(&self.equipe);
            let m = eq.membro(&alvo).ok_or_else(|| Erro::Valor(format!("{alvo:?} não é da equipe")))?;
            let nomes: Vec<(String, String, Option<String>)> = eq.membros.iter().map(|m| (m.slug.clone(), eq.primeiro_nome(&m.slug), m.email.clone())).collect();
            (eq.primeiro_nome(&alvo), m.email.clone(), nomes)
        };
        let ini = match opcional(args, "inicio") {
            "" => now,
            s => le_dia(s, false)?,
        };
        let fim = match opcional(args, "fim") {
            "" => ini + chrono::Duration::days(7),
            s => le_dia(s, true)?,
        };
        if fim <= ini {
            return Err(Erro::Valor("fim antes do inicio".into()));
        }
        let g = self.google()?;
        let eventos: Vec<Value> = g.list_events(&ini, &fim, None)?.into_iter().filter(|e| quem_do_evento(e).contains(&alvo)).collect();
        let mut linhas = vec![];
        for e in &eventos {
            let quem: Vec<String> = quem_do_evento(e)
                .iter()
                .filter_map(|s| nomes.iter().find(|n| &n.0 == s))
                .map(|(_, n, m)| format!("{n}{}", m.as_deref().map(|m| status(e, m)).unwrap_or_default()))
                .collect();
            let de = nomes.iter().find(|n| n.0 == de_do_evento(e)).map(|n| n.1.as_str()).unwrap_or("?");
            linhas.push(format!("{} · com {} · marcou: {de} [id {}]", agenda_line(e)?, quem.join(", "), e.get("id").and_then(Value::as_str).unwrap_or("?")));
        }
        let mut r = if linhas.is_empty() { format!("nenhuma reunião marcada por mim com {nome} nesse período") } else { format!("reuniões de {nome}:\n{}", linhas.join("\n")) };
        // o resto da agenda dele(a), só como ocupado
        match email {
            None => r += &format!("\n(sem e-mail cadastrado: não vejo a agenda pessoal de {nome})"),
            Some(m) => match g.free_busy(std::slice::from_ref(&m), &ini, &fim)?.pop().map(|x| x.1) {
                Some(Ok(busy)) => {
                    let outros: Vec<String> = busy
                        .iter()
                        .filter_map(|b| Some((from_iso(b.pointer("/start/dateTime")?.as_str()?).ok()?, from_iso(b.pointer("/end/dateTime")?.as_str()?).ok()?)))
                        .filter(|(a, b)| !sobrepoe(a, b, &eventos))
                        .map(|(a, b)| linha_livre(&a.with_timezone(&argos::util::fuso()).naive_local(), &b.with_timezone(&argos::util::fuso()).naive_local()))
                        .collect();
                    if !outros.is_empty() {
                        r += &format!("\nocupado na agenda pessoal (sem detalhes):\n{}", outros.join("\n"));
                    }
                }
                Some(Err(motivo)) => r += &format!("\n(não vejo o livre/ocupado de {nome}: {motivo}; falta compartilhar a agenda com a minha conta)"),
                None => {}
            },
        }
        Ok(r)
    }

    fn livres_juntos(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let pessoas = self.participantes(args)?;
        let dias = args.get("dias").and_then(Value::as_i64).unwrap_or(5).clamp(1, 14);
        let min = args.get("duracao_min").and_then(Value::as_i64).unwrap_or(self.config.reuniao_min).max(5);
        let fds = args.get("fim_de_semana").and_then(Value::as_bool).unwrap_or(false);
        let ini = match opcional(args, "inicio") {
            "" => now,
            s => le_dia(s, false)?.max(now),
        };
        let fim = le_dia(&(ini.date_naive() + chrono::Duration::days(dias)).format("%Y-%m-%d").to_string(), false)?;
        let janela = parse_janela(&format!("{}-{}", self.config.janela_reuniao.0, self.config.janela_reuniao.1))?;
        let emails: Vec<String> = pessoas.iter().filter_map(|p| p.2.clone()).collect();
        let slugs: Vec<&String> = pessoas.iter().map(|p| &p.0).collect();
        let g = self.google()?;
        let mut ocupado: Vec<Value> = g.list_events(&ini, &fim, None)?.into_iter().filter(|e| quem_do_evento(e).iter().any(|s| slugs.contains(&s))).collect();
        let mut nao_vejo = vec![];
        if !emails.is_empty() {
            for (m, r) in g.free_busy(&emails, &ini, &fim)? {
                let nome = pessoas.iter().find(|p| p.2.as_deref() == Some(m.as_str())).map(|p| p.1.clone()).unwrap_or(m);
                match r {
                    Ok(v) => ocupado.extend(v),
                    Err(motivo) => nao_vejo.push(format!("{nome} (não compartilhou o livre/ocupado: {motivo})")),
                }
            }
        }
        nao_vejo.extend(pessoas.iter().filter(|p| p.2.is_none()).map(|p| format!("{} (sem e-mail cadastrado)", p.1)));
        let livres: Vec<_> = horarios_livres(&ocupado, &[janela], ini.naive_local(), dias, min)
            .into_iter()
            .filter(|(a, _)| fds || a.weekday().number_from_monday() <= 5)
            .collect();
        let nomes: Vec<&str> = pessoas.iter().map(|p| p.1.as_str()).collect();
        let mut r = match linhas_livres(&livres, MAX_DIAS_LIVRES) {
            v if v.is_empty() => format!("nenhum horário livre de {min} min em comum ({}) entre {} e {}", nomes.join(", "), self.config.janela_reuniao.0, self.config.janela_reuniao.1),
            v => format!("livres em comum ({}), {min} min, entre {} e {}:\n{}", nomes.join(", "), self.config.janela_reuniao.0, self.config.janela_reuniao.1, v.join("\n")),
        };
        if !nao_vejo.is_empty() {
            r += &format!("\nnão vejo a agenda de: {} (contei como livre, fora as reuniões que eu marquei)", nao_vejo.join(", "));
        }
        Ok(r)
    }

    fn desmarcar_reuniao(&mut self, args: &Value) -> R<String> {
        let id = arg_str(args, "id")?;
        let (eu, chefe) = {
            let eq = trava(&self.equipe);
            (eq.primeiro_nome(&self.slug), eq.membro(&self.slug).is_some_and(|m| m.chefe))
        };
        let e = self.google()?.get(id.trim())?;
        let de = de_do_evento(&e).to_string();
        if de.is_empty() {
            return Err(Erro::Valor("esse evento não foi marcado por mim".into()));
        }
        if de != self.slug && !chefe {
            let nome = trava(&self.equipe).primeiro_nome(&de);
            return Err(Erro::Valor(format!("só quem marcou ({nome}) ou o chefe desmarca")));
        }
        self.google()?.delete_avisando(id.trim())?;
        let linha = agenda_line(&e)?;
        let msg = format!("🚫 <b>{}</b> desmarcou: {}", esc(&eu), esc(&linha));
        let mut avisei = vec![];
        let eu_slug = self.slug.clone();
        for s in quem_do_evento(&e).into_iter().filter(|s| *s != eu_slug) {
            if self.envia_para(&s, &msg, None)?.is_some() {
                avisei.push(trava(&self.equipe).primeiro_nome(&s));
            }
        }
        Ok(format!("desmarcada: {linha}; o Google avisou os convidados{}", if avisei.is_empty() { String::new() } else { format!("; avisei aqui: {}", avisei.join(", ")) }))
    }
}
