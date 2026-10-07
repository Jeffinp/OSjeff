# OSjeff — Relatório consolidado da auditoria (Fase 3, revisão independente)

Branch `audit/performance-security`. Este documento consolida `00-mapa`, `01-memoria-unsafe`, `02-interrupcoes-scheduler`, `03-superficie-ataque`, `04-desempenho`, `05-qualidade-rust` e `adr-isolamento`. Escrito pelo revisor com o HEAD `50c4e16`; **atualizado depois da Fase 4** (correções do kernel e integração do 04): a coluna Status e a seção H refletem o estado final. O revisor não escreveu nenhum deles e tratou cada achado CRÍTICO/ALTO como hipótese até ver o trecho de código ou reproduzir.

**Números de desempenho:** os do `04-desempenho` prevalecem sobre os dos outros relatórios; resumo na seção H. Todos são de QEMU/TCG sem KVM e valem como proporções, não como valores absolutos de hardware real.

Legenda de prova: **PROVADO** = reproduzido (teste, QEMU, execução) ou trecho conclusivo que li; **SUPOSIÇÃO** = raciocínio sem reprodução. Quando escrevo "confirmado por mim" fui eu quem rodou ou leu.

---

## A. Resumo de uma página (as 3 perguntas do dono)

### (i) O que pode derrubar ou corromper o sistema hoje, e o que vem de fora?

**Não há corrupção de memória achada.** `osjeff_core` é `forbid(unsafe_code)` e 3 alvos de fuzz (38 M execuções de rede, 1 M de disco, ~690 mil de HTML/CSS/HTTP) não acharam nenhum bug de memória. O que existe é **indisponibilidade** (a máquina para ou reinicia, quase sempre sem mensagem) e **perda de dado no disco**.

**Vindo de fora, já corrigido (9 commits `fix(...)`, todos com teste de regressão):**
- Rede: corpo `chunked` com tamanho gigante, HTML aninhado (~150 níveis estoura a pilha de 80 KiB), cor CSS com hex não-ASCII, margens CSS gigantes, porta de URL gigante, ARP malformado. Eram 3 travamentos remotos triviais. Reproduzi antes e depois (seção B): HEAD aguenta 20 000 níveis; `fc89615` aborta com 150. O kernel compila e boota em BIOS/128 MiB com os fixes.
- Disco (imagem OJFS forjada ou corrompida): ciclo de `parent` (recursão infinita), campo `size` > 1024, imagem curta. Corrigidos.

**Vindo de fora, ainda aberto:**
- ~~`http_get` sem teto de corpo~~ **corrigido** (`6b0ef64`): teto único de 256 KiB nos dois protocolos, resposta truncada e avisada na página.
- HTTPS **continua sem verificar certificado** (rótulo "Conexao nao verificada" na UI, `2bdc58b`); o RNG agora usa RDRAND quando a CPU tem (`948d460`), com fallback fraco registrado. Hoje isso é integridade do conteúdo, não derrubada (os parsers foram endurecidos). Como a pilha de rede fixa o IP do SLIRP do QEMU, o navegador só funciona sob `-netdev user`, o que limita a exposição real.
- ~~CSS com milhares de regras~~ **limitado** (`635ce91`): 1 000 regras, 2 000 seletores, 8 000 nós (1,59 s → 0,19 s no host com 5 000 × 5 000).
- Disco: **K4** (formatar o disco se a leitura falhar) foi **corrigido** (`11121bf`): falha de leitura agora deixa o disco intocado. Continua aberto: disco com conteúdo não-OJFS (sem magic `OJF2`) ainda é formatado, por desenho; **K3** (gerenciador abre/salva sempre o arquivo da raiz).

**Interno (sem atacante), o que mais derruba:**
- ~~Estouro de pilha do compositor vira triple fault~~ **corrigido** (`a56218d`): GDT/TSS própria, IST para #DF, pilha de boot de 512 KiB. Reproduzido antes (3 relatórios) e depois (BIOS e UEFI: `FATAL EXCEPTION: #DF`, sem triple fault).
- **Ainda aberto:** pilhas das outras threads estão no heap sem página de guarda. O canário de 8 bytes é contornável por um frame grande (corrompeu 80 bytes sem ser detectado, PROVADO) e, quando dispara, o `panic!` roda dentro do ISR e para a máquina inteira (agora com tela de erro).
- ~~Panic, exceção e OOM são mudos~~ **corrigido na serial** (`46ab710`): panic, #DE/#UD/#NP/#SS/#GP/#PF/#DF imprimem vetor, RIP, RSP (e CR2) em COM1; verificado com `ud2`, leitura de endereço inválido, `panic!` e alocação de 200 MiB. Continua sem mensagem **na tela**.
- ~~Um app WASM em laço infinito prende a CPU~~ **corrigido** (`40c0cea`): *fuel* por chamada (20 M; 256 M na inicialização), 24 MiB de memória, o app é encerrado e liberado; provado com guests hostis (laço infinito, `memory.grow` em laço, `proc_exit`, `random_get` gigante).
- UEFI: 128 MiB panica (BSS de 91 MiB; o runner `os` agora usa 256M, `687c08a`); framebuffer maior que 1920x1080x4 agora é recusado com mensagem (`cdfad5b`) em vez de estourar os buffers. Ninguém testou em hardware real.

### (ii) Onde o tempo é gasto?

Resposta curta (detalhe e método no `04-desempenho`, resumo na seção H): **no boot, o splash fixo (4–5 s) e o firmware/bootloader (~3,7 s); o init real do kernel é ~0,18 s. Em regime, o maior custo era escalonamento (o compositor só recebe 1 de cada 3 fatias) e, depois, sombras de janela (~97% do quadro de recomposição) e o repaint da cena inteira a cada segundo por causa do relógio.** Os dois últimos foram corrigidos (seção H). Medições dos outros relatórios (QEMU/TCG sem KVM, **não representativas de hardware real**):
- **Boot:** desktop em < 14 s. Pelo menos 5 s são o splash fixo (`main.rs:259,671`), mais ~0,1 s de calibração do TSC e até ~0,6 s de DHCP.
- **Scheduler:** round-robin sem estado "bloqueado". O compositor roda a **83 Hz** em idle (250/3), medido pelo contador de iterações. O app WASM recebe 1/3 da CPU. A coluna "CPU" do Task Manager conta fatias de threads dormindo em `hlt`, então não mede CPU.
- **ISR do timer:** 6–17 µs por tick (0,15–0,4% do período). Lock do heap: 1–4 µs em média. Nenhum dos dois é gargalo.
- **WASM:** wasmi é 25–37x mais lento que nativo num laço de pixels (host). DOOM exige ~4,5–9 M de fuel por quadro (~35–70% de um núcleo, extrapolação). No TCG, DOOM roda a 2–4 quadros/s.
- **Render:** BIOS entrega framebuffer de 24 bpp (medido `bytes_per_pixel: 3`), então o caminho rápido de 32 bits de `fb.rs:165` não é usado. Inferência pela leitura, ainda sem benchmark.
- **Memória:** BSS = 91 MiB; heap (64 MiB) + 3 buffers de 1080p (24 MiB) são 96% disso. O excedente de RAM física não é usado.

### (iii) Qual o próximo passo de arquitetura que vale o esforço?

O ADR recomenda a **Opção C**: endurecer o kernel (S0–S4), tornar o WASM a fronteira de isolamento com fuel e limites (S5), e **adiar o ring 3** (S7, 22–33 dias de MVP, 50–75 com apps portados) até haver gatilho concreto. **Concordo, com um ajuste.** O S0 (falhas visíveis, só na serial), o S1 (GDT/TSS/IST e pilha de boot) e o S2 (limite de profundidade) já foram feitos. A principal motivação do S6 (portar o motor web para wasm porque HTML hostil derrubava a máquina) perdeu força: o cap de 40 níveis mais o fuzz resolvem esse caso por uma fração do custo. Ordem que eu seguiria agora: **S5 → S3/S4**, e só então decidir se S6 ainda é necessário. W^X/NX já funciona (medido pelo ADR), não precisa de código novo.

---

## B. Verificação independente (o que eu mesmo conferi)

| Item | Resultado | Como |
|---|---|---|
| `cargo test -p osjeff_core` | **201 passam** (README diz 152; os relatórios 00/01/05 viram 189, os +12 são testes de regressão dos fixes) | rodei |
| Cobertura de linhas `osjeff_core` (bruta, com módulos de teste) | **93,25%** agora (o 94,46% que apareceu em rascunhos é cobertura de *regiões*; 05 mediu 92,33% de linhas antes dos novos testes; README diz ~98%) | `cargo llvm-cov -p osjeff_core --summary-only` |
| `unsafe` no kernel | **116 ocorrências**; só **2 linhas** com `SAFETY` (`grep -rn SAFETY kernel/src`); `static mut` = 0; `osjeff_core` sem `unsafe` | `grep` |
| HTML aninhado, pilha de 80 KiB, release+LTO | **antes (`fc89615`): 100 ok, 150 e 300 abortam por estouro de pilha. Depois (HEAD): 100, 150, 300 e 20 000 ok** | crate descartável no scratchpad, 80 KiB |
| `dechunk("FFFFFFFFFFFFFFFFF")` e `color:#é1` | **antes: panic nos dois. Depois: ok** | idem |
| OJFS `parent` auto-referente + `purge_slot` | **antes: stack overflow. Depois: ok** (`alloc` ainda aceita `parent` inválido, mas a recursão não anda mais) | idem |
| OJFS `size` = 0xFFFF | **antes: panic (`fs.rs:139`). Depois: `size_at` = 1024** | idem |
| Kernel com os fixes | `cargo build -p kernel --release` ok; imagem BIOS 128 MiB boota ao desktop (screenshot, `thr 3`) | `tools/qemu-headless.sh bios` |
| `cargo lint-kernel` | falhava (1 erro, `render.rs:74`); **corrigido** em `bfe6166`, hoje `lint-kernel` e `lint-host` passam | rodei |
| TLS sem verificação | `UnsecureProvider` (`netstack.rs:247`) e `impl CryptoRng for Rdtsc` (`:424`) | li |
| `panic_handler` mudo | `main.rs:777-780`: só `halt()`. Handlers #PF/#GP/#DF idem (`interrupts.rs:147-159`) | li |
| Sem GDT/TSS/IST | nenhum `gdt`/`tss`/`set_stack_index` em `kernel/src` | `grep` |
| Canário checado dentro do ISR | `sched.rs:160-162`, `panic!` com IF=0 | li |
| `http_get` sem teto; HTTPS com 256 KiB | `netstack.rs:174-190` vs `:277` | li |
| K4 (formatar em falha de leitura) | `desktop/mod.rs:202-203`, só IDE secundário master (`ata.rs:3,11`) | li |
| K3 (arquivo da raiz) | `fs::read` → `find` → `find_in(ROOT)`; `fs_save` usa `fs::write` (raiz) | li |
| Buffers estáticos 1920x1080x4 | `main.rs:60,104`; `Canvas` indexa com o `info` real e panica fora do buffer | li |
| Sem fuel/limiter no wasm | `grep fuel\|limiter\|StoreLimits kernel/src` = vazio; `Engine::default()` | `grep` |
| Kernel sem SSE | target spec: `-sse...,+soft-float` | `rustc --print target-spec-json` |
| DHCP ignorado | `netstack.rs:19-21` fixa 10.0.2.15/2.2/2.3 | li |
| Padding frontal do allocator vaza | `allocator.rs:187-199`: só `excess` é reinserido | li (a simulação em host é do 01) |

**Não reproduzi** os experimentos de QEMU do 01/02/ADR (triple fault, panic mudo, canário contornado, guest em laço). Marquei PROVADO porque três relatórios independentes concordam nos mesmos resultados e eu li o código. Quem quiser provar de novo: `tools/qemu-headless.sh` com os patches descritos nos apêndices.

**Divergências entre relatórios que corrigi:**
- Os hashes do 03 (`a4a11e6`, `1da81dc`...) são de outra worktree e não estão nesta branch. Os corretos estão na tabela abaixo.
- 02 chama de ALTA o que 01 chama de MÉDIA (canário/pilhas). Usei MÉDIA nos dois.
- Contagem de `unsafe`: 116 ocorrências (grep, 00), 100 blocos sem `SAFETY` (clippy, 01/05), 114/116 sem `SAFETY:` (script do 00). Todas coerentes, medem coisas diferentes.

---

## C. Tabela única priorizada

Ordem: itens abertos por prioridade (alavanca ÷ esforço, depois severidade), depois os já corrigidos. Severidade **já reavaliada**; "↓" indica rebaixamento e o motivo está na seção "Rebaixados e descartados". Esforço em dias de um dev (estimativa; as do ADR são NV).

| # | Sev. | Esforço | Área | Título | Status | Prova |
|---|---|---|---|---|---|---|
| 1 | MÉDIA | 1–2 d | Kernel/diagnóstico | Panic, exceções, OOM e canário são mudos (só `hlt`); `panic!` do canário roda no ISR e mata a máquina toda; threads panicadas viram zumbis | **CORRIGIDO** `46ab710` (serial) e `4d241a0` (tela de erro, panic/exceção/OOM); falta tratar thread panicada (S4) | PROVADO antes e depois (QEMU) |
| 2 | MÉDIA ↓ | 1–2 d | Kernel/IRQ | Sem GDT/TSS/IST: estouro da pilha de boot (80 KiB) → #PF → #DF → triple fault; #UD/#DE/#NP e IRQ 7/15 sem handler caem em #DF | **CORRIGIDO** `a56218d` (IST/#DF), `46ab710`+`74d6249` (todas as exceções e IRQ 7/15) | PROVADO antes e depois (BIOS e UEFI) |
| 3 | MÉDIA | 0,5 d | Disco | K4: erro transitório de leitura ou disco sem magic `OJF2` → `fs::format` + `write_image` sobrescreve o LBA 0 | **CORRIGIDO** `11121bf` (falha de leitura); disco não-OJFS ainda é formatado (por desenho) | PROVADO (hash do disco igual após falha injetada) |
| 4 | MÉDIA | 1 d | Disco/UI | K3: gerenciador abre/salva por nome na raiz; arquivo dentro de pasta não abre e Ctrl+S cria ou sobrescreve o homônimo da raiz | **CORRIGIDO** `a969ec4` (+`e63fa38`): abre e salva na pasta certa; screenshots em `docs/img/k3-*` | PROVADO por leitura |
| 5 | MÉDIA ↓ | 0,5 d | Rede/UI | `https://` sem aviso de "não verificado"; RNG xorshift declarado `CryptoRng` (chave efêmera previsível) | **PARCIAL** `2bdc58b` (rótulo "Conexao nao verificada"), `948d460` (RDRAND); verificação de certificado: A FAZER (ROADMAP 2) | PROVADO por leitura; ataque SUPOSIÇÃO |
| 6 | MÉDIA | 2–3 d | WASM | Sem fuel, `ResourceLimiter`, kill/restart; fechar a janela não libera a `Store`; laços `n`/`len` do guest nas host fns; `proc_exit` finge | **CORRIGIDO** `40c0cea`, `88539c7` (fuel, 24 MiB, término real, tetos WASI); *fuel* retomável: A FAZER (ROADMAP 4) | PROVADO (ADR: laço infinito, grow 513 pág.; mitigação validada) |
| 7 | MÉDIA | 0,5 d | Rede/memória | `http_get` sem teto de corpo (HTTPS tem 256 KiB) e DOM/CSS sem teto de nós/regras; heap único de 64 MiB sem cota | **CORRIGIDO** `6b0ef64` (teto único de 256 KiB), `635ce91` (nós/regras) | OOM PROVADO (mudo); vetor SUPOSIÇÃO |
| 8 | MÉDIA | 2–3 d | Scheduler/perf | Round-robin sem estado bloqueado: compositor a 83 Hz idle, WASM com 1/3 da CPU, Task Manager "CPU" conta fatias dormindo | **CORRIGIDO** `ca996c1`, `79d23dc`: compositor idle 83 → 250 it/s, tecla→captura ~11 → ~0,4 ms (medido) | PROVADO (medido, 02 e 04) |
| 9 | MÉDIA ↓ | 4–6 d | Kernel/threads | Pilhas de thread no heap sem guard page; canário de 8 B contornável (corrompeu 80 B sem detecção); uso real 9–13 KiB de 128 KiB | PROPOSTA-ADR (S3/S4); ROADMAP 1 | mecanismo PROVADO; gatilho real SUPOSIÇÃO |
| 10 | MÉDIA | 2–3 d | Boot/hardware | UEFI com 128 MiB panica (BSS 91 MiB; `os/src/main.rs` usa `-m 128M`); framebuffer > 1920x1080x4 estoura buffers estáticos → panic mudo no splash | RAM **CORRIGIDO** `687c08a`; framebuffer grande **CORRIGIDO** `cdfad5b` (recusa com tela de erro); adaptar à resolução: ROADMAP 5 | RAM PROVADO; resolução: mecanismo PROVADO, cenário SUPOSIÇÃO |
| 11 | BAIXA | 1 d | CI/qualidade | Não há CI; `lint-kernel` e `lint-host` falham (`render.rs:74`); `fmt` com 27 diffs; badges do README mentem | **CORRIGIDO** `bfe6166`, `ea1c4c6`, `3ba2b12` (fmt bloqueante) | PROVADO (rodei) |
| 12 | BAIXA | 0,5 d | Deps | `spin 0.9.8` (yanked) linkado no kernel via wasmi; `bootloader_api` 0.11.15 vs 0.11.17; `license` ausente nos 3 crates; sem `deny.toml` | **CORRIGIDO** `3f679af`, `677af15`, `8ebcc89` (`deny.toml`, `publish = false`) | PROVADO |
| 13 | BAIXA | 1 d | Docs | README/ARCHITECTURE com afirmações falsas (seção F) | **CORRIGIDO** `ea1c4c6` e a reescrita dos READMEs/ARCHITECTURE | PROVADO |
| 14 | MÉDIA ↓ | 5–10 d (incremental) | Qualidade | Kernel com 0 testes; 6 682 linhas sem cobertura; ~2 800 migráveis para o core (regras de janela e entrada, ring SPSC, DHCP, redirect); fuzz agora existe (`6c88b95`) mas só roda a mão | **PARCIAL**: ~700 linhas migradas, +144 testes (`7fc53c8`…`0d3dc36`); resto: ROADMAP 6 | PROVADO |
| 15 | BAIXA | 2–4 d | `unsafe` | 114 de 116 sem `SAFETY:`; `fn` seguras devolvem `&'static mut` (`disk()`, `scratch_slice()`, `scheduler()`, `app_mut()`); aliasing real em `files_rows`; `RacyCell: Sync` sem `T: Send`; `outb` seguro | **CORRIGIDO** (`SAFETY` em 100% dos blocos, `9726f45`; lint ligado `46acdb0`); `fn` seguras que devolvem `&'static mut` continuam, documentadas como NOTE | PROVADO por leitura (UB formal; nenhuma miscompilação demonstrada) |
| 16 | BAIXA ↓ | 1 d | Rede/DHCP | K2: DHCP lido e descartado; IP/gateway/DNS fixos do SLIRP, browser só funciona sob QEMU user-net | A FAZER (ROADMAP 3) | PROVADO por leitura |
| 17 | BAIXA ↓ | 1–2 d | Rede/NE2000 | K5/K7: frame que cruza o fim do anel lido errado; laço de poll sem orçamento (flood); `send` sem teto; `curr-1` | **PARCIAL** `ed90a2d` (defensivo: teto de `send`, `curr-1`, orçamento); frame que cruza o anel: A FAZER (não provado) | SUPOSIÇÃO |
| 18 | BAIXA | 0,5 d | Memória | Allocator perde o padding frontal com `align > 8`; `align_up` sem `checked_add` | **CORRIGIDO** `d88d138` (testado com modelo de 20 mil operações: 0 B vazados) | PROVADO (sim. em host; kernel 7 952 B); impacto atual ~0 |
| 19 | BAIXA | 1 d | Web | Cascata CSS O(n²) sem teto; `resolve_redirect` troca o esquema (e permite https→http); `decode_utf8(&[])` panica; `parse_url`/`Location` sem filtro de controles | **CORRIGIDO** `8ad6e77`, `4ba113d`, `b77d79c` (redirect, `decode_utf8`, controles) e `635ce91` (cascata limitada) | cascata PROVADA no host (0,39 s); resto por leitura |
| 20 | BAIXA | 1 d | Diversos | `next_pid` u16 dá a volta (pid 0 = sentinela); `anim_signature` quebra com ≥ 9 janelas; calibração do TSC sem timeout; `fxsave/fxrstor` inócuos (kernel soft-float); virtio sem validação de `qsize`/BAR (latente: virtio-gpu não é instanciado no runner padrão) | **PARCIAL**: `anim_signature` **CORRIGIDO** `cbd212d`; virtio validado `62ac012`; resto A FAZER | pid PROVADO; resto leitura/SUPOSIÇÃO |
| 21 | BAIXA | 22–33 d (MVP) | Arquitetura | Ring 3 + paginação por processo (ADR S7) | PROPOSTA-ADR (adiar); ROADMAP 9 | — |
| 22 | ALTA ↓ | feito | Rede/web | HTML aninhado ≥ ~150 níveis estoura a pilha de 80 KiB (parser recursivo); `MAX_DEPTH = 40` | CORRIGIDO `3c66ca6` | PROVADO (reproduzi antes e depois) |
| 23 | ALTA ↓ | feito | Rede/web | `dechunk`: tamanho de chunk gigante → panic no corpo HTTP | CORRIGIDO `daeb5be` | PROVADO (reproduzi) |
| 24 | ALTA ↓ | feito | Rede/web | `parse_color("#é1")` → panic (CSS remoto) | CORRIGIDO `f19d50b` | PROVADO (reproduzi) |
| 25 | MÉDIA ↓ | feito | Disco | OJFS: `parent` em ciclo → recursão infinita em `trash_slot`/`purge_slot` | CORRIGIDO `59749a2` | PROVADO (reproduzi) |
| 26 | MÉDIA ↓ | feito | Disco | OJFS: `size` > 1024 lê além do registro / panica em `fs_load` | CORRIGIDO `04e824f` | PROVADO (reproduzi) |
| 27 | MÉDIA | feito | Rede/web | CSS `margin:2147483647` estoura i32 (coordenadas negativas em release) | CORRIGIDO `bc18c88` | PROVADO (03, testes debug e release) |
| 28 | MÉDIA | feito | Rede | Porta da URL/`Location` estoura u32 (conecta na porta errada) | CORRIGIDO `f827ead` | PROVADO (03) |
| 29 | BAIXA | feito | Rede | ARP: `respond` panica com `out` pequeno; ARP com hlen/plen estranhos respondido | CORRIGIDO `3810d41` | PROVADO (03, fuzz com 1 byte) |
| 30 | BAIXA | feito | Disco | OJFS: imagem menor que `IMAGE_SIZE` → panic em `is_used` | CORRIGIDO `34ef07b` | PROVADO (03) |

Resíduo dos fixes: o cap de 40 níveis descarta o *wrapper* e mantém o conteúdo no pai, então páginas reais muito aninhadas perdem estilo, não texto. Nenhum relatório reexecutou o cenário no QEMU após o fix; eu o reproduzi no host (80 KiB) e confirmei que o kernel compila e boota. `alloc` do OJFS ainda aceita `parent` inválido (05 A-04 propõe validar); sem consequência de travamento agora.

---

## D. As 10 primeiras coisas a fazer

**Estado final (2ª rodada):** feitos 1 (serial e tela), 2, 3 (K3), 4, 5 em parte (rótulo e RDRAND), 6, 7, 8, 10 (RAM e framebuffer grande). Restam: verificação de certificado TLS, guard pages e thread morta em vez de máquina morta, adaptar à resolução de tela, e o resto do [ROADMAP](../ROADMAP.md). A lista original segue abaixo; o texto riscado é o que já foi entregue.

1. ~~**Tornar falhas visíveis (#1, ADR S0).**~~ *Feito na serial em `46ab710`; falta a tela de erro e o `cli` explícito.* Imprimir `PanicInfo`, vetor, RIP, CR2 e código de erro na serial (e uma tela de erro simples) nos handlers de panic/#PF/#GP/#DF; preencher o resto da IDT com um handler genérico; `cli` antes do `hlt`. Porquê: todo o resto falha em silêncio hoje; sem isso você não consegue nem provar que os outros fixes funcionam. Transforme as teclas de falha do ADR (F1–F12) em `selftest` para regressão, incluindo o HTML aninhado pós-fix.
2. ~~**Parar de formatar o disco por engano (#3).**~~ *Feito para falha de leitura em `11121bf`; falta exigir setores zerados antes de formatar um disco sem magic (muda comportamento de primeiro boot, pergunte antes).* Só formatar se a leitura teve sucesso **e** os primeiros setores estão zerados; falha de leitura mantém o FS só em RAM sem gravar. Porquê: é a única perda de dado em uso normal, e custa meio dia.
3. **Corrigir K3 no gerenciador de arquivos (#4).** Guardar o slot (ou o `parent`) do arquivo aberto e usar `read_slot`/`write_in`. Porquê: a hierarquia de pastas é o commit mais recente e hoje não dá para abrir um arquivo dentro de uma pasta; é bug em fluxo comum.
4. ~~**GDT própria + TSS + IST para #DF/#PF e pilha de boot maior (#2, ADR S1).**~~ *Feito em `a56218d` (IST só no #DF).* `kernel_stack_size = 512 KiB` no `BOOT_CONFIG`. Porquê: transforma triple fault (reset sem pista) em mensagem e `hlt`. Teste em BIOS **e** UEFI (a GDT do bootloader fica em endereços diferentes).
5. **Honestidade no HTTPS (#5).** Rótulo "conexão não verificada" na barra, nunca mostrar cadeado; remover `impl CryptoRng for Rdtsc` e semear com `RDRAND` (checando CPUID); não rebaixar https→http no redirect. Porquê: custa meio dia e remove a falsa sensação de segurança. A verificação completa de certificado fica para depois (ver seção E).
6. **Limites baratos de recurso (#6 e #7, parte do ADR S5).** `consume_fuel` com orçamento por `render`/`on_key`, `StoreLimitsBuilder::memory_size(24 MiB)` (protótipo já validado: `OutOfFuel` e 513 → 321 páginas), teto de 256 KiB no `http_get`, teto de iovecs/`len` nas host functions, teto de nós/regras no `web::render`. Porquê: poucas linhas cada, validados em protótipo, e fecham os travamentos que sobraram.
7. ~~**CI mínimo e higiene (#11, #12, #13).**~~ *Feito em `bfe6166`, `3f679af`, `677af15`, `ea1c4c6`; falta `cargo fmt` (23 diffs) e `deny.toml`.* `.github/workflows/ci.yml` com `cargo test -p osjeff_core`, build `no_std` do core, clippy (`lint-host` só com `-p osjeff_core`), `lint-kernel` (corrigir `render.rs:74`), build da imagem, `cargo fmt --check`; um job curto rodando as entradas de `fuzz/regressions/`. `cargo update -p spin -p anyhow -p bootloader_api`, `license = "MIT"` nos 3 crates, e corrigir o README (seção F). O YAML pronto está em 05 §2. Porquê: os badges do README passaram mentindo justamente porque nada roda sozinho.
8. **Estado "bloqueada" no scheduler (#8).** `Ready`/`Blocked(wake_tick)`, `hlt` nos workers vira `yield`/`block_until`, e o Task Manager passa a contar CPU útil. Porquê: é o maior ganho de desempenho identificado (compositor 83 Hz → 250 Hz; app WASM de 1/3 para o resto da CPU) e destrava uma métrica honesta. Confirmar com o 04 antes de medir "depois".
9. **Guard page nas pilhas de thread e thread morta em vez de máquina morta (#9, ADR S3/S4).** Pilha alinhada a 4 KiB com página não presente abaixo; #PF/panic em thread com IF=1 marca a thread como morta, imprime e troca de contexto. Porquê: é o que separa "um app caiu" de "o sistema caiu" e elimina o único caminho real de corrupção silenciosa do heap. Depende dos itens 1 e 4.
10. **Boot robusto em UEFI e telas grandes (#10).** Dimensionar os buffers pelo framebuffer (ou recusar/reduzir a resolução com mensagem), validar `Canvas::new` contra `stride*height*bpp`, e usar `-m 512M` em `os/src/main.rs` quando for UEFI (ou reduzir o BSS). Porquê: o README promete "boota num PC real", e nenhum teste em hardware foi feito; é o ponto mais provável de falhar na primeira demonstração.

---

## E. O que NÃO vale a pena fazer agora, e por quê

| Ideia | Por que não agora |
|---|---|
| **Ring 3 completo / paginação por processo (S7)** | 22–33 dias só para o MVP, 50–75 com apps portados (estimativa). Os apps nativos são Rust seguro e o que derruba o sistema é pilha/panic/heap, que S0–S4 resolvem sem MMU. Reabrir só com um dos gatilhos do ADR: binários nativos de terceiros, escape comprovado do wasmi, multiusuário ou exploração real. |
| **Portar o motor web para wasm (S6)** | A motivação principal (HTML hostil → triple fault) já foi tratada com `MAX_DEPTH` e fuzz. Wasmi é 25–37x mais lento num laço de pixels; parser/layout CPU-bound sofreria. Esperar S5 e decidir com dados. |
| **`clippy::pedantic` inteiro** | 1 147 avisos no kernel (557 só em `font.rs`) e 160 no core, quase tudo ruído (`must_use_candidate`, `unreadable_literal`, casts de pixel). Ligar só `undocumented_unsafe_blocks` (warn), `string_slice` e `indexing_slicing` por módulo nos parsers (`fs`, `net`, `browser`, `web/dom`). |
| **Trocar ou reescrever o allocator** | Lock medido em 1–4 µs; picos são ruído do TCG. O vazamento do padding frontal tem impacto ~0 hoje (3 alocações com `align > 8` na sessão inteira). Corrigir as 3 linhas quando tocar no arquivo; extrair para crate testável com miri é bom, mas não é urgente. |
| **Verificação completa de certificado TLS (trust store + relógio RTC)** | Esforço alto, e o navegador não guarda segredo nenhum. O rótulo "não verificado" + RNG decente (item 5) já retira o risco de enganar o usuário. Fazer se o navegador for tratar login ou dados pessoais. |
| **Reescrever o scheduler (prioridades, SMP, APIC)** | O problema medido é só a falta de estado `Blocked`/`yield`. O resto é escopo de outro projeto. |
| **Migrar as ~2 800 linhas do kernel para o core de uma vez** | Faça incremental, ao tocar em cada arquivo, começando por `resolve_redirect`, ring SPSC, regras de janela e DHCP. Uma migração em bloco bloqueia o resto sem ganho imediato. |
| **Newtypes para tudo (`Port<T>`, `PciAddr`, `Pid`, `WinId`...)** | Valor real, mas nenhum desses causou bug. Aplicar só em código novo ou ao refatorar o driver. |
| **Tirar `abi_x86_interrupt` / `-Z bindeps` do nightly** | Pin datado funciona e o `rust-toolchain.toml` já diz para atualizar de propósito. Um job semanal "nightly latest" não bloqueante (05) basta. |
| **Atualizar majors (`smoltcp` 0.14, `wasmi` 2.0, `sha2` 0.11)** | Sem vulnerabilidade conhecida (`cargo audit`: 0). Só patches seguros (`cargo update -p spin ...`). |
| **Remover `fxsave/fxrstor`** | Custo ~200 ciclos/tick, inofensivo. Documentar "kernel soft-float" e não mexer. |
| **Otimizar o caminho de render de 24 bpp** | Ainda é inferência. Esperar o benchmark do 04. |
| **Fuzz de TCP/TLS/DNS** | `smoltcp` e `embedded-tls` são externos. Mais rendimento em rodar os 3 alvos existentes por horas e em CI. |

---

## F. Afirmações do README/ARCHITECTURE que são falsas ou desatualizadas

| Onde | Afirma | Realidade |
|---|---|---|
| README:12,42,192,198; ARCH:33 | 152 testes | **201** (confirmado por mim) |
| README:42,193,198; ARCH:27 | ~98% de cobertura; fs·net ~99% | **93,25%** bruta de linhas (inclui os próprios módulos de teste), ~88% só produção, 72,5% de branches (05); kernel **0%** |
| README:13,195,208 | clippy `-D warnings` verde; `lint-host` e `lint-kernel` | os **dois falham** (`render.rs:74`); `lint-host` falha porque `-p os` compila o kernel (confirmado por mim) |
| README:52; ARCH:183 | `worker-a` e `worker-b` provam preempção com CPU idêntica | os workers não existem mais; a "CPU idêntica" é artefato do round-robin contando fatias de threads dormindo em `hlt` |
| README:132; ARCH:86; ARCH:314 | exceções fatais "travam visível em vez de triple-fault"; canário gera `panic` com o nome da thread | falso: nada é impresso (serial e tela vazias); estouro da pilha de boot dá **triple fault**; o canário não cobre o compositor, é de 8 B, e o `panic!` roda no ISR e mata a máquina |
| ARCH:28,307 | `unsafe` "isolado e auditável" | 116 ocorrências em 18 arquivos do kernel, 2 linhas com `SAFETY` (confirmado por mim) |
| ARCH:313 | `RacyCell`: "soundness justificada (single-core + exclusão por flag de interrupção)" | só o ring de entrada tem essa exclusão; o resto depende de "dono único por thread". `RacyCell` é `Sync` sem `T: Send` e há 4 `fn` seguras que devolvem `&'static mut` |
| README:252; ARCH:141-145,175-176 | `fxsave/fxrstor` protege `xmm`; errar o alinhamento da pilha dá `movaps` #GP | o kernel é compilado sem SSE (`+soft-float`, confirmado por mim) e `CR4.OSFXSR=0`: sem efeito |
| README:41,226 | `kernel/src/desktop.rs` | agora é o diretório `kernel/src/desktop/` |
| ARCH:248,267 | 16 registros, imagem de ~17 KiB | `MAX_FILES = 48`, com diretórios e lixeira |
| README e ARCH | silenciam wasm/wasmi, TLS, smoltcp, browser HTML/CSS, DOOM, heap de 64 MiB | existem e são grande parte do código; HTTPS **não verifica certificado**, nada no README avisa |
| README:24 ("boota num PC real") | PC real | nenhum relatório testou hardware; BIOS limita a 1280x720 em 24 bpp, UEFI exige 512 MiB e panica acima de 1080p, a NIC é NE2000 ISA, o IP é fixo do SLIRP |
| `os/src/main.rs:19` | `-m 128M` "plenty" | só vale para BIOS; UEFI panica (medido) |
| `wasm/mod.rs:40-42,79,308`; `main.rs:633-637`; `process.rs:1-6` | "single-threaded"/"compositor only"; "timer a 250 Hz acorda o compositor"; "no preemptive scheduler yet" | o app roda em thread própria desde `33b5908`; o compositor roda a ~83 Hz; o scheduler preemptivo existe |
| `wasm-apps/` (implícito) | apps WASM embutidos | `plasma` é órfão (nenhum caminho do `build.rs` o seleciona); DOOM exige `wasi-sdk`, rede e um WAD fora do repo, não reproduzível do checkout puro |

---

## G. Rebaixados e descartados

**Rebaixados (e por quê):**
- **CRÍTICA → ALTA** (03 #1–#3: `dechunk`, HTML aninhado, `parse_color`): efeito é parar a máquina por conteúdo remoto, sem corrupção de memória nem execução de código; exige que o usuário abra a página ou haja MITM. Reproduzi os três.
- **ALTA → MÉDIA** (02 #1 pilhas sem guarda/canário; 02 #2 sem IST): o mecanismo é provado, mas o gatilho conhecido (HTML) foi corrigido e o pico medido de pilha é 11 KiB de 80 KiB (compositor) e 9–13 KiB de 128 KiB (threads). Subiria de novo se alguém medir o handshake TLS acima de ~100 KiB (ninguém mediu: sem rede no sandbox) ou se o DOOM/wasmi estourar a pilha de 128 KiB.
- **ALTA → MÉDIA** (03/05 K1/A-03, TLS sem verificação e RNG fraco): para derrubar não serve mais (parsers endurecidos); sem segredos para roubar; o IP fixo do SLIRP limita o navegador a QEMU. Continua real como integridade e como falsa sensação de segurança. Prova do ataque (MITM, previsão da semente) **não existe**, só a leitura do código. Para provar: servidor TLS com certificado inválido respondendo ao guest, e medir a entropia da semente em boots repetidos.
- **ALTA/MÉDIA → MÉDIA** (OJFS `parent` e `size`): vêm do disco IDE secundário (acesso físico ou imagem forjada), não da rede.
- **MÉDIA → BAIXA:** K2 (DHCP: recurso ausente, não falha), K5/K7 (NE2000 em hardware ISA raro, tudo SUPOSIÇÃO), A-08 (`anim_signature`, WIN_COUNT = 7), A-07/A-06 (documentação; mantive o item de soundness em BAIXA porque nenhuma miscompilação foi mostrada).
- **K4:** mantida MÉDIA. O caminho de código é certo; o gatilho (erro transitório do ATA ou disco alheio no master secundário) é raro. O custo da correção é de meio dia.
- **Item 1 (panic mudo):** severidade MÉDIA, mas prioridade 1 porque custa pouco e multiplica o valor de todo o resto.

**Descartados ou sem ação:**
- `SpinLock` do heap segurando IF=0 (02 #11, 01 seção 3.3): medido 1–4 µs, os picos são ruído do TCG.
- `options(nomem)` em `in/out` e `cld` no ISR (01 #10): cosmético, sem efeito comprovado.
- "Race do `RacyCell` sob preempção" (01): o 01 verificou cada estático contra as 3 threads e **não há** acesso concorrente hoje; só é frágil por convenção.
- Margem CSS negativa escrevendo fora do framebuffer (03 #6): `fill_rect`/`draw_char` checam limites; o efeito é desenho lixo, não escrita fora do buffer.
- Overflow de `align_up`/`adjust_request` (01 #8): inalcançável via `Layout` (limitado a `isize::MAX`).
- virtio sem validação (01 #9, 00): latente, o virtio-gpu não é instanciado no runner padrão.

---

## H. Desempenho (resumo do `04-desempenho`)

Método: QEMU/TCG sem KVM, mediana de 3 execuções, comparação por proporção e `compare` de screenshots. **Não representa hardware real.** Entregue pelo agente D e integrado na branch; cada otimização foi aceita só com screenshot idêntico (AE = 0 depois de mascarar HUD e relógio, reconferido por mim no HEAD, BIOS e UEFI).

**Onde o tempo é gasto (medido):**

| # | Onde | Número | Estado |
|---|---|---|---|
| 1 | Boot até o desktop | ≈ 8,3 s: firmware + bootloader ≈ 3,7 s, splash artificial 4,1–5,2 s, **init real do kernel ≈ 0,18 s** (demo WASM 38 ms, calibração do TSC 104 ms por espera ocupada, `Desktop::new` ≈ 105 ms) | splash não alterado (muda comportamento visível) |
| 2 | Scheduler | `fetcher` e `wasmapp` dão `hlt` dentro da fatia; o compositor recebe 1 de cada 3 fatias de 4 ms. Quadro de tecla: 13,4 ms de parede para 5,2 ms de CPU. Sem as duas threads: 5,06 ms, latência IRQ→captura 9,25 → 0,014 ms, 238 vs 48 quadros de animação em 16 s. O HUD mostra tempo de parede e superestima o custo ≈ 2,6x | **corrigido** (`ca996c1`, `79d23dc`): compositor idle 83 → 250 it/s; tecla→captura ~11 → ~0,4 ms; com o snake aberto o app WASM cai de 30% para 0,4% da CPU (pacing de 16 ms) e o compositor sobe de 16% para 49% |
| 3 | Sombras de janela | ≈ 97% do quadro de recomposição (≈ 346 mil pixels por janela a 24 ciclos/px) | **corrigido**: tabela de blend (24 → 10 ciclos/px, `b55c6ac`) e sem sombra sob o corpo opaco (`81ab639`) |
| 4 | Tick do relógio (1x/s) recompunha a cena inteira | 15,6 ms (BIOS) / 15,3 ms (UEFI) → **0,21 / 0,22 ms** | **corrigido** `77c8b8c` |
| 5 | Tecla no terminal | 26,2 → 13,4 ms (BIOS), 26,5 → 16,1 ms (UEFI); CPU ocupada digitando 5,3% → 2,5% | **corrigido** (efeito dos itens 3–4) |
| 6 | Preenchimento 24 bpp (BIOS entrega 3 bytes/pixel) | 14 → 3 ciclos/px; UEFI já estava em 1 | **corrigido** `148f6d9` |
| 7 | Texto 8x8, sem cache de glifos | ≈ 1,9 mil ciclos/caractere, ≈ 1/3 do quadro de digitação já otimizado | aberto |
| 8 | Upload à VRAM | ≈ 1 MB por tecla (janela focada vai 2x) para ≈ 0,3 KB que mudam; framebuffer WB, sem PAT/WC; o HUD lê e escreve VRAM 10x/s. Custo em hardware real é **suposição** | aberto (WC/PAT não medido) |
| 9 | ATA PIO | flush reescreve 99 setores e trava o compositor: 45 ms no TCG (1 amostra) | aberto |
| 10 | Allocator | **0 alocações por quadro** em todos os cenários medidos, então não é gargalo do desktop; O(n): com 2 000 buracos, 4 KiB custa 44 mil ciclos contra 114 com a heap limpa. `web::render` faz ≈ 0,7 alocações por byte de HTML | aberto, baixa prioridade |

**Não medido** (hipótese e como medir no próprio 04): variações de profile (`panic=abort`, `strip`, LTO thin), benchmarks `criterion` no host, navegador com página real, WC/PAT, custo de VRAM em hardware real.

**Achados incidentais (BAIXA):** a faixa de sombra do HUD escurece a cada refresh; o menu de contexto deixa "fantasma"; o comentário do splash diz "≥ 5 s" mas ele dura 4–5 s.

**Infraestrutura entregue:** `kernel/src/trace.rs` (marcos de boot na serial, estatísticas opcionais com `--features perf-trace`), `tools/perf/` (harness de cenários, A/B) e `bench/` (criterion, crate fora do workspace). Ver `5acff24` e `6587db6`.

---

## I. Antes e depois (medido nesta sessão)

QEMU/TCG sem KVM: valores de desempenho são proporções, não hardware real. Duas rodadas de correções (a 1ª até `1f8667b`, a 2ª depois).

| Métrica | Antes | Depois |
|---|---|---|
| Build do `master` | **não compilava** (nightly sem data + x86_64 0.15.4 + bootloader 0.11.15) | compila; nightly fixado em `nightly-2026-10-05` |
| Testes em `osjeff_core` | 189 (README dizia 152) | **379** |
| Cobertura de linhas (`cargo llvm-cov`, bruta) | 92,33% (README dizia ~98%) | **96,08%** (bruta, inclui os módulos de teste; só produção não foi remedida, era ~88%) |
| Linhas de lógica testável (core) / kernel | — | ~10,7 mil / ~10,5 mil |
| Blocos `unsafe` sem `// SAFETY:` (clippy) | 100 | **0** (lint `warn` ligado; CI usa `-D warnings`) |
| `static mut` | 0 (mas 23 `RacyCell`, mesmo padrão) | 0 (idem) |
| Crashes de fuzzing encontrados / corrigidos | — | **9 / 9** (8 achados pelo fuzzer, 1 por leitura, todos com teste de regressão) |
| Falhas (panic, #PF/#GP/#DF/#UD, OOM) | `hlt` mudo | **tela de erro + serial** (vetor, RIP, RSP, CR2, thread) |
| Estouro da pilha do compositor | triple fault (reset mudo) | `#DF` reportado na pilha IST, sem reset (BIOS e UEFI) |
| IRQ espúria 7/15 | #NP/#DF, e travava o boot com `virtio-gpu-pci` | ignorada e contada |
| Disco após erro de leitura no boot | formatado e sobrescrito | intocado (hash idêntico, falha injetada) |
| `cargo run -p os -- uefi` | panic no bootloader (128M) | boota (256M) |
| `lint-kernel` / `lint-host` / `fmt` | **falhavam** | passam, bloqueiam no CI |
| `cargo audit` | 4 avisos | 1 (`bincode`, sem correção; só build) |
| Parsers vs. entrada hostil | 3 travamentos remotos triviais + 2 OJFS | tetos, redirect seguro, fuzz |
| App WASM hostil | CPU presa para sempre, 51% do heap | encerrado por *fuel*, 24 MiB |
| Compositor em idle | 83 iterações/s | **250** |
| Latência tecla → captura | ~11 ms | **~0,4 ms** |
| Tick do relógio (BIOS / UEFI) | 15,6 ms / 15,3 ms | **0,21 ms / 0,22 ms** |
| Quadro de tecla no terminal (BIOS / UEFI) | 26,2 ms / 26,5 ms | **13,4 ms / 16,1 ms** (+ a melhora do scheduler) |
| Preenchimento de retângulo em 24 bpp | 14 ciclos/px | **3 ciclos/px** |
| Gerenciador de tarefas | "CPU" igual para as 3 threads (artefato) | CPU real (compositor 1187, fetcher 0, wasmapp 0 em idle) |
| Tempo de boot até o desktop (QEMU) | ≈ 8,3 s (splash 4–5 s + firmware ≈ 3,7 s; init do kernel ≈ 0,18 s) | igual (splash e init não foram alterados) |
| Tamanho da imagem (BIOS / UEFI) | 4 686 848 B / 4 259 840 B | igual (o builder alinha a imagem) |
| Aparência do desktop | — | idêntica (0 pixels de diferença fora de HUD e relógio, BIOS e UEFI, em todos os commits de kernel) |
| CI | nenhum | `.github/workflows/ci.yml` (não executou no GitHub nesta sessão; cada comando rodou localmente) |

**Ficou de fora, e por quê:** ver a seção E e o [`ROADMAP`](../ROADMAP.md). Os mais relevantes: thread morta em vez de máquina morta e guard pages (exigem o kernel editar suas page tables), verificação de certificado TLS, DHCP alimentando o `netstack` e driver de NIC comum, *fuel* retomável do WASM, teste em hardware real, e o frame que cruza o anel do NE2000 (K5, não provado em QEMU).
