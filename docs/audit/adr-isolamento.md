> *Written when the project was called OSjeff (renamed Kitsune in 2026-10; names and paths below are the historical ones).*

# ADR: Isolamento e privilégio — paginação + ring 3 ou WebAssembly como sandbox?

| | |
|---|---|
| Status | **Proposto** (decisão a tomar pelo mantenedor) |
| Data | 2026-10-06 |
| Escopo | `kernel/`, `osjeff_core/`, `os/`, `wasm-apps/`, `tools/`; crates do bootloader 0.11.17 e wasmi 1.1.0 em `~/.cargo/registry` |
| Branch | `audit/performance-security` (HEAD `1c14b3d` no início do trabalho) |
| Toolchain | `nightly-2026-10-05` (`rust-toolchain.toml:3`), bootloader 0.11.17, x86_64 0.15.5, wasmi 1.1.0 (`Cargo.lock`) |
| Autor | Agente F (arquitetura: isolamento e privilégio) |

Legenda de evidência usada no documento:

- **[M]** medido: saída de comando ou execução no QEMU 8.2.2 (TCG, sem KVM; `/dev/kvm` não existe neste sandbox).
- **[L]** lido no código (`arquivo:linha`), sem executar.
- **[NV]** não verificado: suposição, extrapolação ou conhecimento externo.

Convenções: nenhum arquivo de `kernel/`, `osjeff_core/` ou `os/` do checkout principal foi alterado. Todas as medições dinâmicas foram feitas num worktree descartável instrumentado (`.../scratchpad/wtF`); o patch de instrumentação está em `.../scratchpad/adr-isolamento-experimentos.patch` (fora do repositório, 212 linhas) e descrito no Apêndice A. As imagens medidas sem instrumentação (registradores, mapa de memória) vieram de uma cópia da imagem já construída em `target/`.

---

## 1. Contexto

O OSjeff é um SO x86_64 bare-metal em Rust. O README descreve um desktop com apps (Terminal, Editor, Gerenciador de tarefas, Calculadora, Arquivos, Navegador). O código vai além: tem um escalonador preemptivo (`kernel/src/sched.rs`), pilha TCP/IP + TLS 1.3 (`netstack.rs`, smoltcp + embedded-tls), um motor HTML/CSS (`osjeff_core/src/web/`) e um runtime WebAssembly (wasmi) com subconjunto de WASI que roda Snake, Plasma, um app C e DOOM (`kernel/src/wasm/{mod,wasi}.rs`, `wasm-apps/`).

A pergunta que este ADR responde: **o que separa hoje um app do kernel e dos outros apps, qual o custo de continuar assim, e qual o próximo passo de arquitetura (modo usuário com paginação, ou o WebAssembly que já existe, ou os dois em sequência)?** Este ADR não implementa modo usuário nem altera código do produto.

## 2. Estado atual, com evidências

### 2.1 Resumo das respostas

| Pergunta | Resposta | Evidência |
|---|---|---|
| Kernel e todos os apps rodam em ring 0, único espaço de endereçamento? | **Sim.** Nenhum app tem privilégio, CR3 ou pilha próprios; os apps nativos são structs dentro de `Desktop`, chamadas pela thread do compositor. O app wasm roda em outra *thread de kernel*, também ring 0. | [M] `CPL=0`, `CS=0008 DPL=0 CS64` em todos os dumps `info registers`; [L] `desktop/mod.rs:157-176`, `main.rs:246,255` |
| GDT própria? | **Não.** Usa a GDT que o bootloader cria: 3 descritores (nulo, código kernel, dados kernel). Sem descritores ring 3. | [M] `GDT= 0000000006d2e000 00000017` (limite 0x17 = 3 entradas); [L] `bootloader-x86_64-common-0.11.17/src/gdt.rs:17-25`; o kernel só *lê* CS/SS (`sched.rs:120-121`) |
| TSS / IST? | **Não existem.** `TR=0000`, nenhum `ltr`, nenhum `set_stack_index`. | [M] `TR =0000 ... TSS64-busy` (estado de reset do QEMU, seletor nulo); [L] `interrupts.rs:41-50`, nenhum `gdt`/`tss`/`ist` no kernel (grep) |
| CR3 | **O do bootloader**, nunca recarregado. O kernel só *lê* CR3 para traduzir endereços. | [M] `CR3=00000000011c8000` (BIOS), `0000000000101000` (UEFI); [L] único uso: `virtio.rs:17-25` (`Cr3::read`) |
| Memória física inteira mapeada? | **Sim, RW+NX, supervisor, 1 TiB de espaço virtual** em `0x28000000000`. Qualquer código do kernel lê/escreve toda a RAM e todo MMIO. | [L] `main.rs:50-53` (`physical_memory = Some(Mapping::Dynamic)`); [M] tabela 2.2 e teste F12 (leitura de `phys[0]` pelo mapa) |
| W^X / NX? | **Sim, no que o bootloader mapeia.** NXE ligado, WP ligado. Texto é R-X, rodata R--, dados/heap/pilha RW-NX. Escrita em `.text` e execução no heap dão #PF (testado). | [M] `EFER=0xd00` (NXE=bit 11), `CR0=0x80010011` (WP=bit 16); [M] tabela 2.2; [M] testes F10/F11 (2.4); [L] `bootloader-x86_64-common-0.11.17/src/lib.rs:195-197`, `load_kernel.rs:176-181` |
| SMEP/SMAP | **Desligados** (sem efeito hoje, não há páginas de usuário). | [M] `CR4=0x20` (BIOS), `0x668` (UEFI): sem bits 20/21 |
| `syscall`/`sysret` | **Desabilitado**: `EFER.SCE=0`, STAR/LSTAR nunca escritos. | [M] `EFER=0xd00` (SCE=bit 0 = 0); [L] nenhum `Msr`/`Star` no kernel |
| "Processos" (`osjeff_core/src/process.rs`) são só entradas de tabela? | **Sim.** `ProcessTable` é um array de 8 structs (pid, nome, estado, ticks). Não há ligação com threads, endereços ou recursos. `kill` remove a linha. | [L] `process.rs:44-49,133-148`; [M] screenshot do Task Manager (2.4) |
| Existe "processo travou" vs "sistema travou"? | **Não.** Só existe "o sistema parou" (silencioso) ou "a thread parou e virou zumbi". Nenhuma mensagem em serial ou tela. | [M] testes F1-F5 (tabela 2.4); [L] `main.rs:777-780`, `interrupts.rs:147-161` |
| Panic de um app | **`halt()`**: loop de `hlt`, sem imprimir nada. Em thread com IF=1 deixa as outras threads vivas; no compositor congela a UI; em contexto de interrupção (IF=0) mata a máquina. | [L] `main.rs:777-780`; [M] F3, F4, F5 |
| Limites do guest wasm (fuel/memória/tempo)? | **Nenhum.** Fuel desligado (padrão do wasmi), sem `ResourceLimiter`, sem timeout. Loop infinito do guest prende a thread para sempre. | [L] `wasm/mod.rs:249,277` (`Engine::default()`), wasmi `config.rs:48` (`consume_fuel: false`); [M] teste do guest em loop (2.5) |

### 2.2 Registradores e mapa de memória (medidos)

Imagem BIOS de `target/` (HEAD), QEMU `-m 128M`, parada no `hlt` ocioso do compositor.

```
CPL=0   CS=0008 DPL=0 CS64   SS/DS/ES=0010 DPL=0
GDT=0000000006d2e000 limite 00000017      IDT=000001000016c680 limite 00000fff (dentro do .data do kernel)
TR=0000  (sem TSS)                         CR0=80010011  CR3=00000000011c8000  CR4=00000020
EFER=0000000000000d00   (LME, LMA, NXE; SCE=0)
```

UEFI (mesma imagem, OVMF): `CR0=80010033`, `CR3=0x101000`, `CR4=0x668` (firmware liga OSFXSR/MCE/DE), `GDT=0x6be0000 limite 0x17`, `TR=0`, `EFER=0xd00`. Mesmo quadro: GDT de 3 entradas, sem TSS, NXE ligado, sem SMEP/SMAP. [M]

Mapa virtual ativo, obtido por `pmemsave` da RAM + caminhador de tabelas de página escrito para a auditoria (`walk.py`, no scratchpad; NX = bit 63 da PTE). "s" = supervisor; **nenhuma página tem U=1**. [M]

| Região virtual | Tamanho | Perm. | O que é |
|---|---|---|---|
| `0x13a000-0x13c000` (identidade) | 8 KiB | R-X s | `context_switch` do bootloader, deixado mapeado (`lib.rs:237-260`) |
| `0x6d2e000` (identidade) | 4 KiB | R-X s | página da GDT do bootloader |
| `0x10000000000-0x10000024000` | 144 KiB | R-- | ELF LOAD 1 (`.rodata`) |
| `0x10000024000-0x1000016a000` | 1304 KiB | **R-X** | `.text` |
| `0x1000016a000-0x1000016c000` | 8 KiB | R-- | RELRO (o bootloader remove W, `load_kernel.rs:~690-715`) |
| `0x1000016c000-0x10005c85000` | ~90,7 MiB | **RW- (NX)** | `.data` + `.bss`: heap de 64 MiB (`main.rs:79-80`), 3 buffers de 8 MiB (`BACK/BG/STATIC`), IDT |
| `0x18000001000-0x18000015000` | 80 KiB | RW- NX | pilha de boot do compositor; **página de guarda** em `0x18000000000` (não mapeada) |
| `0x20000000000` | 2700 KiB | RW- NX | framebuffer |
| `0x28000000000-0x38000000000` | **1 TiB** | RW- NX s | **toda a memória física** (RAM, e provavelmente as regiões reservadas/MMIO do mapa BIOS [NV: motivo do 1 TiB não investigado]) |
| `0x38000000000` | 4 KiB | RW- | `BootInfo` |

Pontos que seguem da tabela:

1. **W^X vale para a imagem do kernel** porque o bootloader mapeia cada segmento ELF com NX se não-executável e W só se gravável (`load_kernel.rs:176-181`). O ELF do kernel tem 4 LOADs `R`, `R E`, `RW`, `RW` e nenhum RWX (`readelf -lW`). O heap de 64 MiB é `.bss`, logo **não é executável** [M, teste F11].
2. **O kernel nunca toca nas tabelas de página**, só traduz endereços (`virtio.rs:17-25`). Não há alocador de frames físicos: `memory_regions` não é usado em lugar nenhum (grep) [L].
3. **Pilhas das threads secundárias** (`fetcher`, `wasmapp`) são `vec![0u8; 128 KiB]` no heap (`sched.rs:19,111`): **sem página de guarda**; só há um canário de 8 bytes verificado a cada troca de contexto (`sched.rs:28,161`). A pilha de boot (compositor) tem 80 KiB (padrão do `bootloader_api`, `config.rs:54`) e guarda real, mas sem IST o overflow vira triple fault (F1).
4. A 1 TiB de mapeamento RW+NX inclui as próprias tabelas de página e a GDT/IDT: **um ponteiro errado no kernel pode reescrever as tabelas de página** [L, consequência direta do mapa; não explorado].

### 2.3 Onde cada app roda

| App | Lógica | Onde executa | Evidência |
|---|---|---|---|
| Terminal, Editor, Calculadora, Navegador (estado) | `osjeff_core` (`#![forbid(unsafe_code)]`, `lib.rs:8`): structs `Terminal`, `Editor`, `Calc`, `Browser` | campos de `Desktop`, chamados pela **thread do compositor** (thread 0, pilha de boot) | `desktop/mod.rs:160-163,287-290`; `main.rs` (loop principal) |
| Navegador (renderização) | `osjeff_core::web::render` (parser HTML recursivo + layout) | thread do compositor | `desktop/mod.rs:563-569` |
| Navegador (rede, TLS) | `netstack.rs`, `fetch.rs` (smoltcp, embedded-tls) | thread `fetcher` (128 KiB de pilha no heap) | `main.rs:246`, `fetch.rs`, `netstack.rs:223-247` |
| Gerenciador de arquivos, Gerenciador de tarefas | `kernel/src/desktop/{files,files_ui,apps}.rs` | thread do compositor | `apps.rs:557-610` |
| App WASM (Snake/Plasma/C/DOOM) | guest em wasmi, `kernel/src/wasm/` | thread `wasmapp` (sempre spawnada), ring 0 | `main.rs:253-256`, `wasm/mod.rs:446-481` |

Consequência: **o que hoje separa "app" de "kernel" é só a convenção de código e o `forbid(unsafe_code)` de `osjeff_core`**, não hardware. O `kernel/` tem ~119 linhas com `unsafe` [M, `grep`]; `osjeff_core` tem zero blocos [L]. O wasmi tem 135 ocorrências de `unsafe` nas crates `wasmi`, `wasmi_core`, `wasmi_ir`, `wasmi_collections` (maiores: `executor/cache.rs` 26, `stack/values.rs` 15, `instrs.rs` 11, `call.rs` 10) [M, `grep -rn unsafe` no registry].

Nota honesta sobre o modelo de falha. O código de apps é quase todo Rust seguro; um bug no parser HTML **não corrompe o heap** (isso exigiria `unsafe` ou bug do compilador). O que ele pode fazer é: panic por índice, **estourar a pilha** (provado abaixo), esgotar o heap, ou entrar em loop. Corrupção de memória precisa de bug no `unsafe` do kernel (`allocator.rs` 17 ocorrências, `virtio*.rs` 45), em dependências com `unsafe` (wasmi, smoltcp, embedded-tls) ou em overflow de pilha de thread (que *escreve fora* do buffer, F2).

DMA: o NE2000 é ISA com "remote DMA" via porta de E/S byte a byte, não bus-master (`ne2000.rs:1-9`), e o ATA é PIO (`ata.rs:4`), então nenhum dos dois acessa a RAM por conta própria [L]. O virtio-gpu é bus-master de verdade (`pci.rs:59`, `virtio_gpu.rs:45-47,80-93`), mas **não é instanciado no runner padrão** (`os/src/main.rs` não passa `-device virtio-gpu`; serial: `virtio-gpu: absent — using the VBE framebuffer`) [M]. Logo o risco de DMA é **latente**: sem IOMMU, um byte errado em `QUEUE_MEM`/`CMD_MEM` (estáticos no `.bss`) redirecionaria a escrita do dispositivo para qualquer endereço físico. Só se materializa se o virtio-gpu for ligado.

A superfície de entrada real é a **rede**: TLS usa `UnsecureProvider` (sem verificação de certificado, `netstack.rs:223,247`) e aceita respostas de até 256 KiB (`netstack.rs:278`). Qualquer atacante no caminho entrega o HTML que o motor vai processar.

### 2.4 Experimentos de falha (medidos no QEMU, worktree instrumentado)

Cada linha é uma injeção de falha numa boot limpa (tecla F-n lida no loop principal, patch no Apêndice A). `-d cpu_reset,int` do QEMU registra as exceções. "Tela" = dois screenshots com 4 s de intervalo comparados pixel a pixel (o relógio da barra muda a cada segundo se a UI está viva).

| # | Falha injetada | O que aconteceu | Evidência |
|---|---|---|---|
| F1 | Recursão infinita na thread do compositor (pilha de boot, 80 KiB) | #PF na guarda (`CR2=0x18000000fb8`) → **#DF** → **triple fault** (reset silencioso em HW real; o QEMU parou). Não há IST, o handler de #DF nunca roda. | log QEMU: `v=0e e=0002 ... CR2=0000018000000fb8`, `v=08`, `Triple fault` |
| F12 | `osjeff_core::web::render` com `<div>` aninhados, na thread do compositor (o mesmo caminho de `browser_load`, `desktop/mod.rs:566`) | profundidade 50 e 100 passam; **200 `<div>` (1 001 bytes de HTML) → triple fault** (mesmo #PF→#DF→triple da F1). | serial: `depth 100 ok`, `render nested depth 200`, sem retorno; log: `Triple fault`, `CR2=0000018000000fd8` |
| F2 | Recursão infinita numa thread nova (pilha de 128 KiB no heap, sem guarda) | O overflow **escreve sobre memória vizinha do heap** sem fault; a máquina morreu em seguida: primeiro #PF *de escrita* com `CR2=0` e `SP=0x1000040e040` (dentro do `.bss`), depois `hlt` com IF=0. Nenhuma mensagem. Mecanismo exato da morte = interpretação: ponteiro/estrutura vizinha corrompida. | log: `v=0e e=0002 ... SP=...40e040 CR2=0`; `info registers`: `RFL=0x93`, `HLT=1` |
| F3 | `panic!` numa thread de worker | **Sistema segue vivo** (relógio avança: 520 px diferem entre screenshots), HUD passa a `thr 4`: a thread panicada fica em loop de `hlt` e continua gastando fatias do round-robin ("zumbi"). Nenhuma mensagem. | screenshots `a/b`; `serial.log` sem linha de panic |
| F4 | `panic!` na thread do compositor | **UI congela para sempre** (0 px de diferença), sem mensagem na tela nem na serial; threads fetcher/wasmapp seguem rodando. `RFL` com IF=1. | screenshots idênticos; `HLT=1` |
| F5 | Corromper o canário da pilha de uma thread (simula overflow que o alcançou) | panic **dentro da ISR do timer** (`sched.rs:161`) → `hlt` com **IF=0** → máquina inteira morta, UI congelada, sem mensagem. A detecção "por canário" converte falha de uma thread em morte total. | `RFL=0x17` (IF=0), 0 px de diferença |
| F-heap | Esgotar o heap do kernel (63 MiB reservados) e fazer uma alocação normal de 4 MiB | `handle_alloc_error` → panic → **UI congelada** (0 px), sem mensagem. (O "heap full after 63 MiB" apareceu na serial; "survived" não.) | serial; screenshots idênticos |
| F10 | Escrever num endereço de `.text` | **#PF** (`e=0003`, CR2 = endereço do texto) → handler faz `hlt` com IF=0: bloqueado pelo hardware (WP), mas a resposta é a morte total. | log: `v=0e e=0003 ... CR2=0000010000064bd8`; `RFL=0x46` |
| F11 | Executar código num `Vec<u8>` do heap | **#PF** com `e=0011` (bit de busca de instrução): NX efetivo no heap. Mesma resposta: morte total silenciosa. | log: `v=0e e=0011 ... CR2=0000010001185db0` |
| F-phys | Ler a memória física 0 e 0x7136000 pelo mapa de 1 TiB | Leitura bem-sucedida (`phys[0]=0xf000ff53f000ff53`, a IVT do BIOS). Qualquer código do kernel pode. | serial: `exp F12: phys[0]=...` (numa versão anterior do patch esta tecla era F12; depois virou o teste de HTML aninhado) |

Conclusões das medições:

- **Os mecanismos de hardware que existem funcionam** (WP, NX, guard page da pilha de boot): eles *detectam*. O que falta é a **resposta**: todas as detecções terminam em `hlt` silencioso, e a pilha de boot não tem IST para chegar ao handler.
- **Uma página HTML de ~1 KiB (200 `<div>` aninhados, ou ~200 tags não fechadas) pode reiniciar a máquina.** Foi provado ponta a ponta no kernel bare-metal chamando `web::render` na thread do compositor; **não** foi provada a entrega pela rede (a auditoria `02-interrupcoes-scheduler.md` também não conseguiu servir a página no QEMU user-net). O parser recursivo (`dom.rs:89-170`) e o layout (`layout.rs:89-165`) não têm limite de profundidade (grep: nenhum `depth`/`MAX`).
- Três processos "do usuário" (compositor, fetcher, wasmapp) **compartilham o destino**: não existe "este app caiu".

O Task Manager mostra isso visualmente [M, screenshot do QEMU]: duas tabelas. A de cima ("PID NAME ST UP") são as entradas de `ProcessTable` (kernel, compositor, shell, wasmapp, taskmgr), cujo "UP" é um contador de segundos. A de baixo ("KERNEL THREADS CPU") mostra `compositor 9685`, `fetcher 9658`, `wasmapp 9658`: **a coluna "CPU" é a contagem de fatias entregues**, idêntica para threads ociosas e ocupadas (`sched.rs:177` credita o tick à thread que estava rodando, mesmo em `hlt`). `DEL: end` só fecha a janela (`input.rs:236-255` → `request_close` → `procs.kill`, `desktop/mod.rs:406`); **não encerra nenhuma thread nem libera memória**. O comentário de `process.rs:1-6` ("OSjeff has no preemptive scheduler yet") está desatualizado.

### 2.5 A fronteira guest/kernel no WebAssembly

**O que o guest alcança.** Os imports são 5 funções próprias + 25 WASI + `env.system` = **31 funções** (`wasm/mod.rs:176-237`, `wasi.rs:263-328`). O guest não vê ponteiros do kernel: a memória linear do guest é um `Vec<u8>` do wasmi no heap do kernel, e o acesso do host passa por `guest_bytes` (`get(ptr..ptr+len)`, com checagem de limites, `mod.rs:542-547`) ou `Memory::read/write` (`wasi.rs:30-35`). Os desenhos vão para o **buffer offscreen `SURFACE`**, não para o framebuffer real (o worker aponta `st.fb` para `SURFACE[back]`, `mod.rs:458-466`); o compositor copia o quadro pronto (`blit_surface`, `mod.rs:486-527`). Todo primitivo é recortado à caixa do conteúdo (`host_fill`, `host_text`, `host_blit`, `mod.rs:94-174`; `Canvas::fill_rect` recusa `x0>=width` e usa fatias com limite, `fb.rs:148-166`). Revisão do código: **não achei como o guest alcança memória do kernel por essas funções** [L; revisão, sem fuzzing].

| Host function | Memória do guest tocada | Do kernel | Observações |
|---|---|---|---|
| `host.log` (`mod.rs:181-189`) | lê `[ptr,len)` e valida UTF-8 | escreve em COM1 | **sem limite de taxa**: o guest "grow" gerou ~79 000 linhas em 8 s [M] |
| `fill_rect`, `draw_text`, `blit` (`mod.rs:190-225`) | `blit` lê `w*h*4` bytes | desenha em `SURFACE` | recortados à caixa; `blit` faz `fill_rect` por pixel |
| `time_ms` (`mod.rs:228-232`) | — | lê `TICKS` | — |
| WASI `fd_write`/`fd_read` (`wasi.rs:69-113`) | iovecs e buffers | `fd_write` → serial; `fd_read` lê o WAD (`.rodata`) | **loops `for i in 0..n` com `n` do guest, sem limite** (`wasi.rs:72,98`) |
| `random_get` (`wasi.rs:223-239`) | escreve `len` bytes | — | **loop de `len` iterações** (até 2³¹); PRNG xorshift **não criptográfico**, semente = ticks |
| `path_open`/`path_filestat_get` (`wasi.rs:188-212`) | lê caminho (≤259 B) | só reconhece `doom1.wad` | ignora `dirfd`; ops mutantes fingem sucesso (`wasi.rs:313-316`) |
| `proc_exit` (`wasi.rs:318-320`) | — | só loga e **retorna** | pela especificação WASI não retorna; o guest continua |
| `env.system` (`wasi.rs:324-326`) | — | devolve -1 | ok |

**Limites hoje** [L]: sem fuel (`consume_fuel: false` é o padrão, wasmi `config.rs:48`; `Engine::default()` em `mod.rs:249,277`); sem `ResourceLimiter` (grep `limiter` no kernel: nenhum); pilha do wasmi: recursão máx. 1000, pilha de valores máx. 1 000 000 células (`wasmi limits/stack.rs:7,13`); memória do guest sai do **heap compartilhado de 64 MiB**; a `Store` fica residente para sempre em `static APP` (`mod.rs:273`) e **fechar a janela não a libera**; o `worker` só consulta `ACTIVE` *entre* chamadas (`mod.rs:448`).

**Medições com guests hostis** (WAT montado no `build.rs` do worktree; abrir o app pela dock):

| Guest | Resultado | Evidência [M] |
|---|---|---|
| Loop infinito em `render` | O worker nunca volta; **continua girando depois de fechar a janela** (8 amostras de `info registers` com a janela fechada: 3 com `HLT=0` e RIP em `wasmi::engine::executor::instrs::execute_instrs`; linha de base sem guest: 100% `HLT=1`). A UI segue viva porque o timer preempta, mas a CPU nunca mais fica ociosa e o app **não tem como ser morto**. | amostras `c1..c8`; `nm` do RIP |
| Acesso fora dos limites | **Contido**: `Trap(MemoryOutOfBounds)`, registrado uma vez, kernel segue. Este é o caso em que o sandbox funciona. | serial: `wasm app: render trap: Error { kind: TrapCode(MemoryOutOfBounds) }` |
| `memory.grow` em laço | O guest fica com **513 páginas = 32 MiB, 51% do heap do kernel** (o `Vec::try_reserve` do wasmi falha ao dobrar para 64 MiB; sem pânico). Fica retido até reiniciar. Com isso o resto do sistema tem ~49% e F-heap fica mais próximo. | serial `grow-done pages=513`; HUD `heap 51%` |
| `panic` do próprio guest | Snake e Plasma definem `#[panic_handler] fn panic(...) -> ! { loop {} }` (`plasma/src/lib.rs:13-15`, `snake/src/lib.rs:14-16`): **um bug do guest vira loop infinito**, que sem fuel prende a thread. | [L] |

**Mitigações validadas** (mesmo worktree: `Config::consume_fuel(true)`, `set_fuel(20_000_000)` por quadro, `StoreLimitsBuilder::memory_size(24 MiB)`):

| Teste | Antes | Depois |
|---|---|---|
| Guest em loop infinito | gira para sempre | **`Trap(OutOfFuel)`** a cada 20 M de fuel (serial: `wasm app: render trap ... OutOfFuel`); com a janela fechada, **8/8 amostras em `HLT=1`** (CPU ociosa) |
| `memory.grow` | 513 páginas (32 MiB) | **321 páginas (20 MiB)** = limite de 24 MiB respeitado em passos de 64 páginas |

Ressalva: o fuel mede instruções wasm, **não o trabalho do host**. Um `random_get(len=0x7fffffff)` ou `fd_write(n=0x7fffffff)` roda 2³¹ iterações no host sem consumir fuel [L, não executado]; os limites de `n`/`len` precisam ser aplicados nas próprias host functions.

### 2.6 Estado de `wasm-apps/` (funcional, abandonado ou em andamento?)

Veredito: **funcional e dormente.** Protótipo ponta a ponta que roda, sem commits de wasm desde 2026-06-22.

| Item | Estado | Evidência |
|---|---|---|
| Histórico | 15 commits que citam wasm/doom: 7 em 2026-06-21 e 7 em 2026-06-22 (+1 de auditoria em 2026-10-06). Último de código: `33b5908` (app em thread própria). Desde então só FS/gerenciador (06-26) e build/harness (10-06). | `git log -i --grep='wasm\|doom'` [M] |
| O que vai na imagem | **Um único app, escolhido em tempo de build** (`kernel/build.rs:44-54`): padrão = `snake` (`build.rs:50-53`); `WASI_SDK_PATH` sem `DOOM` = `cdemo`; `DOOM=1` + `WASI_SDK_PATH` = DOOM. Não há loader nem lista de apps. | [L] |
| Snake | Padrão, compila e roda (screenshot: título "snake.wasm - jogo nativo em Rust", jogo desenhado, "GAME OVER" sem entrada). | [M] |
| Plasma | **Órfão**: nenhum caminho do `build.rs` o seleciona (`"snake"` é literal em `:51`). Compila sozinho para `wasm32` (usado nos benchmarks). | [L], [M] |
| cdemo | Opcional (precisa de `WASI_SDK_PATH`); não construído nesta auditoria. | [L] |
| DOOM | **Roda.** Construído com `wasi-sdk-25` + `DOOM=1` e **Freedoom 0.13 (`freedoom1.wad`, 28 MiB) renomeado `doom1.wad`** como substituto, porque o `doom1.wad` shareware não estava acessível pelo proxy do sandbox. Screenshot: tela de jogo com HUD de DOOM dentro da janela, ~22% de heap. Para o checkout puro, **precisa do WAD externo** (`wasm-apps/doom/doom1.wad`, `build.rs:166-172`; `.gitignore` exclui `*.wad`), de `WASI_SDK_PATH` e de rede (o `build-doom.sh:25-26` clona `ozkl/doomgeneric`, GPLv2). | [M], [L] |
| Documentação | `00-mapa.md` §7 marcou DOOM como "não verificado"; esta medição o verifica (com a ressalva do WAD substituto). | — |

Custo de execução, medido de duas formas independentes do QEMU/TCG:

| App | Medida | Valor |
|---|---|---|
| Plasma (compute puro, 57 600 px/quadro) | wasmi 1.1.0 no host (Xeon 2,1 GHz, std, `-O3`+LTO) | **2,8–2,9 ms/quadro**; nativo `-O3` do mesmo laço: **0,076–0,113 ms**: **≈25–37× mais lento** (laço auto-vetorizável: é o pior caso; código com ramificações tende a ficar mais perto [NV]) |
| Fuel/overhead | mesma medição com `consume_fuel` | 2,95–3,23 ms: **+3% a +15%** (ruído alto, VM compartilhada) |
| Vazão do wasmi | host | **430–470 Mfuel/s** (1 fuel ≈ 1 instrução wasm) |
| DOOM (Freedoom) | fuel por `render` no kernel (determinístico, independe do host) | init (`doomgeneric_Create`): **38,6 M**; quadro estável: **4,5 M** (tela-título) a **~8–9 M** (demo em jogo); um quadro estourou 20 M (carga de nível) → `OutOfFuel` |
| DOOM, extrapolação | 35 ticks/s × 4,5–9 M | **160–320 Mfuel/s ≈ 35–70% de um núcleo** do host acima [NV: não medido em bare-metal nem em KVM] |
| TCG (este sandbox) | fuel/s do wasmi bare-metal no QEMU | 15–20 Mfuel/s (~25× mais lento que o host): DOOM a 2–4 quadros/s. **Não representativo de hardware real.** |

Observação: a build bare-metal do wasmi (`x86_64-unknown-none`, sem SSE) pode ser mais lenta que o benchmark do host [NV].

## 3. Opções

### Opção A: paginação + ring 3 (processos nativos)

Espaços de endereçamento por processo, GDT com segmentos ring 3, TSS, `syscall`/`sysret`, ELF, validação de ponteiros.

### Opção B: WebAssembly como formato de app, sandbox por software

Todo app não confiável (e, no limite, os apps do desktop) vira módulo wasm no wasmi. O kernel segue em ring 0. Endurecer o runtime: fuel, limites de memória, kill/restart, lista de apps, host calls auditadas.

### Opção C: B agora, A depois, com um passo comum de endurecimento do kernel antes

Primeiro corrigir o que independe da escolha (pilhas, IST, observabilidade de falhas), depois B, e só então decidir A com dados.

### Comparação honesta

| Critério | A: ring 3 + paginação | B: wasm (wasmi) |
|---|---|---|
| **Garantia de isolamento** | **Hardware** (MMU + CPL). Vale contra código de máquina arbitrário, inclusive C/asm malicioso. A TCB é o kernel inteiro (VM, syscalls, validação de ponteiros), tudo em ring 0. | **Software**. A TCB é **wasmi (~135 `unsafe`, concentrados no executor: cache de instância e pilha de valores) + as 31 host functions + o kernel inteiro em ring 0**. Um bug no wasmi ou numa host function é **escape total**: o guest passa a ser código ring 0 com acesso ao mapa de 1 TiB, às tabelas de página e às portas de E/S. Em compensação a superfície do guest é pequena por construção (sem ponteiros, saltos só dentro do módulo validado, 31 pontos de entrada contra uma ABI de syscalls ainda por desenhar). Histórico de vulnerabilidades do wasmi: [NV, consultar avisos]. |
| **Contenção de bugs do próprio kernel/compositor** | Não protege o que continua dentro do kernel. | Idem, **salvo** se esse código passar a rodar como guest. |
| **Desempenho** | ~nativo (+custo de syscall e troca de CR3; sem PCID hoje). | **25–37× mais lento** num laço de pixels (medido); DOOM exige ~35–70% de um núcleo do host (extrapolação). JIT não é viável aqui (no_std, W^X, TCB maior) [NV]. |
| **Esforço (1 dev)** | MVP (1 processo ring 3 que faz syscalls): **~25–35 dias**; portar todos os apps: **+30–45** (ver §5). | Endurecimento sólido (§6, passos 5-6): **~8–15 dias** sobre o que existe. Portar o motor web para wasm: +5–8. |
| **Ergonomia (linguagens)** | Precisa de ABI de syscalls própria, libc/porte próprio, linker script, loader ELF. Hoje: **zero** infraestrutura. | **Já funciona**: Rust → `wasm32-unknown-unknown` (snake, plasma) e C → clang/wasi-sdk (cdemo, DOOM, que traz libc grátis via wasi-libc). Qualquer linguagem com alvo wasm. |
| **Loops infinitos** | O timer preempta o processo por hardware; matar é fácil. | **Fuel** (validado: `OutOfFuel`, CPU volta a ficar ociosa). Não cobre trabalho dentro de host functions (limites a impor). |
| **Memória limitada** | Contabilidade de frames por processo (a construir). | `ResourceLimiter` pronto no wasmi (validado: 32 → 20 MiB). Memória do guest segue no heap compartilhado; limite explícito necessário. |
| **Superfície de chamadas** | A desenhar (mínimo realista: 10-15 syscalls). | 31 funções existentes, auditáveis; `host.*` já recortam ao quadro do app. |
| **Portabilidade/ecossistema** | Apps só nativos x86_64. | Binário único portável; o mesmo `.wasm` roda no host/CI. |
| **Falha de um app** | Page fault de usuário → matar o processo, sistema segue. | Trap → matar o guest, sistema segue (testado com `MemoryOutOfBounds`). |

Pontos a registrar com franqueza:

1. **Wasm não é "isolamento grátis".** Substitui a MMU por correção do wasmi + das host functions, e deixa a Opção B ao mesmo nível de risco do kernel atual **para um guest que explore um bug**. O que a Opção B compra é: (a) contenção de falhas *acidentais* e de loops/memória, (b) uma superfície de ataque minúscula e auditável, (c) tudo isso com um custo de dias, não de semanas.
2. **Ring 3 também não é grátis.** Exige código novo `unsafe` em caminhos críticos (troca de CR3, validação de ponteiros, `sysret`), sem testes nem fuzzing hoje. Para um desenvolvedor só, o risco de introduzir uma vulnerabilidade na própria camada de isolamento é real.
3. **Nenhuma das duas opções corrige as falhas medidas na §2.4** (overflow de pilha, panic silencioso, heap esgotado) quando o código que falha é o *do kernel*. Por isso o passo comum (S0-S4) vem antes.
4. Uma oportunidade específica deste código: `osjeff_core` é `no_std` + `alloc`, sem `unsafe` e sem hardware (`lib.rs:1-8`). Em princípio **compila para `wasm32`** [NV: não tentei]. O motor web (que derrubou o sistema com 1 KiB de HTML, F12) poderia rodar como guest: o mesmo HTML passaria a produzir `Trap`/`OutOfFuel` em vez de triple fault. Custo: a lentidão do wasmi num parser/layout CPU-bound, e uma ABI de "janela de app" (hoje só há o `blit` de pixels e eventos de teclado/ponteiro).

## 4. Decisão proposta

**Recomendo a Opção C: endurecer o kernel (S0-S4), depois tornar o WebAssembly o formato de app e a fronteira de isolamento (S5-S6), e adiar o ring 3 (S7) até haver um gatilho concreto.**

Por quê, em ordem de peso:

1. **Já existe e funciona.** O runtime, o subconjunto WASI, a ABI de desenho, o worker com buffer duplo, o pipeline Rust→wasm e C→wasm, e o DOOM rodando são o ativo mais caro do projeto neste tema. O ring 3 começaria do zero (não há alocador de frames, nem gerenciador de página, nem GDT própria, nem syscalls).
2. **As duas mitigações que mais importam já foram validadas em protótipo** (fuel por quadro e `StoreLimits`), com diferenças medidas, e custam algumas linhas (`Config::consume_fuel`, `Store::set_fuel`, `Store::limiter`).
3. **O risco dominante hoje não é malícia de apps, é falha acidental e conteúdo remoto** (F1, F12, F4, F5, F-heap): o que derruba o sistema é pilha de 80 KiB, ausência de IST, panic mudo e heap compartilhado. Ring 3 não corrige isso no código que continuaria no kernel; os passos S0-S4 corrigem, por uma fração do custo.
4. **O ring 3 tem custo alto e retorno incerto para um dev só** (§5: 25-35 dias para o MVP), e só vale quando houver necessidade de código nativo não confiável, de drivers fora do kernel ou de garantia contra um escape do interpretador.

Gatilhos para reabrir a decisão e fazer o S7: (a) querer rodar binários nativos de terceiros; (b) um bug de escape no wasmi/host functions que mostre que a TCB de software é insuficiente; (c) o desktop virar multi-usuário; (d) o compositor/rede passarem a ser alvo de exploração real.

## 5. O que seria necessário para modo usuário (ring 3)

Estimativas em **dias de trabalho de um desenvolvedor só**, para quem já conhece o código; incluem depuração no QEMU, não incluem revisão de segurança externa. Todas são estimativas [NV], não medidas.

| # | Item | Arquivos do OSjeff afetados | Esforço | Notas |
|---|---|---|---|---|
| 1 | **Alocador de frames físicos** a partir de `BootInfo.memory_regions`, com zeragem de página | novo `kernel/src/frames.rs`; `main.rs` (hoje ignora `memory_regions`) | 2 | Cuidado: o bootloader marca como `Bootloader` os frames de GDT, tabelas de página e pilha em uso (`bootloader_api info.rs:164,179`); só reaproveitar depois de trocar GDT/CR3 [NV: confirmar] |
| 2 | **Espaços de endereçamento** (struct `AddressSpace`, map/unmap, cópia das entradas PML4 do kernel) | novo `kernel/src/vm.rs`; `virtio.rs:17-25` (já mostra como obter `OffsetPageTable`) | 3-5 | O kernel já vive em entradas PML4 distintas (`0x1000...` = idx 2, pilha idx 3, fb idx 4, phys-map idx 5, bootinfo idx 7, todas supervisor): o user cabe nas demais **sem realocar nada**; "higher-half" é opcional. Hoje as páginas do kernel são de 4 KiB, o que facilita guard pages. |
| 3 | **GDT própria + TSS** (seletores kernel+user, `TSS.RSP0`, IST1 para #DF, IST2 para #PF/NMI) | novo `kernel/src/gdt.rs`; `main.rs` (ordem de init); **`sched.rs:120-121`** (hoje copia CS/SS da thread atual para o frame inicial) | 1-2 | Precisa de CS/SS de usuário e da ordem exigida pelo `STAR`. Também resolve o triple fault da F1. |
| 4 | **Interface de syscall**: `EFER.SCE`, `STAR/LSTAR/SFMASK`, stub de entrada (troca de pilha, salva registradores), tabela de syscalls, retorno por `sysretq` (ou `iretq`) | novo `kernel/src/syscall.rs` + `syscall.s` | 3-4 | Monoprocessador: pilha de kernel em variável global em vez de `swapgs`. Armadilha do `sysret` com RIP não canônico. |
| 5 | **Validação de ponteiros do usuário** (`copy_from_user`/`copy_to_user`, strings, tamanhos) | novo `kernel/src/uaccess.rs` | 2-3 | Caminho simples: traduzir VA→PA página a página e acessar pelo mapa físico. SMAP só depois. |
| 6 | **Loader**: ELF64 estático (ou formato plano), mapeamento W^X, pilha de usuário, argv; mais uma **crate de userland** (`osjeff_user`: wrappers de syscall, alocador, `_start`, linker script) e integração no build (o `build.rs` já embute artefatos) | novo `kernel/src/loader.rs`; nova crate `user/`; `kernel/build.rs` | 4-6 | Não há libc; ver §3 (ergonomia). |
| 7 | **Scheduler e `switch.s`**: `Process` com CR3; `mov cr3` na troca; atualizar `TSS.RSP0` a cada troca; pilha de kernel por thread; estados `Dead/Zombie`; `exit/kill/wait`; liberar espaço de endereçamento | `switch.s:28-62`, `sched.rs` (`Thread`, `spawn`, `switch_current`), `interrupts.rs:124-129` | 4-6 | O `iretq` de `switch.s` já suporta frame com CS/SS RPL=3. **Sem `RSP0`, uma interrupção em ring 3 empilharia na pilha do usuário** (falha de segurança). O `fxsave` por thread já existe (`sched.rs`). |
| 8 | **Exceções**: handlers de #PF/#GP de usuário matam o processo em vez de `halt`; IDT com DPL correto | `interrupts.rs:147-161` | 2 | Hoje todo #PF é `hlt` (F10/F11). |
| 9 | **Entrada e janelas**: `ps2.rs` e o anel do `interrupts.rs:62-104` alimentam o compositor; o compositor encaminha eventos ao processo focado (IPC) e mapeia uma superfície compartilhada | `desktop/input.rs:60-70` (hoje chamadas diretas a `wasm::on_key` e ao `Terminal`), `desktop/render.rs`, `wasm/mod.rs` (modelo de superfície já existe) | 4-6 | O modelo "app desenha numa superfície offscreen, compositor copia" já está implementado para o wasm (`mod.rs:326-335,486-527`): reaproveitável. |
| 10 | **Alocação de memória do usuário** (`brk`/`mmap`), limites por processo | `frames.rs`, `syscall.rs` | 2-3 | Contabilidade necessária para "memória limitada". |
| 11 | **Endurecimento do kernel** para a isolação valer: SMEP/SMAP, guard pages das pilhas de kernel, `RacyCell` e `static` auditados para reentrada (hoje a premissa é "1 núcleo + IF" e o código roda só com threads de kernel) | `sync.rs`, todos os `RacyCell` (`sched.rs`, `fetch.rs`, `wasm/mod.rs`, `ps2.rs`, `ne2000.rs`) | 3-5 | Usuário e kernel passam a se intercalar de formas novas. |
| 12 | **Observabilidade**: panic/exceção imprimem na serial e na tela; teste de regressão no QEMU | `main.rs:777-780`, `interrupts.rs:147-161`, `serial.rs` | 1-2 | Sem isso, depurar ring 3 é adivinhação. É o S0 do plano. |
| 13 | **Portar os apps**: Terminal, Editor, Calc, Navegador, Arquivos viram processos com UI própria (a lógica de `osjeff_core` já é pura; o desenho está em `kernel/src/desktop/apps.rs` e `files_ui.rs`, ~1 000 linhas acopladas a `Canvas`) | `desktop/*.rs`, `osjeff_core` | **+25-40** | Opcional: dá para manter os apps no kernel e usar ring 3 só para apps novos. |
| | **Total do MVP** (itens 1-8 e 12: um processo ring 3 que escreve na serial, desenha e sai; kill e fault tratados) | | **≈ 22-33** | |
| | **Total com apps portados** | | **≈ 50-75** | |

## 6. Plano incremental com critérios de aceite

Princípio: cada passo é independente, vale por si e não exige reescrita. Todos os critérios são verificáveis no QEMU (serial + `info registers` + log `-d int` + screenshots) e **reaproveitam os testes F1-F12 do Apêndice A**, que passam a ser testes de regressão (uma feature `selftest` no kernel). Esforços em dias, mesma ressalva da §5.

| Passo | O que muda | Arquivos | Esforço | Critério de aceite (QEMU) |
|---|---|---|---|---|
| **S0. Falhas observáveis** | `#[panic_handler]` e handlers de exceção imprimem na serial: mensagem, `arquivo:linha`, nome da thread, vetor, RIP, CR2, código de erro. Tela de panic simples. | `main.rs:777-780`, `interrupts.rs:147-161`, `serial.rs` | 1-2 | F4/F3: o serial termina com `PANIC thread=... at arquivo:linha: msg`. F10/F11: serial com `#PF rip=... cr2=... err=0x3 / 0x11`. Antes: nenhuma linha (medido). |
| **S1. GDT/TSS/IST própria** | GDT com código/dados kernel + TSS; IST1 (pilha estática) para #DF, IST2 para #PF e NMI; `ltr`; `set_stack_index` no IDT. Aumentar a pilha de boot: `c.kernel_stack_size = 512 KiB` em `BOOT_CONFIG`. | novo `gdt.rs`, `main.rs:50-56`, `interrupts.rs:41-50` | 1-2 | `info registers`: `TR` ≠ 0, `GDT` dentro do `.data` do kernel. **F1** passa de `Triple fault` para `DOUBLE FAULT (IST1) rip=... cr2=0x18000000...` na serial e `hlt` sem reiniciar. Antes: `Triple fault` no log (medido). |
| **S2. Limite de profundidade no parser e layout** | `parse_element`/`layout_block` com `MAX_DEPTH` (por ex. 128-256; achatar ou truncar além disso) | `osjeff_core/src/web/dom.rs:126-170`, `layout.rs:89-165` | 0,5 | Teste unitário `deep_nesting_does_not_overflow` (host). **F12 com 6 400 `<div>`** retorna e imprime `ok`. Antes: triple fault com 200 (medido). |
| **S3. Pilhas de thread com página de guarda** | Cada pilha de thread alinhada a 4 KiB com uma página não presente abaixo (as páginas do `.bss` são de 4 KiB, basta limpar `PRESENT` e `invlpg`; restaurar ao liberar). Alternativa mais limpa: região virtual própria + alocador de frames (item 1 da §5). Remover o canário-que-causa-panic-na-ISR. | `sched.rs:19,111,161`, novo `vm.rs` (mínimo), `virtio.rs:17-25` (modelo) | 2-3 | **F2** (overflow em thread) gera `#PF` na guarda com `CR2` na faixa da pilha e a serial imprime `stack overflow thread 'exp_ovf'`; a thread é marcada morta (S4) e a UI **continua viva** (relógio avança entre dois screenshots). Antes: corrupção silenciosa e morte total (medido). |
| **S4. Contenção por thread** | `Thread` ganha estado (`Running/Dead`); #PF/#GP/panic em contexto de thread (IF=1) marcam a thread morta, imprimem (S0) e trocam de contexto, em vez de `hlt`. O escalonador pula threads mortas. O Task Manager lista o estado real das threads. Panic em ISR/IF=0 segue fatal, com mensagem. | `sched.rs`, `interrupts.rs`, `main.rs:771-780`, `desktop/apps.rs:557-610` | 2-3 | **F3**: `thr` volta a 3, serial com `thread 'exp_panic' panicked`, Task Manager mostra `DEAD`. **F5** deixa de congelar a máquina (a thread do canário é encerrada). |
| **S5. Endurecimento do runtime wasm** | `Config::consume_fuel(true)` com orçamento por `render`/`on_key` (e maior para a inicialização); `ResourceLimiter` (p. ex. 32 MiB, 1 memória, 1 instância); limites nas host functions (`n` de iovecs ≤ 16, `len` ≤ 64 KiB, taxa de `host.log`); `OutOfFuel`/trap marca o app como "travado" e oferece **Encerrar/Reiniciar**; **fechar a janela descarta a `Store`** (libera a memória) e reabrir reinstancia; `proc_exit` encerra de fato; `APP` estático vira uma estrutura por app; lista de apps embutidos (Snake, Plasma, DOOM) e seleção em tempo de execução. | `wasm/mod.rs` (`build_app`, `worker`, `APP`), `wasm/wasi.rs:72,98,230,318`, `kernel/build.rs:34-54`, `desktop/mod.rs`, `desktop/input.rs` | 5-8 | (a) Guest em loop: serial `OutOfFuel`, UI viva, **após fechar a janela 8/8 amostras de `info registers` em `HLT=1`** (protótipo já obteve este resultado). (b) Guest `grow`: serial `pages=` ≤ limite (protótipo: 321 com limite de 24 MiB, contra 513 sem limite). (c) Guest chamando `random_get(0x7fffffff)`: devolve `INVAL` em < 10 ms. (d) Fechar o DOOM faz o HUD voltar de ~22% de heap ao patamar de base. (e) Task Manager lista o app wasm com estado real e o "DEL" libera a memória. |
| **S6. Apps do desktop como guests (primeiro o navegador)** | Compilar o motor web (e `Browser`) de `osjeff_core` para `wasm32` como app: o guest recebe os bytes da página e devolve a lista de desenho (ou pinta na superfície). Ganha: parser/layout recursivo e fora de controle viram `Trap`/`OutOfFuel`. | `osjeff_core/Cargo.toml` (crate-type), nova pasta `wasm-apps/web/`, `desktop/mod.rs:563-569`, `desktop/apps.rs` | 5-8 | F12 (6 400 `<div>`, agora entregue ao guest): a janela mostra "página travou/limite excedido", **sem triple fault**, UI viva, heap volta ao patamar. Medição de desempenho: tempo de `render` de uma página de teste com e sem wasm (esperado: bem mais lento) [NV]. |
| **S7. Primeiro processo ring 3 (opcional, só com gatilho)** | Itens 1-8 e 12 da §5, nesta ordem: (7a) frames + espaço de endereçamento; (7b) GDT com ring 3 e entrada de `syscall` com um programa mínimo; (7c) kill/fault; (7d) loader ELF + crate de userland; (7e) um app (Calculadora) portado. | §5 | 22-33 (MVP) | **7b**: processo faz `syscall write` e a serial mostra `hello from ring3`; um loop de usuário com `hlt` interno é capturado com `CPL=3` no `info registers`. **7c**: o processo escreve em `0x10000000000` (kernel): `#PF` com U=1, processo morto, kernel e relógio vivos. Processo em loop infinito: preemptado, o Task Manager o encerra e o heap volta ao patamar. **7e**: Calculadora funciona da dock em ring 3, e F4 deixa de afetá-la. |

Observação sobre S3 vs S1: S1 é pré-requisito do critério de S3 (o #PF da guarda precisa de uma pilha de IST para ser entregue com segurança se a pilha da thread estiver estourada). Fazer S0 e S1 primeiro.

Observação sobre W^X (o item (a) do pedido original): **já está feito pelo bootloader** (§2.2, F10/F11). O que falta é (i) um teste de regressão (as teclas F10/F11 viram `selftest`), (ii) não introduzir regiões RWX no futuro e (iii) as guard pages (S3). Não é preciso código novo para W^X/NX.

## 7. Consequências

### 7.1 Se nada for feito (estado atual)

Exemplos concretos deste código, todos com medição na §2.4:

- Um HTML de ~1 KiB com 200 `<div>` aninhados (ou ~200 tags não fechadas, que o parser aninha) **reinicia a máquina**. O TLS não verifica certificados, então qualquer atacante no caminho serve esse HTML.
- Um bug de recursão em qualquer app que rode na thread do compositor: triple fault. Em qualquer thread secundária: **corrupção silenciosa do heap vizinho** e depois morte total.
- Um panic de qualquer app = tela congelada sem mensagem. Um panic na thread do wasm = zumbi que continua consumindo fatias.
- Um guest wasm com loop infinito (inclusive por um bug no próprio app: o `#[panic_handler]` do Snake/Plasma é `loop {}`) **prende uma thread para sempre e não pode ser encerrado**; fechar a janela não adianta.
- Um guest pode reter 32 MiB (51%) do heap de 64 MiB, que é compartilhado com o navegador/TLS; com o heap cheio, a próxima alocação do kernel dá panic e congela a UI.
- Memória: o Task Manager não ajuda, pois mostra uma tabela de nomes e um contador de fatias.

### 7.2 Com o plano recomendado (S0-S6)

- Falhas deixam de ser silenciosas (S0) e deixam de ser fatais por thread (S3-S4).
- O pior caso medido (HTML hostil) vira um `Trap`/`OutOfFuel` num guest (S6) ou, no mínimo, uma truncagem do parser (S2).
- O wasm deixa de poder monopolizar CPU e memória (S5).
- **Continua sendo verdade** que um bug no wasmi, numa host function ou em qualquer `unsafe` do kernel é um escape total, e que o kernel e o compositor seguem num único espaço de endereçamento com 1 TiB de mapa físico RW. Isso só muda com o S7.

## 8. Riscos

| Risco | Impacto | Mitigação / como detectar |
|---|---|---|
| Bug de escape no wasmi (executor com ~135 `unsafe`) ou numa host function | Escape total (guest vira ring 0) | Manter `wasmi` fixado e atualizar com os avisos; fuzz das host functions (entrada = memória do guest); reduzir a superfície (`env.system`, `poll_oneoff`, `path_*` que "fingem sucesso" só para o DOOM); gatilho (b) de §4 para o S7. [NV: histórico de CVEs do wasmi não consultado] |
| Medições em TCG (sem KVM) | Tempos de quadro e fps **não** valem para hardware real | Os números citados como evidência são determinísticos (fuel, ciclos de falha, flags de CPU) ou do host (benchmark); os fps do HUD não foram usados como evidência. Repetir em KVM/hardware real. |
| WAD substituto (Freedoom 28 MiB) em vez do shareware | Custo de DOOM pode diferir um pouco; a imagem fica de 34 MiB | A engine é a mesma; reproduzir com o `doom1.wad` oficial antes de fixar orçamentos de fuel. |
| Orçamento de fuel mal calibrado | Falso positivo (DOOM estourou 20 M ao carregar nível) → app morto sem culpa | Orçamento por tipo de chamada (init alto, `render` típico 10× a média); ao estourar, **pausar e continuar** em vez de abortar quando for carga legítima [NV: wasmi permite reabastecer e retomar com resumable calls; não testado]. |
| S1 troca a GDT que o bootloader deixou | Se o bootloader ainda referenciar a página antiga (identidade, R-X) o `iretq` pode falhar | Manter a página mapeada; testar BIOS **e** UEFI (a GDT está em endereços diferentes: `0x6d2e000` vs `0x6be0000`). Só BIOS foi exercitado com patch; UEFI foi só inspecionado (§2.2). |
| S3 mexe em PTEs do `.bss` | Erro de TLB/`invlpg` ou página de guarda restaurada fora de hora corrompe o heap | Fazer a guarda sempre em páginas alinhadas fora do alocador, com teste de ida e volta (spawn/free em laço) e F2 como regressão. |
| `RacyCell` e a premissa "1 núcleo + IF" | Introduzir novas ISRs/threads/ring 3 quebra a exclusão mútua implícita (há 12+ estáticos assim, ver `00-mapa.md`) | Auditar cada `RacyCell` no S4 e S7. |
| Estimativas de esforço (§5, §6) | Podem estar ±50% | São estimativas de um desenvolvedor familiarizado; os itens com mais incerteza são o 2, 7 e 11 da §5. |
| Compilar `osjeff_core` para wasm (S6) | Pode exigir ajustes (alocador no guest, `core::fmt`, tamanho do `.wasm`) | Prova de conceito de meio dia antes de comprometer o S6 [NV]. |

## 9. O que já existe e dá suporte à recomendação

- Runtime wasmi integrado, ABI de desenho com recorte, subconjunto WASI e a camada de plataforma do DOOM (`kernel/src/wasm/`, `wasm-apps/doom/doomgeneric_osjeff.c`).
- Thread própria com superfície offscreen em buffer duplo e fila de eventos (`wasm/mod.rs:326-481`).
- Pipeline de build Rust→wasm e C→wasm (`kernel/build.rs`, `tools/build-doom.sh`) e DOOM comprovadamente funcionando.
- APIs do wasmi para fuel (`Config::consume_fuel`, `Store::set_fuel/get_fuel`) e limites (`StoreLimitsBuilder`), **validadas aqui**.
- Escalonador preemptivo com `fxsave` por thread (`sched.rs`, `switch.s`), base do S4 e do S7.
- `osjeff_core` puro, testável no host e sem `unsafe`: candidato natural a guest (S6).
- Harness QEMU headless (`tools/qemu-headless.sh`) e a infraestrutura de teste usada neste ADR.

---

## Apêndice A: como reproduzir as medições

1. Registradores e mapa de memória: iniciar a imagem BIOS de `target/release/build/os/*/out/osjeff-bios.img` com `-monitor unix:...`, `info registers`; `pmemsave 0x0 0x8000000 ram.bin`; caminhar as tabelas de página a partir de `CR3` (script `walk.py` no scratchpad).
2. Experimentos de falha: worktree `.../scratchpad/wtF` com (a) `kernel/src/exp.rs` novo, onde uma tecla F-n dispara a falha, `main.rs` (+2 linhas) para chamá-lo, `sched.rs::exp_clobber`; (b) `kernel/build.rs` com três WATs hostis (`exp_loop`, `exp_grow`, `exp_trap`); (c) `wasm/mod.rs` com seleção de app, fuel por quadro, `StoreLimits` e log de desempenho. As teclas: F1 estouro da pilha de boot; F2 estouro em thread; F3 panic em thread; F4 panic no compositor; F5 canário corrompido; F6/F7/F8/F9 guests loop/grow/trap/snake; F10 escrita em `.text`; F11 execução no heap; F12 HTML aninhado; ScrollLock heap esgotado; NumLock guest grow sem limite. O patch completo: `.../scratchpad/adr-isolamento-experimentos.patch`.
3. Cada falha: boot limpo (~20-70 s em TCG), `sendkey fN`, dois screenshots com 4 s (`screendump` + comparação de pixels), `info registers`, log `-d cpu_reset,int`.
4. Benchmark wasmi vs nativo: crate `bench/` no scratchpad (wasmi 1.1.0, `--release`, LTO), carregando o `plasma.wasm` compilado de `wasm-apps/plasma`.
5. DOOM: `wasi-sdk-25.0` (GitHub), `doomgeneric` (clonado por `tools/build-doom.sh`), `freedoom1.wad` (Freedoom 0.13) renomeado `doom1.wad`; `DOOM=1 WASI_SDK_PATH=... cargo build --release -p os`; QEMU com `-m 256M`.

Todos os downloads ficaram em diretórios novos do scratchpad, fora do repositório.

## Apêndice B: relação com as outras auditorias

- `02-interrupcoes-scheduler.md` já documenta, com os mesmos mecanismos, a ausência de TSS/IST, a pilha de thread sem guarda e o panic silencioso. Este ADR **confirma por execução** a cadeia HTML→triple fault até a thread do compositor (que lá ficou como "não provada ponta a ponta"), e acrescenta o comportamento dos guests wasm.
- `00-mapa.md` §7 descreve `wasm-apps/`; as divergências são só de estado: DOOM aqui foi **construído e executado** (com WAD substituto) e o Plasma segue órfão.
- `01-memoria-unsafe.md` cobre o `unsafe` do kernel; aqui foi só contado.
