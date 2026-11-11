//! Clientes da equipe em `dados/clientes.json`: fazendas, desde quando, flexibilidade, estilo, contatos
//! e os detalhes que a equipe vai anotando. Fica na `Time`, sob a mesma trava do estado; grava com
//! tmp + rename. Qualquer membro cadastra, edita e anota; só o chefe arquiva.
//!
//! O id do cliente é o slug do nome (o mesmo `slug_de` dos membros) e é o que vai nas ferramentas.

use crate::agenda::maiuscula;
use crate::atendente::{trava, Atendente};
use crate::membros::slug_de;
use argos::agenda::fold;
use argos::rascunho::esc;
use argos::util::{agora, arg_str, from_iso, iso, Dt, Erro, R};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// quantos detalhes (os mais novos) vão na ficha
pub const DETALHES_NA_FICHA: usize = 10;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Clientes {
    #[serde(default)]
    pub clientes: Vec<Cliente>,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Flex {
    Baixa,
    Media,
    Alta,
}

impl Flex {
    pub fn de_texto(s: &str) -> R<Flex> {
        Ok(match fold(s.trim()).as_str() {
            "baixa" => Flex::Baixa,
            "media" => Flex::Media,
            "alta" => Flex::Alta,
            o => return Err(Erro::Valor(format!("flexibilidade {o:?}: use baixa, media ou alta"))),
        })
    }

    pub fn nome(self) -> &'static str {
        match self {
            Flex::Baixa => "baixa",
            Flex::Media => "média",
            Flex::Alta => "alta",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Fazenda {
    pub nome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub municipio: Option<String>,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

/// gente do cliente: o e-mail é o que recebe o convite das reuniões
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Contato {
    pub nome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telefone: Option<String>,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

/// o que alguém da equipe anotou: só cresce
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Detalhe {
    pub texto: String,
    /// slug de quem anotou
    pub por: String,
    pub em: String,
}

/// trabalho com começo e fim; pode ser de uma fazenda só. O id é único dentro do cliente
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Projeto {
    pub id: String,
    pub nome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fazenda: Option<String>,
    pub criado: String,
    /// slug de quem criou
    pub por: String,
    /// quando encerrou; None = ativo
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encerrado: Option<String>,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Cliente {
    pub id: String,
    pub nome: String,
    #[serde(default)]
    pub fazendas: Vec<Fazenda>,
    /// início da parceria: AAAA, AAAA-MM ou AAAA-MM-DD
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desde: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flexibilidade: Option<Flex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estilo: Option<String>,
    #[serde(default)]
    pub contatos: Vec<Contato>,
    #[serde(default)]
    pub detalhes: Vec<Detalhe>,
    #[serde(default)]
    pub projetos: Vec<Projeto>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub arquivado: bool,
    #[serde(flatten)]
    pub resto: Map<String, Value>,
}

pub fn arquivo(dir: &Path) -> PathBuf {
    dir.join("dados").join("clientes.json")
}

/// sem arquivo = nenhum cliente. JSON quebrado não sobe: melhor parado do que gravar por cima do cadastro
pub fn carregar(path: &Path) -> R<Clientes> {
    let Ok(b) = std::fs::read(path) else { return Ok(Clientes::default()) };
    serde_json::from_slice(&b).map_err(|e| Erro::Msg(format!("{}: {e}", path.display())))
}

pub fn gravar(c: &Clientes, path: &Path) -> R<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(c)? + "\n")?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// "2021", "2021-03" ou "2021-03-15"
pub fn valida_desde(s: &str) -> R<String> {
    let s = s.trim();
    let ok = match s.len() {
        4 => s.parse::<u32>().is_ok(),
        7 => chrono::NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").is_ok(),
        10 => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok(),
        _ => false,
    };
    if ok {
        Ok(s.to_string())
    } else {
        Err(Erro::Valor(format!("desde {s:?}: use AAAA, AAAA-MM ou AAAA-MM-DD")))
    }
}

/// "2021-03" → "03/2021"; "2021-03-15" → "15/03/2021"
fn fmt_desde(s: &str) -> String {
    let p: Vec<&str> = s.split('-').collect();
    p.iter().rev().copied().collect::<Vec<_>>().join("/")
}

fn mesmo(a: &str, b: &str) -> bool {
    fold(a.trim()) == fold(b.trim())
}

fn texto_ou_none(s: &str) -> Option<String> {
    Some(s.trim().to_string()).filter(|s| !s.is_empty())
}

/// o que o `salvar_cliente` muda: None = fica como está
#[derive(Debug, Default)]
pub struct Mudanca {
    pub nome: Option<String>,
    pub desde: Option<String>,
    pub flexibilidade: Option<Flex>,
    pub estilo: Option<String>,
    pub fazendas_add: Vec<Fazenda>,
    pub fazendas_remover: Vec<String>,
    pub contatos_add: Vec<Contato>,
    pub contatos_remover: Vec<String>,
    pub arquivar: Option<bool>,
}

impl Clientes {
    pub fn achar(&self, id: &str) -> Option<&Cliente> {
        self.clientes.iter().find(|c| c.id == id)
    }

    /// o cliente, ou o erro que volta pro modelo
    pub fn cliente(&self, id: &str) -> R<&Cliente> {
        self.achar(id.trim()).ok_or_else(|| Erro::Valor(format!("cliente {id:?} não existe: confira o id na lista ou pergunte se é cliente novo")))
    }

    fn cliente_mut(&mut self, id: &str) -> R<&mut Cliente> {
        self.clientes.iter_mut().find(|c| c.id == id.trim()).ok_or_else(|| Erro::Valor(format!("cliente {id:?} não existe")))
    }

    pub fn ativos(&self) -> impl Iterator<Item = &Cliente> {
        self.clientes.iter().filter(|c| !c.arquivado)
    }

    /// Cria (sem `id`: o id sai do nome) ou edita (com `id`). Devolve o id e se é novo.
    pub fn salvar(&mut self, id: Option<&str>, m: Mudanca) -> R<(String, bool)> {
        let (id, novo) = match id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => {
                self.cliente(id)?;
                (id.to_string(), false)
            }
            None => {
                let nome = m.nome.as_deref().and_then(texto_ou_none).ok_or_else(|| Erro::Valor("cliente novo precisa do nome".into()))?;
                let id = slug_de(&nome);
                if id.is_empty() {
                    return Err(Erro::Valor(format!("nome sem letra nem número: {nome:?}")));
                }
                if self.achar(&id).is_some() {
                    return Err(Erro::Valor(format!("já tem cliente com o id {id:?}: pra editar, mande o id; se é outro, mude o nome")));
                }
                let c = Cliente {
                    id: id.clone(),
                    nome,
                    fazendas: vec![],
                    desde: None,
                    flexibilidade: None,
                    estilo: None,
                    contatos: vec![],
                    detalhes: vec![],
                    projetos: vec![],
                    arquivado: false,
                    resto: Map::new(),
                };
                self.clientes.push(c);
                (id, true)
            }
        };
        let c = self.cliente_mut(&id)?;
        if let Some(n) = m.nome.as_deref().and_then(texto_ou_none) {
            c.nome = n;
        }
        if let Some(d) = &m.desde {
            c.desde = texto_ou_none(d).map(|d| valida_desde(&d)).transpose()?;
        }
        if let Some(f) = m.flexibilidade {
            c.flexibilidade = Some(f);
        }
        if let Some(e) = &m.estilo {
            c.estilo = texto_ou_none(e);
        }
        for nome in &m.fazendas_remover {
            let antes = c.fazendas.len();
            c.fazendas.retain(|f| !mesmo(&f.nome, nome));
            if c.fazendas.len() == antes {
                return Err(Erro::Valor(format!("{} não tem a fazenda {nome:?}", c.nome)));
            }
        }
        for f in m.fazendas_add {
            if f.nome.trim().is_empty() {
                return Err(Erro::Valor("fazenda sem nome".into()));
            }
            match c.fazendas.iter_mut().find(|x| mesmo(&x.nome, &f.nome)) {
                Some(x) => x.municipio = f.municipio.or(x.municipio.take()),
                None => c.fazendas.push(f),
            }
        }
        for nome in &m.contatos_remover {
            let antes = c.contatos.len();
            c.contatos.retain(|x| !mesmo(&x.nome, nome));
            if c.contatos.len() == antes {
                return Err(Erro::Valor(format!("{} não tem o contato {nome:?}", c.nome)));
            }
        }
        for k in m.contatos_add {
            if k.nome.trim().is_empty() {
                return Err(Erro::Valor("contato sem nome".into()));
            }
            match c.contatos.iter_mut().find(|x| mesmo(&x.nome, &k.nome)) {
                Some(x) => {
                    x.email = k.email.or(x.email.take());
                    x.telefone = k.telefone.or(x.telefone.take());
                }
                None => c.contatos.push(k),
            }
        }
        if let Some(a) = m.arquivar {
            c.arquivado = a;
        }
        Ok((id, novo))
    }

    /// Projeto novo (ou reabre o encerrado de mesmo id). A fazenda tem que ser do cliente. Devolve o id.
    pub fn projeto_novo(&mut self, cliente: &str, nome: &str, fazenda: Option<&str>, por: &str, now: &Dt) -> R<String> {
        let nome = texto_ou_none(nome).ok_or_else(|| Erro::Valor("projeto sem nome".into()))?;
        let id = slug_de(&nome);
        if id.is_empty() {
            return Err(Erro::Valor(format!("nome sem letra nem número: {nome:?}")));
        }
        let c = self.cliente_mut(cliente)?;
        let fazenda = match fazenda.and_then(texto_ou_none) {
            None => None,
            Some(f) => Some(c.fazendas.iter().find(|x| mesmo(&x.nome, &f)).map(|x| x.nome.clone()).ok_or_else(|| Erro::Valor(format!("{} não tem a fazenda {f:?}: cadastre antes (salvar_cliente)", c.nome)))?),
        };
        match c.projetos.iter_mut().find(|p| p.id == id) {
            Some(p) if p.encerrado.is_none() => return Err(Erro::Valor(format!("{} já tem o projeto {:?} (id {id})", c.nome, p.nome))),
            Some(p) => {
                p.encerrado = None;
                p.fazenda = fazenda.or(p.fazenda.take());
            }
            None => c.projetos.push(Projeto { id: id.clone(), nome, fazenda, criado: iso(now), por: por.to_string(), encerrado: None, resto: Map::new() }),
        }
        Ok(id)
    }

    pub fn encerrar_projeto(&mut self, cliente: &str, projeto: &str, now: &Dt) -> R<()> {
        let c = self.cliente_mut(cliente)?;
        let nome = c.nome.clone();
        let p = c.projetos.iter_mut().find(|p| p.id == slug_de(projeto) && p.encerrado.is_none()).ok_or_else(|| Erro::Valor(format!("{nome} não tem projeto ativo {projeto:?}")))?;
        p.encerrado = Some(iso(now));
        Ok(())
    }

    /// O (cliente, projeto) de uma tarefa, conferido: cliente ativo, projeto ativo dele. Projeto sem
    /// cliente vale se só um cliente tem esse projeto.
    pub fn vinculo(&self, cliente: Option<&str>, projeto: Option<&str>) -> R<Option<(String, Option<String>)>> {
        let projeto = projeto.and_then(texto_ou_none).map(|p| slug_de(&p));
        let c = match cliente.and_then(texto_ou_none) {
            Some(id) => self.cliente(&id)?,
            None => {
                let Some(p) = &projeto else { return Ok(None) };
                let v: Vec<&Cliente> = self.ativos().filter(|c| c.projetos_ativos().any(|x| &x.id == p)).collect();
                match v.as_slice() {
                    [c] => *c,
                    [] => return Err(Erro::Valor(format!("nenhum cliente tem o projeto ativo {p:?}"))),
                    _ => return Err(Erro::Valor(format!("mais de um cliente tem o projeto {p:?}: mande o cliente"))),
                }
            }
        };
        if c.arquivado {
            return Err(Erro::Valor(format!("{} está arquivado", c.nome)));
        }
        if let Some(p) = &projeto {
            if !c.projetos_ativos().any(|x| &x.id == p) {
                return Err(Erro::Valor(format!("{} não tem projeto ativo {p:?}", c.nome)));
            }
        }
        Ok(Some((c.id.clone(), projeto)))
    }

    pub fn anotar(&mut self, id: &str, texto: &str, por: &str, now: &Dt) -> R<()> {
        let texto = texto_ou_none(texto).ok_or_else(|| Erro::Valor("detalhe vazio".into()))?;
        let c = self.cliente_mut(id)?;
        c.detalhes.push(Detalhe { texto, por: por.to_string(), em: iso(now) });
        Ok(())
    }
}

/// "Boa Vista (Rio Verde), Santa Rita"
fn texto_fazendas(c: &Cliente) -> String {
    c.fazendas.iter().map(|f| match &f.municipio {
        Some(m) => format!("{} ({m})", f.nome),
        None => f.nome.clone(),
    }).collect::<Vec<_>>().join(", ")
}

impl Cliente {
    pub fn projetos_ativos(&self) -> impl Iterator<Item = &Projeto> {
        self.projetos.iter().filter(|p| p.encerrado.is_none())
    }
}

/// a linha do cliente no system: "- Nome (`id`): fazendas Boa Vista, Santa Rita · projetos Pivô (`pivo`)"
pub fn linha(c: &Cliente) -> String {
    let mut partes = vec![];
    if !c.fazendas.is_empty() {
        partes.push(format!("fazendas {}", c.fazendas.iter().map(|f| f.nome.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let ps: Vec<String> = c.projetos_ativos().map(|p| format!("{} (`{}`)", p.nome, p.id)).collect();
    if !ps.is_empty() {
        partes.push(format!("projetos {}", ps.join(", ")));
    }
    let mut l = format!("- {} (`{}`)", c.nome, c.id);
    if !partes.is_empty() {
        l += &format!(": {}", partes.join(" · "));
    }
    l
}

/// A ficha do cliente, em HTML do Telegram. `nome` dá o primeiro nome de quem anotou; `extras` são
/// blocos prontos (tarefas, reuniões) que entram antes dos detalhes.
pub fn ficha(c: &Cliente, nome: impl Fn(&str) -> String, extras: &[String]) -> String {
    let mut l = vec![format!("🌾 <b>{}</b>{}", esc(&c.nome), if c.arquivado { " <i>(arquivado)</i>" } else { "" })];
    if !c.fazendas.is_empty() {
        l.push(format!("Fazendas: {}", esc(&texto_fazendas(c))));
    }
    let mut perfil = vec![];
    if let Some(d) = &c.desde {
        perfil.push(format!("cliente desde {}", fmt_desde(d)));
    }
    if let Some(f) = c.flexibilidade {
        perfil.push(format!("flexibilidade {}", f.nome()));
    }
    if !perfil.is_empty() {
        l.push(esc(&maiuscula(&perfil.join(" · "))));
    }
    if let Some(e) = &c.estilo {
        l.push(format!("Estilo: {}", esc(e)));
    }
    if !c.contatos.is_empty() {
        let k: Vec<String> = c
            .contatos
            .iter()
            .map(|k| {
                let meios: Vec<&str> = [k.email.as_deref(), k.telefone.as_deref()].into_iter().flatten().collect();
                if meios.is_empty() { k.nome.clone() } else { format!("{} ({})", k.nome, meios.join(" · ")) }
            })
            .collect();
        l.push(format!("Contatos: {}", esc(&k.join(", "))));
    }
    let mut blocos = vec![l.join("\n")];
    let ps: Vec<String> = c.projetos_ativos().map(|p| format!("• {}{}", esc(&p.nome), p.fazenda.as_deref().map(|f| format!(" ({})", esc(f))).unwrap_or_default())).collect();
    let encerrados = c.projetos.len() - ps.len();
    if !ps.is_empty() || encerrados > 0 {
        let mut b = vec!["<b>Projetos</b>".to_string()];
        b.extend(ps);
        if encerrados > 0 {
            b.push(format!("<i>{encerrados} encerrado{}</i>", if encerrados == 1 { "" } else { "s" }));
        }
        blocos.push(b.join("\n"));
    }
    blocos.extend(extras.iter().filter(|e| !e.is_empty()).cloned());
    if !c.detalhes.is_empty() {
        let pula = c.detalhes.len().saturating_sub(DETALHES_NA_FICHA);
        let mut d = vec![format!("<b>Detalhes</b>{}", if pula > 0 { format!(" (os {DETALHES_NA_FICHA} mais novos)") } else { String::new() })];
        for x in &c.detalhes[pula..] {
            let dia = from_iso(&x.em).map(|t| t.format("%d/%m/%y").to_string()).unwrap_or_default();
            d.push(format!("{dia} {}: {}", esc(&nome(&x.por)), esc(&x.texto)));
        }
        blocos.push(d.join("\n"));
    }
    blocos.join("\n\n")
}

// --- ferramentas ---

fn opcional(args: &Value, k: &str) -> Option<String> {
    args.get(k).and_then(Value::as_str).map(|s| s.trim().to_string())
}

fn lista_de_texto(args: &Value, k: &str) -> Vec<String> {
    args.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()).collect()).unwrap_or_default()
}

/// `[{nome, municipio?}]` ou `["nome"]`
fn fazendas_de(args: &Value) -> Vec<Fazenda> {
    let Some(a) = args.get("fazendas_add").and_then(Value::as_array) else { return vec![] };
    a.iter()
        .filter_map(|f| match f {
            Value::String(n) => Some(Fazenda { nome: n.trim().to_string(), municipio: None, resto: Map::new() }),
            Value::Object(_) => Some(Fazenda { nome: opcional(f, "nome")?, municipio: opcional(f, "municipio").filter(|m| !m.is_empty()), resto: Map::new() }),
            _ => None,
        })
        .collect()
}

fn contatos_de(args: &Value) -> Vec<Contato> {
    let Some(a) = args.get("contatos_add").and_then(Value::as_array) else { return vec![] };
    a.iter()
        .filter_map(|k| {
            let nada = |s: String| Some(s).filter(|s| !s.is_empty());
            Some(Contato { nome: opcional(k, "nome")?, email: opcional(k, "email").and_then(nada), telefone: opcional(k, "telefone").and_then(nada), resto: Map::new() })
        })
        .collect()
}

impl Atendente {
    pub fn clientes(&mut self, nome: &str, args: &Value) -> R<String> {
        match nome {
            "salvar_cliente" => self.salvar_cliente(args),
            "anotar_cliente" => {
                let id = arg_str(args, "id")?;
                let mut eq = trava(&self.equipe);
                eq.clientes.anotar(&id, &arg_str(args, "texto")?, &self.slug, &agora())?;
                eq.gravar_clientes()?;
                Ok(format!("anotado em {}", eq.clientes.cliente(&id)?.nome))
            }
            "projeto_cliente" => {
                let (cliente, nome) = (arg_str(args, "cliente")?, arg_str(args, "nome")?);
                let encerrar = args.get("encerrar").and_then(Value::as_bool).unwrap_or(false);
                let mut eq = trava(&self.equipe);
                let r = if encerrar {
                    eq.clientes.encerrar_projeto(&cliente, &nome, &agora())?;
                    format!("projeto {nome} encerrado")
                } else {
                    let id = eq.clientes.projeto_novo(&cliente, &nome, opcional(args, "fazenda").as_deref(), &self.slug, &agora())?;
                    format!("projeto {nome} criado (id {id})")
                };
                eq.gravar_clientes()?;
                Ok(format!("{r} em {}", eq.clientes.cliente(&cliente)?.nome))
            }
            "ver_cliente" => {
                let id = arg_str(args, "id")?.trim().to_string();
                trava(&self.equipe).clientes.cliente(&id)?;
                // a agenda é rede: fora da trava
                let reunioes = self.reunioes_do_cliente(&id);
                let eq = trava(&self.equipe);
                let c = eq.clientes.cliente(&id)?;
                let tarefas = self.bloco_tarefas(&eq, c);
                Ok(format!("{}\n{}", crate::agenda::BLOCO, ficha(c, |s| eq.primeiro_nome(s), &[tarefas, reunioes])))
            }
            _ => Err(Erro::Valor(format!("ferramenta desconhecida: {nome}"))),
        }
    }

    /// "<b>Tarefas abertas</b>\n• #3 mandar o orçamento · faz Beltrano · Pivô · até sex 13/11 09:00"
    /// (as que quem fala pode ver); vazio se não tem
    fn bloco_tarefas(&self, eq: &crate::atendente::Time, c: &Cliente) -> String {
        use crate::estado::{self, Filtro};
        let ts = estado::visiveis(&eq.estado, &self.slug, &Filtro::Cliente(c.id.clone()));
        if ts.is_empty() {
            return String::new();
        }
        let mut l = vec!["<b>Tarefas abertas</b>".to_string()];
        for t in ts {
            let mut notas = vec![if estado::pessoal(t) { "lembrete seu".to_string() } else { format!("faz {}", eq.primeiro_nome(estado::para(t))) }];
            if let Some(p) = estado::projeto(t) {
                notas.push(c.projetos.iter().find(|x| x.id == p).map(|x| x.nome.clone()).unwrap_or_else(|| p.to_string()));
            }
            let prazo = crate::ferramentas::nota_prazo(t);
            if let Some(p) = prazo.trim().strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
                notas.push(p.to_string());
            }
            l.push(format!("• #{} {} · {}", t.id, esc(&estado::titulo(t)), esc(&notas.join(" · "))));
        }
        l.join("\n")
    }

    fn salvar_cliente(&mut self, args: &Value) -> R<String> {
        let arquivar = args.get("arquivar").and_then(Value::as_bool);
        let mut eq = trava(&self.equipe);
        if arquivar.is_some() && !eq.membro(&self.slug).is_some_and(|m| m.chefe) {
            return Err(Erro::Valor("só o chefe arquiva ou desarquiva cliente".into()));
        }
        let m = Mudanca {
            nome: opcional(args, "nome"),
            desde: opcional(args, "desde"),
            flexibilidade: opcional(args, "flexibilidade").filter(|f| !f.is_empty()).map(|f| Flex::de_texto(&f)).transpose()?,
            estilo: opcional(args, "estilo"),
            fazendas_add: fazendas_de(args),
            fazendas_remover: lista_de_texto(args, "fazendas_remover"),
            contatos_add: contatos_de(args),
            contatos_remover: lista_de_texto(args, "contatos_remover"),
            arquivar,
        };
        // erro no meio não deixa o cadastro pela metade
        let antes = eq.clientes.clone();
        let (id, novo) = match eq.clientes.salvar(opcional(args, "id").as_deref(), m) {
            Ok(r) => r,
            Err(e) => {
                eq.clientes = antes;
                return Err(e);
            }
        };
        eq.gravar_clientes()?;
        let c = eq.clientes.cliente(&id)?;
        let o_que = match (novo, arquivar) {
            (true, _) => "cadastrado",
            (_, Some(true)) => "arquivado",
            (_, Some(false)) => "desarquivado",
            _ => "atualizado",
        };
        Ok(format!("{} {o_que} (id {id}). {}\n{}", c.nome, crate::agenda::BLOCO, ficha(c, |s| eq.primeiro_nome(s), &[])))
    }
}
