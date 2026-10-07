# Compilar e rodar

Este guia leva do `git clone` a um desktop OSjeff na tela, em Linux, WSL, macOS
(só QEMU) ou Windows. Todos os comandos foram executados de verdade nesta árvore.

## 1. Pré-requisitos

| O quê | Para quê | Observação |
|---|---|---|
| [`rustup`](https://rustup.rs) | toolchain | O `rust-toolchain.toml` fixa **`nightly-2026-10-05`** e baixa sozinho `rust-src`, `llvm-tools`, `clippy`, `rustfmt` e os alvos `x86_64-unknown-none` e `wasm32-unknown-unknown`. Não use `nightly` solto: ver [Problemas comuns](#5-problemas-comuns). |
| QEMU (`qemu-system-x86_64`) | rodar a imagem | Não precisa de KVM nem de tela para os testes (`-display none`). |
| OVMF | boot UEFI | Ubuntu/Debian: `apt install ovmf`. Só é necessário para `uefi`. |
| `socat` e ImageMagick | harness headless | Só para `tools/qemu-headless.sh` e `tools/verify-boot.sh` (screenshots). |
| `wasi-sdk` | apps C em WASM | Opcional (DOOM e `cdemo`). O app padrão (`snake`) é Rust e não precisa. |

Ubuntu/Debian de uma vez:

```bash
sudo apt install qemu-system-x86 ovmf socat imagemagick
```

## 2. Compilar a imagem

```bash
cargo build --release -p os
```

O crate `os` embute o kernel como *artifact dependency* (`-Z bindeps`, já
habilitado em `.cargo/config.toml`) e gera duas imagens de disco em
`target/release/build/os-*/out/`: `osjeff-bios.img` (~4,7 MB) e
`osjeff-uefi.img` (~4,3 MB). A primeira compilação leva ~1–2 min (baixa e compila
o bootloader); as seguintes ~25 s.

## 3. Rodar

### Linux / WSL / macOS

```bash
tools/run.sh            # BIOS, janela do QEMU
tools/run.sh uefi       # UEFI (usa OVMF)
tools/run.sh bios -- -accel kvm   # com KVM, bem mais rápido
```

O disco do filesystem (`osjeff-fs.img`, 64 KiB) é criado na primeira execução e
persiste entre boots.

> `cargo run -p os -- uefi` só funciona se `OVMF_PATH` apontar para um firmware
> **único** (`OVMF.fd`). O Ubuntu moderno distribui o OVMF dividido em
> `CODE`+`VARS`, que exige `pflash`; o `tools/run.sh` já cuida disso.

### Windows (aceleração WHPX)

```powershell
.\run.ps1              # compila no WSL e roda com WHPX
.\run.ps1 -NoAccel     # TCG (software)
.\run.ps1 -SkipBuild   # só boota a imagem existente
.\run.ps1 -Usb         # gera osjeff-uefi.img para gravar em pendrive
.\run.ps1 -Doom        # variante com DOOM (ver §4)
```

### Sem tela (CI, servidor, nuvem)

```bash
# sobe, espera 25 s, salva serial.log e screen.png em /tmp/osj
tools/qemu-headless.sh bios /tmp/osj 25
QEMU_MEM=256M tools/qemu-headless.sh uefi /tmp/osj-uefi 40

# build + boot BIOS e UEFI + comparação do desktop com uma baseline
tools/verify-boot.sh /tmp/osj-novo /tmp/osj-baseline
```

Para mandar teclas e mouse enquanto roda:
`echo "sendkey a" | socat - UNIX-CONNECT:$(cat /tmp/osj/mon.path)`
(o socket fica em `/tmp` porque caminhos de socket Unix têm limite de 107 bytes).

## 4. Variantes de app WebAssembly

O kernel embute **um** app WASM com janela, escolhido na compilação (`kernel/build.rs`):

| Compilação | App |
|---|---|
| padrão | `snake` (Rust → `wasm32-unknown-unknown`, em `wasm-apps/snake`) |
| `WASI_SDK_PATH=...` | `cdemo` (C freestanding) |
| `DOOM=1 WASI_SDK_PATH=...` | DOOM ([doomgeneric](https://github.com/ozkl/doomgeneric), GPLv2) |

DOOM **não** vem pronto no checkout: precisa do `wasi-sdk`, de rede para clonar o
doomgeneric (`tools/build-doom.sh`) e de um WAD (`doom1.wad`, não redistribuído; o
Freedoom serve). Use 512 MB de RAM no QEMU. O app `plasma` existe em
`wasm-apps/plasma` mas nenhum caminho do `build.rs` o seleciona.

## 5. Problemas comuns

| Sintoma | Causa | Solução |
|---|---|---|
| `E0046: not all trait items implemented, missing: forward_overflowing` | Toolchain `nightly` sem data com `x86_64` < 0.15.5 | Use o pin do repo (`nightly-2026-10-05`) e `cargo update -p x86_64`. |
| `invalid rustc abi: 'x86-softfloat'` no `build.rs` do bootloader | `bootloader` < 0.11.17 com nightly novo | `cargo update -p bootloader` (0.11.17+). |
| UEFI: `panicked at load_kernel.rs:285 ... None` | RAM insuficiente: o BSS do kernel tem ~91 MiB e o bootloader o zera antes de entrar | Use `-m 192M` ou mais (os scripts usam 256M). |
| QEMU não sobe e não há `serial.log` | Caminho do socket do monitor > 107 bytes | Use `tools/qemu-headless.sh` (socket em `/tmp`) ou um `outdir` mais curto. |
| `cargo clippy` falha com erro de unwinding | O kernel `no_std` não compila no host | Use os aliases: `cargo lint-kernel`, `cargo lint-host`. |
| `cargo tree` / `cargo metadata` dá ICE | `-Z bindeps` | `cargo tree -p kernel --target x86_64-unknown-none`. |
| Tela preta no hardware real | Secure Boot ligado ou modo Legacy/CSM | Ver [`BOOT-USB.md`](BOOT-USB.md). |

## 6. Hardware real

Veja [`BOOT-USB.md`](BOOT-USB.md). Nenhum teste em hardware real foi feito durante
a auditoria; os números de desempenho e o comportamento de vídeo vêm do QEMU.
