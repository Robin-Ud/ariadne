# Ariadne

Secretária de uma equipe pequena no Telegram. Cada pessoa conversa com ela no privado, e ela leva
lembretes, recados e tarefas de um pro outro, marca reunião e cuida das viagens a trabalho.

É **um bot só pra equipe toda**: quem administra cria o bot no BotFather uma vez, e os outros só
tocam num link. Ninguém mais precisa criar bot nem fazer login no Google.

Usa o [Argos](https://github.com/Robin-Ud/argos) como biblioteca. O plano completo está no
[`PLANO.md`](PLANO.md).

> **Estado:** fase 1 (tarefas entre membros, lembretes pessoais, cobrança e recados) e fase 2
> (agenda: reunião com convite, livres juntos) prontas no código, testadas contra Telegram, modelo e
> Google falsos.

## Pôr no ar

```bash
cargo build --release
mkdir -p ~/deving/ariadne-dados/credenciais && cd ~/deving/ariadne-dados   # fora do repo: o token nunca vai pro git
echo '{}' > configs.json                      # tudo tem padrão (ver abaixo)
install -m 600 /dev/null credenciais/telegram_token
$EDITOR credenciais/telegram_token            # o token que o BotFather deu
install -m 600 /dev/null credenciais/anthropic_key
$EDITOR credenciais/anthropic_key             # a chave da equipe (sem ela, só /ping e /equipe)
export ARIADNE_DIR=~/deving/ariadne-dados    # sem ela, a pasta atual
ariadne --membro "Fulana" --email fulana@exemplo.com
ariadne --checar-config                       # config, token (getMe), chave e membros; antes de todo restart
ariadne                                       # roda (systemd na VM)
```

Membro já pareado com `--email` ou `--chefe` só é atualizado, sem link novo, e o bot rodando pega a
mudança em até 30 s. Link novo (celular novo) é com `--membro "Nome"` sozinho.

`--chefe` marca o membro como chefe (pode pôr depois, rodando de novo com o mesmo nome): o pedido
dele chega em destaque e ele recebe o resumo quinzenal (ver Chefe).

O `--membro` imprime um link `https://t.me/<bot>?start=<código>` pra mandar pra pessoa. Ela toca, o
Telegram abre o bot, ela toca em **Começar** e pronto. O link vale 7 dias e uma vez só. Rodar o
`--membro` de novo pro mesmo nome gera outro link (celular novo, por exemplo). Pode cadastrar com o
bot rodando.

**Agenda (opcional):** uma conta Google só dela. Logado nessa conta, no [Google Cloud
Console](https://console.cloud.google.com/):

1. crie um projeto e ligue a **Google Calendar API**;
2. na tela de consentimento OAuth, escolha **Externo** e **publique o app** (em produção). Em teste, o
   token vence em 7 dias. Publicar pede, no Branding, a página inicial e a política de privacidade:
   servem o link deste repo e o do [`PRIVACIDADE.md`](PRIVACIDADE.md). Sem logotipo, que obriga a
   verificar o app;
3. em Credenciais, crie um **ID do cliente OAuth** do tipo **App para computador** e salve o JSON como
   `credenciais/google_client.json`;
4. numa máquina com navegador, rode `ariadne --google-login` e entre com a conta dela. O app não é
   verificado, então o Google avisa: é só ir em Avançado → continuar. O `credenciais/google_token.json`
   que sai dali vai pro servidor;
5. cada membro compartilha a própria agenda com o e-mail dela, com **"Ver apenas livre/ocupado"**, e
   ganha o e-mail na Ariadne (`--membro "Nome" --email x`).

O `--checar-config` mostra de quem ela vê o livre/ocupado. Sem o login, "marca reunião" manda só o
botão "📅 Salvar na agenda".

**Na VM** (Oracle Linux 9): `./build-ol9.sh` compila num container `oraclelinux:9` (compilar no
Fedora dá um binário que pede glibc nova demais) e deixa o binário em `target/ol9/release/ariadne`.
Precisa do `../argos` ao lado. Lá, o binário vai pra `/usr/local/bin/` e o serviço é o
[`exemplos/ariadne.service`](exemplos/ariadne.service), trocando `SEU_USUARIO`.

`ariadne --selftest` sobe 3 membros numa pasta temporária contra um Telegram falso local, manda 600
mensagens e imprime o pico de memória (VmHWM no release: ~5,5 MB).

## Comandos

Cada membro tem o próprio menu do "/":

- `/ping`: ela responde se está no ar.
- `/equipe`: quem está na equipe. ✅ pareado, ⏳ convite mandado, ➖ sem convite.
- `/ajuda`: o que ela faz.

Fora dos comandos, é conversa: o texto vai pro modelo (precisa de `credenciais/anthropic_key`), que
usa as ferramentas dela. A conversa fica na memória e recomeça depois de `conversa_reset_min` parada.

## Tarefas

- **Delegar:** "pede pro Fulano mandar a proposta até sexta". O Fulano recebe na hora ("📌 Beltrana
  te pediu: …").
- **Lembrete pessoal:** "me lembra de ligar pro contador amanhã". Só quem pediu vê.
- **Ver:** "o que tenho pra fazer?", "o que eu pedi?", "o que a equipe tem?", "o que o Fulano tem?".
- **Fechar:** quem faz conclui, adia ou recusa (com motivo), e quem pediu recebe o aviso. Quem pediu
  pode cancelar, e quem ia fazer recebe o aviso.
- **Botões:** a tarefa chega com ✅ feito / 🕒 depois / ❌ recusar. O 🕒 pergunta "pra quando?" ("18h",
  "amanhã 9", "sexta às 14") e o ❌ pergunta o motivo, sem passar pelo modelo. A pergunta vale 15 min.
  No lembrete pessoal, o ❌ vira 🗑 cancelar.

## Recados

"Avisa o Fulano que chego 10h", "fala pra todo mundo que amanhã não tem reunião". Sai na hora
("💬 Beltrana: chego 10h") ou, com hora ("amanhã às 8 avisa…"), fica agendado e sai às 8. Recado
não é tarefa: ninguém é cobrado. Agendado pra quem nunca entrou some depois de 7 dias.

Pra responder, é só responder (reply) à mensagem do recado: a resposta vai direto pra quem mandou
("↩️ Beltrano respondeu ao seu recado …"), sem passar pelo modelo. Vale por 7 dias.

## Cobrança

Tarefa sem prazo já começa a ser cobrada ("🔔 …", com os botões), uma espera (`cobrar_a_cada_h`)
depois, dentro da `janela_cobranca`. Com prazo, o "⏰" sai no prazo (só o dia = às 09:00), e a
cobrança começa dali. Sem resposta, cada cobrança espaça 1,5× a anterior (24 h → 36 h → 54 h → 72
h), e o intervalo nunca fica abaixo de `cobrar_a_cada_h`: com o padrão, nunca saem duas no mesmo dia.
Concluir, adiar ou recusar para tudo. São as regras de tempo do Argos.

Depois de `escalar_apos` cobranças sem resposta, quem pediu recebe "⚠️ Fulano não respondeu 3
cobranças de …", uma vez só. A cobrança continua.

## Chefe

Quem tem `"chefe": true` no `membro.json` (`--membro "Nome" --chefe`):

- **Pedido em destaque:** a tarefa que ele pede pra alguém chega como "📌 Fulano (chefe) te pediu: ⭐
  …" e é cobrada todo dia no mesmo horário, sem espaçar (ainda no máximo uma por dia). O escalar
  depois de `escalar_apos` vale igual.
- **Resumo quinzenal:** sexta às 17h, a cada 14 dias, só pra ele. Sai sem modelo: quantas cada um
  concluiu, as recusadas com o motivo, as canceladas, e o que está aberto por pessoa, com o que
  está atrasado e o que tem 2+ cobranças sem resposta. Lembrete pessoal dos outros não entra.

## Agenda

As reuniões ficam na agenda Google dela, que é quem organiza.

- **Marcar:** "marca reunião com o Fulano quinta 15h". Antes de marcar, ela confere o livre/ocupado
  de todos. Se alguém está ocupado, ela não marca, diz quem e pergunta. Quem tem e-mail recebe o
  convite do Google e aceita por lá. Todo mundo recebe o aviso aqui, em bloco ("📅 Beltrana marcou",
  dia e hora em negrito, assunto, com quem), e quem não tem e-mail recebe também o botão "📅 Salvar
  na agenda".
- **Livres juntos:** "quando a gente tá livre essa semana?". Ela junta o livre/ocupado de cada um com
  as reuniões que já marcou e devolve os buracos em dia útil, dentro da `janela_reuniao`. Quem não
  compartilhou a agenda conta como livre, e ela avisa.
- **Ver:** "o que tenho essa semana?". Um bloco por dia útil: as reuniões que ela marcou, com quem
  confirmou (✅ aceitou, ❌ recusou, ❔ talvez, ⏳ não respondeu), e os horários livres dentro da
  `janela_reuniao` (buraco de 30 min ou mais), tirados do livre/ocupado da agenda pessoal. Fim de
  semana só aparece se tem reunião. O id do evento fica só com ela.
- **Desmarcar:** só quem marcou ou o chefe. O Google avisa os convidados, e ela avisa aqui.

O que é de cada reunião (quem marcou, quem vai) fica no próprio evento, em
`extendedProperties.private`. Nada de agenda vai pro `estado.json`.

## Como funciona

Um processo só faz o `getUpdates`, porque o Telegram não deixa dois processos lerem o mesmo bot. Ele
entrega cada update, pelo chat, pra thread daquele membro, e cada membro tem a sua. Chat que não é
de ninguém só entra com o `/start <código>` do link; o resto é ignorado e vai pro log. Grupo é
sempre ignorado.

## `configs.json`

| Chave | Padrão | O quê |
|---|---|---|
| `assistente` | `"a Ariadne"` | o nome dela, com artigo |
| `fuso` | `"America/Sao_Paulo"` | só os fusos do `.cargo/config.toml` vão no binário |
| `cred_dir` | `"credenciais"` | relativo = a partir da pasta da Ariadne |
| `modelo` | `"claude-haiku-5-5"` | o modelo da conversa |
| `esforco` | `"medium"` | o esforço do modelo |
| `cobrar_a_cada_h` | `24` | horas entre cobranças (24 = no máximo uma por dia); sem resposta, espaça 1,5× até 72 h |
| `janela_cobranca` | `["08:00", "20:00"]` | cobrança só sai entre essas horas |
| `escalar_apos` | `3` | depois de tantas cobranças sem resposta, avisa quem pediu (uma vez) |
| `resumo_dia` | `"sexta"` | dia do resumo do chefe (segunda…domingo) |
| `resumo_hora` | `"17:00"` | hora do resumo |
| `resumo_a_cada_dias` | `14` | período do resumo (7, 14, 21…) |
| `janela_reuniao` | `["09:00", "18:00"]` | onde o "livres juntos" procura horário |
| `reuniao_min` | `60` | duração padrão da reunião |
| `conversa_reset_min` | `30` | a conversa com o modelo recomeça depois de tanto tempo parada |

## Pastas

```
configs.json
credenciais/telegram_token          0600
credenciais/anthropic_key           0600, uma chave pra equipe toda
credenciais/google_client.json      o cliente OAuth (agenda, opcional)
credenciais/google_token.json       0600, do --google-login da conta dela
membros/<slug>/membro.json          {nome, email, chefe}
membros/<slug>/convite              0600, até a pessoa tocar no link
membros/<slug>/chat_id              0600, depois
membros/<slug>/conversa.jsonl       conversa recente (24 h)
dados/estado.json                   tarefas abertas, recados agendados
dados/historico.jsonl               tarefa fechada (feita, recusada, cancelada), uma por linha
```

## Licença

[FSL-1.1-ALv2](LICENSE.md).
