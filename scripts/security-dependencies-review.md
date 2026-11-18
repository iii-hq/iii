# Correção das dependências vulneráveis da main

Base revisada: `31a58c59269891bbcfefa8513231ff0e9c751b78` (`origin/main`).

## Diagnóstico

O Dependabot retornou 78 alertas abertos: 8 críticos, 13 altos, 41 médios e
16 baixos, distribuídos entre Node, Python, Go e Rust. A contagem inclui
vários advisories para um mesmo pacote e ocorrências em locks diferentes.
A auditoria Rust também encontrou três vulnerabilidades em `quinn-proto` /
`quinn-udp` e um aviso de unsoundness em `rustix` que não constavam dessa lista.

## Correções

- **Node:** atualizar Turbo para uma versão que inclui a correção de 2.9.14;
  esbuild para >=0.28.1; minimatch para >=11.0.4 em todo o workspace; KaTeX
  para >=0.18.2. Manter o patch existente de compatibilidade do TypeDoc.
- **Python:** atualizar os quatro locks, incluindo aiohttp >=3.14.4, AnyIO
  >=4.14.2, certifi >=2026.10.18, idna >=3.15, multidict >=6.9.1,
  OpenTelemetry API/SDK >=1.45.2, Pygments >=2.20.0 e pytest >=9.0.3.
  O uv mantém resoluções compatíveis com os intervalos de Python declarados.
- **Go:** atualizar a família OpenTelemetry para 1.47.0 nos módulos SDK e
  exemplo. **Isso eleva a versão mínima do Go de 1.25 para 1.26**, exigida
  pelos próprios módulos upstream. Nenhuma assinatura do SDK foi alterada.
- **Rust:** atualizar xxhash-rust para 0.8.16, quinn-proto para 0.11.19,
  quinn-udp para 0.5.16 e rustix para 1.1.6.

### Picomatch: mitigação local, sem release upstream corrigida

[GHSA-fc5q-39xc-vjpf](https://github.com/advisories/GHSA-fc5q-39xc-vjpf)
afeta as duas versões resolvidas, 2.3.2 e 4.0.7. No momento da revisão,
o advisory não indicava versão corrigida e `latest` era 4.0.7.

Os patches em `patches/picomatch@*.patch`, aplicados pelo pnpm e registrados
com hash no lockfile, estendem a defesa de extglobs repetidos para ramos
únicos que contêm quantificadores não limitados. Padrões perigosos como
`+(*)`, `*(*)` e `+(*a)` passam a ser tratados como literais, seguindo o
fallback de segurança já existente no parser. Isso deliberadamente altera
o significado de alguns extglobs com curingas repetidos. Globs comuns,
alternativas literais e os casos seguros cobertos pelos testes permanecem
funcionais. A opção upstream `maxExtglobRecursion: false` continua sendo um
opt-out da proteção e não deve ser usada com globs não confiáveis.

`pnpm audit` e Dependabot ainda podem reportar esse advisory: não interpretam
o conteúdo do patch. O gate `scripts/audit-sdk.mjs` continua exibindo os
avisos e só trata essas versões como mitigadas quando o hash do arquivo de
patch corresponde ao registrado no lockfile e não há snapshot sem patch.
A exceção antiga de minimatch foi removida. Remover esses patches quando
uma release upstream corrigida estiver disponível e passar na regressão.

## Validação local

- `pnpm install --frozen-lockfile --ignore-scripts`: aprovado.
- `pnpm test:security`: 4 testes aprovados. As regressões do picomatch falham
  nas versões sem patch; cada execução usa um processo filho com timeout.
  Inclui smoke tests de minimatch e renderização KaTeX.
- Gate de auditoria: aprovado com hashes válidos; um teste negativo com hash
  alterado confirmou que a mitigação deixa de ser aceita.
- Builds dos pacotes Node, geração TypeDoc e build da UI de observabilidade:
  aprovados.
- Helpers Node: 21 testes aprovados. SDK de navegador: 636 aprovados.
- Site: 127 testes aprovados, 1 ignorado; type-check aprovado depois de gerar
  os arquivos locais de roadmap, benchmarks e contribuidores em modo offline.
- Python: 6 testes de helpers e 161 testes selecionados do SDK aprovados;
  os quatro ambientes sincronizam com `uv sync --locked`.
- `pip-audit` dos quatro locks, incluindo extras: nenhuma vulnerabilidade.
- Go: `go test ./...`, `go vet ./...`, `go mod verify` no SDK e testes /
  verificação do exemplo aprovados; `govulncheck` sem vulnerabilidades nos
  dois módulos.
- `cargo audit`: nenhuma vulnerabilidade ou aviso de unsoundness restante;
  `cargo metadata --locked` e `cargo fmt --all`: aprovados.
- `pnpm fmt` executado; alterações de formatação fora do escopo descartadas.

Não foi executada a suíte completa Rust nem os testes SDK que dependem de
uma engine real. Os avisos de manutenção de `chrono` (RUSTSEC-2026-0543) e
`unsafe-libyaml` (RUSTSEC-2024-0449) permanecem: não há versão corrigida
indicada para esses avisos. Nenhum advisory foi descartado no GitHub, e
nenhuma configuração de CI foi alterada.
