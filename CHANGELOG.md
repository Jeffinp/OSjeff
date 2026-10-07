# Changelog

Formato inspirado em [Keep a Changelog](https://keepachangelog.com). O OSjeff não
tem releases versionadas; as seções são marcos na `master`.

## 2026-10 — Editor e Terminal de verdade (W15b)

- **Terminal:** o motor `osjeff_core::shell` (52 comandos, pipes, variáveis, scripts) com histórico
  (↑/↓, Ctrl+R), Tab (comandos, variáveis, caminhos), Ctrl+C / Ctrl+L / `clear`, rolagem (PageUp/
  PageDown, roda, Ctrl+Home/End; até 5000 linhas), colar sem executar, prompt colorido e grade que
  acompanha a janela. Sistema de arquivos pelo VFS (`VfsFs`, diretório corrente por terminal; `rm`
  vai para a lixeira); `date`, `uptime`, `free`, `df`, `ps`, `kill`, `ping`, `ifconfig` com dados
  reais (`KSys`); **novos** `nslookup`, `curl`, `wget`, `ifconfig` (`SysInfo::resolve`, `http_get`,
  `net_info`) e `edit`, `files`, `tasks`, `calc`, `reboot`, `shutdown`.
- **Comandos longos sem congelar:** duas threads `shelld` executam as linhas; Ctrl+C cancela
  (`SysInfo::interrupted`, status 130). Nova caixa de correio bloqueante no `fetcher`
  (`fetch::run_job`: DNS e GET) com estado `ABANDONED` para quem desiste no meio.
- **Editor:** `osjeff_core::editor2` no lugar da grade 44x18: números de linha, UTF-8, desfazer/
  refazer, Ctrl+F/H/G, mouse (clique, duplo, triplo, arrastar), roda, arquivos de até 16 MiB (testado
  com 1 MB e 165 mil linhas), Ctrl+O / Ctrl+S / Ctrl+Shift+S pelo VFS (seletor de arquivos com
  confirmação de substituição). **Fechar com alterações pergunta** Salvar / Descartar / Cancelar
  (botão, Ctrl+Q, Task Manager, `kill`, Reiniciar/Desligar); o Arquivos abre texto no mesmo editor.
- **Removidos:** `osjeff_core::editor` e `terminal` (grade fixa). Esc não fecha mais o editor.
- **Achados dirigindo a interface:** `tr` não entendia `\n` (corrigido), o campo "Salvar como"
  anexava ao nome sugerido (digitar agora o substitui), Alt+A/R/C não chegavam ao editor.
- **Testes:** 2071 -> 2076 (+61 novos, -56 dos módulos removidos): `Screen`, `Term`, `Picker`,
  `CloseAsk`, comandos de rede, Ctrl+C. Fuzz: `shell_parse` agora executa uma sessão de terminal
  inteira e os comandos de rede; novo alvo `editor_dialog`. Cenários `tools/perf/scen/w15b-*.sh`
  (`typestr` em `lib.sh`).
## 2026-10 — Apps e gerenciamento do sistema sobre o disco e a rede reais (W18)

- **Apps no disco** (`osjeff_core::appfs::VolumeFs`): `/apps/<id>.wasm`, `/data/<id>` e `/home`
  vivem no volume OJFS v3 (o mesmo do Arquivos; sem disco v3, no volume em RAM do desktop) e
  **persistem entre boots**. O adaptador só alcança essas três árvores, protege as pastas-raiz,
  limita arquivos a 64 MiB e mantém a cota exata depois de um reboot; uma chamada = uma seção
  crítica do `YieldMutex` do volume, sem mascarar interrupções e sem atravessar o guest.
  Provado em QEMU: o Notas salva em `/data/notes`, o sistema reinicia com a mesma imagem e a nota
  abre. `seed_once` (`/apps/.seeded`): um app embutido removido não volta no boot seguinte.
  `fuzz/app_sandbox` também roda sobre `VolumeFs`; 18 testes novos (paridade com `MemFs`,
  remount, cota, disco cheio, só-as-três-árvores).
- **Arquivos, lugar Apps:** barra lateral e tecla `A`; lista pacotes instalados e embutidos com
  ícone, tamanho e estado; `Enter` executa (instalando antes), `I` instala, `Del` remove, menu
  de contexto, erros na linha de estado. **Propriedades** de um app e de qualquer `.wasm` mostram
  o manifesto (permissões e cotas). `Desktop::{app_rows, install_bundled, remove_app}` voltaram a
  ter uso (sem `#[allow(dead_code)]`); arquivos escritos por apps aparecem no Arquivos em 100 ms.
- **Rede dos apps:** `net_http_get` deixou de devolver `ERR_NOSYS`; usa o `fetcher`/`netd`, com a
  política (permissão, filtro de destinos, **`net_hosts`** novo no manifesto) aplicada antes e em
  cada redirecionamento, checagem do endereço **resolvido**, TLS com verificação completa e sem
  "continuar mesmo assim", só corpo 2xx decodificado. Provado em QEMU contra um servidor falso
  (`tools/nettest-server.py`, app de teste `wasm-apps/nettest`). Limite documentado: um app
  esperando a rede atrasa os outros apps.
- **Boot e logs:** a ordem `storage::init` < `Desktop::new` < `load_settings` está documentada e
  provada (imagem de fundo, destaque, relógio 12 h, fuso e ABNT2 voltam depois de reiniciar com o
  mesmo disco); o caminho de papel de parede sem barra (`papel.png`) deixou de falhar; "Salvar"
  escreve `/var/log/syslog.txt`; a nova thread `logd` grava `/var/log/boot.log` (limitado a
  96 KiB, fora do compositor e de IRQ). Falhas de E/S de arquivos do sistema e de ATA agora
  deixam um WARN. O monitor e as configurações leem os contadores de rede de `netd::stats()`
  (o módulo `netstats.rs` saiu; o gráfico agora anda também com virtio-net).
- Testes: 2071 -> 2112 no core (VolumeFs, `seed_once`, lugar Apps, `net_hosts`, `authorize`,
  `app_response`, `dump_bounded`, `absolute_path`); cenários `tools/perf/scen/w18-*.sh`.

## 2026-10 — Desktop sobre o OJFS v3: gerenciador de arquivos e visualizador de imagens

- **VFS do desktop** (`desktop/vfs.rs` + `osjeff_core::vfs`): uma API de caminhos absolutos
  para gerenciador, visualizador, editor e terminal, sobre o OJFS v3 montado (`storage`).
  Disco pequeno/sem disco/desconhecido: volume de 4 MiB na RAM com aviso (v2 de um disco de
  64 KiB é importado). O v2 saiu do desktop (`disk()`, `PERSIST`, `ata::read_image/write_image`);
  a semente de boas-vindas existe num lugar só (`vfs::seed_welcome`).
- **Gerenciador de arquivos v2:** caminhos, migalhas, histórico, barra lateral com uso do disco,
  colunas ordenáveis, seleção múltipla, novo arquivo/pasta, renomear (F2), copiar/recortar/colar
  entre janelas, lixeira (excluir, restaurar, esvaziar), exclusão permanente com confirmação,
  propriedades, menu de contexto, 20 000 linhas, nomes UTF-8 de 255 bytes, cópias grandes em
  passos por quadro com barra de progresso e cancelamento. Corrige o texto claro sobre fundo claro.
- **Visualizador de imagens** (`Kind::Viewer`, no Painel Iniciar; o dock não mudou): PNG/BMP/PPM,
  ajustar/zoom/roda/arrastar/girar/espelhar, próxima/anterior da pasta, informações, fundo xadrez
  para transparência, salvar como PNG/BMP/PPM. Arquivo corrompido vira mensagem na janela.
- **Mouse com roda** (protocolo IntelliMouse do PS/2, negociado no boot) e teclas F2/F5/PageUp/PageDown.
- **Pontos de extensão:** `desktop::apps_hook::{open_wasm, set_wallpaper}` (as frentes de apps WASM e
  de Configurações ligam depois).
- Ferramenta de host `fs3_inject` (injeta arquivos numa imagem v3). Testes: 1546 -> 1680 (+134).
## 2026-10 — Navegador completo e roda do mouse

- **Roda do mouse** no sistema todo: negociação IntelliMouse no PS/2 (pacote de 4 bytes, com
  queda para o de 3), `dz` no `Event::Mouse`, e a rolagem vai para a janela sob o ponteiro
  (Navegador, Task Manager, Arquivos, Editor).
- **Navegador:** imagens PNG/BMP/PPM (`<img>`, `data:` base64, `<a><img>`, no máximo 8 por página,
  512 KiB cada, 2 Mpx, decodificadas na thread de rede, cache LRU de 6 MiB), formulários GET
  (campos editáveis, Tab, Enter, acentos por teclas mortas, query UTF-8), botões de
  voltar/avançar, cursor de mão, sugestões (favoritos e histórico), favoritos (Ctrl+D,
  `osjeff://favoritos`, `BookmarkStore`), páginas `osjeff://`, PageUp/PageDown/Home/End/Espaço,
  busca na página (Ctrl+F), zoom 50-300%, seleção e cópia de texto, título da janela.
- `web::Doc` separa análise e diagramação; `base64` puro; `keymap::Key` ganhou PageUp/PageDown;
  a URL do navegador passou de 220 para 480 bytes.
- Testes: 1695 para 1910 (215 novos); novo alvo de fuzz `html_img_form` (um bug achado e corrigido).

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

## 2026-10 — Plataforma de apps WebAssembly

O WebAssembly deixou de ser "um app embutido numa thread" e virou a plataforma de apps do
SO: pacote com manifesto e permissões, vários apps ao mesmo tempo, instalação e lançador
([`docs/design/apps.md`](docs/design/apps.md), [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) §10).

- **Pacote e manifesto** (`osjeff_core::{wasmsec, appmanifest}`): um `.wasm` com as seções
  `osjeff.manifest` (`chave=valor`: id, nome, versão, permissões `fs`/`net`/`clipboard`,
  quotas, janela) e `osjeff.icon` (PNG até 64x64). Leitor de seções que nunca entra em
  pânico, quotas com teto imposto pelo sistema, alvo de fuzz `app_manifest`.
- **ABI v2** (módulo `osj`): janela, desenho (`blit_rgba`, `draw_image_png`), entrada por
  exports (`on_key/on_text/on_pointer/on_resize/on_tick/on_close`), tempo, `random`, `log`,
  clipboard, **arquivos** com raiz por app (`/data/<id>/`, `/home`), cota de disco e teto de
  descritores (`osjeff_core::appfs`, tabela de caminhos hostis, alvo de fuzz `app_sandbox`) e
  **rede** com filtro de destinos (`osjeff_core::appnet`; o transporte ainda não está ligado).
  O `host.*` v1 segue igual (snake, plasma, DOOM).
- **Execução multi-app**: `AppManager` com uma `Store` por app, thread `appd` em round-robin
  por fatia, contabilidade de CPU/memória/combustível por app, estados e término real
  (trap, falta de combustível, ponteiro inválido encerram só aquele app; a janela mostra "O
  app encerrou: <motivo>"). Cada janela WASM é uma instância do window manager dinâmico,
  redimensionável conforme o manifesto. Task Manager com seção APPS (`R` reinicia).
- **Instalação e lançador**: `/apps/<id>.wasm`, instalador que valida antes de gravar
  (`osjeff_core::appinstall`), Painel Iniciar com os apps instalados (ícone, nome, rolagem),
  vista **Apps** no Gerenciador de arquivos, apps de exemplo semeados no primeiro boot. O
  desktop do boot continua idêntico (dock inalterado, `verify-boot` com 0 pixels).
- **SDK e apps** (`wasm-apps/sdk`): wrappers seguros da ABI v2 e macros `manifest!`/`icon!`/
  `export_app!`; apps `hello`, `clock`, `notes` (arquivos em `/data/notes`), `paint` (BMP);
  `snake` e `plasma` empacotados. Passo a passo em [`docs/BUILDING.md`](docs/BUILDING.md).
- Limitações conhecidas: `/apps` e `/data` vivem em RAM até o `kernel::storage` existir
  (um só ponto de troca, `appfs_backend.rs`); `net_http_get` valida mas não transporta;
  sockets TCP e a roda do mouse (`on_scroll`) não existem; o menu do dock não lista os apps.

## 2026-10 — Gerenciamento do sistema

Log do kernel, monitor de recursos, configurações e notificações
([`docs/design/sysmgmt.md`](docs/design/sysmgmt.md)).

- `klog`: anel de 64 KiB sem alocação (seguro em ISR, com teste de zero alocações e de
  100 000 mensagens), `klog!(Warn, ...)` com a mesma saída serial de antes, espelho de todo
  `serial_println!`, app **Log do sistema** (filtro, busca, salvar).
- **Monitor** (Processos, Desempenho com gráficos de 60 s, Sistema com CPUID/RAM/boot) e
  **Configurações** (papel de parede incluindo imagem PNG/BMP/PPM do disco, destaque,
  relógio 12/24 h, fuso, ajuste do RTC, teclado ABNT2, rede, armazenamento, energia),
  persistidas em `osjeff.conf`; toasts para WARN/ERROR e `notify!`.
- Traits para outras frentes (`DiskUsage`, `NetStats`, `NetControl`, `LogSink`,
  `SettingsStore`); o dock e o desktop ocioso continuam idênticos.

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
