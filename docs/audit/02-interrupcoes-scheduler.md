> *Written when the project was called OSjeff (renamed Kitsune in 2026-10; names and paths below are the historical ones).*

# Auditoria 02 — Interrupções, scheduler e troca de contexto

Escopo: `kernel/src/switch.s`, `sched.rs`, `interrupts.rs`, `io.rs`, `ps2.rs`, `main.rs`, `boot.rs`, `sync.rs`
(mais `allocator.rs`, `perf.rs`, `fetch.rs`, `wasm/mod.rs` onde tocam o scheduler). Branch `audit/performance-security`.

Metodologia: leitura do código, `objdump` do binário `kernel`, e experimentos no QEMU headless (TCG, sem KVM) com um
worktree descartável (`scratchpad/wtB`) que **não** foi commitado. O patch de instrumentação
(rdtsc no ISR, high-water mark de pilha, contador de iterações do loop, threads de teste selecionadas por
`OSJ_EXP=n` em tempo de compilação) está em `scratchpad/B-instrumentation.patch`. Evidências brutas (serial, screenshots,
logs `-d int,cpu_reset`) estão em `scratchpad/B-*/` e `scratchpad/B-*.qlog`.

Legenda: **PROVADO** = reproduzido no QEMU ou trecho de código conclusivo; **SUPOSIÇÃO** = raciocínio sem reprodução
ponta a ponta (a lacuna é dita explicitamente).

## Resumo executivo

| # | Achado | Severidade | Status |
|---|--------|-----------|--------|
| 1 | Pilhas das threads no heap, sem guard page; canário contornável e a detecção termina em freeze silencioso | ALTA | PROVADO (mecanismo); gatilho em uso real = SUPOSIÇÃO |
| 2 | Sem TSS/IST: estouro da pilha de boot (compositor) vira #PF -> #DF -> triple fault (reboot silencioso) | ALTA | PROVADO |
| 3 | Parser/layout HTML recursivos e sem limite rodam na pilha de boot de 80 KiB: página remota profunda derruba a máquina | ALTA | mecanismo PROVADO; cadeia ponta a ponta = SUPOSIÇÃO |
| 4 | Falhas fatais são mudas (panic/exceções só fazem `hlt`); panic em thread com IF=1 deixa o resto rodando com tela congelada | MÉDIA | PROVADO |
| 5 | Round-robin puro sem estado "bloqueada": threads em `hlt` gastam a fatia; compositor roda a 83 Hz; app WASM recebe 1/3 da CPU | MÉDIA | PROVADO |
| 6 | Exceções e IRQs sem handler (#UD, #DE, #NM, #NP, #SS, #AC, #MC, NMI, IRQ7/15 espúrias) escalam para #DF | MÉDIA | #UD PROVADO; IRQ espúria = SUPOSIÇÃO |
| 7 | Salvar/restaurar FPU/SSE é inócuo: kernel sem nenhuma instrução SSE e CR4.OSFXSR=0 | BAIXA | PROVADO |
| 8 | Task Manager "CPU" mede ticks creditados a threads que estão em `hlt` | BAIXA | PROVADO |
| 9 | `scheduler()` devolve `&'static mut` (aliasing UB); `spawn` depende de o chamador desligar IRQ; threads nunca terminam | BAIXA | SUPOSIÇÃO |
| 10 | Tempo: calibração do TSC numa janela de 100 ms sem timeout; PIT 250,04 Hz; `ticks*4` assume 250 Hz exato | BAIXA | calibração PROVADA (dispersão); hang sem PIT = SUPOSIÇÃO |
| 11 | `SpinLock` do heap mantém IF=0 durante varreduras O(n); possível perda de ticks | BAIXA | SUPOSIÇÃO |

O que está **correto** (verificado): salvamento completo de GPRs + `iretq`, alinhamento do frame inicial (rsp ≡ 8 mod 16),
RFLAGS=0x202, CS/SS válidos, EOI em todos os caminhos (`info pic`: ISR=00 após tráfego de teclado/mouse/timer), máscaras
do PIC (0xF8/0xEF), ISRs sem alocação e sem lock, ISR do timer barato, ring de input SPSC correto, `hlt` no loop ocioso
(QEMU usa ~5% de um núcleo host em idle). Detalhes na seção de medições.

---

## Achados

### [ALTA] Pilhas de thread no heap sem guard page; canário contornável e detecção termina em freeze silencioso
Onde: `kernel/src/sched.rs:111-114` (alocação/canário), `sched.rs:160-162` (checagem), `interrupts.rs:124-129` (checagem roda dentro do ISR)
O que acontece:
```rust
let mut stack = vec![0u8; STACK_SIZE].into_boxed_slice();          // 128 KiB no heap
stack[..8].copy_from_slice(&STACK_CANARY.to_ne_bytes());           // 8 bytes no fundo
...
if !s.threads[cur].stack_intact() { panic!("stack overflow in thread '{}'", ...); }
```
1. Não há guard page: a pilha é uma fatia do heap estático de 64 MiB, que está sempre mapeado. Estouro não gera #PF; escreve no
   vizinho. As pilhas são alocadas lado a lado pelo first-fit: no experimento, `fetcher` 0x100011458f0..0x100011658f0,
   `wasmapp` 0x10001165bb0..0x10001185bb0, o objeto "vítima" do experimento em 0x10001185db0 e a pilha de teste em 0x10001195db0
   (colada ao fim da vítima), com os endereços do build EXP=2. Ou seja, o que fica abaixo de uma pilha é o topo (frames vivos) da pilha anterior ou um objeto de heap.
2. O canário é uma única palavra de 8 bytes, só verificada para a thread *que está saindo*, só quando o tick cai nela. Um frame grande
   (array local grande, p.ex. buffer de TLS/rede) atravessa o canário sem tocá-lo e escreve bem abaixo da pilha.
3. Quando o canário é detectado, o `panic!` roda **dentro do ISR do timer** (IF=0) e o panic handler faz só `hlt`: freeze total,
   sem mensagem (o nome da thread prometido nunca aparece).
Por que importa: é o pior modo de falha para um kernel (corrupção silenciosa de heap/pilha vizinha, ou freeze sem diagnóstico).
O doc afirma que o canário "troca corrupção silenciosa por panic com o nome da thread": falso na prática.
Como provar (rodei no QEMU, build com `OSJ_EXP=1` e `OSJ_EXP=2`):
- EXP=1: thread de teste recursiva ultrapassa a base da pilha em 2928 bytes. Resultado: tela preta (nem o splash aparece),
  serial sem nenhuma linha depois de `EXP=1`; monitor `info registers`: `HLT=1`, `RFLAGS=0x17` (IF=0), RIP no loop `hlt` do panic handler.
  Com um `serial_println!` temporário antes do `panic!` confirmei a causa: `canary clobbered in thread 'ovf1' (stack base 0x10001184db0, rsp 0x10001184240)`.
- EXP=2: um único frame de 150 KiB (`MaybeUninit`, escrita nos 16 bytes mais baixos do frame, ~22 KiB abaixo da base) sobre um `Vec` vítima de 64 KiB
  preenchido com 0x5A: `EXP2 victim corrupted bytes: 80` e o sistema segue rodando (linhas `DBG` continuam, nenhum panic): canário intacto, corrupção não detectada.
- Uso real (high-water mark medido varrendo a pilha zerada): `fetcher` 9-12 KB (caminho DNS/TCP; handshake TLS **não** medido: o DNS de IP literal não resolve no slirp do sandbox),
  `wasmapp` 1,8 KB parado e 12,9 KB com o snake rodando. Nenhum overflow real observado; o risco é de capacidade (TLS 1.3 com dados do servidor, wasmi, DOOM), SUPOSIÇÃO.
Correção proposta: (a) alocar pilhas por páginas com uma página de guarda não mapeada (precisa de um mapeador de páginas do kernel; o bootloader já mapeia toda a memória física
em `physical_memory_offset`, então dá para remapear uma página do vão); (b) enquanto isso, canário de várias palavras/pattern fill e checagem em todas as pilhas a cada N ticks, mais
checar `rsp` do frame salvo contra `[base, top)`; (c) trocar o `panic!` dentro do ISR por um caminho que imprime na serial (ver achado 4); (d) pilhas maiores para quem roda TLS/wasmi (ou medir o HWM e dimensionar).
Esforço: médio (guard page) / baixo (canário melhor + mensagem)

### [ALTA] Sem TSS/IST: estouro da pilha de boot vira #PF -> #DF -> triple fault, silencioso
Onde: `kernel/src/interrupts.rs:38-52` (IDT sem `set_stack_index`), `interrupts.rs:147-149` (`double_fault` só `halt()`); GDT é a do bootloader (`bootloader-x86_64-common/src/gdt.rs`: só nulo, código, dados; `TR=0`); pilha de boot configurada em `main.rs:50-56` sem `kernel_stack_size` (padrão 80 KiB)
O que acontece:
```rust
idt.double_fault.set_handler_fn(double_fault);   // sem .set_stack_index(IST)
extern "x86-interrupt" fn double_fault(_f: InterruptStackFrame, _code: u64) -> ! { halt() }
```
O kernel nunca carrega GDT/TSS próprios (sem `ltr`, sem IST). A thread de boot (compositor) tem pilha do bootloader com guard page. Estourar essa pilha gera #PF; para entregar o #PF a CPU precisa empilhar o frame
no mesmo `rsp` inválido -> falha de novo -> #DF; entregar o #DF empilha de novo no mesmo `rsp` -> triple fault. O handler de #DF nunca chega a rodar.
Por que importa: a afirmação do ARCHITECTURE.md ("exceções fatais travam com `hlt` em vez de triple-faultar") é falsa exatamente no caso mais provável de crash do kernel (pilha). Em hardware real é reset imediato e silencioso (loop de reboot); nenhum log.
A thread que mais usa pilha (desktop, render web, wasmi no boot via `wasm::run_demo`, buffers de 1600 B de rede no `main`) tem a menor pilha (80 KiB) e nenhum canário.
Como provar (rodei: `OSJ_EXP=3`, recursão com frame de 512 B na thread do compositor, `-d int,cpu_reset`):
```
3003: v=0e e=0002 ... SP=0010:0000018000000ea0 CR2=0000018000000e98     (#PF na guard page)
3004: v=08 e=0000 ... SP=0010:0000018000000ea0                           (#DF, mesmo rsp)
check_exception old: 0x8 new 0xe
Triple fault
```
Serial sem nenhuma mensagem.
Correção proposta: GDT própria com TSS e pelo menos 2 pilhas IST (uma para #DF, uma para #PF/NMI) em memória estática; `idt.double_fault.set_handler_fn(..).set_stack_index(DF_IST)`; no handler, imprimir na serial (RIP, CR2, código de erro) antes do `hlt`. Aumentar a pilha de boot (`c.kernel_stack_size = 512 * 1024` em `BOOT_CONFIG`).
Esforço: médio

### [ALTA] HTML aninhado fundo estoura a pilha de 80 KiB do compositor (DoS remoto: triple fault)
Onde: `osjeff_core/src/web/dom.rs:89,126` (`parse_nodes` <-> `parse_element`), `osjeff_core/src/web/layout.rs:89,119` (`layout_children` <-> `layout_block`), chamado de `kernel/src/desktop/mod.rs:563-566` (`browser_load`, na thread do compositor)
O que acontece:
```rust
fn layout_block(el: &Element, ...) -> i32 { ... y = layout_children(&el.children, sheet, c, cx, y, cw, p); ... }
fn layout_children(nodes: &[Node], ...) -> i32 { ... layout_block(el, &c, sheet, x, y, width, p) ... }
```
Não há limite de profundidade (nem no parser, nem no layout, nem no `Drop` recursivo de `Vec<Node>`). Cada nível consome dezenas/centenas de bytes de pilha, e o resultado do `fetcher` (conteúdo remoto, sem verificação de certificado, ver auditoria de rede) é renderizado na pilha de boot de 80 KiB.
Por que importa: uma página com algumas centenas de `<div>` aninhados derruba/reinicia a máquina (achado 2). Conteúdo controlado por qualquer servidor (ou MITM, já que o TLS não valida certificado).
Como provar: **mecanismo provado, cadeia ponta a ponta não**. Exemplo host (`osjeff_core/examples/deep.rs` no worktree; `web::render` numa thread com 80 KiB de pilha, build release): profundidade 100 passa, 150, 200, 250, 500, 1000 e 3000 abortam com "stack overflow". No kernel existem outros frames em cima (loop principal grande, buffers) e o compilador é outro, então o limiar real é parecido, não idêntico. Não consegui servir a página ao guest: o DNS de IP literal (10.0.2.2) não resolve no QEMU user-net deste sandbox e não consegui adicionar entrada em /etc/hosts. Se confirmado, elevar para CRÍTICA (travamento explorável de fora).
Correção proposta: limitar profundidade (p.ex. 64 níveis: o parser trata acima disso como texto/ignora; layout recusa), ou converter para iteração com pilha explícita no heap; executar o render numa thread com pilha grande (a do `fetcher` já tem 128 KiB) em vez do compositor; aumentar a pilha de boot.
Esforço: baixo (limite de profundidade)

### [MÉDIA] Falhas fatais são mudas; panic em thread com IF=1 deixa o sistema "vivo" com a tela congelada
Onde: `kernel/src/main.rs:777-779` (panic handler), `main.rs:771-775` (`halt`), `interrupts.rs:147-162` (handlers de exceção)
O que acontece:
```rust
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! { halt(); }          // descarta a mensagem, não escreve na serial
extern "x86-interrupt" fn page_fault(_f: InterruptStackFrame, _code: PageFaultErrorCode) { halt() }
```
Nem panic nem exceções escrevem na serial ou na tela (a serial existe, `serial.rs`, e é segura em ISR). Dois comportamentos distintos:
- exceção (#PF/#GP/#DF): gate de interrupção -> IF=0 -> `hlt` eterno, **tudo** morre (timer, mouse, outras threads);
- `panic!` em thread com IF=1 (p.ex. compositor): o `hlt` é interrompido pelo timer, o scheduler segue trocando de thread; compositor "panicado" volta a dar `hlt` a cada fatia; fetcher/wasmapp continuam rodando. A tela fica congelada sem qualquer indicação.
Por que importa: o ARCHITECTURE.md vende "bugs ficam visíveis"; na prática nada é visível. Também mascara os achados 1-3 (todos terminam em freeze/reset sem pista). O `panic!` do `assert!(.. "too many threads")` e do canário também somem.
Como provar (rodei, `-d int` e threads de teste):
- `OSJ_EXP=4` (panic no compositor em tick>3000 + thread "hb" que imprime `HB tick=N` a cada 250 ticks): `HB` continua indefinidamente (27 linhas até o fim da captura), nenhuma linha de panic, screenshot com relógio parado em 18:56:35, `info registers`: `HLT=1`, IF=1.
- `OSJ_EXP=5` (leitura de 0x10): `v=0e ... CR2=0x10`, `HB` para no mesmo tick, `RFLAGS` sem IF (0x46), `HLT=1`.
Correção proposta: panic handler e handlers de exceção escrevem `PanicInfo`/frame/CR2 na serial (e opcionalmente pintam uma tela de erro direto no framebuffer), depois `cli; hlt` em loop; no panic handler, desligar interrupções primeiro para o sistema parar de forma determinística.
Esforço: baixo

### [MÉDIA] Scheduler round-robin sem estado "bloqueada": threads ociosas gastam a fatia inteira
Onde: `kernel/src/sched.rs:184-186` (`next = (cur+1) % n`), `fetch.rs:75-97`, `wasm/mod.rs:446-481`, `main.rs:638`
O que acontece:
```rust
let next = (cur + 1) % n;     // sempre a próxima, esteja ela pronta ou parada em `hlt`
```
Uma thread sem trabalho faz `hlt` e dorme até o próximo tick, entregando 4 ms de CPU a ninguém; só então o scheduler passa à seguinte. Com compositor + fetcher + wasmapp:
- o loop do compositor roda a **250/3 ≈ 83 Hz** em idle (o comentário em `main.rs:633-637` diz que o timer a 250 Hz o acorda);
- IRQ de teclado/mouse que cai enquanto outra thread está "em `hlt`" só é vista pelo compositor depois da rotação (latência estimada de até ~8-12 ms; não medida);
- com a janela WASM aberta, o worker (loop sem `hlt` nem ritmo) recebe só 1/3 do tempo; os outros 2/3 viram `hlt` e o host fica a ~45-50% de um núcleo em vez de ~100% (o guest tem trabalho pendente mas a CPU dorme);
- durante um fetch, o `Net::pump` faz busy-wait (`spin_loop`) por até 5-10 s sem `hlt`.
Por que importa: desperdício de CPU/latência e frame rate do app WASM limitado artificialmente; piora com mais threads.
Como provar (medi): `DBG loops=...` (contador de iterações do loop principal): +164 iterações por 500 ticks (= 82/s) em idle, em todas as janelas de 2 s. `ticks=[1018,991,991]` aos 3000 ticks: fatias iguais mesmo com 2 threads em `hlt`.
CPU do processo QEMU (clock ticks de 1/100 s, TCG): idle 25-28 por 5 s (~5% de um núcleo); com o snake rodando 218-251 por 5 s (~45-50%).
Correção proposta: dar às threads um estado (`Ready`/`Blocked(wake_tick)`) e pular as bloqueadas; `hlt`/`yield` passam a chamar `sched::block_until(...)`/`yield_now()` que entrega a fatia na hora; se nenhuma estiver pronta, executa `hlt` no contexto atual. Alternativa mínima: um `yield` cooperativo (`int` ou chamada direta de `switch_current` com IF=0) nos pontos onde hoje há `hlt` nos workers.
Esforço: médio

### [MÉDIA] Exceções e IRQs sem handler escalam para #DF
Onde: `kernel/src/interrupts.rs:38-52` (só breakpoint, #GP, #PF, #DF, timer, teclado, mouse têm entrada), `interrupts.rs:168-185` (máscaras 0xF8/0xEF)
O que acontece: #DE, #UD, #NM, #SS, #NP, #AC, #MC, NMI e as IRQs espúrias 7/15 (vetores 0x27/0x2F) caem numa entrada IDT *não presente*. A CPU levanta #NP e, sem handler, #DF, que cai no `halt()` silencioso. Um #UD (instrução inválida, p.ex. de um módulo WASM com bug de host ou de uma instrução SSE acidental, ver achado 7) vira freeze sem pista.
Por que importa: o ARCHITECTURE.md cobre só #GP/#PF/#DF. IRQ7/15 espúria acontece em hardware real mesmo com a linha mascarada (a espúria sai pelo vetor 7/15 quando a linha some antes do INTA). Os estados do PIC estão corretos (ver medições) e não vi espúria no QEMU, então essa parte é SUPOSIÇÃO.
Como provar (rodei `OSJ_EXP=6`, `ud2` na thread do compositor, `-d int`):
```
3003: v=06 e=0000 ...        (#UD)
3004: v=0b e=0032 ...        (#NP: entrada 6 da IDT não presente)
3005: v=08 e=0000 ...        (#DF)
```
`HB` para, IF=0, `HLT=1`.
Correção proposta: handlers para todas as exceções (gerando mensagem na serial + halt) e handlers "iretq/ignorar" para vetores 0x27 e 0x2F (ler ISR do PIC para decidir se é espúria; só dar EOI no mestre para IRQ15 espúria). Preencher a IDT inteira com um handler genérico.
Esforço: baixo

### [BAIXA] Salvar/restaurar FPU/SSE é inócuo: kernel soft-float, CR4.OSFXSR=0
Onde: `kernel/src/sched.rs:30-52, 166-173, 189-195`; `.cargo/config.toml` (alvo `x86_64-unknown-none` sem SSE)
O que acontece:
```rust
"fxsave [{}]", in(reg) s.threads[cur].fpu_ptr(), ...   // 512 B por thread, a cada tick
```
O alvo é compilado sem SSE: `objdump -d` do `kernel` tem **zero** ocorrências de `xmm` (352k linhas) e só 4 de `fxsave/fxrstor` (as do scheduler). O monitor mostra `CR4=0x20` (OSFXSR=0): instruções SSE dariam #UD. O float do desktop/WASM roda em soft-float (compiler-builtins), então não há estado SSE para preservar. O ARCHITECTURE.md descreve o `fxsave` como proteção contra `xmm` clobberado e o frame de entrada ≡ 8 mod 16 como proteção contra `movaps` #GP: ambos sem efeito neste build.
Por que importa: baixo custo (~2 x 100 ciclos/tick) mas é código e documentação enganosos; se alguém ligar SSE, falta setar CR0/CR4 (`OSFXSR`, `OSXMMEXCPT`) e hoje o primeiro `xmm` dá #UD silencioso (achado 6). `FxArea::seeded` é executada com o estado da thread que chama `spawn`.
Como provar: `objdump` + `info registers` (feito). Alinhamento: `FxArea` é `#[repr(C, align(16))]`, o `Box` respeita (o sistema boota com 3-4 threads, `fxsave` desalinhado daria #GP).
Correção proposta: ou remover o fxsave/fxrstor e documentar "kernel sem SSE", ou habilitar SSE de verdade (CR0.EM=0/MP=1, CR4.OSFXSR/OSXMMEXCPT) e então manter. Ajustar o doc.
Esforço: baixo

### [BAIXA] Task Manager "CPU" mede ticks creditados a threads que estão em `hlt`
Onde: `kernel/src/sched.rs:176-178` (`TICKS[cur]` creditado a quem estava rodando no tick), `kernel/src/desktop/apps.rs:600-609`
O que acontece: a coluna "CPU" é o número bruto de ticks em que a thread era a corrente, inclusive quando ela está parada em `hlt` esperando trabalho.
Como provar (medi): em idle, `ticks=[1352,1325,1325]`: três threads "usando" 1/3 cada, embora duas estejam dormindo.
Por que importa: métrica enganosa (o ARCHITECTURE.md vende como "prova de preempção"); impede enxergar o achado 5.
Correção proposta: contar tick só se a thread não estava em `hlt` (flag setada antes do `hlt` pelo próprio `sched::idle()`), ou medir TSC em execução útil.
Esforço: baixo (depende do achado 5)

### [BAIXA] `scheduler()` devolve `&'static mut`; `spawn` exige que o chamador desligue IRQ; threads nunca terminam
Onde: `kernel/src/sched.rs:220-222`, `sched.rs:107-109`, `sched.rs:151-158`, `main.rs:243-256`
O que acontece:
```rust
fn scheduler() -> &'static mut Scheduler { unsafe { (*SCHED.get()).as_mut().expect(...) } }
```
O ISR (`switch_current`) e `spawn`/`thread_*` formam `&mut Scheduler` aliasados para a mesma estática: UB formal (sem consequência prática hoje porque o único `spawn` acontece no boot dentro de `without_interrupts`, mas a função é `pub` e não impõe isso). Não existe saída de thread: o `rsp` inicial deixa 0 na posição do endereço de retorno (pilha zerada), então se um entry `-> !` retornasse (UB do contrato) daria #PF em RIP=0 e freeze. `MAX_THREADS=8` com `assert!` (freeze silencioso, achado 4). Não há remoção de thread nem liberação de pilha.
Como provar: leitura de código (SUPOSIÇÃO quanto a dano real). Verifiquei empiricamente que a pilha inicial está correta: `top=0x100011648f0`, `thread_rsp = (top & !0xF) - 8` ≡ 8 mod 16, frame `iretq` com RFLAGS=0x202 e CS/SS lidos do segmento atual; as 3-4 threads rodam normalmente.
Correção proposta: `spawn` faz `without_interrupts` por dentro; `Scheduler` acessado por `RacyCell` com `unsafe fn with(|s| ...)` curto em vez de `&'static mut`; uma função de saída que marca a thread como morta (e o scheduler passa a pulá-la).
Esforço: baixo

### [BAIXA] Tempo: calibração do TSC numa janela de 100 ms sem timeout; PIT 250,04 Hz
Onde: `kernel/src/perf.rs:12-23`, `kernel/src/interrupts.rs:188-193`, `kernel/src/wasm/mod.rs:230`, `wasm/wasi.rs:217`
O que acontece:
```rust
while interrupts::ticks() == t0 {}                       // sem timeout: trava o boot se o PIT não disparar
while interrupts::ticks() < start_tick + 25 {}           // 25 ticks = 100 ms de janela única
let divisor = (1_193_182 / hz) as u16;                   // 4772 -> 250,04 Hz; ticks*4 ms no WASM assume 250 exato
```
Por que importa: HUD/ms ligeiramente imprecisos; boot pendura para sempre em máquina sem PIT/8259 legado (alguns firmwares UEFI recentes). Contadores `u64` de ticks (250 Hz) não dão wraparound na prática (>2 bilhões de anos); aritmética de tempo em `u64` sem overflow plausível.
Como provar: dispersão medida em 18 boots no mesmo host: `TSC calibrated` de 2.080.874 a 2.325.544 kHz (±6%). Hang sem PIT: SUPOSIÇÃO (não testado: QEMU sempre emula o PIT).
Correção proposta: timeout nos laços (`rdtsc` contra um limite alto) com fallback para um kHz padrão; calibrar em janela maior ou com CPUID leaf 0x15/0x16; usar `TICKS * 1000 / TIMER_HZ` ou o divisor real em vez de `* 4`.
Esforço: baixo

### [BAIXA] `SpinLock` do heap segura IF=0 durante varreduras O(n) e todo `alloc`
Onde: `kernel/src/allocator.rs:32-46, 68-75`, `main.rs` (HUD chama `ALLOCATOR.free_bytes()` ~10x/s)
O que acontece: `lock()` guarda `are_enabled()`, faz `cli` e só religa em `drop` se estava ligado (isso é *melhor* que "cli/sti" cego: é seguro com aninhamento). Mas toda operação do heap, inclusive `free_bytes()` (percorre a free list inteira) e a primeira-ajuste de um `vec![0u8; 128K]`, roda com interrupções desligadas. Com free list longa (fragmentação), um tick de 4 ms pode ser perdido (IRQ0 é por borda: ticks perdidos = relógio atrasado; `TICKS` conta interrupções). Reentrância (alocar dentro do lock) trava com IF=0 e `locked=true` (spin infinito, sem diagnóstico).
Como provar: leitura de código (SUPOSIÇÃO; não medi perda de ticks; `isr_max` no TCG é dominado por ruído do host).
Correção proposta: manter, mas limitar o custo (cache de `free_bytes` mantido em `alloc`/`dealloc`), detectar reentrância (contador) e imprimir na serial.
Esforço: baixo

---

## Medições e verificações sem achado

- **ISR do timer** (`timer_schedule`): só incrementa `TICKS`, checa canário, `fxsave`/`fxrstor` (512 B) e escolhe a próxima thread; sem alocação, sem lock, EOI antes do `iretq`. Medido com rdtsc dentro do ISR (QEMU TCG, TSC virtual ≈ 2,1 GHz): média **12-35 mil ciclos ≈ 6-17 µs** por tick conforme a carga do host (≈0,15-0,4% do período de 4 ms). Os máximos (4 a 17 milhões de ciclos) são ruído do host (load average 8-16 durante as medições), não custo do ISR; não tenho medida confiável de pior caso em hardware real.
- **ISRs de teclado/mouse**: leem 0x60, `ring_push` (SPSC, sem alocar, sem lock), EOI (mouse: escravo e mestre). Interrupt gates (IF=0): sem aninhamento. Ring de 512 entradas descarta bytes quando cheio (pode dessincronizar um pacote de mouse; o decoder ressincroniza pelo bit 3).
- **PIC**: `info pic` do QEMU após mexer mouse e teclado: `pic0 imr=f8 isr=00`, `pic1 imr=ef isr=00`: nenhum IRQ preso em serviço (EOI correto em todos os caminhos). `pic1 irr=c0` (IRQ14/15 do IDE pendentes, mascaradas): inofensivo. Não há `io_wait` entre as escritas de ICW (ok em QEMU; antigas máquinas reais podem pedir).
- **Troca de contexto**: salva rax..r15 e usa o frame do `iretq` do CPU (RIP/CS/RFLAGS/RSP/SS); callee-saved cobertos. Alinhamento no `call timer_schedule`: CPU alinha rsp a 16, 5 palavras de frame + 15 pushes = 160 bytes, `call` coloca rsp ≡ 8 mod 16 na entrada: correto. Pilha inicial da thread nova correta (ver achado 9).
- **Idle**: o loop do compositor termina em `hlt` (`main.rs:638`); fetcher e wasmapp fazem `hlt` quando sem trabalho. QEMU TCG: ~5% de um núcleo host em idle. Com WASM ativo ~45-50% (achado 5).
- **HUD**: mostra tempo de frame, draws/s, heap e número de threads; não mostra CPU%.
- **Pilha: uso real** (high-water mark): fetcher 9-12 KB (sem TLS), wasmapp 1,8 KB parado / 12,9 KB com snake. Pilha de boot: não medida (tem guard page; ver achado 2).

---

## Afirmações do ARCHITECTURE.md verificadas

| Afirmação | Veredito | Evidência |
|-----------|----------|-----------|
| "Exceções fatais (#GP, #PF, double fault) travam com `hlt` em vez de triple-faultar" | **Parcial** | #GP/#PF/#DF travam com `hlt` (EXP=5), mas só se a pilha for utilizável: estouro da pilha de boot dá triple fault (EXP=3); #UD/#DE/#NM etc. não têm handler e caem em #DF (EXP=6). |
| "...bugs ficam visíveis durante o desenvolvimento" | **Falso** | Nenhuma saída na serial nem na tela em panic/exceção (EXP=4/5/6). |
| "Stack canary por thread" (tabela de decisões, `sched.rs`) | **Parcial** | Existe só nas threads spawnadas (não no compositor), 8 bytes, checado só na thread que sai do tick. Contornável com frame grande (EXP=2). |
| "...checado em cada troca de contexto -> `panic` com o nome da thread" | **Falso na prática** | O `panic!` roda no ISR com IF=0 e o handler só faz `hlt`: freeze com tela preta, sem nome (EXP=1). |
| "stacks são heap, sem guard page" | **Verdadeiro** | `sched.rs:111`; pilhas adjacentes no heap. |
| "Spin lock com `cli`/`sti`" | **Parcial (melhor que o doc)** | `lock()` salva o IF e `drop` só religa se estava ligado (`allocator.rs:35-36, 70-74`): seguro com aninhamento. Não há `sti` incondicional. |
| "Os ISRs de teclado/mouse rodam com interrupções desabilitadas, único produtor/consumidor" | **Verdadeiro** | Interrupt gates; ring SPSC lido só pelo compositor. |
| "PIC remapeado para 0x20+; máscaras liberam timer, teclado, cascata e mouse" | **Verdadeiro** | `remap_pic`, `info pic`: 0xF8/0xEF, ISR=00. |
| "PIT a 250 Hz (canal 0, modo 3)" | **Verdadeiro (aprox.)** | Divisor 4772 = 250,04 Hz. |
| "`spawn` fabrica pilha com frame `iretq` + 15 regs; rsp ≡ 8 mod 16" | **Verdadeiro** | `sched.rs:118-140`; verificado nos endereços reais. |
| "`fxsave`/`fxrstor` salvam xmm0..15/MXCSR por thread" | **Verdadeiro, mas sem efeito** | O kernel não tem nenhuma instrução SSE e CR4.OSFXSR=0 (achado 7). |
| "Errar o alinhamento da pilha faz `movaps` dar #GP" | **Irrelevante neste build** | Sem SSE no kernel. |
| "Prova de preempção: compositor, worker-a e worker-b com CPU idêntica" | **Obsoleto/enganoso** | `worker-a/b` não existem mais (`main.rs` comenta a remoção); a "CPU idêntica" hoje é artefato do round-robin que conta ticks de threads em `hlt` (achados 5 e 8). |
| (comentário de `main.rs:633-637`) "o timer (250 Hz) acorda o compositor" | **Falso com 3 threads** | O compositor roda a ~83 Hz em idle (`DBG loops`). |
