# Ariadne

Secretária de uma equipe pequena no Telegram. Cada pessoa conversa com ela no privado, e ela leva
lembretes, recados e tarefas de um pro outro, marca reunião e cuida das viagens a trabalho.

É **um bot só pra equipe toda**: quem administra cria o bot no BotFather uma vez, e os outros só
tocam num link. Ninguém mais precisa criar bot nem fazer login no Google.

Usa o [Argos](https://github.com/Robin-Ud/argos) como biblioteca. O plano completo está no
[`PLANO.md`](PLANO.md).

> **Estado:** fase 1 em andamento: tarefas entre membros e lembretes pessoais. Os botões, a
> cobrança e os recados vêm em seguida.

## Pôr no ar

```bash
cargo build --release
mkdir -p ~/deving/ariadne-dados/credenciais && cd ~/deving/ariadne-dados   # fora do repo: o token nunca vai pro git
echo '{}' > configs.json                      # tudo tem padrão (ver abaixo)
install -m 600 /dev/null credenciais/telegram_token
$EDITOR credenciais/telegram_token            # o token que o BotFather deu
export ARIADNE_DIR=~/deving/ariadne-dados    # sem ela, a pasta atual
ariadne --membro "Fulana" --email fulana@exemplo.com
ariadne --checar-config                       # config, token (getMe) e membros; antes de todo restart
ariadne                                       # roda (systemd na VM)
```

O `--membro` imprime um link `https://t.me/<bot>?start=<código>` pra mandar pra pessoa. Ela toca, o
Telegram abre o bot, ela toca em **Começar** e pronto. O link vale 7 dias e uma vez só. Rodar o
`--membro` de novo pro mesmo nome gera outro link (celular novo, por exemplo). Pode cadastrar com o
bot rodando.

`ariadne --selftest` sobe 3 membros numa pasta temporária contra um Telegram falso local, manda 600
mensagens e imprime o pico de memória (VmHWM; na fase 0, ~5 MB).

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

Tarefa sem prazo já começa a ser cobrada, uma espera (`cobrar_a_cada_h`) depois, dentro da
`janela_cobranca`. Com prazo, o aviso sai no prazo (só o dia = às 09:00).

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
| `cobrar_a_cada_h` | `3` | horas entre cobranças; sem resposta, espaça 1,5× até 72 h |
| `janela_cobranca` | `["08:00", "20:00"]` | cobrança só sai entre essas horas |
| `escalar_apos` | `3` | depois de tantas cobranças sem resposta, avisa quem pediu (uma vez) |
| `conversa_reset_min` | `30` | a conversa com o modelo recomeça depois de tanto tempo parada |

## Pastas

```
configs.json
credenciais/telegram_token          0600
credenciais/anthropic_key           0600, uma chave pra equipe toda
membros/<slug>/membro.json          {nome, email}
membros/<slug>/convite              0600, até a pessoa tocar no link
membros/<slug>/chat_id              0600, depois
membros/<slug>/conversa.jsonl       conversa recente (24 h)
dados/estado.json                   tarefas abertas, recados agendados
dados/historico.jsonl               tarefa fechada (feita, recusada, cancelada), uma por linha
```

## Licença

[FSL-1.1-ALv2](LICENSE.md).
