# CLAUDE.md

Ariadne: bot de Telegram de uma equipe pequena (lembretes, recados e tarefas entre membros, agenda,
viagem a trabalho), em Rust, usando o `../argos` como biblioteca. Ainda no plano: ver `PLANO.md`.

## Rodar

```bash
cargo test                     # tudo contra o Telegram falso (src/falso.rs), sem rede
cargo run -- --selftest        # 3 membros, 600 mensagens, VmHWM
cargo run -- --checar-config   # com ARIADNE_DIR: config, token (getMe), membros e Google
cargo run -- --google-login    # login da conta Google dela (navegador)
```

Mapa:
- `config.rs`: configs.json.
- `estado.rs`: `dados/estado.json` (tarefas, recados agendados, mapa do reply). Tarefa fechada vai
  pro `historico.jsonl`. `cliente`/`projeto` da tarefa no `resto` (`liga`, filtro `cliente:<id>`).
- `membros.rs`: membro.json, convite, pareamento.
- `roteador.rs`: o único `getUpdates`, `despacha` por chat.
- `atendente.rs`: thread por membro (`handle`/`tick`, menu por chat). `botoes.rs`: ✅ / 🕒 / ❌ da
  tarefa e a pergunta que vem depois.
- `resumo.rs`: resumo quinzenal do chefe (sem modelo).
- `clientes.rs`: `dados/clientes.json` (fazendas, desde, flexibilidade, estilo, contatos, detalhes,
  projetos), `salvar_cliente`/`anotar_cliente`/`projeto_cliente`/`ver_cliente`; os ativos (com os
  projetos ativos) vão no system.
- `recados.rs`: recado na hora ou agendado (sai no `tick` de quem recebe).
- `agenda.rs`: reunião na agenda Google dela (convite, `extendedProperties.private` com quem marcou,
  quem vai e o cliente), livres juntos pelo freeBusy, ver e desmarcar. Reunião com cliente: os
  contatos com e-mail em `attendees`, Meet opcional (`conferenceData.createRequest` → `hangoutLink`). `google()` cria o `GoogleCalendar` no
  primeiro uso.
- `modelo.rs`: persona, definição das ferramentas, laço com o modelo. `ferramentas.rs`: o que cada
  ferramenta faz.
- `falso.rs`: Telegram, modelo e Google falsos. `main.rs`: CLI.

## Regras

- **Repo público:** nada de nome, dado ou config de equipe real. Fixtures inventados. O que é de uma
  equipe de verdade vai no complemento privado.
- Código que serve pro Argos também (Telegram, Google, util, tempo das tarefas) nasce no
  `../argos`, que é público; aqui só o que é de equipe. Mudou a biblioteca → `cargo test` nos dois.
- As regras do Argos valem aqui: sem tokio, dependências mínimas, números pelos formatadores do
  `argos::util`, erro não derruba o laço, nunca logar token, teste de modelo sem API paga.

## Commits

Padrão do `~/.claude/CLAUDE.md`. Escopos:

| Escopo | O quê |
|---|---|
| `roteador` | `getUpdates`, threads, pareamento por link, menu por chat |
| `tarefas` | delegar, cobrança, aviso de volta, escalar |
| `recados` | recado, resposta |
| `agenda` | link, `.ics`, convite, livres juntos |
| `clientes` | cadastro, fazendas, detalhes, projetos |
| `viagens` | viagem, gastos, relatório |
| `persona` | prompt e ferramentas do modelo |
| `config` | `configs.json`, `--membro`, `--checar-config` |
| `build` | cargo, build da VM |
