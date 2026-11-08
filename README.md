# Ariadne

Secretária de uma equipe pequena no Telegram. Cada pessoa conversa com ela no privado, e ela leva
lembretes, recados e tarefas de um pro outro, marca reunião e cuida das viagens a trabalho.

É **um bot só pra equipe toda**: quem administra cria o bot no BotFather uma vez, e os outros só
tocam num link. Ninguém mais precisa criar bot nem fazer login no Google.

Usa o [Argos](https://github.com/Robin-Ud/argos) como biblioteca. O plano completo está no
[`PLANO.md`](PLANO.md).

> **Estado:** fase 0 (esqueleto: entrada por link, um atendente por membro, `/ping`). Recados e
> tarefas vêm na fase 1.

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

## Pastas

```
configs.json
credenciais/telegram_token          0600
membros/<slug>/membro.json          {nome, email}
membros/<slug>/convite              0600, até a pessoa tocar no link
membros/<slug>/chat_id              0600, depois
membros/<slug>/conversa.jsonl       conversa recente (24 h)
```

## Licença

[FSL-1.1-ALv2](LICENSE.md).
