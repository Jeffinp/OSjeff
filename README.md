# OSjeff

Sistema operacional x86_64 mínimo, escrito em **Rust** (bare metal, `no_std`, sem
GC, sem runtime). Boota via BIOS/UEFI e sobe um desktop estilo Windows 11 com
**window manager**, **terminal** e um **editor de texto** — tudo desenhado direto
no framebuffer.

## Recursos

- Desktop: wallpaper em gradiente, taskbar, relógio em tempo real (RTC/UTC)
- **Window manager**: múltiplas janelas, foco, z-order, arrastar pela barra de
  título, botão fechar, lançar apps pela taskbar
- **Animações de abertura/fechamento**: fade + slide (easing smoothstep),
  compostas via blend contra o wallpaper, paceadas por TSC (sem timer)
- **Gerenciador de processos**: abrir um app spawna um processo (pid novo),
  fechar a janela encerra (remove da tabela); processos System protegidos;
  coluna `UP` = uptime em segundos
- **Terminal** (`OSJEFF SHELL`): histórico com scroll, linha de input com caret
  navegável, comandos `HELP CLS TIME VER ECHO EDIT PS`
- **Editor** (`OSJEFF EDIT`): texto multi-linha, cursor 2D, inserir/quebrar/juntar
  linhas, navegação por setas/Home/End, status `Ln/Col` + marcador de modificação
- **Task Manager** (`TASK MANAGER`): lista processos, navega com ↑/↓,
  Enter foca/abre, Del encerra (apps); processos System são protegidos
- **Mouse PS/2** (cursor) e **teclado PS/2** (com Shift, Caps Lock, setas, Del)
- **Double buffering** com fundo cacheado → render sem flicker
- Fonte bitmap 8x8 própria (maiúsculas, minúsculas, dígitos, símbolos)

## Arquitetura

A lógica pura é separada do hardware para ser **testável no host**:

```
OSjeff/
├── osjeff_core/        # lib no_std + testável (cargo test) — ~98% de cobertura
│   └── src/
│       ├── keymap.rs    # scancode PS/2 Set 1 -> Key, com shift/caps
│       ├── terminal.rs  # shell: histórico, input, parser de comandos
│       ├── editor.rs    # modelo de texto multi-linha + cursor 2D
│       ├── window.rs    # geometria/hit-testing de janelas
│       ├── anim.rs      # easing/estado das animações de janela
│       └── process.rs   # tabela de processos (estados, seleção, ticks)
├── kernel/             # no_std, target x86_64-unknown-none (hardware + render)
│   └── src/
│       ├── main.rs      # entry point + loop do compositor
│       ├── desktop.rs   # window manager + render dos apps
│       ├── fb.rs        # primitivas de framebuffer (pixel/rect/gradiente)
│       ├── font.rs      # fonte bitmap 8x8
│       ├── io.rs        # port I/O (inb/outb)
│       ├── ps2.rs       # driver PS/2 (mouse + teclado, polling)
│       └── rtc.rs       # relógio CMOS
└── os/                 # builder: gera imagem booteável (crate `bootloader`) + QEMU
```

**Por que separar `osjeff_core`?** Um kernel `no_std`/`no_main` não roda
`cargo test` no host. Movendo toda a *decisão* (parser, edição, hit-testing) para
uma lib que compila com `std` sob teste, conseguimos cobertura real. O kernel fica
só com glue de hardware (pixels, portas, z-order).

## Pré-requisitos

- Rust **nightly** (fixado em `rust-toolchain.toml`, instala sozinho — necessário
  para `-Z bindeps`, que embute o binário do kernel no builder)
- Target `x86_64-unknown-none` (instalado pelo toolchain)
- **QEMU**: `qemu-system-x86_64`
  - Windows: instalador oficial em https://qemu.weilnetz.de/
  - Ubuntu/WSL: `sudo apt install qemu-system-x86`

## Rodar

### Windows (recomendado — aceleração WHPX)

```powershell
cd C:\...\OSjeff
cargo build --package os --release
Copy-Item (Get-ChildItem target\release\build\os-*\out\osjeff-bios.img | Select -Last 1) osjeff-bios.img
.\run.ps1                # WHPX (rápido, quase nativo)
.\run.ps1 -NoAccel       # software (TCG), fallback
```

> Se o PowerShell bloquear o script: `Set-ExecutionPolicy -Scope Process Bypass`.

### Linux / WSL

```bash
cd OSjeff
cargo run --package os            # BIOS
cargo run --package os -- uefi    # UEFI (precisa OVMF; veja run.ps1/os/src/main.rs)
```

## Testes e cobertura

Toda a lógica vive em `osjeff_core` e é testada no host:

```bash
cargo test -p osjeff_core
```

Cobertura (precisa `cargo install cargo-llvm-cov`):

```bash
cargo llvm-cov -p osjeff_core --summary-only
```

Estado atual: **60 testes**, ~**98% de regiões / ~97% de linhas**.

| Módulo       | Linhas |
|--------------|--------|
| keymap.rs    | 100%   |
| terminal.rs  | 97%    |
| editor.rs    | 96%    |
| window.rs    | 100%   |

> O kernel e o `os` ficam fora do `cargo test` (bare-metal / sem testes); por isso
> os comandos acima são escopados com `-p osjeff_core`.

## Usando o sistema

- **Botão direito** no desktop: abre menu de contexto (Terminal / Editor / Task
  Manager) — clique num item para abrir.
- **Taskbar**: ícone azul = terminal, verde = editor, âmbar = task manager.
- **Terminal**: `HELP` lista comandos. `EDIT` abre o editor, `PS` o task manager.
- **Editor**: digite normalmente; setas/Home/End navegam; `Esc` fecha.
- **Task Manager**: ↑/↓ seleciona, `Enter` foca/abre, `Del` encerra (apps).
- **Janelas**: arraste pela barra de título; clique para focar; bolinha vermelha fecha.

## Performance

O compositor evita redesenhar a tela inteira quando dá:

- **Cursor por dirty-rect**: mover o mouse só restaura o retângulo antigo do
  cursor (a partir do back buffer sem cursor) e redesenha o sprite — nada de
  copiar ~16 MB por movimento. Recompõe a cena cheia só quando ela muda
  (janela, texto, relógio, animação, menu).
- **`fill_rect` por linha**: caminho rápido RGB/BGR escreve bytes direto, sem
  passar pixel a pixel por `put` (bounds + match de formato).
- **Release com LTO**: `opt-level=3`, `lto=true`, `codegen-units=1`.
- **Animações paceadas por TSC** (`rdtsc`), sem depender de timer interrupt.

## Gravar em hardware real (pen drive)

> **Atenção:** `dd` apaga o disco de destino. Confira o device antes.

```bash
sudo dd if=osjeff-bios.img of=/dev/sdX bs=4M status=progress && sync
```

## Próximos passos

- IDT + PIC + IRQ → interrupções reais (substituir polling)
- Heap allocator (`alloc`) → estruturas dinâmicas
- Persistência de arquivos (driver de disco + FS simples)
- Mais apps e copy/paste entre janelas
