> *Written when the project was called OSjeff (renamed Kitsune in 2026-10; names and paths below are the historical ones).*

# Auditoria 01 — Memória e `unsafe`

Branch `audit/performance-security`, commit base `1c14b3d`. Escopo: `kernel/src` (bare-metal) e
`osjeff_core/src/heap.rs`. Nada foi commitado; os experimentos rodaram num `git worktree` separado
(`.../scratchpad/wtA`) e os artefatos citados abaixo estão lá.

Legenda: **PROVADO** = reproduzido (teste no host, boot em QEMU) ou trecho de código conclusivo.
**SUPOSIÇÃO** = raciocínio sem reprodução. Nenhum achado foi classificado CRÍTICA nem ALTA: o que
poderia ser (deadlock do allocator com IRQ, corrupção do heap, race em `RacyCell` sob preempção) foi
verificado e **não existe hoje** (ver seções 3 e 4).

## 0. Resumo

| # | Severidade | Achado | Status |
|---|---|---|---|
| 1 | MÉDIA | Panic, exceções e OOM são mudos: o `panic_handler` e os handlers #PF/#GP/#DF só dão `hlt` | PROVADO |
| 2 | MÉDIA | Estouro de pilha: o canário só vale para 2 das 3 threads, panica dentro do ISR (trava tudo) e a thread do compositor vira triple fault | PROVADO |
| 3 | MÉDIA | Framebuffer maior que 1920x1080x4 estoura os buffers estáticos e dá panic mudo no boot | mecanismo PROVADO, cenário de hardware SUPOSIÇÃO |
| 4 | MÉDIA | Heap de 64 MiB sem limites por consumidor (wasm `memory.grow`, `http_get` sem teto) e sem degradação graciosa | OOM PROVADO, vetores SUPOSIÇÃO |
| 5 | BAIXA | Allocator perde o padding da frente em alocações com `align > 8` | PROVADO (host e kernel), impacto atual ~0 |
| 6 | BAIXA | `RacyCell: Sync` incondicional + funções seguras que devolvem `&'static mut` (aliasing real em `files_rows`) | PROVADO por leitura |
| 7 | BAIXA | 100 blocos `unsafe` sem `// SAFETY:` (só 5 comentários no kernel inteiro) | PROVADO (clippy) |
| 8 | BAIXA | Robustez do allocator: `align_up` sem checar overflow, `fit_region` rejeita sobra < 16 B, `dealloc` não valida layout | PROVADO (testes), inalcançável hoje |
| 9 | BAIXA | MMIO/DMA virtio: valores vindos do dispositivo sem validação (`% qsize` com 0, BAR/offset) | SUPOSIÇÃO |
| 10 | BAIXA | `options(...)` de `asm!`: `nomem` em port I/O e `fxrstor` sem clobbers | SUPOSIÇÃO (inofensivo hoje) |
| 11 | BAIXA | `cargo lint-kernel` falha na branch (1 erro clippy) | PROVADO |
| 12 | BAIXA | Data races benignas entre threads (`SURFACE` rasga quadro, `serial` intercala) e comentários desatualizados | PROVADO por leitura |

Medições que descartam riscos: pico de pilha do compositor 11 232 B de 81 920 B; `fetcher` 9 112 B e
`wasmapp` 12 792 B de 131 072 B; tempo máximo com interrupções desligadas pelo lock do heap: média
1 a 4 µs, picos esporádicos de 0,13 a 2,7 ms (ruído do QEMU/TCG, ver seção 3.3).

---

## 1. Achados

### [MÉDIA] Panic, exceções e OOM são mudos
Onde: `kernel/src/main.rs:777-780`, `kernel/src/interrupts.rs:147-159`
O que acontece: o `panic_handler` descarta o `PanicInfo` e entra em loop de `hlt`. Os handlers de
#DF, #GP e #PF fazem o mesmo, sem imprimir RIP, CR2 nem código de erro. Não existe
`alloc_error_handler`; uma alocação que falha vira `panic` e cai no mesmo buraco negro.
```rust
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt();
}
extern "x86-interrupt" fn page_fault(_f: InterruptStackFrame, _code: PageFaultErrorCode) {
    halt()
}
```
Por que importa: toda falha fatal (OOM, canário de pilha, índice fora de limites, falha de página) é
invisível na serial. O ARCHITECTURE.md afirma que o canário "faz `panic` com o nome da thread"; a
mensagem existe, mas nunca sai da máquina. Um panic numa thread comum deixa as outras rodando (o `hlt`
roda com IF=1), então o sistema fica meio vivo, com a tela congelada.
Como provar (QEMU, rodado): build do worktree com experimento `oom` (`AUDIT_EXP=oom`): a thread do
compositor pede `Vec::with_capacity(100 MiB)`, uma segunda thread imprime a cada segundo.
Resultado em `runOOM/serial.log`: após `EXP oom: chamando with_capacity(100MiB)...` nunca aparece
`SOBREVIVEU`, nenhuma mensagem de panic; a thread `audit` continua imprimindo `tick=252, 504, ... 9072`.
`try_reserve(100 MiB)` retornou `Err` normalmente, ou seja, o allocator devolve `null` corretamente; o
silêncio vem só do handler. Com um `serial_println!("PANIC: {}", info)` no handler (build `ovf2`) a
mensagem aparece: `panicked at kernel/src/sched.rs:161:9: stack overflow in thread 'audit'`.
Correção proposta: imprimir `info` (arquivo, linha, mensagem) na serial e na tela no `panic_handler`;
nos handlers de exceção imprimir `InterruptStackFrame`, CR2 e o código de erro; usar
`#[alloc_error_handler]` (ou `Vec::try_reserve` nos pontos de carga grande) com mensagem própria.
Esforço: baixo
Severidade: MÉDIA — PROVADO

### [MÉDIA] Estouro de pilha: canário parcial, panic dentro do ISR e triple fault no compositor
Onde: `kernel/src/sched.rs:22-28, 111-114, 160-162`, `kernel/src/interrupts.rs:147-149`, `kernel/src/main.rs:56`
O que acontece, em três partes:
1. O canário (8 bytes no fim baixo da pilha) só existe para threads criadas por `spawn` (`fetcher`,
   `wasmapp`). O compositor roda na pilha de 80 KiB do bootloader, sem canário.
2. A checagem acontece no ISR do timer, só para a thread que acabou de rodar, a cada 4 ms. Se falha,
   `panic!` dispara **dentro do ISR com IF=0** e o `halt()` trava a máquina inteira (todas as threads),
   sem mensagem (achado anterior).
3. Estourar a pilha do compositor causa #PF (a página de guarda do bootloader), mas não há IST nem TSS
   próprio: o #DF tenta empilhar na mesma pilha quebrada e dá triple fault, que reinicia a CPU. O handler
   de #DF nunca roda.
```rust
if !s.threads[cur].stack_intact() {
    panic!("stack overflow in thread '{}'", s.threads[cur].name);
}
```
Por que importa: a defesa descrita ("em vez de corrupção silenciosa do heap") só funciona em parte. As
pilhas das threads são blocos do heap sem página de guarda; antes do próximo tick o código já escreveu
em blocos vizinhos (possivelmente nós da free list). E a promessa "exceções travam a tela em vez de
reiniciar" é falsa para o caso mais perigoso, o estouro da pilha do compositor.
Como provar (QEMU, rodado):
- Thread `audit` recursa com frames de 1 KiB, ~134 KiB (excede 128 KiB em ~6 KiB). Build stock
  (`runOVF`): serial para depois de `EXP ovf: recursão voltou (9045)`, tela totalmente preta (nem o splash
  seguinte aparece): o sistema inteiro travou. Build com print no handler (`runOVF2`): `PANIC: panicked at
  kernel/src/sched.rs:161:9: stack overflow in thread 'audit'`.
- Recursão de 100 frames de 1 KiB na thread do compositor (`runTriple`, com `-d int,cpu_reset`): log do
  QEMU mostra `v=0e ... CR2=0x18000000d18`, em seguida `v=08` (#DF) e `CPU Reset`. Serial sem nenhuma
  linha após `EXP triple: recursao...`.
- Folga atual (medida, não é risco hoje): compositor com Terminal, Editor, Navegador e Arquivos abertos
  usa no máximo 11 232 B de 81 920 B; `wasmapp` 12 792 B e `fetcher` 9 112 B de 131 072 B. Não medi o
  `fetcher` durante um handshake TLS (sem rede no ambiente), então o pico real do TLS é SUPOSIÇÃO.
Correção proposta: (a) panic mudo corrigido como no achado 1; (b) `kernel_stack_size` explícito no
`BootloaderConfig` e IST dedicado para #DF (GDT/TSS próprios); (c) alocar pilhas de thread com página
de guarda (mapear páginas não presentes) em vez de `Box<[u8]>`; (d) mover a checagem do canário para fora
do ISR (marcar a thread como morta e continuar) ou ao menos imprimir antes de travar; (e) incluir o
compositor no esquema de canário.
Esforço: médio
Severidade: MÉDIA — PROVADO (mecanismo); sem risco imediato dada a folga medida

### [MÉDIA] Framebuffer maior que 1920x1080x4 estoura os buffers estáticos
Onde: `kernel/src/main.rs:60, 104-114`
O que acontece: `BACK`, `BG` e `STATIC` têm `MAX_BYTES = 1920*1080*4`. O código só trunca o tamanho
(`n = len.min(MAX_BYTES)`), mas o `Canvas` continua usando o `info` real (largura, altura, stride) do
framebuffer. Qualquer escrita além de `n` é um índice fora de limites.
```rust
const MAX_BYTES: usize = 1920 * 1080 * 4;
let n = framebuffer.buffer().len().min(MAX_BYTES);
let back: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(BACK.get() as *mut u8, n) };
```
Por que importa: em hardware real com tela 2560x1440, 4K ou até 1920x1200 em 4 B/pixel (9,2 MB > 8,3 MB)
o boot termina em panic mudo logo no splash. Não é corrupção de memória (os índices são checados), é
falha de disponibilidade.
Como provar: mecanismo PROVADO no QEMU com o limite reduzido artificialmente (`AUDIT_SMALL=1` →
`MAX_BYTES = 1_000_000`, tela 1280x720): `PANIC: panicked at kernel/src/fb.rs:203:17: index out of
bounds: the len is 1000000 but the index is 1000000`. O cenário de hardware com resolução maior é
SUPOSIÇÃO: tentei forçar 1920x1200 e 2560x1440 no OVMF (`-global VGA.xres=...`), mas o bootloader UEFI
respondeu `no framebuffer` em qualquer resolução forçada, então não consegui reproduzir.
Correção proposta: alocar os buffers com o tamanho do framebuffer (heap ou página alinhada) ou recusar/
reduzir a resolução com mensagem; no mínimo `Canvas::new` deveria validar `buf.len() >= stride*height*bpp`.
Esforço: médio
Severidade: MÉDIA — mecanismo PROVADO, cenário real SUPOSIÇÃO

### [MÉDIA] Heap fixo de 64 MiB sem limites por consumidor
Onde: `kernel/src/main.rs:79-83`, `kernel/src/netstack.rs:174-190`, `kernel/src/wasm/mod.rs:251, 285`
O que acontece: um único heap de 64 MiB serve compositor, fetcher, wasmi e TLS. Quem estoura derruba os
outros, porque a próxima alocação de qualquer thread falha e o panic é mudo (achado 1).
- `http_get` (HTTP puro) acumula a resposta num `Vec` sem teto durante 10 s; o caminho TLS limita a 256 KiB.
- `Store::new(...)` do wasmi não instala `ResourceLimiter`: o guest pode pedir `memory.grow` até acabar o heap.
```rust
let mut out = Vec::new();
...
let _ = s.recv(|data| { out.extend_from_slice(data); (data.len(), ()) });
```
Por que importa: DoS por servidor HTTP hostil ou por app wasm. Hoje os apps wasm são embutidos em tempo de
build (`APP_WASM`/`DEMO_WASM`), o que reduz o risco do vetor wasm.
Como provar: o efeito final (OOM → thread morre calada, as outras seguem) está PROVADO pelo experimento
`oom` (achado 1). Que o `Vec` do `http_get` chegue a 64 MiB em 10 s, e que o wasm cresça a memória, é
SUPOSIÇÃO: não tenho servidor hostil nem guest malicioso no ambiente (sem internet).
Correção proposta: teto no `http_get` igual ao do TLS; `Store::limiter` com `StoreLimitsBuilder`
(memória máxima por instância); reservar cota de heap para o compositor.
Esforço: baixo
Severidade: MÉDIA — OOM PROVADO, vetores SUPOSIÇÃO

### [BAIXA] Allocator perde o padding da frente em alocações com `align > 8`
Onde: `kernel/src/allocator.rs:172-202` (`find_region` + `alloc`), `osjeff_core/src/heap.rs:21-34`
O que acontece: `find_region` remove a região inteira da lista e devolve `alloc_start`
alinhado. `alloc` recoloca só a cauda (`excess`). O trecho `[region.start, alloc_start)` nunca volta
para a lista: vaza para sempre. `fit_region` nem reporta esse padding a quem chama.
```rust
Some((region, alloc_start)) => {
    let alloc_end = alloc_start + size;
    let excess = region.end_addr() - alloc_end;
    if excess > 0 { unsafe { self.add_free_region(alloc_end, excess) }; }
    alloc_start as *mut u8
}
```
Por que importa: em teoria acumula fragmentação permanente. Na prática o impacto atual é nulo: a sessão
inteira (boot, 4 janelas, app wasm) faz só 3 alocações com `align > 8` (os três `Box<FxArea>`, align 16) e
perdeu 0 bytes, porque as regiões livres daquele momento já começavam alinhadas.
Como provar (rodado):
- Cópia literal do trecho do allocator numa crate de host (`heapsim`, mesmo código por `diff`). Teste
  `front_padding_is_leaked_forever`: alloc(16, align 4096) numa região que começa em 8 mod 4096, free,
  `free_bytes` cai de 1 048 568 para 1 044 480: **4 088 B perdidos**.
- `align16_leaks_8_bytes_per_misaligned_region`: 1 000 pares (24 B align 8, 32 B align 16), libera tudo:
  7 992 B perdidos.
- Fuzz de 300 mil operações (tamanhos 1-3000, aligns 8/16/64/4096): **163 728 B (1,95% de 8 MiB) perdidos
  e 580 nós livres** no fim, contra 0 B e 1 nó quando todos os aligns são 8 (`fuzz_align8_only_has_no_leak`).
- No kernel real (experimento `pad`): 7 952 B perdidos com o mesmo padrão; alloc align 4096 não perdeu nada
  porque a região já estava alinhada (honestamente: o caso 4 KiB não reproduziu no kernel).
- Teste só do core (`fit_region_hides_front_padding`): `front=4088 size=16 excess=4088`, soma 8192 =
  região, mas `fit_region` só devolve `(start, excess)`.
Correção proposta: em `alloc`, se `alloc_start - region.start >= node_size`, reinserir o trecho frontal
(`add_free_region(region.start, front)`); se for menor, rejeitar a região em `fit_region` (assim como já
faz com a cauda). Adicionar o teste do fuzz ao core (a lógica pode ser testada se `fit_region` devolver o
front).
Esforço: baixo
Severidade: BAIXA — PROVADO, sem impacto hoje

### [BAIXA] `RacyCell` é `Sync` para qualquer `T` e há funções seguras que fabricam `&'static mut`
Onde: `kernel/src/sync.rs:25`, `kernel/src/desktop/mod.rs:50`, `kernel/src/desktop/widgets.rs:290`,
`kernel/src/sched.rs:220`, `kernel/src/wasm/mod.rs:309`, `kernel/src/desktop/mod.rs:607-630`
O que acontece: `unsafe impl<T> Sync for RacyCell<T>` não exige `T: Send`, então até um `Option<Net>`
(smoltcp) ou `Option<App>` (wasmi `Store`) pode ser compartilhado entre threads sem o compilador
reclamar. Quatro funções **seguras** (`disk()`, `scratch_slice()`, `scheduler()`, `app_mut()`) devolvem
`&'static mut` de um static: qualquer chamador pode criar duas referências mutáveis vivas. Isso já
acontece: `files_rows` e `files_slot` seguram `let img = disk();` e, dentro do filtro que a usa, chamam
`self.files_cwd()`, que chama `disk()` de novo.
```rust
pub(crate) fn files_rows(&self) -> usize {
    let img = disk();                       // &'static mut A
    ... let cwd = self.files_cwd();         // chama disk(): &'static mut B, A ainda viva
```
Por que importa: aliasing de `&mut` é UB formal; hoje só há leituras e o código gerado funciona, mas é
exatamente o padrão que o ARCHITECTURE.md diz evitar.
Como provar: raciocínio sobre o código (leitura), sem reproduzir miscompilação. Tentei rodar Miri no
`heapsim`, mas o componente não está instalado neste toolchain.
Correção proposta: trocar por API de fechamento (`with_disk(|d| ...)`) ou `SpinLock<T>` (o do allocator
já desliga interrupções e pode virar `sync::Mutex`); exigir `T: Send` no `Sync` de `RacyCell`; para o
estado compartilhado com ISR usar atômicos (como já é feito no ring e em `fetch::STATE`).
Esforço: médio
Severidade: BAIXA — PROVADO por leitura

### [BAIXA] 100 blocos `unsafe` sem `// SAFETY:`
Onde: todo `kernel/src`
O que acontece: 97 blocos `unsafe {}`, 14 `unsafe fn`, 4 `unsafe impl`, 1 `unsafe extern`, 14 `asm!`.
Só 5 comentários `SAFETY`/`# Safety` no kernel todo (`allocator.rs:115, 237`, `virtio.rs:78`,
`wasm/mod.rs:79, 308`).
Como provar (rodado, worktree): `#![warn(clippy::undocumented_unsafe_blocks)]` +
`#![deny(unsafe_op_in_unsafe_fn)]` em `main.rs`: **100 avisos** de `undocumented_unsafe_blocks` e **0
erros** de `unsafe_op_in_unsafe_fn` (a edição 2024 já exige blocos dentro de `unsafe fn`). Por arquivo:
`virtio.rs` 21, `virtio_gpu.rs` 16, `allocator.rs` 10, `sched.rs` 9, `wasm/mod.rs` 7, `io.rs` 7, `fetch.rs` 6,
`main.rs` 5, `ne2000.rs` 4, `interrupts.rs` 4, `ps2.rs` 2, `netstack.rs` 2, `desktop/widgets.rs` 2,
`sync.rs`, `power.rs`, `perf.rs`, `fb.rs`, `desktop/mod.rs` 1 cada.
Correção proposta: ligar `undocumented_unsafe_blocks` como `warn` no `lint-kernel` e documentar por
padrão (os 21 de `virtio.rs` são o mesmo invariante repetido; ver seção 2).
Esforço: baixo a médio
Severidade: BAIXA — PROVADO

### [BAIXA] Robustez do allocator em casos de borda
Onde: `osjeff_core/src/heap.rs:6-8, 27-44, 71-79`, `kernel/src/allocator.rs:204-208`
Resultados dos testes (host), todos ao nível de "inalcançável pelo `GlobalAlloc` real" mas relevantes
porque a função é `pub`:
- `align_up(usize::MAX - 3, 8)`: soma sem checagem; em debug dá `attempt to add with overflow`, em release
  embrulha e retorna 0 (calculado, não executei em release). `adjust_request(usize::MAX-3, ..)` idem.
  `Layout` limita o tamanho a `isize::MAX`, então não é alcançável via `alloc`. Fuzz de esgotamento: pedidos
  de `isize::MAX - 7` devolvem `null` corretamente (teste `exhaustion_returns_null_and_recovers`: ok).
- `fit_region` rejeita região com sobra de 8 B (< `min_block` 16): pedido de 4 088 B com 4 096 B livres
  devolve `null` (`region_8_bytes_bigger_than_request_is_rejected`). Só importa perto do OOM.
- `dealloc` com `Layout` maior que o do `alloc` (violação do contrato do `GlobalAlloc`): o nó livre cobre
  o vizinho vivo e a próxima alocação de 400 B cai **dentro** do bloco vivo (`dealloc_with_wrong_layout_corrupts`:
  `p4` sobrepõe `p2`). O allocator não guarda cabeçalho nem valida; é o contrato padrão, mas vale registrar.
- Zero-size: `adjust_request` sobe para `node_size` (16), então `alloc(0)` devolve bloco válido e único.
- Coalescência nas bordas: **correta** (ver seção 3.1).
Correção proposta: `checked_add` em `align_up`; documentar o contrato de `dealloc`.
Esforço: baixo
Severidade: BAIXA — PROVADO nos testes, inalcançável hoje

### [BAIXA] MMIO e DMA do virtio: sem validação do que vem do dispositivo
Onde: `kernel/src/virtio.rs:197-238`, `kernel/src/virtio_gpu.rs:88-89, 152`, `kernel/src/pci.rs:53-55`
O que acontece:
- `slot = avail_idx % self.qsize` com `qsize = common.queue_size().min(QSIZE)`: se o dispositivo informar 0,
  divisão por zero (panic mudo antes de `sched::init`, IF=0).
- `bar` e `offset` lidos da capability PCI não são validados: `0x10 + i*4` (u8) estoura para `i > 59`, e
  `bar_base + phys_offset + offset` é dereferenciado sem checar `offset + len <= tamanho do BAR`.
- `cap_read32(off + 16)` com `off` (u8) perto de 0xFC embrulha.
- Barreiras: o código usa só `compiler_fence(SeqCst)` entre publicar o descritor, o `avail.idx` e o
  `notify` (volatile). Em x86 (TSO) isso basta; não é portável.
- Mapeamento: o MMIO é acessado pelo mapeamento linear da memória física do bootloader (atributos de WB
  por padrão); em hardware real depende de o MTRR classificar a região como UC. Em QEMU funciona.
Como provar: leitura de código. O driver funciona de ponta a ponta em QEMU com `-device virtio-gpu-pci`,
BIOS e UEFI/512M (`runGPU`, `runGPUuefi`: `virtio-gpu: control queue up, DRIVER_OK`, `display 0: 1280x800`,
`2D command path ... -> true`). Pontos corretos: toda leitura/escrita de MMIO e de anel usa
`read_volatile`/`write_volatile`; `QUEUE_MEM`/`CMD_MEM`/`BACKING` são páginas alinhadas a 4 KiB e
`virt_to_phys` percorre as tabelas reais; offsets dos anéis (0/256/512) cabem na página para QSIZE=16.
Correção proposta: validar `qsize != 0`, `bar < 6`, `offset+len <= bar_len`; usar `fence(Release)` antes
do notify para não depender de x86.
Esforço: baixo
Severidade: BAIXA — SUPOSIÇÃO (dispositivo confiável no modelo atual)

### [BAIXA] `options(...)` de `asm!`
Onde: `kernel/src/io.rs:9-72`, `kernel/src/sched.rs:44-46, 169-172, 191-194`, `kernel/src/switch.s`
O que acontece (inventário na seção 5): todos os `options` conferem com o que a instrução faz, com três
ressalvas:
- `in`/`out`/`rdtsc` com `nomem` permitem ao compilador mover acessos à memória por cima da instrução.
  Para port I/O de dispositivos polled (NE2000, ATA) não há DMA, então é inofensivo; para `rdtsc`
  em `perf` isso só imprecisa a medição (`rdtsc` não serializa).
- `fxrstor` (readonly) altera x87/SSE sem declarar clobbers de `xmm`/`st`. Como o alvo é compilado com SSE
  desligado, o compilador nunca usa esses registradores; se alguém ligar SSE no kernel, passa a mentir.
- `switch.s` não limpa DF (`cld`) na entrada do ISR; só importa se código interrompido deixar DF=1.
Como provar: leitura. Nada falhou em boot.
Correção proposta: remover `nomem` de `outb/outw/outl` (custa nada) e declarar clobbers de `xmm0-15`/`st`
se SSE for habilitado; adicionar `cld` no ISR.
Esforço: baixo
Severidade: BAIXA — SUPOSIÇÃO

### [BAIXA] `cargo lint-kernel` falha na branch
Onde: `kernel/src/desktop/render.rs:74`
O que acontece: o alias `lint-kernel` usa `-D warnings` e há 1 aviso `clippy::needless_range_loop` (`for w in
0..WIN_COUNT` indexando `self.windows[w]`).
Como provar (rodado): `cargo lint-kernel` termina com `error: could not compile kernel ... due to 1
previous error`. `cargo test -p osjeff_core`: 189 testes passam (o README diz 152).
Correção proposta: `for (w, win) in self.windows.iter().enumerate()` ou `#[allow(...)]` justificado.
Esforço: baixo
Severidade: BAIXA — PROVADO

### [BAIXA] Races benignas entre threads e comentários desatualizados
Onde: `kernel/src/wasm/mod.rs:40-42, 307-309, 456-478, 510`, `kernel/src/serial.rs:30-47`
O que acontece:
- `APP` é lido só pela thread do worker (`app_mut()` é chamado em `worker`), mas os comentários dizem
  "compositor thread only" e "single-threaded".
- O worker reaproveita o buffer `back` do quadro anterior assim que publica o novo; se o compositor for
  preemptado no meio de `blit_surface` (cópia de 1,1 MB, mais de um tick), o worker pode escrever no buffer
  que ainda está sendo copiado: quadro rasgado (sem UB de memória, mas data race não atômica).
- `serial_println!` não tem lock: as threads intercalam caracteres na saída.
Como provar: leitura. Sem impacto de memória.
Correção proposta: contador de geração por buffer (ou 3 buffers); lock curto na serial; corrigir os
comentários.
Esforço: baixo
Severidade: BAIXA — PROVADO por leitura

---

## 2. Catálogo de `unsafe` por padrão (os mais perigosos primeiro)

| Padrão | Onde | Invariante exigida | Garantida por quem chama? | Documentada? |
|---|---|---|---|---|
| Lista livre intrusiva com ponteiros brutos e `&'static mut FreeNode` | `allocator.rs:126-186` | região alinhada a 8, `size >= 16`, sem sobreposição, lista ordenada | Sim para blocos próprios; só `debug_assert!` (desligado em release) para endereço/tamanho de `init` | Parcial (`# Safety` em `init`) |
| Fabricar frame de `iretq` na pilha de uma thread nova | `sched.rs:107-146` + `switch.s` | layout idêntico à ordem de `push`/`pop` do ISR, RSP ≡ 8 mod 16, RFLAGS com IF, CS/SS válidos | Sim (conferido: 5 palavras do iretq + 15 GPRs, alinhamento certo) | Só comentários de prosa, sem `SAFETY` |
| `unsafe impl Sync` em `SpinLock`, `InputRing`, `RacyCell` | `allocator.rs:22`, `interrupts.rs:72`, `sync.rs:25` | exclusão mútua real | `SpinLock` e `InputRing` sim; `RacyCell` só por convenção, sem `T: Send` | Comentário, sem `SAFETY` |
| `&mut *(l4_virt as *mut PageTable)` sobre a tabela de páginas viva | `virtio.rs:20-21` | `phys_offset` mapeia toda a RAM; ninguém mais muta a tabela | Sim (bootloader; `phys_offset` é `Option` checado) | Não |
| MMIO por endereço calculado do BAR (`Common::new(addr)`, `notify_addr`) | `virtio.rs:78-117`, `virtio_gpu.rs` | endereço mapeado, dentro do BAR, alinhado | Não valida (achado 9) | `# Safety` em `Common::new` |
| `from_raw_parts_mut` de statics (`BACK`, `BG`, `STATIC`, `TLS_RX/TX`, `DISK`, `SCRATCH`) | `main.rs:111-114`, `netstack.rs:115-118`, `desktop/mod.rs:51`, `widgets.rs:291` | um único dono vivo por vez; `n <= tamanho` | Compositor/worker únicos (ver tabela da seção 3.2); `disk()`/`scratch_slice()` são funções seguras e não impedem aliasing (achado 6) | Não |
| `HostState.fb: *mut u8` do wasm | `wasm/mod.rs:79-83, 457-467` | ponteiro aponta para `SURFACE` enquanto `info` é `Some` | Sim (o worker zera `info` ao final de cada quadro; o static nunca é liberado) | Sim (`SAFETY` em `canvas`, mas diz "single-threaded", o que está desatualizado) |
| `fxsave`/`fxrstor` com ponteiro de `Box<FxArea>` | `sched.rs:44-46, 169-194` | 16 B alinhado, 512 B, válido | Sim (`FxArea` é `align(16)`; o allocator honra o alinhamento) | Comentários |
| `in`/`out`/`hlt`/`rdtsc` | `io.rs`, `power.rs`, `main.rs:773` | privilégio ring 0 | Sim | Não |
| `from_utf8_unchecked` | `perf.rs:610`, `widgets.rs:270` | bytes ASCII | Sim (montados só com dígitos e literais ASCII), mas o `unsafe` é desnecessário | Comentário em `perf.rs` |
| `align_to_mut::<u32>()` | `fb.rs:171` | nenhuma (API sound) | n/a | n/a |

---

## 3. Verificações pedidas

### 3.1 Allocator (`allocator.rs`, `heap.rs`)
- **Alinhamento em `alloc`**: respeitado (fuzz verificou `p % align == 0` em 300 mil operações com aligns até 4096).
  Vazamento do padding frontal: achado 5.
- **`dealloc`/`realloc`**: `realloc` usa o padrão de `GlobalAlloc` (aloca novo, copia, libera): correto, mas
  pico de 2x e O(n) por crescimento de `Vec`. `dealloc` recalcula o tamanho com `adjust_request(layout)`:
  consistente com `alloc` (fuzz: 0 B perdidos com aligns de 8). Layout trocado: achado 8.
- **Coalescência nas bordas**: **correta e provada**. Teste `border_coalescing_after_full_exhaustion_random_free_order`
  (20 sementes): preenche o heap até `null` (bordas inicial e final ocupadas), libera em ordem aleatória, exige
  1 nó com o total original: passa. O sentinela `head` nunca funde (tamanho 0 e endereço fora do heap).
- **Overflow de tamanho**: `fit_region` usa `checked_add` (testado); `align_up` não (achado 8), inalcançável via `Layout`.
- **Esgotamento**: `alloc` devolve `null` (PROVADO no host e no kernel: `try_reserve` → `Err`); o que falta é a
  política depois (achados 1 e 4). A cauda < 16 B é rejeitada em `fit_region` (decisão consciente).
- **Zero-size**: ok (vira bloco de 16 B).
- **Heap estático**: `HEAP: [u8; 64 MiB]` tem alinhamento 1 em Rust; o linker o pôs em `0x1140000` (4 KiB
  alinhado), então `init` recebe endereço alinhado por sorte; só um `debug_assert!` protege isso.
- **O lock desliga interrupções?** Sim (ver seção 1 de "Afirmações"). `SpinGuard::drop` restaura o IF salvo,
  então locks aninhados seguem a ordem LIFO; se alguém dropasse na ordem inversa reabriria interrupções cedo
  (SUPOSIÇÃO, não há aninhamento no código).
- **Custo da seção crítica**: instrumentei o lock (worktree, experimento `hold`) e medi o tempo entre `lock()` e
  `drop`: média 1 a 4 µs; máximo 997 µs, 208 µs, 131 µs e 2 742 µs em quatro execuções, valores esporádicos
  coerentes com o QEMU/TCG pausando o vCPU, não com o percurso da lista (poucos nós no kernel; no fuzz
  pesado do host a lista chegou a 555 nós). Não é um problema hoje.

### 3.2 `RacyCell`/statics mutáveis: soundness sob preempção por timer
Single-core com preempção por timer **não** é single-thread. Verifiquei cada static contra as 3 threads
(compositor, `fetcher`, `wasmapp`) e o ISR do timer:

| Static | Quem acessa | Proteção real | Veredito |
|---|---|---|---|
| `IDT` (`interrupts.rs:36`) | boot | roda antes de habilitar interrupções | ok |
| `SCHED` (`sched.rs:88`) | ISR do timer; compositor (`init`, `spawn`, leitura) | `spawn` sempre dentro de `without_interrupts` (`main.rs:245, 254`); leituras do Task Manager não mudam a estrutura | ok (aliasing formal de `&mut Scheduler`, achado 6) |
| `fetch::NET/REQ_URL/REQ_LEN/RESULT` | compositor escreve em IDLE, worker lê em REQUESTED/RUNNING | máquina de estados atômica `STATE` com Acquire/Release | ok |
| `ne2000::NEXT` + NIC | main loop e `fetcher` | só um deles toca a NIC: o main loop espera `fetch::is_idle()`; quem posta é o próprio compositor | ok |
| `netstack::TLS_RX/TLS_TX` | só `fetcher` | dono único | ok |
| `wasm::APP`, `FB_INFO` | só worker (`FB_INFO` escrito antes do `spawn`) | dono único | ok (comentário errado, achado 12) |
| `wasm::SURFACE` + `FRONT`/`READY` | worker escreve `back`, compositor lê `front` | atômicos Acquire/Release | ok, mas rasga quadro (achado 12) |
| `wasm::EVENTS` + `EV_HEAD/EV_TAIL` | compositor produz, worker consome | SPSC com atômicos | ok |
| `interrupts::RING` | ISRs produzem (IF=0, sem reentrância), compositor consome | SPSC com atômicos | ok |
| `ps2::DECODER` | só compositor (`poll`) | dono único | ok |
| `BACK`, `BG`, `STATIC`, `SCRATCH`, `DISK` | só compositor | dono único | ok |
| `virtio_gpu::QUEUE_MEM/CMD_MEM/BACKING` | boot | roda antes das threads | ok |
| `HEAP` | só o allocator | sob `SpinLock` com `cli` | ok |

Conclusão: **nenhum `RacyCell` é compartilhado hoje por duas threads preemptíveis sem sincronização**.
A justificativa escrita em `sync.rs` ("exclusão por flag de interrupção") só vale para o ring de
entrada; para o resto a segurança vem de "dono único por thread" e da máquina de estados do fetcher,
verdades do código atual e frágeis (nada no tipo impede um terceiro acessor). Se o timer preemptar uma
thread no meio de um trecho de `RacyCell`, nada acontece de errado porque nenhuma outra thread toca nele.

### 3.3 Deadlock clássico e IRQ
- ISRs existentes: `timer_isr` → `timer_schedule` → `sched::switch_current` (só `Vec` indexado e atômicos,
  nenhuma alocação nem lock); `keyboard`/`mouse` (`inb`, `ring_push`, `outb`); `breakpoint` (vazio); `#DF/#GP/#PF` (`hlt`).
  **Nenhum aloca**, então o cenário "IRQ aloca com o lock preso" não existe. O único alocador em caminho de
  ISR seria o `panic!` do canário, e o formato de `PanicInfo` não aloca.
- `SpinLock::lock` desliga interrupções antes de girar, então a thread seguinte nunca é escalonada com o lock preso.
- `lock()` não é reentrante: se algo alocasse dentro de uma seção crítica (hoje nada faz) seria deadlock
  duro com IF=0. Não há watchdog.
- `spawn` mexe na lista de threads com interrupções desligadas, correto.
- Exceções (#PF/#GP/#DF) com IF=0 que parem em `hlt` travam a máquina (aceito como fail-stop, ver achado 1).

### 3.4 Lint
- `cargo lint-kernel` (stock): **falha** com 1 erro (`needless_range_loop`, `desktop/render.rs:74`).
- `undocumented_unsafe_blocks`: 100 avisos (achado 7). `unsafe_op_in_unsafe_fn`: 0.
- `cargo test -p osjeff_core`: 189 passam.

---

## 4. Afirmações do README/ARCHITECTURE verificadas

| Afirmação | Veredito | Evidência |
|---|---|---|
| "Spin lock com `cli`/`sti`", "desabilita interrupções enquanto travado" (ARCHITECTURE §4, decisões) | **VERDADEIRA** para o único spin lock existente (o do allocator) | `allocator.rs:35-36` (`are_enabled` + `disable`), `:70-75` (restaura só se estava habilitado); nenhum outro lock existe |
| "zero `static mut` (`RacyCell`)" | **LITERALMENTE verdadeira, enganosa**: `grep "static mut"` dá 0, mas `RacyCell` é um `static mut` com outro nome (`Sync` incondicional, `get()` devolve `*mut T`) | `sync.rs:25`; achado 6 |
| "soundness justificada (single-core + exclusão por flag de interrupção)" | **FALSA como justificativa geral**: só o ring de entrada tem exclusão por IF/atômicos; o resto depende de "dono único por thread" | seção 3.2 |
| "Stack canary por thread" | **PARCIALMENTE verdadeira**: só `fetcher`/`wasmapp`, não o compositor; checado só no tick e só para a thread que saiu | `sched.rs:112-113, 160-162`; achado 2 |
| "`panic` com o nome da thread" | **FALSA na prática**: a mensagem existe e nunca é impressa; no ISR o panic trava a máquina inteira | `main.rs:778`; achados 1 e 2 (provado em QEMU) |
| "Exceções → tela travada, em vez de reboot silencioso" | **PARCIALMENTE falsa**: #PF/#GP travam (mudos), mas estouro da pilha do compositor vira triple fault e reinicia a CPU (sem IST) | `runTriple`: `v=0e`, `v=08`, `CPU Reset`; achado 2 |
| "ISRs de teclado/mouse rodam com interrupções desabilitadas (não se preemptam)" | **VERDADEIRA** (portas de interrupção do crate `x86_64`, IF limpo) | `interrupts.rs:131-143` |
| "ISRs nunca alocam" (comentário em `allocator.rs:21`) | **VERDADEIRA** hoje | seção 3.3 |
| "Heap com coalescência de blocos livres" | **VERDADEIRA** | seção 3.1, 20 sementes |
| "a matemática do allocator é testada; o `unsafe` fica isolado e auditável" | **PARCIAL**: o vazamento do padding frontal escapa dos testes do core porque `fit_region` não expõe o `front`; 97 blocos `unsafe` com 5 comentários | achados 5 e 7 |
| README "152 testes" | **DESATUALIZADO**: 189 testes passam | `cargo test -p osjeff_core` |
| Comentários de `wasm/mod.rs` "single-threaded/compositor only" | **FALSOS**: `APP` é usado pela thread `wasmapp` | achado 12 |

---

## 5. Inventário de `asm!` e `options`

| Local | Instrução | `options` | Confere? |
|---|---|---|---|
| `io.rs:9-72` | `in`/`out` b/w/l | `nomem, nostack, preserves_flags` | flags e pilha corretos; `nomem` é otimista (achado 10) |
| `io.rs` `rdtsc` | `rdtsc` | `nomem, nostack, preserves_flags` | ok, mas permite reordenar memória e não serializa |
| `power.rs`, `main.rs:773` | `hlt` | `nomem, nostack, preserves_flags` | ok |
| `sched.rs:44-46` | `fxsave [reg]` | `nostack, preserves_flags` | ok (escreve memória, sem `nomem`) |
| `sched.rs:169-172` | `fxsave [reg]` | `nostack, preserves_flags` | ok |
| `sched.rs:191-194` | `fxrstor [reg]` | `nostack, readonly, preserves_flags` | `readonly` certo; sem clobbers de xmm/x87 (achado 10) |
| `switch.s` (`global_asm!`) | salva 15 GPRs, `call`, troca RSP, restaura, `iretq` | n/a | alinhamento de pilha correto (5 palavras do CPU + 15 GPRs = 160 B, chamada alinhada em 16); sem `cld` |
| `interrupts.rs` | `global_asm!` + `sym timer_schedule` | n/a | ok |

---

## 6. Como reproduzir (artefatos no worktree)

Todos em `/tmp/claude-0/-home-user-OSjeff/0f58a1c9-b4c7-5882-a7fe-882fa852e898/scratchpad/`:
- `wtA/heapsim/src/lib.rs`: cópia literal do allocator + 8 testes (`cargo test --manifest-path heapsim/Cargo.toml`).
- `wtA/osjeff_core/src/heap.rs` (módulo `audit_tests` ao fim): 3 testes do core (não commitados).
- `wtA/kernel/src/audit.rs` e edições em `main.rs`, `allocator.rs`, `sched.rs`: experimentos no kernel,
  selecionados por `AUDIT_EXP=pad,hold,stack,oom,ovf,triple,leak,wstack` (e `AUDIT_SMALL=1`) em tempo de build.
- Logs: `runA`, `runA2`, `runOOM`, `runOVF`, `runOVF2`, `runTriple` (+ `runTriple.qemu.log`), `runSmall`,
  `runLeak`, `runW`, `runGPU`, `runGPUuefi`; imagens em `imgs/`.
- Cliques via monitor QEMU: `drive.sh`.

Não verificado: Miri (componente ausente), TLS real (sem rede), resolução maior que 1280x720 em UEFI
(o bootloader não entregou framebuffer com resolução forçada), execução dos testes em modo release.

## 7. Fora do escopo, para outro agente

- `desktop/mod.rs:202-215`: se `ata::read_image` falhar por timeout transitório, o código chama `fs::format` e
  `write_image`, sobrescrevendo um sistema de arquivos válido (perda de dados). SUPOSIÇÃO, não reproduzido.
- `netstack.rs`: TLS com `UnsecureProvider` (sem verificação de certificado) e RNG xorshift semeado por TSC; o
  próprio código avisa que é demo.
- Se não houver NIC (`net_up == false`), `fetch::try_post` ainda é chamado e deixa `STATE = REQUESTED` para
  sempre (o worker não é criado): o navegador ficaria em "Carregando". SUPOSIÇÃO por leitura (`main.rs:243-248, 606-611`).
