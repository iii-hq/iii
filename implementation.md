# Compose — plano de implementação

Documento de trabalho, **não commitado** (planos não entram em commit).
Branch: `feat/compose-daemon`, base `feat/namespare`.
Substitui o antigo `COMPOSE-HANDOFF.md`.

**Onde estamos:** a fundação (namespace na engine + SDKs) está pronta na branch
base. O crate `iii-compose` existe, valida um projeto offline de ponta a ponta e
tem todas as peças locais (spawn, hooks, supervisão, estado durável) prontas e
testadas — mas **ninguém as chama ainda**, porque o loop que amarra tudo (`up`)
precisa de uma conexão com a engine, e é aí que batem as decisões pendentes.

```bash
cd ~/iii/iii.feat-compose-daemon
cargo build -p iii --bin iii
./target/debug/iii compose validate -f <worker-compose.yaml>
cargo test -p iii-compose      # 88 verdes
cargo test -p iii --test compose_daemon_e2e   # 5 verdes (engine real)
cargo test -p iii --bin iii    # 142 verdes
```

Legenda: `[X]` feito e testado · `[~]` parcial · `[ ]` não começou · 🚧 bloqueado
por decisão.

---

## Fase 0 — Fundação: namespace (branch `feat/namespare`)

Já mergeada na base desta branch. Listada só para contexto.

- [X] **Protocolo** — `DEFAULT_NAMESPACE`, `effective_namespace`, campo
      `namespace` em `RegisterTrigger` e `InvokeFunction`, mensagem
      `RegistrationRejected`, códigos `WORKER_NAMESPACE_CONFLICT` /
      `FUNCTION_NAMESPACE_CONFLICT` (`engine/src/protocol.rs`)
- [X] **Registries por `(namespace, id)`** — `function.rs`, `services.rs`,
      `trigger.rs`, `worker_connections/`
- [X] **Roteamento estrito no invoke** — ns explícito resolve só nele; sem ns
      resolve só no `default`; sem best-fit
- [X] **Lease `(ns, worker_name)`** com rejeição fatal e fechamento da conexão
- [X] **Builtins e RBAC namespace-aware**, prefixo `engine::` reservado, filas
      duráveis com identidade injetiva
- [X] **Introspecção por namespace** — `engine::workers::list` / `::info` aceitam
      `namespace` opcional (é o que a readiness do compose vai usar)
- [X] **SDKs Node, Browser, Python, Rust** — `InitOptions.namespace`,
      `III_NAMESPACE`, namespace no trigger, `RegistrationRejectedError` /
      `fatal_error()`
- [~] **SDK Go** — só pass-through de namespace para providers de trigger; sem
      namespace de worker, sem tratamento de rejeição
- [ ] **Docs de arquitetura e usuário** — zero menção a namespace em `docs/`

---

## Fase 1 — O crate offline `crates/iii-compose`

Tudo verificado por teste. Commits `fd38743f`, `d69c06d4`.

- [X] **Task 1 — Crate + boundary do CLI**
  - `iii compose --id <ID> [--engine <URL>] [--namespace <NS>] --file <PATH>` e
    `iii compose validate --file <PATH>`
  - Delega direto para `iii_compose::run`; o braço nunca toca `EngineBuilder`
  - `crates/iii-compose/src/cli.rs`, `engine/src/main.rs`
  - CLI reference regenerada (`docs/next/cli-reference/index.mdx`), job
    `cli-docs-built` continua verde
  - 6 testes de modo + 3 no binário, incluindo o guard de que `compose` não
    sombreia o alias `iii trigger`

- [X] **Task 2 — Schema v1 estrito + DAG** (Decisão #1 fechada)
  - Campo desconhecido é erro no topo, no container e dentro de `scripts`
  - Chave de container duplicada rejeitada (`serde_yaml` manteria a última e
    sumiria com um worker em silêncio)
  - Grafo: dependência inexistente, auto-dependência, ciclo com caminho em ordem
    de declaração; ordem de start por Kahn
  - `environment`, `env_file`, `startup_timeout` e `stop_timeout` adotados;
    chave duplicada rejeitada também no `environment`
  - `src/config.rs`, `src/dag.rs` · 22 testes

- [X] **Task 3 — Manifest opcional + precedência de `run`**
  - Parser próprio do subset de `iii.worker.yaml` (`name`, `scripts.start`); não
    importa nada de `crates/iii-worker`
  - `run` do compose ganha do `scripts.start`; manifest não pode renomear o
    container
  - `src/manifest.rs` · 9 testes

- [X] **Namespace do projeto** (não era task própria, mas é pré-requisito)
  - `--namespace` > `<nome>-<sha256(path canônico)[..8]>`, determinístico
  - `src/namespace.rs`

---

## Fase 2 — Execução local (sem engine)

Bibliotecas prontas e testadas, **ainda não chamadas por ninguém**.
Commits `d5fe343b`, `b1393191`.

- [X] **Task 5 — Contrato de spawn**
  - Ambiente **construído**, não herdado: baseline do host → `env_file` (na
    ordem declarada) → `environment` → as 4 reservadas. `env_clear()` antes
  - Chave reservada vinda de `environment`/`env_file` é erro de validação
  - `env_file` lido no spawn (tem segredo), mas ausência falha na validação
  - Plano computado como dado (`SpawnPlan`) antes de virar processo, então o
    contrato é assertável sem spawnar nada
  - `src/spawn.rs`, `src/config.rs::resolve_user_env` · 5 testes de env +
    4 unit

- [X] **Task 6 — Hooks `pre_start` / `post_run`**
  - `pre_start` bloqueia com orçamento, mata o próprio grupo no timeout, e drena
    os pipes enquanto roda (hook falante não vira timeout falso)
  - `post_run` dispara após a saída e nunca é aguardado; falha vira log
  - `src/hooks.rs` · 6 testes

- [X] **Task 7 — Supervisão unix**
  - Cada filho é líder do próprio process group; teardown SIGTERM → 10s → SIGKILL
    alcança netos (teste com worker que spawna filho)
  - Birth identity via start-time do `/proc` no linux
  - `src/process/{mod,unix}.rs` · 6 testes

- [X] **Task 8 — Supervisão windows** ⚠️ compilado, nunca executado
  - Job Object no lugar do group, `CTRL_BREAK` no lugar do SIGTERM,
    `TerminateJobObject` no lugar do SIGKILL; hooks também recebem job
  - `KILL_ON_JOB_CLOSE` desligado de propósito (ver Dívidas)
  - `src/process/windows.rs` · validado só por
    `cargo check --target x86_64-pc-windows-msvc --all-targets`
  - Job `compose-test-matrix` no CI (ubuntu/macos/windows) fecha a lacuna

- [~] **Task 4 — Resolução de config**
  - [X] Merge do `config_override`: mapas recursivos; arrays, escalares e `null`
        explícito substituem; posição das chaves preservada
  - [X] Entrega como arquivo `0600` (`src/configuration.rs`)
  - [ ] **Fetch da base no configuration worker** — precisa da conexão (Fase 4).
        `configuration::get` já existe na engine
  - [ ] Fetch-or-fail: `config_name` presente e worker fora do ar deve falhar o
        container antes do spawn

- [~] **Task 10 — Estado durável + reconciliação**
  - [X] `~/.iii/compose/<id>/state.json`, `0600` em dir `0700`, escrita atômica
        (temp + rename)
  - [X] `reconcile` é **read-only** e resolve em `Adopt` / `Gone` /
        `Unverifiable`; PID reciclado nunca é sinalizado, só reportado
  - [X] Estado corrompido é erro, não reset silencioso
  - [X] `--id` ligado a um compose file (`STATE_BINDING_MISMATCH`)
  - [ ] **Executar** a decisão: em `Gone`, disparar `post_run` + cascade dos
        dependentes locais → é parte da Task 9
  - [ ] Shutdown intencional do daemon (SIGTERM) → down local completo → Task 9
  - `src/state.rs` · 9 testes

---

## Fase 3 — Decisões ✅ TODAS FECHADAS (2026-07-30)

Nenhuma decisão bloqueia mais a Fase 4.

- [X] **Decisão #1 — Qual é o schema v1?** — FECHADA (2026-07-30)
  - **Field set:** plan_d + `environment`, `env_file`, `startup_timeout`
    (arquivo + override por container) e `stop_timeout`
  - **Nomes:** `config_name` + `config_override` (mantidos); `config_uri` só na
    forma `worker://configuration/get/<name>`, como alias
  - **Fora do v1:** `schema_version`, `config` inline, `image://` — guardados
    pelo teste `rejects_fields_still_outside_v1`
  - **Fonte:** este `implementation.md` + o código. A referência oficial do
    schema vira doc em `docs/` na Fase 6; o tech pack ausente não bloqueia mais
  - **Consequência implementada:** o ambiente do filho passou a ser *construído*
    (baseline do host → `env_file` na ordem → `environment` → reservadas), não
    herdado — IMPL_PLAN §2.4 e §6.5. Chave reservada vinda do usuário é erro
    (`RESERVED_ENV_OVERRIDE`), não descarte silencioso

- [X] **Decisão #2 — Em que namespace o daemon conecta?** — FECHADA (2026-07-30)
  - **O daemon conecta no seu próprio namespace** (`--id`) e registra
    `compose::*` lá. Dois daemons nunca colidem; os filhos seguem no namespace
    do projeto, que é independente
  - **Alcance do operador:** `iii trigger compose::up --namespace host-a`
  - **`id=` no payload:** opcional, como guard. Ausente → executa; presente e
    diferente do `--id` → `WRONG_DAEMON` com a invocação certa na mensagem
  - [X] `iii trigger --namespace` implementado e verificado contra engine viva
        (`engine/src/cli_trigger/`, commit `6e9b07f6`)

- [X] **Decisão #3 — Como o daemon detecta que perdeu o registro?** —
      DISPENSADA pela #2
  - Com cada daemon no próprio namespace não existe mais conflito de registro
    para observar: o fallback do plan_d Task 11 deixa de existir
  - A lacuna em si continua real e vale uma task própria fora do compose:
    `FUNCTION_NAMESPACE_CONFLICT` só vira `tracing::warn!` no SDK Rust
    (`sdk/packages/rust/iii/src/iii.rs:1897`), então nenhum worker consegue
    reagir a ter perdido um id. Não bloqueia mais nada aqui

- [X] **Decisão #4 — `III_URL` nos SDKs** — FECHADA (2026-07-30)
  - O `achados.md` estava desatualizado: o fleet **já migrou**. Estado real do
    contrato de spawn hoje:

    | Variável | Quem consome | Estado |
    |---|---|---|
    | `III_URL` | CLI de cada worker + agora os SDKs | ✅ 44/45 no fleet |
    | `III_NAMESPACE` | o SDK, sozinho | ✅ de graça |
    | `III_WORKER_NAME` | o SDK, sozinho | ✅ de graça |
    | `III_CONFIG` | ninguém ainda | ❌ 0 workers leem |

  - [X] **`III_URL` nos 3 SDKs** (commit `5566a706`): `registerWorker()` /
        `register_worker()` / `register_worker_from_env()`; resolução
        explícito > `III_URL` > `ws://127.0.0.1:49134`. Browser mantém endereço
        obrigatório (não há env num navegador). `getAddress()` /
        `get_address()` / `address()` nos três
  - [X] **`III_CONFIG`:** o fleet adota `env = "III_CONFIG"` no `--config` que
        já existe — mesma mudança de uma linha que funcionou para `III_URL`. O
        worker segue dono do schema; o compose só entrega o path
  - **Escopo:** o trabalho no repo `workers` fica listado, não feito (ver
    "Tasks no repo workers")

---

## Fase 4 — Daemon conectado ✅ (com 3 pendências listadas)

- [X] **Trabalho de destravamento (fora do plan_d, curto)**
  - [X] `iii trigger --namespace` (Plan B Task 5) — com `--help`
        namespace-aware
  - [X] `III_URL` nos 3 SDKs (Plan B Tasks 1-3)
  - ~~Superfície programática do `FUNCTION_NAMESPACE_CONFLICT`~~ — dispensada
    pela Decisão #2

- [X] **Conexão do daemon** (`src/daemon.rs`, `src/engine.rs`)
  - Registra como worker `--id` no namespace de mesmo nome; filhos vão para o
    namespace do projeto, derivado à parte
  - `--id` duplicado perde a lease `(ns, worker_name)` e o daemon sai com erro
    em vez de servir meio projeto
  - [X] Reconciliação após reconexão: o supervisor observa o estado da conexão
        e, quando ela volta, dá a cada container o próprio `startup_timeout`
        para se registrar de novo; quem não volta é marcado como falho e sofre
        cascade. Cenário `14-engine-restart`

- [X] **Task 4 (resto)** — fetch-or-fail antes do spawn, merge e arquivo `0600`

- [X] **Task 9 — Lifecycle `up` / `down`** (`src/lifecycle.rs`)
  - readiness = registro visível em `(ns, key)`; filho que morre esperando
    curto-circuita com `CHILD_EXITED_BEFORE_REGISTRATION`
  - rollback só do que a operação subiu, ordem reversa; `up` repetido =
    `changed:false`; `down` dependentes primeiro
  - `OpResult` JSON estável, com teste que fixa o shape
  - [X] Cascade automático de saída pós-ready: um loop de supervisão (não uma
        task por filho — `wait` empresta o handle que vive atrás do lock de toda
        operação) marca o container como falho, dispara `post_run` e derruba os
        dependentes. Cenário `12-crash-cascade`

- [X] **Task 10 (resto)** — down local completo no shutdown intencional, estado
      limpo depois; `Gone`/`Unverifiable` reportados no start
  - [X] Re-adoção real: `Supervised::adopt` verifica a identidade e entra no
        mapa `children`, então o `down` alcança o sobrevivente. Antes era só
        reportado, e o teardown reportava sucesso sobre um processo vivo.
        Cenário `11-restart-adoption`

---

## Fase 5 — Superfície remota ✅ (com 3 pendências listadas)

- [X] **Task 11 — `compose::*`** (`src/remote.rs`)
  - `up`, `down`, `list`, `status`, `validate` registrados no namespace do
    daemon; erros cruzam o wire com o código estável via `Error::Remote`
  - `id` ausente executa; divergente → `WRONG_DAEMON` com a invocação certa
  - [X] `compose::logs` responde: `logs.rs` guarda um buffer limitado por
        container (500 linhas), alimentado pelo mesmo pump que escreve no
        console, com stdout e stderr separados. Cenário `13-logs`

- [~] **Task 12 — E2E** (`engine/tests/compose_daemon_e2e.rs`, 5 testes verdes
      contra engine real)
  - [X] `compose::*` responde só no namespace do daemon, e não vaza no `default`
  - [X] `id=` divergente → `WRONG_DAEMON`
  - [X] filho que nunca registra → `STARTUP_TIMEOUT`, rollback, nada em `ready`
  - [X] `--id` duplicado → rejeição fatal, o segundo não fica mudo
  - [ ] **Container que de fato fica ready** — precisa de um binário-fixture que
        fale o SDK; é a lacuna que mais pesa hoje
  - [ ] Engine cai e volta com filhos sobrevivendo
  - [ ] Varredura por process group ao fim de cada e2e

---

## Tasks no repo `/home/doggao/iii/workers`

Decididas, não feitas — outro repo, sua chamada de quando e em que branch.

- [ ] **`env = "III_CONFIG"` no `--config`** de cada worker que tem a flag.
      Mesmo padrão do `III_URL`, uma linha por binário. Sem isso o compose
      resolve a config, escreve o arquivo `0600` e ninguém lê o path
- [ ] **`bridge` sem `III_URL`** — o único dos 45 fora do padrão:
      `rg -L 'env = "III_URL"' bridge/src/main.rs`
- [ ] **Check de conformidade no CI** (Plan B Task 7): roda `<bin> --help` de
      cada binário e falha se faltar `--url`/`--namespace`/`--config`

---

## Fase 6 — Documentação

- [ ] **Plan E — Quick Start em dois estágios** (engine como processo → compor
      dois workers), referência do schema v1, how-to de coexistência
- [ ] **Script E2E copy-paste** que guarda o tutorial contra drift
- [ ] **Docs de namespace** (dívida da Fase 0): `InitOptions.namespace`,
      `III_NAMESPACE`, namespace no trigger, `RegistrationRejectedError`

---

## Dívidas conhecidas

- **Windows nunca executado** — e agora nem o `cargo check` cruzado roda: com o
  `iii-sdk` na árvore veio `ring`, que exige toolchain C para windows-msvc. O
  job `compose-test-matrix` (windows-latest) é a única cobertura
- **`KILL_ON_JOB_CLOSE` desligado** — ligado, matar o daemon mataria todos os
  filhos, o oposto do requisito de readoção. Custo: daemon morto deixa o job sem
  dono até o próximo `down`
- **macOS nunca readota** — sem `libproc` não há fingerprint, então todo PID vivo
  vira `Unverifiable`. Seguro, mas restart no macOS reporta filhos para limpeza
  manual. Fechar é task própria, precisa de um mac
- **`package://` funciona ponta a ponta** — resolve, baixa, confere o sha256
  antes de qualquer byte tocar o diretório final, extrai e cacheia em
  `~/.iii/compose/packages` por `(nome, versão, alvo)`. O `state@0.21.4-alpha.3`
  do registry público sobe e serve dentro do namespace do projeto
  (`06-registry-package`)
- **Worker anterior ao SDK 0.22 é diagnosticado, não esperado** — ele ignora
  `III_NAMESPACE` e registra em `default`, então a readiness no namespace do
  projeto é insatisfazível por construção. O compose devolve
  `WORKER_IGNORED_NAMESPACE` no instante em que o worker aparece em `default`,
  sem gastar o `startup_timeout`. Um worker de mesmo nome que *já* estava em
  `default` antes do spawn não é culpado: a comparação é contra o snapshot do
  início. Cenário `09-registry-legacy-sdk`; `08-registry-missing` cobre o que o
  registry não resolve
- **`image://` não resolve** — sem runtime de container; fora de escopo do MVP
- **Baseline de env é uma lista fixa** (`spawn.rs::BASELINE_ENV`) — um worker
  que dependia de variável herdada (proxy, `SSL_CERT_FILE`, `RUSTUP_*`) passa a
  precisar declarar. Se apertar demais na prática, é uma linha para estender
- **DG-2 (proof of claim) aberto** — a engine confia no `III_WORKER_NAME`/ns que o
  filho manda. O lease dá arbitragem, não autenticação. Aceitável no MVP local,
  mas deve ser decisão explícita, não omissão
- **Comentário obsoleto** em `engine/src/protocol.rs:33-40` diz que
  `WORKER_NAMESPACE_CONFLICT` "não é emitido ainda"; `engine/mod.rs:2670` emite.
  Também referencia `.superpowers/sdd/task-4-report.md`, que não deveria vazar em
  código commitado

---

## Ordem sugerida

1. ~~**Decisão #1** (schema)~~ — fechada
2. ~~**Decisão #2** (namespace do daemon)~~ — fechada; #3 dispensada junto
3. ~~**Decisão #4** (`III_URL` nos SDKs)~~ — fechada
4. **Fase 4** (conexão + Task 4 + Task 9) — o `up` de verdade. **Nenhuma
   decisão pendente bloqueia daqui em diante**
5. **Fase 5** e **Fase 6**

---

## Mapa de arquivos

| Arquivo | O que faz |
|---|---|
| `src/cli.rs` | Superfície `iii compose`, seleção de modo |
| `src/config.rs` | Schema v1 estrito, parse, validação por container |
| `src/dag.rs` | Dependências, ciclos, ordem de start, dependentes transitivos |
| `src/manifest.rs` | `iii.worker.yaml`, precedência de start, relatório do validate |
| `src/namespace.rs` | Derivação do namespace do projeto |
| `src/configuration.rs` | Merge do `config_override`, arquivo `0600` |
| `src/spawn.rs` | Baseline do host, precedência de env, plano de spawn |
| `src/hooks.rs` | `pre_start` (com orçamento), `post_run` (fire-and-forget) |
| `src/process/mod.rs` | Birth identity, `is_running`, tipos comuns |
| `src/process/unix.rs` | Process groups, teardown, reaping |
| `src/process/windows.rs` | Job Objects, `CTRL_BREAK`, teardown |
| `src/state.rs` | Estado durável, `reconcile` |
| `src/error.rs` | `ComposeError` com códigos estáveis |

Testes: `tests/{cli,config_validation,environment,manifest,hooks,process_lifecycle,crash_recovery}.rs`
(6 / 22 / 5 / 9 / 6 / 6 / 9) + 19 unit no lib.
