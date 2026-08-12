# Compose — entrega de configuração

**Estado (2026-08-05): fechado.** Tasks 1-3 feitas; 4-6 **canceladas** — o
caminho que as tornava necessárias foi substituído por um mais curto (ver
abaixo). O compose agora escreve a configuração resolvida no configuration
worker antes do spawn, então um worker do fleet real, sem nenhuma mudança,
sobe com o que o compose file declarou. Provado com o `state` do registry no
cenário `06-registry-package`.

Documento de trabalho, **não commitado** (planos não entram em commit).
Branch: `feat/compose-daemon`. Companheiro de `implementation.md`, que cobre o
resto do compose.

**Decisão tomada (2026-08-04): modelo híbrido (c).** O compose resolve a
configuração final e a entrega ao filho por arquivo (`III_CONFIG`); o worker
**empurra** esse valor para o configuration worker no boot, em vez de usá-lo
só como semente. O configuration worker continua autoritativo em runtime, e
`configuration:updated` / hot reload seguem funcionando.

---

## O que está errado hoje

O compose já resolve tudo: busca a base, mescla o `config_override`, escreve um
arquivo `0600` e injeta o path em `III_CONFIG` (`lifecycle.rs:436`,
`configuration.rs`, `spawn.rs:110`). Funciona — o cenário `04-config-driven`
prova, com três containers do mesmo binário diferenciados só por config.

Mas o fleet real não lê isso. De `workers/http/src/main.rs:6`:

> O `--config` YAML é um **seed only**: popula a entrada de configuração na
> primeira vez, e depois disso o configuration worker é a fonte da verdade.

Resultado: o `config_override` do compose vale no primeiro boot e é **ignorado
em silêncio** em todos os seguintes. É o blocker B3 do `achados.md`.

Dos 45 workers, **24 têm `--config`** e nenhum lê `III_CONFIG`.

---

## O achado que encolhe o plano

`configuration::register` já faz exatamente o que (c) pede
(`engine/src/workers/configuration/store.rs:215-217`):

```rust
let (value, validate) = match (initial_value, prior.as_ref()) {
    // A caller-supplied value is always validated against the schema.
    (Some(v), _) => (v, true),
    ...
```

Com `initial_value: Some(v)` ele **substitui** o valor armazenado, **valida**
contra o schema, e emite `Updated` quando substituiu
(`configuration.rs:423-433`) — então console e assinantes de
`configuration:updated` enxergam a mudança.

**Nenhuma mudança na engine.** O que muda é: o worker deixa de passar
`initial_value` só quando o store está vazio, e passa sempre que `III_CONFIG`
existir.

---

## A sequência de boot, antes e depois

| passo | hoje | depois |
|---|---|---|
| 1 | parse CLI, conecta | igual |
| 2 | `configuration::register(schema, initial_value=seed **se store vazio**)` | `register(schema, initial_value=` **conteúdo do `III_CONFIG`, sempre que presente**`)` |
| 3 | `configuration::get` → autoritativo | igual |
| 4 | boot com esse valor | igual |
| 5 | assina `configuration:updated` → reload | igual |

Sem `III_CONFIG` (worker rodando fora do compose), o passo 2 mantém o
comportamento atual. Aditivo, não quebra ninguém.

## Precedência, ponta a ponta

Da mais fraca para a mais forte:

| # | camada | onde |
|---|---|---|
| 1 | `default_config` do manifest do pacote | só `package://`, `lifecycle.rs:293` |
| 2 | valor armazenado no configuration worker | `configuration::get id=<config_name>` |
| 3 | `config_override` do compose file | `configuration.rs::merge` |

O resultado vira arquivo `0600` → `III_CONFIG` → o worker o empurra de volta
para a camada 2. O ciclo é idempotente e **preserva** edições feitas pelo
console: elas entram pela camada 2 e só são sobrescritas se o
`config_override` tocar a mesma chave.

**Regra de arbitragem, confirmada em 2026-08-04:** chave declarada no
`config_override` pertence ao compose — uma edição de console nela é revertida
no próximo restart do container. Chave que o compose não declara pertence ao
runtime e sobrevive. É o que faz "o que o arquivo diz é o que o processo
recebeu" ser verdade sem congelar o resto da configuração.

Merge: mapas mesclam por chave recursivamente; arrays e escalares substituem
inteiros; `null` explícito é valor, não delete.

---

## Decisões que tomei (revisar)

Você respondeu só o (c); estas são as chamadas que fiz nos itens 2–5 para não
travar. Qualquer uma que não sirva, me diga.

- **`config_override` sem `config_name` continua válido.** Já é assim e o
  `04-config-driven` depende disso. A regra "sem corpo de config no compose"
  do tech pack morreu na prática — o plano a declara morta.
- **`file://` continua rejeitado em v1.** O `config_override` inline já cobre o
  caso local, e uma segunda fonte da verdade para o mesmo campo é exatamente o
  que produz "editei o YAML e nada aconteceu".
- **`config_name` continua global.** É um nome, não um path: dois projetos que
  querem config separada declaram nomes diferentes. O namespace do projeto não
  escopa a config — documentar, não mudar.
- **Segredo:** ver Task 3. `configuration::get` expande `${VAR:default}` contra
  o env do processo da engine; o round-trip do compose transformaria uma
  referência preguiçosa em valor persistido. Isso é um vazamento e tem task
  própria.

---

## Tasks — repo `iii` (compose)

### Task 1: `NOT_FOUND` deixa de matar o container

**Files:**
- Modify: `crates/iii-compose/src/engine.rs:106` (`fetch_config`)
- Modify: `crates/iii-compose/src/lifecycle.rs:448` (`resolve_config`)
- Test: `engine/tests/compose_daemon_e2e.rs`

**Problema:** `fetch_config` mapeia *qualquer* erro para `ConfigFetchFailed`, e
`config_name` declarado + fetch falho = container não sobe. Numa máquina limpa
a entrada ainda não existe — quem a cria é o `register` do worker, no boot que
o compose está impedindo. `config_name` + primeiro boot = deadlock.

`configuration::get` já distingue: `code: "NOT_FOUND"`
(`engine/src/workers/configuration/configuration.rs:525`).

- [X] **Step 1: teste** — cenário `18-configuration` no repo de smoke: o
      container `fresh` nomeia uma entrada que ninguém registrou e sobe mesmo
      assim, com a camada 2 vazia
- [X] **Step 2: rodar e ver falhar** — confirmado antes da mudança:
      `CONFIG_FETCH_FAILED ... remote error (NOT_FOUND)`
- [X] **Step 3: implementar** — `fetch_config` retorna `Result<Option<Value>>`;
      `NOT_FOUND` → `Ok(None)` via `is_not_found`; qualquer outro código ou
      erro de transporte → `Err`. Fetch-or-fail continua valendo para falha
      real
- [X] **Step 4: rodar e ver passar** — 25 checks no cenário 18
- [X] **Step 5** — entra no commit desta rodada

### Task 2: falha de schema é diagnosticada, não é um exit code

**Files:**
- Modify: `crates/iii-compose/src/error.rs` (`ChildExitedBeforeReady`)
- Modify: `crates/iii-compose/src/lifecycle.rs` (caminho do erro)
- Test: `crates/iii-compose/tests/` + cenário no smoke

**Problema:** com (c), um `config_override` com chave inválida faz o
`register(initial_value)` do worker falhar na validação de schema. O worker
sai, e o compose reporta `CHILD_EXITED_BEFORE_REGISTRATION` com um código
numérico — o operador não tem como saber que foi a config dele.

- [ ] **Step 1: teste** — container cujo filho sai com erro tem, na mensagem
      do `OpResult`, as últimas linhas do log dele.
- [ ] **Step 2-4: TDD loop** — anexar o tail do
      `~/.iii/compose/<id>/logs/<container>.log` ao erro. Limite fixo de
      linhas; nunca ecoar valores de env.
- [ ] **Step 5: commit** — `feat(compose): a child that died before readiness reports what it printed`

### Task 3: o round-trip não pode materializar segredo

**Files:**
- Modify: `crates/iii-compose/src/engine.rs` (`fetch_config`)
- Test: `engine/tests/compose_daemon_e2e.rs`

**Problema:** `configuration::get` expande `${VAR:default}` contra o env do
processo da **engine** antes de responder. O compose recebe o valor expandido,
mescla, entrega, e o worker o empurra de volta com `register(initial_value)` —
gravando no store o segredo que antes era só uma referência. Uma entrada que
dizia `password: ${DB_PASSWORD}` passa a dizer a senha, e fica assim.

- [ ] **Step 1: teste** — entrada base com `${VAR}`; após um ciclo de `up`, a
      entrada armazenada ainda contém `${VAR}`, não o valor expandido.
- [ ] **Step 2: rodar e ver falhar**
- [ ] **Step 3: implementar** — `fetch_config` passa `raw: true`, então o
      placeholder atravessa o merge intacto. Consequência a confirmar: quem
      expande passa a ser o `get` do worker, que é onde deveria ser.
- [ ] **Step 4: rodar e ver passar**
- [ ] **Step 5: commit** — `fix(compose): fetch configuration raw so placeholders survive the round trip`

---

## O repo `workers` não precisa de nada

**Tasks 4, 5 e 6 canceladas (2026-08-05).** Elas existiam para ensinar 24
workers a ler o `III_CONFIG`: um crate compartilhado, mudança semântica em 6,
mecânica em 20, e 4 a investigar.

A premissa estava errada. O worker **já sabe** ler de onde precisa — do
configuration worker. O que faltava era o compose escrever lá antes de o
processo começar.

### O que substituiu

`EngineClient::publish_config`, chamado no `resolve_config` antes do spawn:

```
config_name declarado
  → lê schema/metadata existentes (configuration::schema)
  → register { id, name, description, schema, initial_value: <valor mesclado> }
  → só então o filho é spawnado
```

Funciona porque re-registrar **sem** `initial_value` reusa o valor guardado
(`engine/src/workers/configuration/store.rs:226`). O worker sobe, registra o
schema dele, e o valor que encontra é o que o compose escreveu. Verificado
contra os dois comportamentos que existem no fleet:

| grupo | quantos | o que faz | resultado |
|---|---|---|---|
| A | 6 (`http`, `state`, `queue`, `cron`, `pubsub`, `bridge`) | semeia só se o store está vazio | store tem valor → não semeia → o do compose fica |
| B | 20 (`storage`, `database`, `session-manager`, …) | `--config` explícito sempre vence | sem `--config`, cai no gate de default → o do compose fica |

E os `CONFIG_ID` são os próprios nomes (`http`, `state`, `queue`), então
`config_name: state` escreve na entrada certa sem convenção nova.

### O que se ganhou de graça

O schema do worker é **preservado**, não sobrescrito: o `publish_config` lê o
que já existe e carrega adiante. Um worker que já rodou mantém o schema, o
valor escrito é validado contra ele, e o console mantém o nome que o worker deu
à entrada. A validação que eu tinha listado como custo aceitável não se perdeu.

`null` e ausente são tratados igual — uma entrada semeada de arquivo pode ter
`schema: null`, que não é um JSON Schema e seria rejeitado se carregado adiante.

### A prova

`06-registry-package` sobe o `state` **do registry**, publicado antes do compose
existir, que lê sua config do configuration worker sob o id `state` e não sabe
que um compose file o mencionou:

```
ok   the compose value reached the entry the worker reads   ("max_value_bytes": 4096)
ok   and the configuration it ships is still under it        (in_memory)
ok   the worker's schema was not flattened                   ("properties")
```

### O que continua aberto

- **A entrada de configuração é global.** Dois projetos rodando `http` em
  namespaces diferentes escrevem na mesma entrada e se sobrescrevem. Já era
  verdade; o compose escrevendo ativamente torna a colisão mais provável e mais
  silenciosa. Sem teste.
- **`config_name` precisa bater com o id interno do worker.** Funciona no `06`
  porque o container se chama `state` e o `CONFIG_ID` é `state`. Esquecer o
  `config_name` faz o compose não escrever em lugar nenhum — silêncio, não erro.
- **O schema do worker pode ser fechado.** O `state` tem
  `additionalProperties: false`, então um `config_override` com chave inventada
  é escrito com sucesso e deixa o worker sem conseguir ler a própria entrada.
  O erro chega (o tail do log diz `SCHEMA_INVALID`), mas tarde.

## Docs — o que falta

- [ ] `docs/next/using-iii/compose.mdx` — a seção de configuração diz que o
      valor chega por `III_CONFIG`. Chega também pelo configuration worker, e é
      esse o caminho que faz um worker do fleet funcionar. **`config_name` tem
      que bater com o id interno do worker**, e omiti-lo faz o compose não
      escrever em lugar nenhum — silêncio, não erro. É a pegadinha do modelo
- [ ] a mesma página: um `config_override` com chave que o schema do worker não
      aceita é escrito com sucesso e só falha quando o worker lê
- [ ] doc de autoria de worker no repo `workers` — a seção que ensina
      "seed only" continua verdadeira para o worker, e agora é *por isso* que
      funciona. Vale explicar em vez de corrigir

---

## Aceitação

- [X] `config_override` vale em **todo** boot, não só no primeiro
- [X] console e `configuration:updated` continuam funcionando — o
      `register` que o compose faz emite `Updated`
- [X] worker rodando fora do compose não muda de comportamento
- [X] `${VAR}` não vira valor persistido
- [X] `config_name` de primeiro boot não trava o `up`
- [X] cenário `18-configuration` verde (34 checks)
- [X] **um worker do fleet real, sem mudança, sobe com o que o compose
      declarou** — `06-registry-package`, com o `state` do registry

## Fora de escopo

Reload do `III_CONFIG` sem restart (o arquivo é fixo pela vida do processo;
mudança em runtime é o caminho do configuration worker), `file://` como fonte,
escopo de config por namespace, adapters de secret manager.
