# Changelog

Formato inspirado em [Keep a Changelog](https://keepachangelog.com). O OSjeff não
tem releases versionadas; as seções são marcos na `master`.

## 2026-10 — Os apps do sistema (W25)

- **Tarefas** reúne o Gerenciador de tarefas e o Monitor de recursos num monitor de atividade com
  abas **CPU | Memória | Disco | Rede | Processos**: uso total e gráfico de 60 s que desliza a cada
  amostra, barras por processo, carga média, medidor de pressão da memória, memória por app, leitura e
  gravação do disco (contadores novos no driver ATA, sem lock e sem alocação), arquivos e pastas, IP,
  roteador, DNS, concessão e tráfego da rede, e uma tabela de processos com nomes amigáveis (Interface,
  Rede (busca), Aplicativos, Terminal (execução), Registro, Sistema), colunas ordenáveis, busca,
  Reiniciar e Encerrar (um serviço do sistema pede confirmação). Valor ao passar o mouse sobre os
  gráficos; números em português, alinhados; atualização suave e custo zero com a janela oculta.
  O tipo `Monitor` saiu; a Busca ainda o encontra ("monitor", "memória", "disco", "rede").
- **Registro**: tabela com hora, etiqueta colorida do nível, origem e mensagem em fonte mono; busca,
  filtro de nível, chave Seguir, Limpar e Salvar; rolagem suave com barra que some.
- **Ajustes** (eram as Configurações): barra lateral com Aparência, Papel de parede, Barra de apps,
  Teclado, Data e hora, Rede, Disco, Energia e Sobre; chaves, controles deslizantes, amostras de cor com
  anel, miniaturas ao vivo do papel de parede, lista de 52 cidades com busca, ampliação da barra de apps
  e duração dos banners (campos novos no arquivo, leitor ainda total).
- **Calculadora**: teclado arredondado com o destaque na coluna dos operadores, visor grande que encolhe,
  faixa de histórico, copiar, porcentagem, troca de sinal e memória; o teclado funciona.
- **Notificações**: ícone por nível, botão de fechar ao passar o mouse e linha que mostra o tempo que falta.
- Núcleo: `osjeff_core::activity` (nomes, formatação pt-BR, suavização, taxas de contadores que dão a
  volta, carga, pressão, tabela ordenável) e dez glifos novos; `FixedBuf` passou a imprimir UTF-8.
  Testes: 2532 no `osjeff_core` (eram 2497). Detalhes e números em `docs/design/ui-macos.md` (seção 11),
  `docs/design/sysmgmt.md` e `docs/TESTING.md`.
## 2026-10 — Identidade própria do shell (W26)

- **Janelas sem cara de macOS.** Botões de minimizar, maximizar/restaurar e fechar à **direita**,
  planos (células de 40x32, vermelho ao passar no fechar, apagados na janela sem foco), ícone do
  app e título alinhado à esquerda, botão de menu na barra de título com as mesmas entradas de
  Arquivo, Editar e Visualizar, e um fio de destaque na borda de cima da janela em foco. Pesquisa e
  decisões em `docs/design/ui-identity.md`.
- **Encaixe de janelas.** Arrastar à borda de cima maximiza, às laterais encaixa a metade, aos
  cantos o quarto, com um contorno animado; `Alt+setas` (e `Alt+Shift+setas`) encaixam, maximizam,
  restauram e minimizam; arrastar uma janela maximizada ou encaixada pela barra a solta sob o
  ponteiro. Geometria pura em `osjeff_core::snap`.

- **Painel superior no lugar da barra de menus.** Sem nome de app em negrito nem menus por app:
  **Apps** e Busca à esquerda, data e hora no **centro** (abre o calendário com o centro de
  notificações: histórico de avisos e erros, *Limpar*, chave *Não perturbe*) e, à direita, a pílula
  de status (rede, aparência, energia) que abre as **Configurações rápidas**, uma grade de blocos
  (Rede, Aparência, Movimento, Não perturbe, Relógio 24 h, Configurações), cores de destaque e
  Reiniciar / Desligar. Isso substitui a barra de menus e o popover de Controles. Botão direito em
  *Apps*: menu do sistema.

- **Barra de tarefas no lugar do dock com ampliação.** Barra flutuante arredondada no centro de
  baixo: botão Apps, apps fixados, apps abertos sem fixar e a faixa *Mostrar área de trabalho*
  (também `Ctrl+Alt+D`). Sem ampliação: o ícone sobe um pouco sob o ponteiro e mostra uma dica;
  uma pílula marca o app em foco e um ponto os demais; clicar foca, restaura ou minimiza,
  `Shift`+clique abre outra janela, arrastar um ícone fixado reordena (os vizinhos deslizam com
  mola) e o botão direito lista as janelas do app com *Fixar* e *Fechar*. Geometria e regras
  puras em `osjeff_core::taskbar` (layout, hit test, indicador, clique, reordenação).

- **Linguagem visual própria.** Ícones de tile quadrado arredondado (22 %), cor chapada com uma
  faceta de destaque e bisel de 1 px, sem gradiente nem brilho; ponteiro novo (seta fina com cauda
  arredondada e contorno índigo); **seis papéis de parede originais** (Crepúsculo com facetas
  geométricas, Aurora, Mono, Papel, Turquesa com colinas, Pôr do sol em faixas), com polígonos
  translúcidos no `osjeff_core::wallpaper`; janelas com raio de 8 px, borda de 1 px com realce
  interno e sombras menores. A galeria (`Ctrl+Alt+G`) ganhou a aba *Shell* com botões de janela,
  blocos, indicadores, encaixe e ponteiros.

- **Apps com trilho de categorias.** Trilho à esquerda (Todos, Sistema, Internet, Mídia,
  Utilitários, com a contagem de cada uma), busca no topo, linha **Recentes** (os cinco últimos apps
  abertos) e a grade; `Ctrl`+setas percorrem o trilho. Categorias numa tabela embutida e filtro,
  ranking e recentes puros em `osjeff_core::launcher`.

## 2026-10 — Nova interface (W22)

- **Visual novo, claro e escuro.** Barra de menus com menu do sistema, nome e menus do app em
  foco (ligados a ações reais), rede, Controles, Busca e relógio com data; barra de apps flutuante
  com ampliação por mola, pontos de app aberto, dicas e salto ao abrir; overlay **Apps** no lugar
  do painel iniciar; **Busca** (`Ctrl+Space`) com apps, arquivos do volume e uma calculadora;
  janelas com barra de título unificada, botões à esquerda, cantos de 12 px e sombra em duas
  camadas; folha de confirmação para reiniciar e desligar; popovers de Controles e de calendário;
  banners que deslizam no canto superior direito; cursores vetoriais (seta, mão, I). A identidade é
  própria (marca, nomes, ícones, paleta índigo, movimento); detalhes em `docs/design/ui-macos.md`.
- **Texto vetorial em toda parte.** Inter para a interface e JetBrains Mono para o terminal e o
  editor (ambas OFL, em `THIRD-PARTY.md`), por um leitor TrueType e um rasterizador próprios, no
  `osjeff_core`, com atlas lazy (917 glifos, 61 KiB, 34 ms no boot). A fonte 8x8 ficou só na tela
  de pânico.
- **Primitivas e movimento.** Formas com anti-aliasing, sombras analíticas, gradientes, desfoque e
  reamostragem; molas e curvas por tempo real, interrompíveis, com a chave *reduzir movimento*.
  Janelas abrem, fecham, minimizam (voando para o ícone), restauram e fazem zoom animados; o
  desktop ocioso continua custando zero quadros.
- **Configurações:** `appearance` (automática pela hora, clara, escura) e `reduce_motion`, no
  arquivo de configuração (leitor total) e na tela, aplicados na hora; novas cores de destaque e
  papel de parede dinâmico com esquema escuro.
- **Atalhos novos:** `Ctrl+Space` Busca, `Ctrl+W` fechar, `Ctrl+M` minimizar, `Ctrl+Alt+G`
  galeria de componentes, `Ctrl+Alt+H` HUD de desempenho (escondido por padrão).
- **Toolkit** em `desktop/ui.rs` (botões, campos, segmentado, switch, slider, listas, menus, dicas,
  gráficos) com geometria pura em `osjeff_core::{chrome,widgets}`; os apps antigos seguem
  funcionando dentro da moldura nova e seguem a aparência pelas funções de `theme`.
- Testes: 2414 no `osjeff_core` (eram 2331); custo de quadro antes e depois em
  `docs/TESTING.md`.
## 2026-10 — Correções do uso real (W20)

- **Cursor sem rastro.** O sprite do cursor vive só no framebuffer; os caminhos de desenho que
  subiam retângulos sem tratar `cursor_moved` (hover, clique, tecla) deixavam o sprite antigo na
  tela, um rastro de setas sob as janelas. Agora todo quadro que renderiza algo apaga o sprite
  no início e o pinta por último (`osjeff_core::cursor::CursorTrack`, modelo testado no host;
  invariante em `docs/ARCHITECTURE.md` 6.2). Prova no QEMU: 181 pixels fantasma por rodada antes,
  0 depois, em vários ritmos de mouse (`tools/perf/scen/w20-cursor.sh`).
- **Sites reais.** Uma página gzip maior que o limite de resposta ficava cortada no meio do fluxo e
  virava "Falha ao descompactar a pagina". Agora o prefixo decodificado é mostrado com uma faixa
  ("Pagina cortada no limite de tamanho"); CRC errado, conexão interrompida e dados corrompidos
  também mostram o que chegou. `Content-Encoding` em lista, deflate cru ou zlib e
  `Transfer-Encoding: gzip, chunked` entendidos; limites 1 MiB na rede e 4 MiB descompactado
  (orçamento de heap em `browser::MAX_RESPONSE_BYTES`). Novo alvo de fuzz `http_body`.
## 2026-10 — Entropia de verdade (W21)

- **Problema:** no QEMU com WHPX (Windows) a CPU do guest não tem `RDRAND` e o kernel caía num
  misturador de 64 bits (TSC e ticks) para o *client random* e a chave efêmera do TLS, o ISN do TCP
  (que era **zero**), o `xid` do DHCP, o id e a porta do DNS; o toast `RNG: weak fallback`
  aparecia a cada boot.
- **`osjeff_core::entropy`** (puro, `no_std`, `forbid(unsafe)`, sem dependência nova): ChaCha20
  (RFC 8439), DRBG de apagamento rápido de chave com contadores de reseed, pool SHA-256 com crédito em
  milibits e saúde por fonte, estimador de jitter (testes de "preso", variação e repetição) e a nota
  `Quality` (Weak < Mixed < Strong; timing sozinho nunca é Strong). 40 testes novos (vetores da RFC,
  respostas conhecidas calculadas à parte, determinismo, reseed, 1 MiB de saída com bit balance,
  qui-quadrado e transições, varredura de invariantes), mais `rng` (RDSEED, valores sabidamente
  ruins) e os ids PCI do virtio-rng. Novo alvo de fuzz `entropy_api` (14 no total).
- **Kernel:** `rng.rs` (RDSEED/RDRAND com tentativas e checagem de 0/tudo-1/bloco constante; anel de
  timestamps **sem alocação e sem trava** preenchido pelas ISRs do timer, teclado e mouse e pela
  chegada de quadros; dobra em contexto de thread; reseed periódico) e `virtio_rng.rs` (driver virtio
  1.x, `1af4:1005`/`1044`). Um só ponto, `rng::fill`, para TLS, ISN do TCP (semente do `smoltcp`),
  DHCP, DNS, SNTP, portas locais e `random_get`/`random` dos apps WASM (antes um xorshift por app).
- **Política:** Strong/Mixed seguem como antes e a mudança sai **uma vez** como linha INFO
  (`RNG: pool seeded from timing jitter (N bits credited)`), sem toast. Só Weak (< 128 bits) faz o HTTPS
  esperar até 5 s (coletando jitter de CPU) e depois **recusar**, com um aviso: acabou o caminho
  "usa o gerador fraco mesmo assim" (item do ROADMAP).
- **Scripts:** `tools/run.sh`, `tools/qemu-headless.sh`, `tools/perf/run.sh`, `run.ps1` e
  `tools/qemu-up.ps1` passam `-device virtio-rng-pci` por padrão quando o QEMU o tem
  (`QEMU_RNG=none` / `-NoRng` desligam, para exercitar o caminho de jitter).
- **Provado em QEMU** (`design/entropy.md` §5): com virtio-rng `RNG: strong`; sem ele e com
  `-cpu qemu64` o pool passa de 128 bits em ~1-2 s de ticks do timer e o HTTPS chega ao fim da
  verificação de certificado; os *client randoms* capturados no `net.pcap` são todos diferentes entre
  boots e entre conexões.
- Documentação: `design/entropy.md` (novo), SECURITY-MODEL §3.7, ARCHITECTURE §9.1, ROADMAP, TESTING.
- Testes: 2331 -> 2373 no `osjeff_core`; 14 alvos de fuzz.

## 2026-10 — Expansão do sistema: integração (W15–W19)

- Os quatro blocos (arquivos/VFS, editor e terminal, navegador completo, apps e sistema em disco) foram
  integrados na mesma árvore; os pontos de contato foram reconciliados em commits próprios:
  um único caminho da roda do mouse (`Desktop::handle_wheel`, `dz > 0` = para baixo; o decodificador
  PS/2 do W17 ficou), um único mailbox do `fetcher` com três clientes (navegador, apps WASM e shell,
  cada um com seu estado e o resultado só é retirado por quem postou), `DragMode::PageSelect` para a
  seleção do navegador.
- **Favoritos persistentes** em `/home/.bookmarks` (`SavedBookmarks`, texto `url<TAB>título`, leitor
  total e limitado a 64 entradas). Prova em QEMU em dois boots (`w19-bookmark*.sh`).
- **ABI dos apps:** `PageUp`/`PageDown` chegam aos apps WASM como `0x106`/`0x107` (`KEY_PAGE_UP/DOWN` no SDK).
- Testes: 2331 no `osjeff_core`; cobertura de linhas 96,6%; 13 alvos de fuzz.

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
