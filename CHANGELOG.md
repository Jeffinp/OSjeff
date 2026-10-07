# Changelog

Formato inspirado em [Keep a Changelog](https://keepachangelog.com). O OSjeff não
tem releases versionadas; as seções são marcos na `master`.

## 2026-10 — Auditoria e endurecimento

Auditoria completa de desempenho, segurança e boas práticas
([`docs/audit/`](docs/audit/RELATORIO.md)), seguida de duas rodadas de correções.
Cada mudança é um commit separado, com teste ou prova em QEMU.

### Segurança
- Parsers de rede, disco e HTML/CSS: 9 bugs achados por fuzzing/leitura e corrigidos
  (`dechunk` com tamanho gigante, HTML aninhado estourando a pilha, `parse_color`
  não-ASCII, overflow de porta de URL e de comprimentos CSS, ARP com buffer pequeno,
  OJFS com `parent` em ciclo, `size` fora do limite, imagem curta). Todos com teste
  de regressão e entrada mínima em `fuzz/regressions/`.
- Limites explícitos: corpo de 256 KiB (HTTP e HTTPS), profundidade 40, 8 000 nós,
  1 000 regras e 2 000 seletores de CSS por página.
- Redirects: resolvidos em `osjeff_core::redirect` (preserva o esquema, bloqueia
  https→http, rejeita caracteres de controle, no máximo 5 saltos).
- HTTPS: rótulo "Conexao nao verificada" na barra de endereço; RNG do handshake por
  `RDRAND` com fallback explícito e registrado.
- WebAssembly: *fuel* por chamada, limite de memória de 24 MiB, término real do app
  (laço infinito, `proc_exit`, falha de carga), tetos nas host functions WASI.
- virtio: validação de `qsize`, BAR e limites de capability antes de tocar MMIO.
- NE2000: teto de `send`, `curr - 1` sem underflow, orçamento de recepção.
- Disco: uma falha de leitura no boot não reescreve mais o filesystem.

### Robustez
- GDT/TSS próprias com pilha IST para #DF e pilha de boot de 512 KiB: estouro de
  pilha deixa de ser triple fault mudo.
- Panic, todas as exceções da CPU e OOM imprimem na serial e pintam uma tela de
  erro (`crash.rs`); IRQ 7/15 espúrias são ignoradas (isso também corrigiu o boot
  com `-device virtio-gpu-pci`).
- Recusa limpa de framebuffer maior que os buffers estáticos.
- Gerenciador de arquivos abre e salva o arquivo da pasta certa (antes, sobrescrevia
  o homônimo da raiz).
- Allocator devolve o padding de alinhamento à free-list; `anim_signature` não colide
  com 9 ou mais janelas.

### Desempenho (QEMU sem KVM, razões)
- Scheduler com estado "bloqueada" e vetor de yield: compositor em idle de 83 para
  250 iterações/s; latência tecla→captura de ~11 ms para ~0,4 ms; o app WASM deixa de
  ocupar 1/3 da CPU parado.
- Tick do relógio 15,6 → 0,2 ms; quadro de tecla 26 → 13 ms; preenchimento 24 bpp
  14 → 3 ciclos/px; sombras com tabela de blend (24 → 10 ciclos/px).

### Adicionado
- `osjeff_core::{hw, layout, wm, gfx, redirect, rng}` (lógica movida do kernel, com
  testes); `fs::read_in`, `fs::live_dir`.
- `tools/`: `qemu-headless.sh`, `verify-boot.sh`, `run.sh`, harness `perf/` e
  `bench/` (criterion, fora do workspace); `fuzz/` com 3 alvos.
- CI (`.github/workflows/ci.yml`), `deny.toml`, `#![warn(clippy::undocumented_unsafe_blocks)]`.
- Documentação: `BUILDING`, `TESTING`, `CONTRIBUTING`, `SECURITY`, `SECURITY-MODEL`,
  `ROADMAP`, o ADR de isolamento e os relatórios de auditoria.

### Alterado
- Toolchain fixado em `nightly-2026-10-05` (o `nightly` sem data quebrou o build);
  `bootloader` 0.11.17, `x86_64` 0.15.5, `spin` 0.9.9, `anyhow` 1.0.104 e demais
  atualizações compatíveis.
- `os/` roda o QEMU com 256 MB (UEFI entrava em pânico com 128 MB: o BSS do kernel
  tem ~91 MiB).
- Todo bloco `unsafe` do kernel tem `// SAFETY:` (de 100 sem comentário para 0).
- Testes: 189 → 379. `cargo fmt` e `cargo lint-*` passam e são exigidos no CI.

### Conhecido e ainda aberto
HTTPS sem verificação de certificado; sem ring 3; pilhas de thread sem página de
guarda e panic em thread secundária ainda para a máquina; navegador preso ao IP do
SLIRP; nenhum teste em hardware real. Ver [`docs/ROADMAP.md`](docs/ROADMAP.md).
