> *Written when the project was called OSjeff (renamed Kitsune in 2026-10; names and paths below are the historical ones).*

# OSjeff — Mapa técnico do código (auditoria 00)

Branch `audit/performance-security`. Escopo: leitura do código-fonte de `kernel/`, `osjeff_core/`, `os/`, `wasm-apps/`, `tools/` e das crates do bootloader 0.11.17 em `~/.cargo/registry`. Nada em `kernel/`, `osjeff_core/` ou `os/` foi alterado.

Convenções:

- Caminho relativo à raiz do repositório é `arquivo:linha`.
- Código do bootloader aparece como `bl-common/…`, `bl-uefi/…`, `bl-bios-stage-N/…`. Isso abrevia `~/.cargo/registry/src/*/bootloader-x86_64-<common|uefi|bios-stage-N>-0.11.17/src/…`.
- "Não verificado" significa que não foi confirmado por leitura nem por execução.
- Fatos medidos: os do enunciado (BIOS 128 MB boota, UEFI 128 MB panica, UEFI 512 MB boota) mais uma execução BIOS headless feita nesta auditoria (seção 2.6).
- `README.md` e `docs/ARCHITECTURE.md` não foram usados como fonte. As divergências estão na seção 9.

Tamanho do código (`wc -l`):

| Parte | Linhas | Observação |
|---|---|---|
| `kernel/src/**/*.rs` | 8.462 | mais `switch.s` (55) |
| `osjeff_core/src/**/*.rs` | 6.179 | 189 `#[test]` |
| `os/` | 67 | `os/build.rs` 20 + `os/src/main.rs` 47 |

O kernel não tem nenhum teste: `kernel/Cargo.toml:41` define `test = false` e não há `#[test]` em `kernel/`.

---

## 1. Fluxo de boot: do bootloader ao primeiro frame do desktop

### 1.1 Build da imagem

1. `kernel/build.rs` gera `demo.wasm`, `app.wasm` e `doom1.wad` em `OUT_DIR` (`kernel/build.rs:15-57`). O kernel os embute com `include_bytes!` (`kernel/src/wasm/mod.rs:28-33`).
2. `os/build.rs:5` recebe o ELF do kernel como artifact-dependency (`os/Cargo.toml:11-14`, `.cargo/config.toml:3` com `bindeps = true`).
3. `os/build.rs:9-17` gera `osjeff-uefi.img` e `osjeff-bios.img` com `bootloader::UefiBoot` e `bootloader::BiosBoot`.
4. `os/src/main.rs` só lança o QEMU.

### 1.2 Bootloader (crate `bootloader` 0.11.17)

O kernel compila contra `bootloader_api` **0.11.15** (`Cargo.lock`), enquanto `os` usa o `bootloader` **0.11.17**. É compatível: a config é validada em `bl-common/lib.rs:102-111` e o boot foi medido OK.

**Caminho BIOS:**

| Etapa | Onde | O que faz |
|---|---|---|
| boot sector | `bl-bios-boot-sector/main.rs:29` `first_stage` | lê a partição do stage 2 |
| stage-2 | `bl-bios-stage-2/main.rs` | unreal mode (`:46`); carrega o ELF do kernel em `0x0100_0000` (`:32`, `:105`); lê o memory map e-820 (`:128`); **escolhe VESA com no máximo 1280x720** (`:131-147`, valor hardcoded no bootloader); entra em modo protegido → stage-3 (`:182`) |
| stage-3 | `bl-bios-stage-3/main.rs` `_start` | `paging::init` (identity map), GDT de long mode, salta ao stage-4 |
| stage-4 | `bl-bios-stage-4/main.rs:171` | chama `load_and_switch_to_kernel` |

**Caminho UEFI:**

| Etapa | Onde |
|---|---|
| entrada | `bl-uefi/main.rs:55` `main` |
| framebuffer | usa o **modo GOP corrente** (`:348-393`); só troca de modo se a config pedir mínimos (`:352-379`) |
| `exit_boot_services` | `:117` |
| `load_and_switch_to_kernel` | `:154` |

**Comum aos dois caminhos** (`bl-common/lib.rs`):

| Passo | Linha |
|---|---|
| `set_up_mappings` | `:171` |
| habilita NXE | `:195` |
| habilita CR0.WP | `:197` |
| `load_kernel` (mapeia segmentos; flags NX/W vêm do ELF, `bl-common/load_kernel.rs:176-181`) | `:203` |
| pilha do kernel: 80 KiB + 1 página de guarda | `:212-235` |
| GDT com 2 descritores (código e dado de kernel) | `:264-283`, `bl-common/gdt.rs:17-19` |
| mapeia o framebuffer | `:286-317` |
| mapeia **toda** a memória física em `physical_memory_offset` com páginas de 2 MiB | `:351-379` |
| `create_boot_info` | `:~460-589` |
| `switch_to_kernel` → `context_switch` (`mov cr3`, `mov rsp`, `push 0`, `jmp entry`, `rdi=&BootInfo`) | `:592`, `:633-648` |

### 1.3 Kernel (`kernel/src/main.rs`)

| # | Etapa | Local |
|---|---|---|
| 0 | `entry_point!(kernel_main, config=&BOOT_CONFIG)` | `main.rs:56`; `BOOT_CONFIG` pede `physical_memory = Dynamic` em `:50-54` |
| 1 | Serial COM1 (38400 8N1) | `main.rs:88`, `serial.rs:12-21` |
| 2 | Lê `physical_memory_offset` e pega o framebuffer; sem framebuffer → `halt()` | `main.rs:93-102` |
| 3 | Zera o framebuffer (apaga o log do bootloader); `n = min(fb.len, MAX_BYTES)` | `main.rs:103-109` |
| 4 | Fatia as estáticas BACK/BG/STATIC em `&mut [u8]` | `main.rs:111-114` |
| 5 | Heap: `ALLOCATOR.init(HEAP, 64 MiB)` + smoke test | `main.rs:117-120`, `allocator.rs` |
| 6 | **Demo WASM no boot** (`wasm::run_demo`), ainda na pilha de boot, sem IRQ | `main.rs:125-126`, `wasm/mod.rs:240-245` |
| 7 | Enumera o PCI (`0xCF8/0xCFC`); se achar virtio-gpu, negocia, cria a fila de controle (DMA), faz `get_display_info` e `verify_2d`, **sem trocar o scanout** | `main.rs:131-198` |
| 8 | `ata::detect_and_log` (IDENTIFY nos 2 canais IDE) | `main.rs:207` |
| 9 | `sched::init`: registra o contexto de boot como thread 0 "compositor" | `main.rs:209`, `sched.rs:91-103` |
| 10 | `ps2::init` (por polling, antes das IRQs) | `main.rs:214` |
| 11 | `interrupts::init`: IDT, `lidt`, remap do PIC, PIT a 250 Hz, `sti` | `main.rs:218`, `interrupts.rs:38-58`, `:168-194` |
| 12 | Calibra o TSC contra o PIT (25 ticks, ~100 ms) | `main.rs:222`, `perf.rs:12-23` |
| 13 | NE2000 (ISA `0x300`), DHCP (2 esperas de ~300 ms) e ARP gratuito | `main.rs:230-236`, `:709-745` |
| 14 | Se há NIC: `fetch::init` + `sched::spawn("fetcher")`, com IRQs desligadas no `spawn` | `main.rs:243-248` |
| 15 | `wasm::init` + `sched::spawn("wasmapp")` (**sempre** spawnada, mesmo sem janela WASM) | `main.rs:253-256` |
| 16 | **Splash de no mínimo 5 s** (laço ocupado, medido pela RTC) | `main.rs:259`, `:648-675` (`el >= 5` em `:671`) |
| 17 | Pinta o wallpaper em BG | `main.rs:262-265`, `desktop/widgets.rs:197` |
| 18 | `Desktop::new` cria 3 processos, lê ou formata o FS no disco ATA e abre a janela do terminal com `Anim::open()` | `main.rs:267`, `desktop/mod.rs:189-262` (anim em `:225`) |
| 19 | Loop do compositor | `main.rs:294-639` |

**Primeiro frame do desktop.** Como o terminal nasce com animação (`desktop/mod.rs:225`), `any_anim` é verdadeiro e o primeiro frame sai pelo ramo de animação:

1. Compõe a camada estática em STATIC e copia para BACK (`main.rs:358-361`).
2. Desenha a janela animada (`main.rs:367`).
3. Faz a cópia **integral** BACK→framebuffer (`main.rs:368`).
4. Desenha o cursor (`main.rs:369-372`).

O seed `last_tick = interrupts::ticks()` (`main.rs:274`) evita que o delta enorme do splash pule a animação.

**Antes do primeiro frame** o tempo mínimo é: splash ≥ 5 s + TSC ~0,1 s + DHCP (até ~0,6 s) + a demo WASM. Em QEMU/TCG sem KVM o desktop apareceu em menos de 14 s (execução desta auditoria, seção 2.6).

**Preempção.** O timer (vetor 32) entra por `timer_isr` em `switch.s:21-55`:

1. Faz push de 15 GPRs.
2. Chama `timer_schedule` (`interrupts.rs:124-129`) e depois `sched::switch_current` (`sched.rs:151-198`).
3. Troca `rsp`, faz `pop` e `iretq`.
4. O `fxsave/fxrstor` fica em `sched.rs:168-195`.

Os três contextos em operação:

| Thread | Entrada | Observação |
|---|---|---|
| compositor | `kernel_main` | thread 0 |
| fetcher | `fetch::worker` | só se houver NIC |
| wasmapp | `wasm::worker` | sempre |

---

## 2. Layout de memória

### 2.1 Mapeamento do bootloader

- `BootloaderConfig` (`main.rs:50-54`) só muda `mappings.physical_memory = Some(Mapping::Dynamic)`. Todo o resto é o default de `bootloader_api` (`bootloader_api-0.11.15/src/config.rs:52-59, 430-443`):
  - `kernel_stack_size = 80 KiB`.
  - `kernel_base`, `kernel_stack`, `boot_info` e `framebuffer` em `Dynamic`.
  - `aslr = false`, sem recursive page table, sem ramdisk.
- Toda a RAM física (`0..max_phys`) fica mapeada em um offset dinâmico, com páginas de 2 MiB, flags `PRESENT|WRITABLE|NO_EXECUTE` (`bl-common/lib.rs:351-379`).
  - O kernel usa esse offset para acessar BARs MMIO e para percorrer as page tables (`virtio.rs:15-26`, `virtio_gpu.rs:72`, `:294`).
  - Valores medidos em BIOS/QEMU: `physical_memory_offset = 0x28000000000`, kernel (PIE) deslocado de `0x10000000000`, entry `0x10000083f40`.
- O ELF do kernel é `DYN` (PIE) com 4 `LOAD` (`readelf -lW`):

| Segmento | Flags | Observação |
|---|---|---|
| `0x0000..0x237b5` | R | |
| `.text` | R E | |
| `.data.rel.ro/.got` | RW | |
| `.data+.bss` | RW | `FileSiz 0x1351`, `MemSiz 0x5b18318` |

  O mapeamento é W^X: NX em tudo que não é executável (`bl-common/load_kernel.rs:176-181`).
- O kernel **nunca lê** `boot_info.memory_regions`, `kernel_stack_*` nem `rsdp_addr`. Não há alocador de frames físicos no kernel: toda memória dinâmica é o heap estático de 64 MiB.

### 2.2 De onde vêm os 0x5b18318 bytes de BSS

`0x5b18318` são **95.519.512 B = 91,09 MiB** (23.320 páginas). Comando reproduzível (ELF release em `target/x86_64-unknown-none/release/build/kernel/*/artifact/bin/kernel`):

```sh
readelf -lW $K | grep LOAD
nm -S -C $K | awk '$3 ~ /^[bB]$/' | sort -k2 -r | head
```

| Símbolo `.bss` | Bytes | % | Origem |
|---|---|---|---|
| `kernel::HEAP` | 67.108.864 (64 MiB) | 70,3 | `main.rs:79-80` (`HEAP_SIZE = 64 * 1024 * 1024`) |
| `BACK`, `BG`, `STATIC` (3 × 8.294.400) | 24.883.200 | 26,1 | `main.rs:60-73`; 1920·1080·4 cada, alinhados a 64 B |
| `wasm::SURFACE` | 2.291.904 | 2,4 | `wasm/mod.rs:326-333`; 2 × 692·414·4 |
| `desktop::SCRATCH` | 1.126.400 | 1,2 | `desktop/mod.rs:39-42`; 640·440·4 |
| `desktop::DISK` | 50.688 | | `desktop/mod.rs:48` |
| `netstack::TLS_RX/TLS_TX` | 2 × 16.384 | | `netstack.rs:294-296` |
| `virtio_gpu::{QUEUE_MEM,CMD_MEM,BACKING}` | 3 × 4.096 | | `virtio_gpu.rs:45-47` |
| resto (EVENTS 2 KiB, RING 1 KiB, `.data`, alinhamento) | ~13.400 | | |

A imagem em disco tem só 4,6 MiB (`osjeff-bios.img` 4.686.848 B), mas o **bootloader materializa o BSS inteiro**. Em `bl-common/load_kernel.rs:283-308` ele aloca um frame físico por página e o zera uma a uma. Por isso o custo é RAM física, não tamanho do arquivo.

### 2.3 Heap

- Estático em BSS (`main.rs:80`, `RacyCell<[u8; 64 MiB]>`), entregue ao `LockedHeap` em `main.rs:118`.
- `LockedHeap` é uma free-list ordenada por endereço com coalescência (`allocator.rs:78-228`) atrás de um `SpinLock` que desliga as IRQs (`allocator.rs:32-48`).
- `alloc` é first-fit O(n) e `dealloc` insere de forma ordenada em O(n). Não há `realloc` nem `alloc_zeroed` próprios (usam os defaults).
- Falha de alocação devolve null → panic → `halt()`.
- O HUD chama `free_bytes()` (percorre a lista com IRQs desligadas) a cada 25 ticks (`main.rs:584-586`).
- Nenhum ISR aloca (`timer_schedule` e `switch_current` não usam `alloc`).

### 2.4 Buffers

| Buffer | Onde | Tamanho | Uso |
|---|---|---|---|
| BACK | `main.rs:69` | 8.294.400 | alvo de composição |
| BG | `main.rs:70` | 8.294.400 | wallpaper em cache |
| STATIC | `main.rs:73` | 8.294.400 | "tudo menos a janela animando" |
| framebuffer real | memória do bootloader (mapeada em `bl-common/lib.rs:286-317`) | 1280·720·3 = 2.764.800 em BIOS/QEMU (medido) | destino do blit |

O resumo da composição é: `BG→BACK`, desenha, copia `BACK→FB` por retângulos (`main.rs:477-500`).

**Atenção a dois pontos:**

- **BIOS usa 24 bpp.** O stage-2 do bootloader limita a 1280x720 (`bl-bios-stage-2/main.rs:132-133`). A execução desta auditoria registrou `bytes_per_pixel: 3, stride: 1280`. O caminho rápido de 32 bits de `fb.rs:165` (`if bpp == 4`) **não é usado** nesse modo e cai no laço de 3 bytes (`fb.rs:199-207`). Os 3 buffers de 1920x1080x4 são então grandes demais: usam ~2,8 MB cada de 8,3 MB.
- **UEFI com resolução maior que 1080p quebra.** O UEFI mantém o modo GOP corrente (`bl-uefi/main.rs:380`). `n = min(fb.len, MAX_BYTES)` (`main.rs:104`) trunca os buffers, mas `info` mantém a largura e a altura reais. O `Canvas` indexa além de `n` (`fb.rs:170`, `:202`) → panic → tela congelada. Inferido pela leitura, **não executado**.

### 2.5 Pilhas

| Pilha | Onde nasce | Tamanho | Proteção |
|---|---|---|---|
| **Inicial do kernel** (thread 0 "compositor") | bootloader: frames físicos alocados + VA dinâmica (`bl-common/lib.rs:212-235`); `rsp = top` alinhado a 16 B (`:422`, `:638`) | **80 KiB** (default, `config.rs:54`) | 1 página de guarda abaixo, NX. Sem IST: um overflow vira #PF, depois #DF, depois triple fault (reset), sem mensagem |
| **fetcher** | heap do kernel, `vec![0u8; STACK_SIZE]` (`sched.rs:111`) | **128 KiB** (`sched.rs:19`) | canário de 8 B no endereço mais baixo (`sched.rs:28`, `:113`), checado só **na preempção** (`sched.rs:160`). Sem página de guarda: um overflow corrompe o heap antes de ser detectado |
| **wasmapp** | idem | **128 KiB** | idem |
| thread 0 | `sched.rs:91-97` | usa a pilha do bootloader | `stack_intact()` devolve `true` (`sched.rs:75-79`): não é checada |

- `MAX_THREADS = 8` (`sched.rs:20`).
- Uso máximo de pilha: não verificado (sem medição).
- O parse do módulo wasm roda em 128 KiB (`Module::new` no worker, `wasm/mod.rs:278`). Para o DOOM isso não foi verificado.

### 2.6 RAM mínima

**Conta:** BSS 91,09 MiB + ELF ~1,8 MiB + pilha 80 KiB + page tables (~0,2 MiB) ≈ **93,5 MiB de frames alocáveis acima de 1 MiB** (o allocator pula o 1º MiB, `bl-common/legacy_memory_region.rs:55,71`).

- **BIOS (e-820):** 128 MiB sobra. Medido: boota e entrega o desktop em menos de 14 s sob QEMU/TCG sem KVM (execução desta auditoria: `tools/qemu-headless.sh bios … 14`). O log serial mostra `Mapping bss section`, `Jumping to kernel entry`, `TSC calibrated`, e a captura mostra o desktop com HUD `thr 3`.
- **UEFI:** o `LegacyFrameAllocator` só aloca de regiões `CONVENTIONAL` (`bl-uefi/memory_descriptor.rs:20-24`, `bl-common/legacy_memory_region.rs:92-111`). `LOADER_*` e `BOOT_SERVICES_*` só viram usáveis **depois** do boot (`bl-uefi/memory_descriptor.rs:27-37`). O OVMF consome parte dos 128 MiB. Isso é consistente com o panic medido em `bl-common/load_kernel.rs:285` (`allocate_frame().unwrap()` ao mapear o BSS). A causa exata **não foi verificada por mim**.
- **Mínimos práticos:**
  - BIOS 128 MiB (medido); o teórico fica em ~100 MiB.
  - UEFI 512 MiB (medido).
  - UEFI 256 MiB: **não medido**.
  - Os 64 MiB de heap são fixos: o excedente de RAM física não é usado por nada, pois não há alocador de frames.
- **Configs divergentes:**
  - `os/src/main.rs:20` usa `-m 128M` com o comentário "plenty" (`:19`). Isso só serve para BIOS; com `uefi` (`os/src/main.rs:11`) **panica**.
  - `run.ps1:89-90` usa 256M (512M com `-Doom`) e sempre a imagem BIOS.
  - `tools/qemu-headless.sh:33` usa `QEMU_MEM` default 128M também para uefi.
- Reduzir o BSS é a alavanca óbvia: o heap de 64 MiB e os 3 buffers de 1080p (24 MiB) são 96% dele.

---

## 3. Modelo de privilégio

- **Tudo em ring 0.** Não existe ring 3 em lugar nenhum:
  - Busca por `syscall|sysret|swapgs|iretq` em `kernel/`: o único `iretq` é o de `switch.s:55`, que usa CS/SS lidos do contexto atual (`sched.rs:120-121`).
  - A GDT do bootloader só tem código e dado de kernel (`bl-common/gdt.rs:18-19`).
  - Nenhuma página com `USER_ACCESSIBLE` (`grep -rn USER_ACCESSIBLE` no bootloader-common e em `kernel/` não acha nada).
  - O "isolamento" do WASM é só do interpretador (seção 7), não do hardware.
- **GDT:** a do bootloader (2 descritores), em frame identity-mapped (`bl-common/lib.rs:264-283`). O kernel **não cria GDT, TSS nem IST** (`grep -i "gdt|tss|ist"` em `kernel/` e `osjeff_core/`: só falsos positivos).
  - Consequência: `double_fault` (`interrupts.rs:147`) roda na pilha corrente. Estouro de pilha → triple fault.
  - O `#PF`, `#GP` e `#DF` apenas fazem `hlt` em laço (`interrupts.rs:147-163`), sem log, sem CR2, sem RIP.
  - O `panic_handler` (`main.rs:777-780`) também só faz `halt()`: **nenhuma mensagem de panic chega à serial.**
- **CR3:** o do bootloader. O kernel **só lê** CR3 (`virtio.rs:18`, `Cr3::read`) e nunca cria, troca nem altera page tables. Não trata page fault (halt). Não toca CR0/CR4/EFER; o bootloader já ligou NXE e WP (`bl-common/lib.rs:195-197`).
- A memória física inteira está mapeada RW+NX no espaço do kernel (`bl-common/lib.rs:363-374`).
- **Interrupções:** IDT estática (`interrupts.rs:36`) com 4 handlers de exceção (`#BP`, `#GP`, `#PF`, `#DF`), timer (vetor 32, `switch.s`), teclado (33) e mouse (44). PIC 8259 remapeado para 0x20/0x28 (`interrupts.rs:168-185`), máscaras `0xF8`/`0xEF`. Não há APIC nem SMP. Não há handler para outras exceções (`#NP`, `#SS`, `#UD`, `#DE`…).
- **SSE desligado.** Target `x86_64-unknown-none` com `-sse,+soft-float` (`rustc --print target-spec-json`). `f32` é emulado em software (usado em `desktop/mod.rs`, `boot.rs`, `main.rs`). O `fxsave/fxrstor` em `sched.rs:168-195` salva um estado que o código do kernel nunca usa.

---

## 4. Inventário de `unsafe`

Comando reprodutível (rodar na raiz; `python3 -I` para isolar o ambiente). O script abaixo remove comentários e strings, classifica cada `unsafe` (bloco, `fn`, `impl`, `extern`, `trait`, atributo) e testa se há um comentário `SAFETY:` na mesma linha, nas 3 anteriores ou na seguinte:

```sh
python3 -I unsafe_inv.py "$PWD" kernel osjeff_core os
# conferência independente (só contagem por arquivo, ignora comentários de linha):
grep -rnw unsafe kernel os --include=*.rs | grep -v '^[^:]*:[0-9]*:\s*//' \
  | awk -F: '{c[$1]++} END{for(f in c) print c[f], f}' | sort -rn
```

<details><summary>unsafe_inv.py (cole em um arquivo para reproduzir)</summary>

```python
#!/usr/bin/env python3
import re, sys, os, collections
root = sys.argv[1]; dirs = sys.argv[2:]
KINDS = ['block','fn','impl','extern','trait','attr']
def strip(src):
    out=[]; i=0; n=len(src)
    while i<n:
        c=src[i]
        if src.startswith('//',i):
            j=src.find('\n',i); j=n if j<0 else j
            out.append(' '*(j-i)); i=j
        elif src.startswith('/*',i):
            d=1; j=i+2
            while j<n and d:
                if src.startswith('/*',j): d+=1; j+=2
                elif src.startswith('*/',j): d-=1; j+=2
                else: j+=1
            out.append(re.sub(r'[^\n]',' ',src[i:j])); i=j
        elif c=='"':
            j=i+1
            while j<n and src[j]!='"':
                j+= 2 if src[j]=='\\' else 1
            out.append(re.sub(r'[^\n]',' ',src[i:j+1])); i=j+1
        else:
            out.append(c); i+=1
    return ''.join(out)
rows=collections.OrderedDict(); tot=collections.Counter(); files=[]
for d in dirs:
    for dp,_,fs in os.walk(os.path.join(root,d)):
        if '/target' in dp: continue
        for f in fs:
            if f.endswith('.rs'): files.append(os.path.join(dp,f))
for p in sorted(files):
    raw=open(p,encoding='utf8').read(); code=strip(raw); rl=raw.split('\n')
    cnt=collections.Counter(); withs=0; tot_f=0
    for m in re.finditer(r'\bunsafe\b',code):
        rest=code[m.end():m.end()+60]; before=code[max(0,m.start()-3):m.start()]
        if re.match(r'\s*\(',rest) and before.endswith('#['): k='attr'
        elif re.match(r'\s*(const\s+)?(extern\s*"[^"]*"\s*)?fn\b',rest): k='fn'
        elif re.match(r'\s*impl\b',rest): k='impl'
        elif re.match(r'\s*extern\b',rest): k='extern'
        elif re.match(r'\s*trait\b',rest): k='trait'
        else: k='block'
        line=code.count('\n',0,m.start())
        has=any('SAFETY:' in l for l in rl[max(0,line-3):line+2])
        cnt[k]+=1; tot_f+=1; withs+=has
    if tot_f:
        rows[os.path.relpath(p,root)]=(cnt,tot_f,withs)
        for k in KINDS: tot[k]+=cnt[k]
        tot['total']+=tot_f; tot['safety']+=withs
print('| arquivo | block | fn | impl | extern | trait | attr | total | com SAFETY: | sem |')
print('|---|---|---|---|---|---|---|---|---|---|')
for f,(c,t,w) in sorted(rows.items(), key=lambda kv:-kv[1][1]):
    print(f'| {f} | '+' | '.join(str(c[k]) for k in KINDS)+f' | {t} | {w} | {t-w} |')
print('| **TOTAL** | '+' | '.join(str(tot[k]) for k in KINDS)+f' | {tot["total"]} | {tot["safety"]} | {tot["total"]-tot["safety"]} |')
```

</details>

Resultado (o `grep` independente também dá 116 ocorrências):

| arquivo | block | fn | impl | extern | total | com `SAFETY:` | sem |
|---|---|---|---|---|---|---|---|
| `kernel/src/virtio.rs` | 21 | 8 | 0 | 0 | 29 | 0 | 29 |
| `kernel/src/allocator.rs` | 8 | 6 | 2 | 0 | 16 | 0 | 16 |
| `kernel/src/virtio_gpu.rs` | 16 | 0 | 0 | 0 | 16 | 0 | 16 |
| `kernel/src/sched.rs` | 9 | 0 | 0 | 0 | 9 | 0 | 9 |
| `kernel/src/wasm/mod.rs` | 8 | 0 | 0 | 0 | 8 | 2 | 6 |
| `kernel/src/io.rs` | 7 | 0 | 0 | 0 | 7 | 0 | 7 |
| `kernel/src/fetch.rs` | 6 | 0 | 0 | 0 | 6 | 0 | 6 |
| `kernel/src/interrupts.rs` | 3 | 0 | 1 | 1 | 5 | 0 | 5 |
| `kernel/src/main.rs` | 5 | 0 | 0 | 0 | 5 | 0 | 5 |
| `kernel/src/ne2000.rs` | 4 | 0 | 0 | 0 | 4 | 0 | 4 |
| `kernel/src/desktop/widgets.rs` | 2 | 0 | 0 | 0 | 2 | 0 | 2 |
| `kernel/src/netstack.rs` | 2 | 0 | 0 | 0 | 2 | 0 | 2 |
| `kernel/src/ps2.rs` | 2 | 0 | 0 | 0 | 2 | 0 | 2 |
| `kernel/src/desktop/mod.rs` | 1 | 0 | 0 | 0 | 1 | 0 | 1 |
| `kernel/src/fb.rs` | 1 | 0 | 0 | 0 | 1 | 0 | 1 |
| `kernel/src/perf.rs` | 1 | 0 | 0 | 0 | 1 | 0 | 1 |
| `kernel/src/power.rs` | 1 | 0 | 0 | 0 | 1 | 0 | 1 |
| `kernel/src/sync.rs` | 0 | 0 | 1 | 0 | 1 | 0 | 1 |
| **TOTAL** | **97** | **14** | **4** | **1** | **116** | **2** | **114** |

(Não há `unsafe trait` nem `#[unsafe(...)]` no kernel.)

- `osjeff_core` tem **0** `unsafe`: `#![forbid(unsafe_code)]` em `osjeff_core/src/lib.rs:8`. `os/` também tem 0.
- Só 2 de 116 têm `// SAFETY:` no padrão exigido: `wasm/mod.rs:79` (bloco em `canvas`) e `wasm/mod.rs:308` (doc de `app_mut`).
  - A segunda está **desatualizada**: afirma "compositor thread only", mas `app_mut` só é chamado pelo worker (`wasm/mod.rs:452`).
- Com critério frouxo (`Safe:`, `# Safety`, "Safe in this kernel"), o script alternativo dá 9/116, restando **107 sem qualquer justificativa**:
  - `allocator.rs:20` (`impl Sync SpinLock`).
  - `interrupts.rs:70` (`impl Sync InputRing`).
  - `sync.rs:22-24` (`impl Sync RacyCell`).
  - Docs `# Safety` em `allocator.rs:115,237` e `virtio.rs:78`.
- **Total sem `SAFETY:` = 114 de 116.** Sem nenhuma justificativa = 107.

**Natureza dos `unsafe`:**

| Natureza | Arquivos | Exemplos |
|---|---|---|
| port I/O em `asm!` | `io.rs`, `power.rs:28` | wrappers `pub fn` **seguros** para `in/out`: qualquer código chama `outb(0xCF9,…)` sem `unsafe` |
| MMIO e DMA virtio | `virtio.rs` (29), `virtio_gpu.rs` (16) | `read_volatile`/`write_volatile` sobre `base + offset` vindo do PCI sem validar `offset` contra `length` |
| estado global via `RacyCell` | `sched.rs`, `fetch.rs`, `wasm/mod.rs`, `ps2.rs`, `ne2000.rs`, `desktop/mod.rs:51`, `widgets.rs:291`, `netstack.rs:235-237`, `main.rs:111-114` | `*SCHED.get()`, `&mut *DECODER.get()` |
| allocator | `allocator.rs` | ponteiros crus na free-list |
| asm | `sched.rs:43,168,190` (`fxsave/fxrstor`), `interrupts.rs:111` (`global_asm!`) | |
| outros | `fb.rs:171` (`align_to_mut`), `perf.rs:113` (`from_utf8_unchecked`) | |

Dois padrões de risco de **soundness** (funções seguras que entregam `&'static mut` a estáticas):

- `desktop/mod.rs:50` `disk()` e `desktop/widgets.rs:290` `scratch_slice()` são `fn` seguras que devolvem `&'static mut [u8]` (aliasing livre).
- `sched.rs:220` `scheduler()` idem, e `sched::spawn` é `pub fn` segura que exige "IRQs desligadas" só por convenção (`main.rs:245,254`).

---

## 5. Inventário de estáticos globais mutáveis

- `static mut`: **0** em `kernel/`, `osjeff_core/` e `os/`. O padrão foi trocado por `RacyCell<T>`: `UnsafeCell` + `unsafe impl Sync` (`sync.rs:21-25`, contrato de "single-core + exclusão por IF" nos comentários de `sync.rs:1-13`).
- Fora do kernel, `wasm-apps/snake/src/lib.rs:38-49` (12) e `wasm-apps/plasma/src/lib.rs:36-38` (3) usam `static mut`. É memória do guest, sem impacto no kernel.
- Total no kernel: **23 `RacyCell`**, 2 `UnsafeCell` embutidos (`InputRing`, `SpinLock`) e 10 atômicos. O que cada thread ou IRQ toca:

Legenda: **T** = thread normal; **IRQ** = rotina de interrupção; **C** = compositor (thread 0); **F** = fetcher; **W** = wasmapp.

| Estático | Local | Tipo / tamanho | Quem acessa | Sincronização |
|---|---|---|---|---|
| `BACK`, `BG`, `STATIC` | `main.rs:69,70,73` | `RacyCell<AlignedBuf>` 8,3 MB | só C | `&mut` criados uma vez (`main.rs:111-114`) e vivos até o fim |
| `HEAP` | `main.rs:80` | `RacyCell<[u8;64MiB]>` | allocator (C, F, W) | spinlock + IRQ off (`allocator.rs:32-48`) |
| `ALLOCATOR` | `main.rs:83` | `SpinLock<…>` | C, F, W (**nunca IRQ**) | idem |
| `IDT` | `interrupts.rs:36` | `RacyCell<InterruptDescriptorTable>` | escrito 1× por C antes de `sti`; lido pela CPU | ordem de boot |
| `TICKS` (interrupts) | `interrupts.rs:17` | `AtomicU64` | **IRQ** escreve; todos leem | atômico |
| `RING` | `interrupts.rs:74` | `UnsafeCell<[u16;512]>` + head/tail | **IRQ** (kbd/mouse) produz; C consome | SPSC acquire/release; ISR com IF=0 |
| `SCHED` | `sched.rs:88` | `RacyCell<Option<Scheduler>>` | **ambos**: IRQ muta `rsp/current/fpu` (`sched.rs:151-198`); C faz `init`/`spawn` (IRQ off) e lê `thread_count/name` (`:202-218`) **sem** IRQ off | convenção; `spawn` com `without_interrupts` em `main.rs:245,254` |
| `CURRENT`, `TICKS[8]` (sched) | `sched.rs:54,56` | atômicos | **IRQ** escreve; C lê (Task Manager) | atômico |
| `DECODER` | `ps2.rs:90` | `RacyCell<DecoderState>` | só C (`ps2::poll`) | nenhuma necessária (não é IRQ) |
| `NEXT` (ne2000) | `ne2000.rs:64` | `RacyCell<u8>` | **C** (DHCP, ARP, laço principal `main.rs:623-631`) e **F** (`Nic::receive`) | **só por protocolo**: C só usa a NIC se `fetch::is_idle()` |
| `STATE` | `fetch.rs:28` | `AtomicU8` | C e F | acquire/release |
| `NET`, `REQ_URL`, `REQ_LEN`, `RESULT` | `fetch.rs:29-32` | `RacyCell` | C produz o pedido, F consome e devolve | máquina de estados em `STATE` (`fetch.rs:9`) |
| `TLS_RX`, `TLS_TX` | `netstack.rs:295-296` | `RacyCell<[u8;16 KiB]>` | só F (`netstack.rs:235-237`) | exclusividade por fluxo |
| `SCRATCH`, `DISK` | `desktop/mod.rs:42,48` | `RacyCell` 1,1 MB / 50 KB | só C (via `scratch_slice`, `disk()`) | — |
| `QUEUE_MEM`, `CMD_MEM`, `BACKING` | `virtio_gpu.rs:45-47` | `RacyCell<DmaPage>` | C (boot) **e o dispositivo via DMA** | polling no boot; depois sem uso |
| `APP` | `wasm/mod.rs:273` | `RacyCell<Option<App>>` | só W (`app_mut`, `:452`) | — |
| `SURFACE` | `wasm/mod.rs:333` | `RacyCell<[[u8;S];2]>` | W escreve `back`; C lê `front` (`blit_surface`, `:510`) | duplo buffer + `FRONT`/`READY` (sem lock: pode rasgar o frame se C for preemptado) |
| `FRONT`, `READY`, `ACTIVE` | `wasm/mod.rs:334-337` | atômicos | C e W | acquire/release |
| `FB_INFO` | `wasm/mod.rs:339` | `RacyCell<Option<FrameBufferInfo>>` | C escreve 1× antes do spawn; W lê | ordem de boot |
| `EVENTS`, `EV_HEAD`, `EV_TAIL` | `wasm/mod.rs:350-359` | ring de 128 | C produz; W consome | SPSC acquire/release |
| `LOGGED` | `wasm/mod.rs:472` | `AtomicBool` (local à função) | W | — |

Estáticos somente leitura (não mutáveis): `BOOT_CONFIG` (`main.rs:50`), `DEMO_WASM`, `APP_WASM`, `WAD` (`wasm/mod.rs:28-33`).

Pontos para a próxima fase:

- Nenhum estático mutável é compartilhado entre **IRQ e thread** sem atômico ou SPSC, exceto `SCHED` (ambos). Os acessos a `SCHED` em threads são só leitura de `name`/`len`, seguros apenas porque quem faz `spawn` é a mesma thread C.
- A exclusão mútua da NIC (`NEXT` + registradores) depende de um protocolo, não de um lock (`main.rs:623`, `fetch.rs:12-14`).
- O comentário em `wasm/mod.rs:40-42` ("o contexto de boot/desktop é single-threaded") está desatualizado: o app roda em outra thread desde o commit `33b5908`.

---

## 6. O que mora em `kernel/` e poderia ir para `osjeff_core` (testável no host)

`osjeff_core` hoje: 6.179 linhas, 189 testes. O kernel: 8.462 linhas, 0 testes. O próprio `osjeff_core/src/lib.rs:1-5` diz que "toda a lógica de decisão" mora no core, o que **não é verdade** para `desktop/`. Estimativa (linhas aproximadas, sem refatorar nada):

**A. Rasterização (sobre `&mut [u8]` + `FrameBufferInfo`)**

Depende só de `bootloader_api::info::{FrameBufferInfo, PixelFormat}` (`fb.rs:3`), que dá para espelhar em um struct próprio. Testável com buffers e hashes.

| Arquivo | Linhas |
|---|---|
| `fb.rs` (`Canvas`, `fill_rect`, `blend_pixel`, `fill_round_rect[_alpha]`, `isqrt`, `lerp`, `pack32`) | 381 (único `unsafe`: `:171`) |
| `font.rs` | 145 |
| `icons.rs` | 217 |
| `boot.rs` | 109 |
| `theme.rs` | 35 |
| **Subtotal** | **~890** |

**B. Gerenciador de janelas e lógica de `Desktop`**

Hoje presa a `Canvas` e a chamadas de hardware (`power::reboot`, `wasm::on_key`). Dá para inverter com um enum de comandos devolvido.

| Trecho | Linhas aprox. |
|---|---|
| `desktop/mod.rs:316-540` (foco, z-order, `open/close`, `topmost_at`, `dock_hit`, `animate`, menus, bounds) | 225 |
| `desktop/widgets.rs:7-123, 149-184` (layouts e hit-test: dock, calc, browser chrome, start panel) | 165 |
| constantes e geometria `desktop/mod.rs:22-125` | 100 |
| `desktop/input.rs:8-87` (`handle_key`) e `:261-374` (`handle_mouse`) | 195 |
| `desktop/mod.rs:592-761` + `files.rs:8-26` (seleção, cwd, lixeira do gerenciador de arquivos; hoje usa `disk()` global, bastaria receber `&mut [u8]`) | 190 |
| `desktop/render.rs:64-178` (`anim_signature`, retângulos de dano) | 110 |
| `desktop/mod.rs:553-590` (ciclo de pedido/resultado do browser) | 40 |
| **Subtotal** | **~1.025** |

**C. Drivers e utilitários com núcleo puro**

| Item | Local | Linhas aprox. |
|---|---|---|
| decodificador PS/2 (teclado e mouse) | `ps2.rs:84-166` | 55 |
| RTC: BCD, 12h para 24h, fuso | `rtc.rs:31-60` | 30 |
| parse do `IDENTIFY` ATA | `ata.rs:167-209` | 40 |
| `Perf` e formatadores (`put`, `put_u32`, `put_ms`) | `perf.rs:21-58, 118-153` | 75 |
| ring SPSC genérico (`RING` e `EVENTS`, hoje duplicados) | `interrupts.rs:62-104`, `wasm/mod.rs:341-431` | 100 → ~50 |
| quadro inicial de thread (layout do `iretq`) e round-robin | `sched.rs:107-150, 176-186` | 45 |
| laço de redirects e `resolve_redirect` | `fetch.rs:98-162` | 65 |
| RNG xorshift, montagem do GET, `deadline` | `netstack.rs:120-123, 385-424` | 55 |
| DHCP (máquina de estados, genérica sobre uma trait NIC) | `main.rs:709-745` | 37 |
| `blit_rect`, progresso do splash | `main.rs:646-707` | 45 |
| decisão de caminho de render (animação, overlay, estável) | `main.rs:294-601` | ~100 |
| clipping e escala dos host-calls | `wasm/mod.rs:94-174, 540-553` | 95 |
| lógica WASI (`is_wad_path`, `fd_seek`, cursor, xorshift) abstraída sobre uma trait de memória | `wasi.rs:52-132, 223-239` | 90 |
| duplicatas de formatação de inteiros | `files_ui.rs:17-37`, `widgets.rs:321-350` | 50 (a deduplicar) |
| **Subtotal** | | **~880** |

Total estimado: **~2.800 linhas (~33% do kernel)** candidatas, com o maior ganho em B (regras de janela e entrada) e no ring SPSC compartilhado.

**Restrições:**

- `osjeff_core` tem `#![forbid(unsafe_code)]` (`lib.rs:8`) e **nenhuma dependência** (`osjeff_core/Cargo.toml`).
  - `LinkedListAllocator` (`allocator.rs:80-228`, ~150 linhas de ponteiros crus) só poderia ser testado no host em uma crate separada com `unsafe` permitido, usando um `Vec<u8>` como arena. A matemática do alinhamento já está no core (`osjeff_core/src/heap.rs`, 14 testes).
  - Uma versão sem `unsafe` do ring SPSC exige atômicos por slot.
- `wasmi::Memory` não é de `osjeff_core`; a lógica WASI precisaria de uma trait de memória.

---

## 7. `wasm-apps/` e o motor WebAssembly

### 7.1 O que é cada subpasta e o que o build realmente embute

`kernel/build.rs` embute **sempre** dois módulos mais um arquivo de dados:

- `demo.wasm`: WAT inline (`build.rs:21-32`), executado no boot (`main.rs:126`).
- `app.wasm`: o app da janela "WASM App".
- `doom1.wad`: vazio, exceto no modo DOOM.

Quem decide `app.wasm` é o `match` de `build.rs:44-54`:

| Condição | Resultado | Linha |
|---|---|---|
| `DOOM=1` **e** `WASI_SDK_PATH` | `build_doom` | `:45` |
| só `WASI_SDK_PATH` | `build_c_app("cdemo")` | `:46-49` |
| nenhuma (**padrão**) | `build_wasm_app("snake")` | `:50-53` |

| Pasta | Conteúdo | Ligada ao kernel? | Estado |
|---|---|---|---|
| `wasm-apps/snake/` | crate `cdylib` Rust `no_std`, `wasm32-unknown-unknown` (`snake/src/lib.rs`, 210 linhas). Imports `fill_rect`, `draw_text`, `time_ms` (`:20-24`); exports `on_key`, `render` (`:130-131`, `:168-169`) | **Sim, é o padrão.** `cargo` aninhado em `build.rs:69-106`. Medido: `app.wasm` com 2.254–2.284 B em todos os `OUT_DIR` de `target/` | **Funcional** pelo wiring (ABI bate). Não executado nesta auditoria |
| `wasm-apps/plasma/` | crate Rust (`plasma/src/lib.rs`, 93 linhas); usa `blit`, exporta `on_pointer` | **Não.** Nenhum caminho de `build.rs` o seleciona: `"snake"` é literal em `:51` e "plasma" só aparece no comentário `:7` | **Órfão**: só compila se alguém editar o literal. Foi o 1º app (commit `a7f5fa2`), trocado pelo snake (`62f350e`) |
| `wasm-apps/cdemo/` | `cdemo.c` (48 linhas), C freestanding | Só com `WASI_SDK_PATH` e sem `DOOM` (`build.rs:46-49`, `:112-141`). Neste ambiente `WASI_SDK_PATH` está vazio, `~/wasi-sdk` e `/opt/wasi-sdk` não existem | **Opcional, não construído aqui** |
| `wasm-apps/doom/` | só `doomgeneric_osjeff.c` (89 linhas, camada de plataforma) | Só com `DOOM=1 + WASI_SDK_PATH` (`build.rs:45`, `:156-178`). `tools/build-doom.sh:28` **clona `github.com/ozkl/doomgeneric` da rede** (GPLv2, `.gitignore`); o `doom1.wad` **não está no repo** (`.gitignore`: `*.wad`) e `build_doom` dá `assert!` sem ele (`build.rs:168-172`) | **Em andamento / opcional**, não reproduzível do checkout puro. O commit `3e56702` afirma que roda; **não verificado** (não há WAD nem wasi-sdk aqui). O WAD embutido em todos os builds de `target/` tem 0 bytes |

Detalhes do build:

- `build_wasm_app` exige o target `wasm32-unknown-unknown`, que o `rust-toolchain.toml:7` instala. Se faltar, o build do kernel falha (`build.rs:93`).
- `wasm-apps/*/Cargo.toml` tem `[workspace]` vazio para se destacar do workspace raiz. Cada app tem seu próprio `Cargo.lock`.

### 7.2 Runtime e ABI (host-calls)

- Runtime: `wasmi` 1.1.0 (interpretador, `default-features = false`, `kernel/Cargo.toml:32`) em `kernel/src/wasm/mod.rs`. Instanciado com `Engine::default()` (`wasm/mod.rs:277`, `:249`).
- O app roda em uma thread própria (`wasm::worker`, `wasm/mod.rs:446-481`). Fica ocioso (`hlt`) se `ACTIVE` for falso (`:448-451`). Só o worker chama `app_mut()`.
- Exports que o kernel chama: `_initialize` (`:298`), `render` (`:469`), `on_key` (`:416`), `on_pointer` (`:423`); aceita as memórias exportadas `memory` ou `mem` (`:532`).

**Importações disponíveis ao guest (31):**

| Módulo | Função | Local | Efeito |
|---|---|---|---|
| `host` | `log(ptr,len)` | `mod.rs:180-189` | escreve a string UTF-8 do guest na **serial** (sem limite de tamanho) |
| `host` | `fill_rect(x,y,w,h,rgb)` | `:191-198`, `:94-109` | preenche retângulo na superfície do app, com clip ao `(cw,ch)` |
| `host` | `draw_text(x,y,ptr,len,rgb,scale)` | `:200-215`, `:114-137` | texto com clip por glifo à caixa |
| `host` | `blit(off,w,h,dx,dy)` | `:217-225`, `:143-174` | copia RGBA do guest para a superfície, com escala inteira |
| `host` | `time_ms()` | `:228-232` | `ticks()*4` |
| `wasi_snapshot_preview1` (25) | `fd_write` (fd 1/2 → serial, 512 B por iov), `fd_read/seek/tell/close` (só o fd do WAD), `fd_sync`, `fd_datasync`, `fd_fdstat_get`, `fd_fdstat_set_flags`, `fd_prestat_get`, `fd_prestat_dir_name`, `path_open`, `path_filestat_get` (só `doom1.wad`), `clock_time_get`, `random_get` (xorshift de `ticks`, **não criptográfico**), `args_get/args_sizes_get`, `environ_get/environ_sizes_get`, `poll_oneoff`, `path_create_directory`, `path_remove_directory`, `path_unlink_file`, `path_rename` (os 4 últimos fingem sucesso), `proc_exit` (só loga) | `wasi.rs:271-320` | — |
| `env` | `system` | `wasi.rs:324-326` | devolve -1 |

### 7.3 Que memória do kernel o guest alcança

1. **Só a própria memória linear**, via API do `wasmi`. Todos os acessos do host passam por `Memory::read/write/data` com checagem de limites (`mod.rs:542-547`, `wasi.rs:30-35`). **Nenhum ponteiro do kernel é exposto**: `ptr/len` são `i32` offsets do guest.
   - Detalhe: um ponteiro negativo em `rd/wr` é **clampado a 0**, não rejeitado (`wasi.rs:31,34`). Fica confinado ao guest, mas é semântica incorreta.
2. **Superfície offscreen** `SURFACE[back]` (1,1 MB de BSS), por ponteiro cru em `HostState` (`mod.rs:43-57`, `:459-462`).
   - É definido pelo worker antes de cada `render` e `info = None` depois (`:477`). `canvas()` devolve `None` sem `info` (`:77-83`).
   - O `Canvas` descarta qualquer escrita fora de `(692,414)`: `fill_rect` retorna se `x0 >= width` (`fb.rs:149`) e as escritas são limitadas. Mesmo com coordenadas de guest negativas, o `as usize` vira um valor enorme que é descartado pelo clip. **Não encontrei como o guest escreve fora da superfície**; é a parte mais bem contida.
3. **Serial** (`log`, `fd_write`): saída sem limite → o guest pode travar a sua thread a 38400 baud (`serial.rs:23-26`).
4. **`WAD`** (`.rodata`, somente leitura), **ticks** e o PRNG.
5. **Não alcança** rede, disco, FS do OS, teclado fora da fila `EVENTS`, nem outras estruturas do kernel.

**Lacunas de robustez** (não são fuga de sandbox, mas travam ou derrubam o kernel):

- **Sem `fuel` nem `ResourceLimiter`** (`grep fuel|limiter|StoreLimits` em `kernel/src`: 0). Um `render` com laço infinito prende a thread wasmapp para sempre. Fechar a janela só zera `ACTIVE`, verificado *entre* chamadas (`mod.rs:448`).
- `memory.grow` sem limite imposto pelo host. A memória do guest sai do heap fixo de 64 MiB. O comportamento em OOM (se `grow` devolve -1 ou aborta) **não foi verificado**. Um OOM vira panic e `halt`.
- `APP` é construído **uma vez** e nunca reiniciado (`mod.rs:309-315`). Após um trap só se loga uma vez (`:472-476`) e continua chamando `render` na instância.
- A aritmética `i32` com valores do guest em `host_fill/host_text/host_blit` e nas funções WASI usa `+`/`*` sem `checked_`/`wrapping_` (`mod.rs:96-99`, `:120`, `wasi.rs:73,99`).
  - No release (sem overflow-checks, `Cargo.toml:12-15`) ela dá a volta e o clip a descarta.
  - Em **builds debug** ela faria panic (e `halt`) com argumentos do guest.
- Laços com contagem do guest: `fd_write/fd_read` iteram `n: i32` (`wasi.rs:72,98`), `random_get` itera `len` (`:230`). Isso permite DoS de CPU, limitado à thread W.
- O panic handler faz `hlt` com IF=1 (`main.rs:777-780`): o panic de uma thread a deixa congelada, as outras continuam.

---

## 8. Dependências do kernel

`kernel/Cargo.toml` (62 crates transitivos distintos no target `x86_64-unknown-none`, via `cargo tree`; versões de `Cargo.lock`):

| Crate | Versão | Para que serve | Usado em |
|---|---|---|---|
| `bootloader_api` | 0.11.15 | `entry_point!`, `BootInfo`, `BootloaderConfig`, `FrameBufferInfo` | `main.rs:33-35`, `fb.rs:3`, `wasm/mod.rs:22` |
| `osjeff_core` | path | lógica pura: terminal, editor, fs, net (ARP/IPv4/DHCP), browser, web (HTML/CSS), calc… | `desktop/*`, `main.rs`, `fetch.rs`, `allocator.rs` |
| `x86_64` | 0.15.5 | IDT, `hlt`/`interrupts`, segmentos, `Cr3`, `OffsetPageTable` | `interrupts.rs`, `sched.rs`, `allocator.rs`, `virtio.rs`, `main.rs` |
| `smoltcp` | 0.12.0 | pilha TCP/IP (Ethernet, IPv4, DHCP, TCP, DNS) | `netstack.rs` |
| `embedded-tls` | 0.19.0 | TLS 1.3 (blocking), AES-128-GCM-SHA256; usa `UnsecureProvider`, **sem verificação de certificado** (`netstack.rs:223-226,247`) | `netstack.rs` |
| `embedded-io` | 0.7.1 | traits `Read/Write` do stream TCP para o TLS | `netstack.rs` |
| `rand_core` | 0.6.4 | traits `RngCore/CryptoRng` do TLS; o RNG é xorshift semeado por TSC (`netstack.rs:385-424`), **não criptográfico** (o código avisa) | `netstack.rs` |
| `sha2` | 0.10.9 | **não é referenciado no código do kernel**; declarado só para ativar `force-soft` (`kernel/Cargo.toml:26-30`, SIMD indisponível sem SSE) | nenhum |
| `wasmi` | 1.1.0 | interpretador WebAssembly (`no_std`) | `wasm/mod.rs`, `wasm/wasi.rs` |
| `wat` | 1.252.0 (build-dep) | monta o WAT da demo no host (`build.rs:60-63`) | `kernel/build.rs` |
| `bootloader` | 0.11.17 (só em `os`) | gera as imagens BIOS/UEFI | `os/build.rs` |

Outros pontos:

- Transitivas relevantes: RustCrypto (`aes`, `ghash`, `polyval`, `p256`, `hkdf`, `hmac`, `sha2`) via `embedded-tls`, `heapless`, `spin` 0.9.8, `wasmparser` 0.239.0, `libm`.
- As flags `--cfg aes_force_soft/polyval_force_soft/ghash_force_soft` estão em `.cargo/config.toml:10-13`.
- Toolchain: `nightly-2026-10-05` (`rust-toolchain.toml:5`, `#![feature(abi_x86_interrupt)]` em `main.rs:3`) e `-Z bindeps` (`.cargo/config.toml:3`).
- Perfil release: `opt-level=3`, LTO, 1 codegen unit (`Cargo.toml:12-15`); `panic` não é forçado (`Cargo.toml:5-8`, o target já é abort).
- `bootloader_api` (0.11.15) e `bootloader` (0.11.17) divergem em patch. Foi aceito, pois o boot foi medido.

---

## 9. Divergências entre a documentação e o código

(Só o que foi comparado; o resto não foi lido.)

- Nem `README.md` nem `docs/ARCHITECTURE.md` mencionam wasm, wasmi, TLS, smoltcp, virtio-gpu, browser com motor HTML/CSS, DOOM nem o heap de 64 MiB (`grep` por esses termos: 0 ocorrências).
- `docs/ARCHITECTURE.md:248,267` descrevem "16 registros" e "~17 KiB". O código tem `MAX_FILES = 48` (`osjeff_core/src/fs.rs:25`) e o FS tem diretórios.
- `docs/ARCHITECTURE.md:28` diz que o `unsafe` fica "isolado e auditável". Na verdade ele está em **18 arquivos do kernel** (seção 4).
- O comentário de `os/src/main.rs:19` ("128 MiB é suficiente") só vale para BIOS.
- A DHCP obtida em `main.rs:231` **só alimenta o responder ARP/ping** (`main.rs:627`). O `netstack::Net` fixa IP `10.0.2.15`, gateway `10.0.2.2` e DNS `10.0.2.3` (`netstack.rs:19-21`), então o browser só funciona atrás do SLIRP do QEMU.

---

## 10. Itens para as próximas fases (não são conclusões da auditoria 00)

1. **Escalonamento**: o round-robin gira a cada tick (250 Hz), sem estado "bloqueado" (`sched.rs:176-186`). Workers ociosos fazem `hlt` (`fetch.rs:93`, `wasm/mod.rs:449,453`) e queimam o quantum inteiro de 4 ms. Com 3 threads e 2 ociosas, o compositor teria cerca de 1/3 dos ticks. **Inferido do código, não medido.**
2. **24 bpp em BIOS** impede o caminho rápido de `fill_rect` (seção 2.4).
3. **`http_get` sem limite de resposta** (`netstack.rs:175-190`; só `https_get` tem o teto de 256 KiB em `:277`). Um servidor pode estourar os 64 MiB do heap em 10 s.
4. **TLS sem verificação de certificado e RNG fraco** (seção 8).
5. **Falhas silenciosas**: `panic_handler` e handlers de exceção sem log (seção 3).
6. **Resolução UEFI > 1080p**: panic provável (seção 2.4).
7. **`desktop::disk()`, `scratch_slice()`, `scheduler()`**: `&'static mut` devolvido por `fn` segura (seção 4).
