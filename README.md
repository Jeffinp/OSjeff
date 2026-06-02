# OSjeff

OS x86_64 mínimo em Rust. Boota e desenha um desktop estático estilo Windows 11
(wallpaper em gradiente azul + taskbar centralizada com ícones).

Stack: **Rust** (kernel `no_std` + builder). Assembly só no `hlt`. Sem C#, sem runtime, sem GC.

## Estrutura

```
OSjeff/
├── kernel/   # no_std, target x86_64-unknown-none. Desenha no framebuffer.
│   └── src/
│       ├── main.rs   # entry point + desktop
│       └── fb.rs      # primitivas de desenho (pixel, rect, rounded rect, gradiente)
└── os/       # builder: gera imagem booteável (crate `bootloader` 0.11) + roda QEMU
```

## Pré-requisitos

- Rust **nightly** (definido em `rust-toolchain.toml`, instala sozinho)
- Target `x86_64-unknown-none` (instalado sozinho via toolchain)
- **QEMU**: `qemu-system-x86_64`
  - Ubuntu/WSL: `sudo apt install qemu-system-x86`

## Rodar

```bash
cd OSjeff
cargo run --package os          # BIOS (padrão)
cargo run --package os -- uefi  # UEFI (precisa OVMF; veja abaixo)
```

Primeira build baixa o `bootloader` e compila o kernel bare-metal — demora.

### UEFI (opcional)

Precisa do firmware OVMF:

```bash
sudo apt install ovmf
OVMF_PATH=/usr/share/OVMF/OVMF_CODE.fd cargo run --package os -- uefi
```

## Imagem booteável (pen drive / hardware real)

Após `cargo build --package os`, a imagem fica em:
`target/debug/build/os-*/out/osjeff-bios.img`

Gravar (CUIDADO: apaga o disco alvo):

```bash
sudo dd if=osjeff-bios.img of=/dev/sdX bs=4M status=progress && sync
```

## Próximos passos

- Input de teclado/mouse (PS/2) → cursor
- Janelas arrastáveis (compositor simples)
- Fonte bitmap → texto na taskbar/relógio
- Double buffering → anti-flicker / animação
```
