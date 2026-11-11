//! Agenda da equipe, na conta Google dela (fase 2).
//!
//! Reunião = evento na agenda dela com os membros em `attendees` (o Google manda o convite) e quem
//! marcou e quem vai no `extendedProperties.private` (`ariadne_de`, `ariadne_quem`): assim ela acha as
//! reuniões de cada um, até de quem não tem e-mail. O livre/ocupado de cada um vem do freeBusy (a pessoa
//! compartilha "ver apenas livre/ocupado" com a conta dela). Todo mundo também recebe o aviso aqui; quem
//! não tem e-mail recebe o botão "📅 Salvar na agenda". Sem o `--google-login`, marcar manda só o botão.

use crate::atendente::{trava, Atendente};
use argos::agenda::{fold, horarios_livres, linha_livre, linhas_livres, parse_janela};
use argos::gcal::{link_salvar, GoogleCalendar};
use argos::rascunho::esc;
use argos::util::{agora, arg_str, from_iso, fuso, so_data, Dt, Erro, R};
use chrono::{Datelike, NaiveDateTime, NaiveTime};
use serde_json::{json, Value};

/// escopos do `--google-login` além do `calendar.events`: o livre/ocupado das agendas que ela enxerga
pub const ESCOPOS_EXTRA: [&str; 2] = ["https://www.googleapis.com/auth/calendar.events.freebusy", "https://www.googleapis.com/auth/calendar.freebusy"];
const SEM_GOOGLE: &str = "a agenda ainda não está ligada (falta o ariadne --google-login no servidor)";
/// dias de horário livre que vão pro modelo
const MAX_DIAS_LIVRES: usize = 5;
/// buraco menor que isto não aparece como livre no ver agenda
const MIN_LIVRE: i64 = 30;
/// a resposta é um bloco pronto: a persona manda como está (o que vem antes é só pra ela)
pub(crate) const BLOCO: &str = "Mande só este bloco, como está:";

/// quem vai (ids), gravado no evento
fn quem_do_evento(e: &Value) -> Vec<String> {
    e.pointer("/extendedProperties/private/ariadne_quem").and_then(Value::as_str).unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// o cliente da reunião (id); vazio = reunião só da equipe
fn cliente_do_evento(e: &Value) -> &str {
    e.pointer("/extendedProperties/private/ariadne_cliente").and_then(Value::as_str).unwrap_or_default()
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

/// início e fim do evento no fuso configurado (dia inteiro → None). O Google devolve no fuso da
/// agenda dela, que numa conta nova é UTC: sem converter, 18h30 aparece como 21h30
fn horario(e: &Value) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let h = |k: &str| Some(from_iso(e.pointer(&format!("/{k}/dateTime"))?.as_str()?).ok()?.with_timezone(&fuso()).naive_local());
    Some((h("start")?, h("end")?))
}

/// ("ter 15/01", "10h–11h")
fn dia_e_faixa(a: &NaiveDateTime, b: &NaiveDateTime) -> (String, String) {
    let l = linha_livre(a, b);
    let (d, f) = l.split_once(" · ").unwrap_or((&l, ""));
    (d.to_string(), f.to_string())
}

fn titulo_do(e: &Value) -> &str {
    e.get("summary").and_then(Value::as_str).unwrap_or("(sem título)")
}

pub(crate) fn maiuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}


/// (slug, primeiro nome, e-mail) de cada um
type Pessoa = (String, String, Option<String>);

/// o cliente da reunião: id, nome e os contatos que vão (nome, e-mail)
type DoCliente = (String, String, Vec<(String, Option<String>)>);

/// quantos dias de reunião com o cliente aparecem na ficha
const DIAS_REUNIOES_CLIENTE: i64 = 30;

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

    /// O cliente da reunião e quem dele vai: os `contatos` pedidos, ou todos os que têm e-mail
    fn cliente_da_reuniao(&self, args: &Value) -> R<Option<DoCliente>> {
        let id = opcional(args, "cliente");
        if id.is_empty() {
            return Ok(None);
        }
        let eq = trava(&self.equipe);
        let c = eq.clientes.cliente(id)?;
        if c.arquivado {
            return Err(Erro::Valor(format!("{} está arquivado", c.nome)));
        }
        let pedidos: Vec<&str> = args.get("contatos").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let vao = if pedidos.is_empty() {
            c.contatos.iter().filter(|k| k.email.is_some()).map(|k| (k.nome.clone(), k.email.clone())).collect()
        } else {
            pedidos
                .iter()
                .map(|p| {
                    let k = c.contatos.iter().find(|k| fold(k.nome.trim()) == fold(p.trim()));
                    k.map(|k| (k.nome.clone(), k.email.clone())).ok_or_else(|| Erro::Valor(format!("{} não tem o contato {p:?}: cadastre antes (salvar_cliente)", c.nome)))
                })
                .collect::<R<Vec<_>>>()?
        };
        Ok(Some((c.id.clone(), c.nome.clone(), vao)))
    }

    /// "<b>Próximas reuniões</b>\n• qua 14/01 · 15h–16h Visita" (as marcadas com o cliente nos próximos
    /// dias). Vazio sem Google ou sem reunião; erro do Google vira uma nota, sem derrubar a ficha
    pub fn reunioes_do_cliente(&mut self, id: &str) -> String {
        let now = agora();
        let Ok(g) = self.google() else { return String::new() };
        let mut evs: Vec<Value> = match g.list_events(&now, &(now + chrono::Duration::days(DIAS_REUNIOES_CLIENTE)), None) {
            Ok(v) => v.into_iter().filter(|e| cliente_do_evento(e) == id).collect(),
            Err(e) => return format!("<i>Não consegui ver a agenda ({}).</i>", esc(&e.to_string())),
        };
        if evs.is_empty() {
            return String::new();
        }
        evs.sort_by_key(|e| horario(e).map(|h| h.0));
        let mut l = vec!["<b>Próximas reuniões</b>".to_string()];
        for e in &evs {
            let Some((a, b)) = horario(e) else { continue };
            l.push(format!("• {} {}", esc(&linha_livre(&a, &b)), esc(titulo_do(e))));
        }
        l.join("\n")
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
        let do_cliente = self.cliente_da_reuniao(args)?;
        let meet = args.get("meet").and_then(Value::as_bool).unwrap_or(false);
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
                "attendees": emails.iter().chain(do_cliente.iter().flat_map(|c| c.2.iter().filter_map(|k| k.1.as_ref()))).map(|e| json!({"email": e})).collect::<Vec<_>>(),
                "guestsCanModify": false,
                "extendedProperties": {"private": {"ariadne_de": self.slug, "ariadne_quem": pessoas.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(",")}},
            });
            if !local.is_empty() {
                body["location"] = json!(local);
            }
            if let Some((id, _, _)) = &do_cliente {
                body["extendedProperties"]["private"]["ariadne_cliente"] = json!(id);
            }
            if meet {
                body["conferenceData"] = json!({"createRequest": {"requestId": crate::membros::codigo_convite()?, "conferenceSolutionKey": {"type": "hangoutsMeet"}}});
            }
            ev = Some(self.google()?.create_convidando(&body)?);
        }

        let quando = esc(&linha_livre(&ini.naive_local(), &fim.naive_local()));
        let link_meet = ev.as_ref().and_then(|e| e.get("hangoutLink")).and_then(Value::as_str).map(String::from);
        let link = link_salvar(&titulo, &ini, &fim, &descricao, if local.is_empty() { link_meet.as_deref().unwrap_or_default() } else { &local });
        // o cliente e o Meet, embaixo do com quem
        let mut extra = String::new();
        if let Some((_, nome_c, vao)) = &do_cliente {
            extra += &format!("\n🌾 {}", esc(nome_c));
            if !vao.is_empty() {
                extra += &format!(": {}", esc(&vao.iter().map(|k| k.0.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
        if let Some(m) = &link_meet {
            extra += &format!("\n🎥 {}", esc(m));
        }
        let sobre = if local.is_empty() { esc(&titulo) } else { format!("{} · {}", esc(&titulo), esc(&local)) };
        let com = |slug: &str| {
            let outros: Vec<&str> = pessoas.iter().filter(|p| p.0 != slug).map(|p| p.1.as_str()).collect();
            if outros.is_empty() { "só você".to_string() } else { format!("com {}", esc(&outros.join(", "))) }
        };
        let (mut fora, mut cartao_pra_mim) = (vec![], false);
        for (slug, nome, email) in &pessoas {
            let convite = ev.is_some() && email.is_some();
            // quem marcou fica sabendo pela resposta; o cartão só vai pra ele se precisa do botão
            if slug == &self.slug && convite {
                continue;
            }
            let cab = if slug == &self.slug { "📅 Marcado".to_string() } else { format!("📅 <b>{}</b> marcou", esc(&eu)) };
            let mut msg = format!("{cab}\n<b>{quando}</b>\n{sobre}\n{}{extra}", com(slug));
            if convite {
                msg += "\n<i>Convite no seu e-mail.</i>";
            }
            let botao = (!convite).then(|| json!([[{"text": "📅 Salvar na agenda", "url": link}]]));
            match self.envia_para(slug, &msg, botao)? {
                Some(_) => cartao_pra_mim |= slug == &self.slug,
                None => fora.push(nome.clone()),
            }
        }

        let mut notas = vec![];
        match &ev {
            None => notas.push(format!("{SEM_GOOGLE}: cada um recebeu o botão Salvar na agenda")),
            Some(_) => {
                if pessoas.iter().any(|p| p.0 == self.slug && p.2.is_some()) {
                    notas.push("convite no seu e-mail".to_string());
                }
                let outros = |com_email: bool| pessoas.iter().filter(|p| p.0 != self.slug && p.2.is_some() == com_email).map(|p| p.1.as_str()).collect::<Vec<_>>();
                if !outros(true).is_empty() {
                    notas.push(format!("convite pra {}", outros(true).join(", ")));
                }
                if !outros(false).is_empty() {
                    notas.push(format!("sem e-mail, só o botão: {}", outros(false).join(", ")));
                }
            }
        }
        if meet && ev.is_some() && link_meet.is_none() {
            notas.push("o Google não criou o Meet".to_string());
        }
        if let Some((_, nome_c, vao)) = &do_cliente {
            let com_email: Vec<&str> = vao.iter().filter(|k| k.1.is_some()).map(|k| k.0.as_str()).collect();
            let sem_email: Vec<&str> = vao.iter().filter(|k| k.1.is_none()).map(|k| k.0.as_str()).collect();
            if ev.is_some() && !com_email.is_empty() {
                notas.push(format!("convite pra {}", com_email.join(", ")));
            }
            // quem do cliente não recebe o convite ganha o link (o Meet, se tem; senão o Salvar na agenda)
            let repassa = link_meet.as_deref().unwrap_or(&link);
            if ev.is_none() {
                notas.push(format!("link pra repassar a {nome_c}: {repassa}"));
            } else if vao.is_empty() {
                notas.push(format!("{nome_c} não tem contato com e-mail, repasse o link: {repassa}"));
            } else if !sem_email.is_empty() {
                notas.push(format!("{} sem e-mail, repasse o link: {repassa}", sem_email.join(", ")));
            }
        }
        if !fora.is_empty() {
            notas.push(format!("fora do Telegram: {}", fora.join(", ")));
        }
        let notas = if notas.is_empty() { String::new() } else { format!("<i>{}.</i>", esc(&maiuscula(&notas.join("; ")))) };
        let id = ev.as_ref().and_then(|e| e.get("id")).and_then(Value::as_str).map(|i| format!(" (id {i}, não mostre)")).unwrap_or_default();
        Ok(if cartao_pra_mim {
            let resto = if notas.is_empty() { String::new() } else { format!(" Mande só isto, como está:\n{notas}") };
            format!("marcada{id}. O cartão com o botão já chegou pra quem fala: não repita a reunião.{resto}")
        } else {
            format!("marcada{id}. {BLOCO}\n📅 Marcado\n<b>{quando}</b>\n{sobre}\n{}{extra}\n{notas}", com(&self.slug)).trim_end().to_string()
        })
    }

    fn ver_agenda(&mut self, args: &Value) -> R<String> {
        let now = agora();
        let alvo = match opcional(args, "quem") {
            "" | "eu" => self.slug.clone(),
            s => s.to_string(),
        };
        let (nome, email, nomes, clientes) = {
            let eq = trava(&self.equipe);
            let m = eq.membro(&alvo).ok_or_else(|| Erro::Valor(format!("{alvo:?} não é da equipe")))?;
            let nomes: Vec<(String, String, Option<String>)> = eq.membros.iter().map(|m| (m.slug.clone(), eq.primeiro_nome(&m.slug), m.email.clone())).collect();
            let clientes: Vec<(String, String)> = eq.clientes.clientes.iter().map(|c| (c.id.clone(), c.nome.clone())).collect();
            (eq.primeiro_nome(&alvo), m.email.clone(), nomes, clientes)
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
        let mut eventos: Vec<Value> = g.list_events(&ini, &fim, None)?.into_iter().filter(|e| quem_do_evento(e).contains(&alvo)).collect();
        eventos.sort_by_key(|e| horario(e).map(|h| h.0));
        // a agenda pessoal entra só como ocupado, pra achar os livres
        let (mut ocupado, mut nota) = (eventos.clone(), None);
        match email {
            None => nota = Some(format!("sem e-mail cadastrado: não vejo a agenda pessoal de {nome}")),
            Some(m) => match g.free_busy(std::slice::from_ref(&m), &ini, &fim)?.pop().map(|x| x.1) {
                Some(Ok(busy)) => ocupado.extend(busy),
                Some(Err(motivo)) => nota = Some(format!("não vejo a agenda pessoal de {nome} ({motivo}): falta compartilhar o livre/ocupado com a minha conta")),
                None => {}
            },
        }
        let vejo = nota.is_none();
        let janela = parse_janela(&format!("{}-{}", self.config.janela_reuniao.0, self.config.janela_reuniao.1))?;
        let (de, ate) = (ini.with_timezone(&fuso()).naive_local(), fim.with_timezone(&fuso()).naive_local());
        let n = ((ate - chrono::Duration::seconds(1)).date() - de.date()).num_days() + 1;
        let livres = if vejo { horarios_livres(&ocupado, &[janela], de, n, MIN_LIVRE) } else { vec![] };

        let (mut blocos, mut ids) = (vec![], vec![]);
        for d in 0..n {
            let dia = de.date() + chrono::Duration::days(d);
            let do_dia: Vec<&Value> = eventos.iter().filter(|e| horario(e).is_some_and(|(a, _)| a.date() == dia)).collect();
            let fs: Vec<String> = livres.iter().filter(|(a, _)| a.date() == dia).map(|(a, b)| dia_e_faixa(a, b).1).collect();
            let fds = dia.weekday().number_from_monday() > 5;
            // sem reunião: só dia útil, e hoje só se ainda sobra livre
            if do_dia.is_empty() && (!vejo || fds || (d == 0 && fs.is_empty())) {
                continue;
            }
            let meia_noite = dia.and_time(NaiveTime::MIN);
            let cab = dia_e_faixa(&meia_noite, &meia_noite).0;
            let mut linhas = vec![format!("<b>{cab}</b>")];
            for e in do_dia {
                let Some((a, b)) = horario(e) else { continue };
                let faixa = dia_e_faixa(&a, &b).1;
                let outros: Vec<String> = quem_do_evento(e)
                    .iter()
                    .filter(|s| **s != alvo)
                    .filter_map(|s| nomes.iter().find(|n| &n.0 == s))
                    .map(|(_, n, m)| format!("{n}{}", m.as_deref().map(|m| status(e, m)).unwrap_or_default()))
                    .collect();
                let mut com = if outros.is_empty() { String::new() } else { format!(" · com {}", outros.join(", ")) };
                if let Some((_, c)) = clientes.iter().find(|c| c.0 == cliente_do_evento(e)) {
                    com += &format!(" · cliente {c}");
                }
                linhas.push(format!("  {faixa}  {}{}", esc(titulo_do(e)), esc(&com)));
                let quem_marcou = nomes.iter().find(|n| n.0 == de_do_evento(e)).map(|n| n.1.as_str()).unwrap_or("?");
                ids.push(format!("{cab} {faixa} {} = {}, marcou {quem_marcou}", titulo_do(e), e.get("id").and_then(Value::as_str).unwrap_or("?")));
            }
            if vejo {
                linhas.push(if fs.is_empty() { "  sem horário livre".to_string() } else { format!("  livre: {}", fs.join(", ")) });
            }
            blocos.push(linhas.join("\n"));
        }

        let mut r = if blocos.is_empty() { format!("Nenhuma reunião marcada por mim com {} nesse período.", esc(&nome)) } else { blocos.join("\n\n") };
        if alvo != self.slug {
            r = format!("📅 <b>Agenda de {}</b>\n\n{r}", esc(&nome));
        }
        if let Some(n) = nota {
            r += &format!("\n\n<i>{}.</i>", esc(&maiuscula(&n)));
        }
        let ids = if ids.is_empty() { String::new() } else { format!("(ids pra desmarcar, não mostre: {})\n", ids.join("; ")) };
        Ok(format!("{ids}{BLOCO}\n{r}"))
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
        let quando = horario(&e).map(|(a, b)| linha_livre(&a, &b)).unwrap_or_default();
        let cartao = |cab: &str| format!("{cab}\n<b>{}</b>\n{}", esc(&quando), esc(titulo_do(&e)));
        let msg = cartao(&format!("🚫 <b>{}</b> desmarcou", esc(&eu)));
        let mut avisei = vec![];
        let eu_slug = self.slug.clone();
        for s in quem_do_evento(&e).into_iter().filter(|s| *s != eu_slug) {
            if self.envia_para(&s, &msg, None)?.is_some() {
                avisei.push(trava(&self.equipe).primeiro_nome(&s));
            }
        }
        let mut notas = vec!["o Google avisou os convidados".to_string()];
        if !avisei.is_empty() {
            notas.push(format!("avisei aqui: {}", avisei.join(", ")));
        }
        Ok(format!("desmarcada. {BLOCO}\n{}\n<i>{}.</i>", cartao("🚫 Desmarcado"), esc(&maiuscula(&notas.join("; ")))))
    }
}
