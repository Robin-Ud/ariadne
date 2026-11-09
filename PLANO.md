# Ariadne: plano

Bot de Telegram de **uma equipe pequena** (ex.: 3 sócios de uma consultoria): cada um conversa com ela
no privado, e ela leva lembretes, recados e tarefas de um pro outro, marca reunião e cuida das
viagens a trabalho. É um bot só pra equipe toda: ninguém precisa criar bot no BotFather nem fazer
login no Google pra entrar.

Usa o [Argos](https://github.com/Robin-Ud/argos) como biblioteca (Telegram, Claude, Google, util e as
regras de tempo das tarefas), sem copiar código. O que é de uma equipe específica (nomes, configs,
deploy) fica num complemento privado, como o `argos-boi` faz pro Argos.

## Persona

Uma secretária da equipe, com nome de gente mas não banal: incomum o bastante pra não confundir com
cliente ou contato.

- Curta. Chama cada um pelo nome e diz o que fez ("Mandei pro Fulano, ele já recebeu").
- **É uma pessoa só pra equipe toda**, mesmo atendendo cada um no privado: sabe o que o outro pediu
  e lembra quem está esperando o quê.
- Lembra e avisa quando algo está parado, mas não decide prioridade por ninguém.
- `assistente` = "a Ariadne" (o nome muda pelo `configs.json`, como no Argos).

## Arquitetura

```
ARIADNE_DIR/
  configs.json            o que é da equipe toda: assistente, fuso, modelo, janelas padrão
  credenciais/            telegram_token, anthropic_key, google_* (da conta que organiza), 0600
  dados/estado.json       estado da equipe: tarefas, recados agendados, viagens
  dados/viagens/<id>/     comprovantes
  membros/<slug>/         membro.json ({nome, email}), chat_id (0600), convite (0600), histórico do
                          modelo, conversa.jsonl
```

- **Roteador:** o Telegram só deixa um processo fazer o `getUpdates` de um bot (um segundo leva
  409). Uma thread puxa os updates e entrega cada um, pelo `from.id`, pra thread do membro (`mpsc`).
- **Uma thread por membro:** o laço é o mesmo do `Bot::run` do Argos (`recv_timeout` no lugar do
  `getUpdates`: chegou update → `handle`; deu o tempo → `tick` daquele membro). Assim o Haiku lento
  de um não trava os outros. Cada thread tem o próprio `Telegram` (o mesmo token, `conversa` na pasta
  do membro) e o próprio `Claude`.
- **Estado da equipe:** `Arc<Mutex<Equipe>>`. O lock vale só pra ler, mudar e salvar (tmp + rename),
  nunca durante uma chamada ao modelo.
- **Membros fora do `configs.json`:** quem é da equipe é quem tem `membros/<slug>/membro.json`. O
  `--membro` cria sem ninguém editar JSON, e entrar gente nova não pede reinício.
- **Pareamento por link:** `ariadne --membro "Nome" [--email x]` cria `membros/<slug>/` e imprime
  `t.me/<bot>?start=<código>`. O membro só toca no link: o Telegram manda `/start <código>`, o
  roteador acha o convite (relê `membros/` quando chega um `/start` desconhecido), grava o `chat_id` e
  apaga o convite. O convite vale 7 dias. Chat desconhecido sem código é ignorado e vai pro log.
- **Menu do "/" por membro:** `setMyCommands` com `scope: {type: chat}`.
- **Dependência:** `argos = { path = "../argos" }` enquanto nada for público. Na hora de publicar,
  passa pra `git` com `rev` fixo, e o `path` local volta por um `[patch]` em
  `~/deving/.cargo/config.toml` (o cargo lê o config das pastas acima; fica fora dos dois repos).
  O `CHRONO_TZ_TIMEZONE_FILTER` do Argos vem junto no `.cargo/config.toml` daqui.

## Dados

```
Tarefa {
  id, texto,
  de: slug, para: Option<slug>,      // None = "alguém precisa…" (🙋 eu pego); de == para = lembrete pessoal
  criada, prazo: Option<iso>,
  status: aberta | feita | recusada | cancelada, feita_em, motivo,
  sem_resposta, cobrar_em, janelas, pausada_ate,   // as regras de tempo do argos::tarefas
  resto                                            // #[serde(flatten)], como no Argos
}
Recado { id, de, para: Vec<slug>, texto, quando: Option<iso> }   // sem quando = sai na hora, não fica no estado
```

**Visibilidade:** quem pediu e quem recebeu veem a tarefa. O lembrete pessoal (`de == para`) só o
dono vê. "Tarefas da equipe" mostra tudo, menos os lembretes pessoais.

## Ferramentas do modelo

| Ferramenta | O quê |
|---|---|
| `delegar` | `para` (enum com os membros + `eu` + `alguem`), `texto`, `prazo`, janelas |
| `recado` | `para` (array, ou `todos`), `texto`, `quando` |
| `ver_tarefas` | `filtro`: `minhas` / `pedi` / `equipe` / `de:<membro>` |
| `concluir`, `adiar`, `recusar` | recusar leva `motivo`; os três avisam quem pediu |
| `marcar_reuniao` | título, início, duração, `quem`, local/link → convite ou link |
| `livres_juntos` | `quem`, dias, duração → `argos::agenda::horarios_livres` sobre o freeBusy |
| `viagem_*`, `registrar_gasto` | ver Viagem |

Regras de custo do Argos valem: o que é determinístico vai pra atalho sem modelo, e nada que muda a
cada mensagem entra no `system` (cache).

## Recados, tarefas e o aviso de volta

- A tarefa entregue ao membro tem ✅ / 🕒 / ❌ (recusar pergunta o motivo).
- **Aviso de volta:** concluiu → "✅ Fulano concluiu X" pra quem pediu. Recusou → o mesmo, com o
  motivo.
- **Cobrança espaçada:** é a do Argos (`tarefas::espera`, janelas, pausa). Depois de `escalar_apos`
  (3) cobranças sem resposta, avisa **quem pediu**, uma vez só ("Fulano não respondeu 3 cobranças de
  X"), e a cobrança segue espaçada até o máximo.
- **Responder recado:** responder (reply do Telegram) a um recado entregue manda a resposta pra quem
  mandou (mapa `message_id` → recado, curto e podado).

## Agenda

Por ordem de custo:

1. **Link "📅 Salvar na agenda":** botão de URL com
   `calendar.google.com/calendar/render?action=TEMPLATE&text=…&dates=…&ctz=…&details=…&location=…`.
   Não precisa de login nenhum. Também manda um `.ics` como documento, pra quem usa Apple ou Outlook.
2. **Convite:** o evento é criado na agenda da conta que organiza, com os membros em `attendees` e
   `sendUpdates=all`. Cada um aceita pelo próprio Google. Quem não tem e-mail no `configs.json`
   recebe o link. Sugestão: uma conta Google só da Ariadne, assim o convite chega "de Ariadne".
3. **Livres juntos:** `POST /freeBusy` com os e-mails. Cada membro só precisa compartilhar o
   "ver livre/ocupado" com a conta da Ariadne (no Workspace isso costuma vir por padrão). Os
   intervalos ocupados viram eventos pro `horarios_livres` do Argos.

No Argos, falta no `gcal`: `create` com `sendUpdates` e um `free_busy`.

## Viagem (vem do Argos)

O `viagens.rs` do Argos (2026-11-08: abrir, gastos por texto, foto e PDF, relatório em CSV + zip,
carro com km) muda pra cá. **Sai do Argos já** (o Argos fica bem pessoal). O último commit com ele é
o `d499ebf` do argos, e o port parte dali: `git -C ../argos show d499ebf:src/viagens.rs`, e o mesmo
pro `rede.rs` (`com_despesa`, `documento_body`) e pros testes. As peças genéricas continuam na
biblioteca: `util::zip_gravar`, `Telegram::send_document`, multipart.

Mudanças na ideia:

- **Cliente fica só na viagem** (viagem → clientes visitados; gasto → cliente opcional). Tarefa e
  recado não têm cliente.
- **Reembolso deixa de ser o centro.** A viagem é: destino, datas, **quem vai** (membros),
  documentos (passagem, reserva) e gastos com **quem pagou**. O relatório tem visões: por pessoa
  (quem pagou o quê, o acerto entre membros), por cliente (o antigo reembolso, `faturado` fora) e por
  categoria.
- PDF de passagem → além do gasto, oferece o voo como evento na agenda (link ou convite pra quem vai).
- **Sai:** o abastecimento que vira gasto pelo rascunho do Argos e o carro alugado como veículo
  temporário no `motor_dir`. Isso é coisa de veículo do Argos. Na Ariadne, combustível é um gasto
  (foto da nota) e carro é só `km_inicial`/`km_final` + `valor_km` opcional.

## Ponte com o Argos pessoal (depois)

`membros[].argos_dir` opcional: a Ariadne põe na caixa de recados do Argos pessoal do membro (o
mesmo `argos::recados::Recado`), e o Argos ganha um destino "equipe" que escreve na caixa da Ariadne.
Funciona só na mesma máquina, como hoje.

## Fases

Cada fase fecha com testes (servidor falso de Telegram/Google, como no Argos; nada de API paga) e
dá pra usar sozinha.

0. **Esqueleto:** crate, `configs.json`, roteador + threads, `--membro` + link, `/ping`, menu por
   chat, `--checar-config`, `--selftest` (RSS com 3 membros).
1. **Recados e tarefas:** `delegar`, `recado`, `ver_tarefas`, ✅/🕒/❌, aviso de volta, cobrança
   espaçada, escalar pra quem pediu, responder recado.
2. **Agenda:** link + `.ics`, convite, livres juntos.
3. **Viagem:** o port a partir do `d499ebf` do argos, já com as mudanças acima.
4. **Equipe:** encaminhamentos de reunião (texto → tarefas pra cada um, com ✅ antes de gravar),
   🙋 eu pego, fechamento da semana.
5. Ponte com o Argos pessoal.

## v1 (próximo passo, 09/11/2026)

v1 = fase 1 + no ar na VM. Hoje (fase 0) ela tem roteador, convite, `/ping` e `/equipe`.

Fase 1:
1. Estado da equipe (`dados/estado.json`, lock, tmp + rename). A tarefa é a `argos::tarefas::Tarefa`
   com `de`/`para` no `resto`: `espera`, `encaixe` e `proximo_permitido` servem sem mexer no argos.
2. Laço com o modelo próprio, sobre `Claude::create` (o `agenda_ask` do argos é preso ao `Bot`):
   persona, histórico por membro, cache, `log_uso`. É a peça maior.
3. Ferramentas `delegar`, `recado` (na hora ou com `quando`), `ver_tarefas`, `concluir`/`adiar`/`recusar`.
4. Entrega com ✅/🕒/❌ e aviso de volta pra quem pediu (com o motivo na recusa).
5. `tick`: cobrança espaçada, recado agendado, escalar depois de 3 sem resposta.
6. Reply a um recado → resposta pra quem mandou.
7. Testes com o Telegram falso e o modelo simulado.

No ar: `anthropic_key` da equipe, `build-ol9.sh` (como o do argos), usuário + pasta + unit do systemd
na VM (sudo é do Matheus), convites gerados lá.

Pra decidir: o link "📅 Salvar na agenda" (sem login) entra na v1? Se a viagem é o que o patrão
espera, a fase 3 vem antes da 2?

## Em aberto

- Conta Google: uma da Ariadne (recomendado) ou a de um membro?
- Username do bot (o `@…Bot` tem que ser único), fica no complemento.
- Chave da Anthropic: uma pra equipe (padrão); o `log_uso` separa por membro.
- Viagem de quem não está na equipe (cliente que vai junto)? Por ora só membros.
