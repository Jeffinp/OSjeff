# Changelog

Formato inspirado em [Keep a Changelog](https://keepachangelog.com). O OSjeff não
tem releases versionadas; as seções são marcos na `master`.

## 2026-10 — Rede gerenciável

- **NIC:** trait `Nic` e `Port` (dono exclusivo, com contadores); drivers `virtio-net`
  (virtio 1.0, QEMU, BIOS e UEFI) e NE2000; escolha no boot, ou sem rede (a navegação falha
  na hora, em vez de "Carregando").
- **`netd`:** um dono único da NIC (a thread `fetcher`), garantido pelo tipo; o compositor
  não toca mais o hardware.
- **DHCP completo:** máquina de lease pura (T1 RENEW unicast, T2 REBIND, expiração, NAK, ACK
  com configuração nova, RELEASE), DNS inteiro da opção 6, retransmissão com recuo.
- **DNS:** resolvedor próprio com cache TTL e failover entre os servidores do lease.
- **Ping:** `netd::ping_start/ping_poll` e `netd::ping`; **estatísticas** por interface
  (`netd::stats`, linha `[trace] net:` com `perf-trace`).
- Testes: 423 → 525; `fuzz/net_parse` cobre a máquina de lease, o DNS e o ICMP.
- `tools/qemu-headless.sh` ganhou `QEMU_NIC` (`ne2k`, `virtio`, `none`); `tools/pcapsum.py`.

## 2026-10 — Window manager dinâmico

O desktop deixou de ter 7 janelas fixas (uma por app) e passou a gerenciar qualquer
número de janelas, cada uma com a instância de app e o processo próprios
([`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) §7).

- `osjeff_core::winman`: tabela dinâmica (`WindowManager`, limite 32), `WindowId` forte,
  z-order, foco, abrir/fechar/minimizar/maximizar/restaurar, cascata, mover e
  redimensionar por borda/canto com tamanho mínimo, seletor Alt+Tab em ordem de uso
  recente, duplo clique e assinatura da cena que cobre a geometria. 54 testes novos (423 para 477).
- Vários Terminais, Editores, Gerenciadores de arquivos e Calculadoras ao mesmo tempo,
  cada um com seu processo (`shell`, `shell 2`...). Fechar encerra instância e processo;
  `DEL` no Task Manager fecha de fato a janela. `ProcessTable` passou de 8 para 48 entradas.
- Botões minimizar/maximizar (aparecem com o ponteiro sobre a janela, então o desktop
  parado fica idêntico), duplo clique na barra maximiza, redimensionar por qualquer
  borda, ponto no dock para janelas minimizadas, dock foca em vez de abrir outra janela,
  `Ctrl+N` e "Nova janela" (botão direito no dock) abrem outra instância, Alt+Tab.
- Terminal e Editor usam a maior escala inteira que cabe na janela; Task Manager rola;
  o navegador diagrama a página de novo ao redimensionar.
- Ferramentas: cenários `tools/perf/scen/w8-*.sh` e `tools/perf/w8-heap.sh` (soak de
  abrir/fechar com a ocupação exata do heap em builds `perf-trace`).

## 2026-10 — Auditoria e endurecimento

Auditoria completa de desempenho, segurança e boas práticas
([`docs/audit/`](docs/audit/RELATORIO.md)), seguida de duas rodadas de correções.
Cada mudança é um commit separado, com teste ou prova em QEMU.

### Armazenamento
- OJFS v3 no kernel: driver ATA de bloco (`AtaDisk`, `BlockDevice` com LBA28 fatiado,
  `FLUSH CACHE`, cede a CPU entre setores) e serviço `storage` (detecta, migra do v2,
  formata ou monta o v3 no boot). O desktop segue no v2. Os runners passam a criar o
  disco do filesystem com 64 MiB (esparso); um disco de 64 KiB continua no v2.

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
- Uma thread secundária (`fetcher`, `wasmapp`) que entra em panic ou exceção **morre
  sozinha**: a serial registra o motivo, o Gerenciador de tarefas mostra `DEAD`, o
  navegador e a janela WASM mostram o erro, e o compositor continua. Compositor, `#DF`
  e falhas com interrupções desligadas seguem fatais.
- Páginas de guarda (desmapeadas) sob as pilhas das threads, com `#PF` em pilha IST
  própria: um estouro, inclusive de frame grande (150 KiB), deixa de corromper o heap
  vizinho.
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
- Rede: o resultado do DHCP (`NetConfig`) configura a pilha TCP/DNS do navegador (antes só
  o responder ARP/ping; o IP do SLIRP era fixo). Literais IPv4 não consultam o DNS.
  Provado em QEMU numa sub-rede `192.168.77.0/24`.
- Editor: arquivo que não cabe na grade 44×18 agora é marcado `TRUNC` e **não pode ser
  salvo** (antes era truncado em silêncio e o save destruía o resto).
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
- Testes: 189 → 423. `cargo fmt` e `cargo lint-*` passam e são exigidos no CI.

### Conhecido e ainda aberto
HTTPS sem verificação de certificado; sem ring 3; thread morta não é reiniciada nem libera
recursos; lease DHCP sem renovação e um único driver de NIC (NE2000); nenhum teste em
hardware real. Ver [`docs/ROADMAP.md`](docs/ROADMAP.md).
