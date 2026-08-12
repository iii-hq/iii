# Compose — o `id` volta a ser o daemon

**Estado: Tasks 1-4 feitas** (2026-08-04). Falta só documentação (Fase 6 do
`implementation.md`) e o commit.

Documento de trabalho, **não commitado** (planos não entram em commit).
Branch: `feat/compose-daemon`. Irmão de `implementation.md` e
`configuration-plan.md`.

**Decisão tomada (2026-08-04): caminho (a).** O daemon é identificado pelo
namespace em que atende, e o operador o alcança com `--namespace`. Um daemon
segura **vários arquivos**, e cada projeto é o seu arquivo.

O flag chama-se `--ns`, não `--id`: ele *é* o namespace, e chamá-lo de outra
coisa obrigava a explicar a relação toda vez.

---

## O que a refatoração trocou sem querer

O tech pack sempre foi multi-máquina: *"several daemons — several machines —
attach to one engine; a trigger addresses one of them by argument"*. A
refatoração ganhou multi-projeto e perdeu isso. Hoje, em
`daemon.rs:306`:

```rust
/// Fixed, and the exclusion depends on it: `(default, compose)` is a lease the
/// engine hands to one connection, so a second daemon is refused at
/// registration rather than left running unreachable.
pub const DAEMON_WORKER_NAME: &str = "compose";
```

Um segundo daemon recebe `DAEMON_ALREADY_SERVING`, e o texto do erro chama
isso de design. Um engine = uma máquina.

O modelo certo é a soma dos dois: **`id` = daemon, arquivo = projeto.**

| conceito | hoje | depois |
|---|---|---|
| `--ns` | não existe | o namespace em que o daemon atende |
| namespace do daemon | `default`, fixo | o `--ns`; **ausente ⇒ uuid gerado e impresso** |
| nome do worker | `compose`, fixo | `compose`, fixo — a lease vira `(namespace, compose)` |
| `namespace=` no payload | não existe | guarda: erra a máquina, `WRONG_DAEMON` |
| `id=` no payload | nomeia um projeto | **removido** |
| chave do projeto | `id` inventado pelo chamador | caminho canônico do arquivo |
| alcance do operador | trigger sem namespace | `--namespace <ns-do-daemon>` |

**Sem `--ns` o daemon se nomeia** (decisão de 2026-08-04): gera um uuid e o
imprime, no foreground e no `-d`. Não existe default seguro — um nome
bem-conhecido compartilhado é exatamente a colisão que o namespace existe para
impedir, e o segundo daemon a reivindicá-lo perde a lease e é recusado.

```bash
iii compose -d
#   namespace: 550e8400-e29b-41d4-a716-446655440000
iii trigger compose::up --namespace 550e8400-... file=./worker-compose.yaml
```

**Consequência aceita:** um namespace gerado é novo a cada start, e agora que o
estado mora sob ele (Task 2), um daemon que reinicia sem `--ns` não reencontra
os filhos que deixou rodando. Quem precisa de readoção passa `--ns` e o mantém.
**Documentar no Quick Start** — é a única pegadinha do modelo.

## Por que `id=` sozinho não roteia

Vale registrar, porque é a pergunta que volta. Com roteamento estrito, uma
trigger sem namespace resolve só no `default`; dois daemons registrando
`compose::up` lá colidem no segundo. Escolher o daemon por um argumento do
payload exigiria a engine despachar por conteúdo, o que ela não faz — ficou
registrado como decisão futura no plan_d. Então quem seleciona é o
`--namespace`, e o `namespace=` do payload fica como guarda: máquina errada
falha alto em vez de subir o projeto no lugar errado.

Forma final:

```bash
iii compose --ns pc-da-xuxa

iii trigger compose::up   --namespace pc-da-xuxa file=finance-process.yaml
iii trigger compose::up   --namespace pc-da-xuxa file=hr-process.yaml
iii trigger compose::list --namespace pc-da-xuxa
iii trigger compose::down --namespace pc-da-xuxa file=hr-process.yaml
```

---

## Tasks — repo `iii`

### Task 1: `--ns` no CLI e no namespace do daemon ✅

**Files:**
- Modify: `crates/iii-compose/src/cli.rs` (`ComposeCli`)
- Modify: `crates/iii-compose/src/daemon.rs:60` (o `EngineClient::connect`)
- Test: `crates/iii-compose/tests/cli.rs`

`EngineClient::connect(address, daemon_id, namespace)` já recebe os dois
separados (`engine.rs:77`) — a refatoração só fixou `("compose", "default")`.
A mudança é passar o `--id` como namespace e manter `compose` como nome.

- [X] **Step 1: testes** — `tests/cli.rs`: `--id` ausente resolve `default`;
      `--id pc-a` vira o namespace e viaja para `Serve`/`Detach`/`Attach`;
      vazio, `/`, `\` e `..` são `INVALID_DAEMON_ID`
- [X] **Step 2: rodar e ver falhar**
- [X] **Step 3: implementar** — `#[arg(long)] pub id: Option<String>` +
      `daemon_id()` validando no `plan()`; `Daemon::start(url, daemon_id)`
      passa o id como namespace e mantém `compose` como nome, então a lease
      vira `(id, compose)`
- [X] **Step 4: rodar e ver passar** — 123 no crate, 9 no e2e
- [X] **Extra, mesma unidade:**
  - o log do daemon vira `~/.iii/compose/<id>/daemon.log` — antes era um
    arquivo só, e dois daemons na mesma máquina se sobrescreveriam
  - `--detach` confirma o daemon **no namespace dele**: um vizinho respondendo
    o `compose::list` reportaria outro pid e a espera nunca terminaria
  - `compose::list` passa a devolver `daemon_id`
  - e2e `two_daemons_with_distinct_ids_both_serve`: ids distintos coexistem,
    cada um responde no próprio endereço, e uma chamada sem namespace não é
    roteada para uma máquina arbitrária
  - a mensagem de "serving" imprime a invocação com o `--namespace` certo
- [X] **Step 5** — entra no commit único desta rodada

**Resolvido depois:** o flag virou `--ns`, `INVALID_DAEMON_ID` virou
`INVALID_NAMESPACE`, e o campo interno `daemon_id` virou `daemon_namespace`.

### Task 2: projeto passa a ser chaveado pelo arquivo ✅

**Files:**
- Modify: `crates/iii-compose/src/daemon.rs` (`projects`, `project()`)
- Modify: `crates/iii-compose/src/project.rs:80` (`StateStore::for_daemon`)
- Modify: `crates/iii-compose/src/state.rs` (layout do diretório)
- Test: `crates/iii-compose/tests/crash_recovery.rs`, `engine/tests/compose_daemon_e2e.rs`

Hoje `projects: BTreeMap<String, Arc<Project>>` é chaveado pelo id inventado, e
`StateStore::for_daemon(&id)` grava em `~/.iii/compose/<esse-id>/state.json`.
Com o daemon dono do id, o projeto precisa de outra chave, e o caminho
canônico do arquivo é a única sem ambiguidade — o `name:` do YAML já é o
namespace dos workers e dois arquivos podem repeti-lo.

Layout novo: `~/.iii/compose/<daemon-id>/<slug-do-arquivo>/state.json`, com o
slug derivado do caminho canônico (hash curto + basename legível, para o
diretório continuar reconhecível a olho).

- [X] **Step 1: testes** — `project_slug` é legível e único (`crash_recovery`);
      e2e `one_file_is_one_project_however_it_is_spelled`
- [X] **Step 2-4** — `StateStore::for_project(ns, path)`;
      `projects: BTreeMap<PathBuf, _>`; `Project::open` sem id
- [X] **Step 5** — entra no commit único desta rodada

**Feito:** `STATE_BINDING_MISMATCH` saiu do caminho do operador. O
`check_binding` continua, mas só dispara em colisão de slug e agora responde
`INVALID_STATE_FILE` — adotar os filhos de outro projeto é grave demais para
confiar em improbabilidade.

**Bug encontrado no caminho:** o cache de pacotes era derivado subindo um nível
a partir do diretório de estado. Como o estado ganhou um nível, virou cache
**por daemon**, contrariando o próprio comentário. Ancorado em
`StateStore::root()`.

### Task 3: `namespace=` vira guarda, `file=` vira o seletor ✅

**Files:**
- Modify: `crates/iii-compose/src/remote.rs` (`ComposeRequest`, `dispatch`)
- Modify: `crates/iii-compose/src/error.rs` (`WrongDaemon`)
- Test: `engine/tests/compose_daemon_e2e.rs`

- [X] **Step 1: testes** — e2e `naming_another_daemon_in_the_payload_is_refused`
      cobre as duas direções; `a_project_scoped_call_names_the_argument_it_wanted`
      cobre a falta de arquivo
- [X] **Step 2-4** — `id` fora do `ComposeRequest`; `MISSING_ID` virou
      `NO_COMPOSE_FILE`; `up`/`down`/`status`/`validate` caem para
      `worker-compose.yaml` no diretório do daemon
- [X] **Step 5** — entra no commit único desta rodada

**Extra, fora do plano:** `compose::status` passou a devolver `state_dir`. O
diretório é derivado do arquivo, então ninguém o adivinha — e um operador
procurando o log de um container não deveria ter que reproduzir um hash.

### Task 4: dois daemons num engine ✅

**Files:**
- Test: `engine/tests/compose_daemon_e2e.rs`

O teste `a_second_daemon_on_one_engine_is_refused` inverte de sentido: com ids
distintos os dois coexistem; com o **mesmo** id o segundo é recusado, que é
como uma identidade de máquina duplicada aparece.

- [X] **Step 1-4** — `two_daemons_with_distinct_ids_both_serve`: ambos servem,
      cada um responde no próprio endereço e reporta a própria identidade, e
      uma chamada sem namespace não é roteada para uma máquina arbitrária.
      `a_second_daemon_on_one_engine_is_refused` inverteu de sentido: agora é o
      caso do **mesmo** namespace
- [X] **Step 5** — entra no commit único desta rodada

---

## Tasks — repo `compose-smoke-tests`

### Task 5: migrar os 17 cenários

Toda chamada hoje é `compose::up id=<projeto> file=<arquivo>`. Com o `id=`
virando guarda e o daemon rodando sem `--id` (logo, `default`), um `id=config-a`
passaria a significar "máquina config-a" e devolveria `WRONG_DAEMON`.

Migração mecânica: dropar o `id=` e passar `file=` onde falta (`down`,
`status`). O daemon dos cenários continua sem `--id`, então nenhuma chamada
precisa de `--namespace` — a ergonomia curta se mantém para máquina única.

- [X] **Step 1-2** — 67 chamadas migradas; `id=` não existe mais em nenhuma
- [X] **Step 3** — cada cenário ganhou `--ns` próprio (determinístico, estado
      isolado), então nenhum compartilha `default`. O `15-detach` ficou com o
      caminho do **uuid**, que é o que prova a captura
- [X] **Step 4** — `./run-all.sh` 18/18
- [X] **Step 5** — entra no commit único desta rodada

**Três cenários precisaram de correção manual**, que o script não tinha como
acertar: o `10` usava `id=` para escolher entre `project-a/` e `project-b/`; o
`09` mandava o `down` para o arquivo errado; e o `15` subia um "segundo
projeto" com o mesmo arquivo e outro id — que agora é o mesmo projeto, então o
`up` não imprimiria nada.

**E `smoke.sh` expôs uma colisão real:** batizar o daemon com o mesmo nome que
o `name:` do projeto põe o daemon dentro do namespace dos workers que ele
mesmo iniciou.

### Task 6: cenário de duas máquinas — **pendente**

Coberto no e2e (`two_daemons_with_distinct_ids_both_serve`,
`naming_another_daemon_in_the_payload_is_refused`), mas **não** no smoke, que é
onde roda contra uma engine de release e um daemon de verdade.

O `19-attach` cobriu a parte de *encontrar* um daemon entre vários; falta a de
*operar* dois em paralelo:

- [ ] dois daemons, `--ns pc-a` e `--ns pc-b`, ambos servindo
- [ ] cada um sobe o seu arquivo; os workers de um não aparecem no namespace do
      outro
- [ ] um daemon segura **dois** arquivos (`list` mostra os dois, `down` de um
      não toca o outro) — hoje só provado ao vivo, à mão
- [ ] `--namespace pc-a namespace=pc-b` → `WRONG_DAEMON`
- [ ] segundo daemon com `--ns pc-a` é recusado e o primeiro segue intacto

---

## Docs — o que sobrou

- [X] CLI reference regenerada (`--ns`)
- [ ] `architecture/` — o modelo de endereçamento tem **quatro** eixos e nenhum
      deles está escrito em lugar nenhum: o namespace do daemon (`--ns`), o
      namespace do projeto (`name:` no YAML), o arquivo (o projeto), e o nome
      do worker. Confundir os dois primeiros é o erro que o `smoke.sh` cometeu
- [ ] Fase 6 do `implementation.md` — Quick Start com uma seção de segunda
      máquina, e o aviso sobre o namespace gerado não sobreviver ao restart

## Aceitação

- [X] `iii compose --ns pc-da-xuxa` e `--ns pc-do-joao` no mesmo engine, ambos
      servindo — e2e `two_daemons_with_distinct_ids_both_serve`
- [X] um daemon segurando dois arquivos ao mesmo tempo, com estado separado —
      e2e `one_daemon_holds_several_projects_at_once`, e verificado ao vivo com
      `02-daemon` + `18-configuration` num daemon só
- [X] `iii compose` sem `--ns` funciona, gerando e imprimindo o namespace
- [X] `namespace=` errado nunca age na máquina errada
- [X] `./run-all.sh` verde (18/18), mais `19-attach`

## Fora de escopo

Dispatch engine-side por argumento (caminho (b)) — aditivo depois, o
`namespace=` já está no lugar como guarda. Descoberta de daemons
(`compose::list` global). Alias de projeto por `name:` em vez do caminho.
