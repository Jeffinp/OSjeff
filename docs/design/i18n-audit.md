# Auditoria de textos do Kitsune (i18n)

Gerado por `python3 -I tools/i18n-audit.py` (não edite à mão; `--check` confere se está em dia).
Lista os literais de texto visíveis ao usuário em `kernel/src` e `kitsune_core/src`, por app, para que a
migração para o catálogo (`docs/design/i18n.md`) seja dividida **sem sobreposição de arquivos**.

Como ler: *sem acento* = palavra em português que precisa de acento (lista em `tools/i18n/accents.txt`);
*inglês* = texto em inglês numa interface em português; *misto* = os dois idiomas no mesmo texto.
A contagem é heurística (veja o cabeçalho do script): ela erra para mais ou para menos em casos
de borda, mas cada linha abaixo tem arquivo e linha para conferir. Logs (`klog!`, serial) e a tela
de falha ficam em inglês de propósito e não entram na conta.

## Resumo por app (ordem sugerida de migração)

| App / área | Arquivos | Textos | Sem acento | Inglês | Misto | Já no catálogo |
|---|---:|---:|---:|---:|---:|---:|
| Shell (migrado nesta onda) | 3 | 6 | 0 | 0 | 0 | 106 |
| Ajustes | 3 | 6 | 0 | 3 | 0 | 210 |
| Arquivos | 6 | 39 | 0 | 37 | 2 | 218 |
| Editor | 1 | 9 | 0 | 9 | 0 | 49 |
| Terminal | 5 | 12 | 0 | 1 | 0 | 262 |
| Tarefas | 1 | 4 | 0 | 0 | 1 | 136 |
| Registro | 0 | 0 | 0 | 0 | 0 | 26 |
| Calculadora | 0 | 0 | 0 | 0 | 0 | 2 |
| Imagens | 5 | 46 | 0 | 33 | 9 | 46 |
| Navegador | 11 | 36 | 0 | 20 | 1 | 90 |
| Apps de terceiros (WASM) | 8 | 46 | 0 | 40 | 1 | 83 |
| Kit de componentes | 0 | 0 | 0 | 0 | 0 | 75 |
| Sistema (logs e tela de falha: ficam em inglês) | 7 | 33 | 0 | 25 | 1 | 46 |
| Outros | 5 | 18 | 0 | 9 | 1 | 63 |
| **Total** | 55 | 255 | 0 | 177 | 16 | 1412 |

Cada app só mexe nos arquivos da sua linha; os arquivos de `Kit de componentes` e do `Shell` já
foram tratados (Shell) ou só mudam se um app precisar de uma chave nova (use o prefixo do próprio app).

## Arquivos por app

### Shell (migrado nesta onda)

painel, barra de apps, Apps/Busca, diálogo de energia, banners.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/overlays.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/desktop/panel.rs` | 3 | 0 | 0 | 0 |
| `kernel/src/desktop/taskbar.rs` | 1 | 0 | 0 | 0 |

### Ajustes

janela de Ajustes e o modelo de configurações.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/settings_ui.rs` | 2 | 0 | 0 | 0 |
| `kitsune_core/src/settings.rs` | 1 | 0 | 0 | 0 |
| `kitsune_core/src/wallpaper.rs` | 3 | 0 | 3 | 0 |

### Arquivos

gerenciador de arquivos, lixeira, VFS.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kitsune_core/src/fileman.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/fs3/dir.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/fs3/extent.rs` | 2 | 0 | 2 | 0 |
| `kitsune_core/src/fs3/fsck.rs` | 21 | 0 | 21 | 0 |
| `kitsune_core/src/fs3/mod.rs` | 11 | 0 | 9 | 2 |
| `kitsune_core/src/fs3/ops.rs` | 3 | 0 | 3 | 0 |

### Editor

editor de texto e diálogos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kitsune_core/src/editor2/mod.rs` | 9 | 0 | 9 | 0 |

### Terminal

terminal, interpretador e comandos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/shellhost.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/desktop/term.rs` | 2 | 0 | 0 | 0 |
| `kitsune_core/src/shell/builtins.rs` | 3 | 0 | 0 | 0 |
| `kitsune_core/src/shell/exec.rs` | 3 | 0 | 1 | 0 |
| `kitsune_core/src/shell/netcmds.rs` | 2 | 0 | 0 | 0 |

### Tarefas

monitor de atividade.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kitsune_core/src/netstats.rs` | 4 | 0 | 0 | 1 |

### Imagens

visualizador de imagens e decodificadores.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kitsune_core/src/bmp.rs` | 6 | 0 | 5 | 1 |
| `kitsune_core/src/image.rs` | 10 | 0 | 7 | 0 |
| `kitsune_core/src/inflate.rs` | 10 | 0 | 7 | 3 |
| `kitsune_core/src/png.rs` | 15 | 0 | 10 | 4 |
| `kitsune_core/src/ppm.rs` | 5 | 0 | 4 | 1 |

### Navegador

navegador, páginas internas, erros de rede e TLS (outro agente está editando).

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/netd.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/netstack.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/browser.rs` | 4 | 0 | 0 | 0 |
| `kitsune_core/src/browser/body_tests.rs` | 1 | 0 | 0 | 0 |
| `kitsune_core/src/icmp.rs` | 7 | 0 | 6 | 1 |
| `kitsune_core/src/net.rs` | 2 | 0 | 0 | 0 |
| `kitsune_core/src/sntp.rs` | 4 | 0 | 4 | 0 |
| `kitsune_core/src/tlsverify.rs` | 7 | 0 | 7 | 0 |
| `kitsune_core/src/web/form.rs` | 5 | 0 | 0 | 0 |
| `kitsune_core/src/web/imgcache.rs` | 1 | 0 | 0 | 0 |
| `kitsune_core/src/web/style.rs` | 2 | 0 | 2 | 0 |

### Apps de terceiros (WASM)

janela de app, manifesto, instalação, SDK.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/wasmwin.rs` | 1 | 0 | 0 | 0 |
| `kernel/src/wasm/mod.rs` | 4 | 0 | 3 | 0 |
| `kernel/src/wasm/wasi.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/appfs/mod.rs` | 11 | 0 | 10 | 0 |
| `kitsune_core/src/appfs/volume_tests.rs` | 2 | 0 | 1 | 0 |
| `kitsune_core/src/appinstall.rs` | 6 | 0 | 6 | 0 |
| `kitsune_core/src/appmanifest.rs` | 16 | 0 | 14 | 1 |
| `kitsune_core/src/wasmsec.rs` | 5 | 0 | 5 | 0 |

### Sistema (logs e tela de falha: ficam em inglês)

boot, falha grave, drivers.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/boot.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/crash.rs` | 8 | 0 | 2 | 1 |
| `kernel/src/desktop/browser_ui.rs` | 1 | 0 | 1 | 0 |
| `kernel/src/interrupts.rs` | 2 | 0 | 2 | 0 |
| `kernel/src/io.rs` | 6 | 0 | 6 | 0 |
| `kernel/src/main.rs` | 12 | 0 | 12 | 0 |
| `kernel/src/trace.rs` | 2 | 0 | 2 | 0 |

### Outros

núcleo sem dono claro.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kitsune_core/src/base64.rs` | 4 | 0 | 3 | 1 |
| `kitsune_core/src/brand.rs` | 6 | 0 | 4 | 0 |
| `kitsune_core/src/compositor/sim/paint.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/i18n/audit.rs` | 1 | 0 | 1 | 0 |
| `kitsune_core/src/i18n/template.rs` | 6 | 0 | 0 | 0 |

## Textos sem acento (a corrigir ao migrar)

| Arquivo:linha | Texto | Correção |
|---|---|---|
| (nenhum) | | |

## Inglês e textos mistos numa interface em português

| Arquivo:linha | Texto | Tipo |
|---|---|---|
| `kernel/src/crash.rs:234` | `KITSUNE - UNSUPPORTED SCREEN` | en |
| `kernel/src/crash.rs:271` | `ERROR  : {code:#018x}` | en |
| `kernel/src/crash.rs:280` | `The system is halted. Reset or power-cycle the machine to restart.\nThe same report was written to the seri...` | misto |
| `kernel/src/desktop/browser_ui.rs:139` | `load to first paint` | en |
| `kernel/src/interrupts.rs:314` | `stack overflow in thread '{owner}': guard page hit at {cr2:#x} ({code:?})` | en |
| `kernel/src/interrupts.rs:321` | `page fault accessing {cr2:#x}: {code:?}` | en |
| `kernel/src/io.rs:12` | `in al, dx` | en |
| `kernel/src/io.rs:22` | `out dx, al` | en |
| `kernel/src/io.rs:30` | `out dx, ax` | en |
| `kernel/src/io.rs:39` | `in ax, dx` | en |
| `kernel/src/io.rs:48` | `out dx, eax` | en |
| `kernel/src/io.rs:57` | `in eax, dx` | en |
| `kernel/src/main.rs:145` | `this screen resolution is not supported` | en |
| `kernel/src/main.rs:147` | `Detected {}x{} (stride {}, {} bytes/pixel): the screen needs {} bytes, the bootloader provided a framebuffe...` | en |
| `kernel/src/main.rs:195` | `wasm demo done` | en |
| `kernel/src/main.rs:273` | `pci scan + virtio-gpu probe done` | en |
| `kernel/src/main.rs:283` | `ata detect done` | en |
| `kernel/src/main.rs:321` | `nic init done` | en |
| `kernel/src/main.rs:326` | `dhcp done` | en |
| `kernel/src/main.rs:364` | `storage init done` | en |
| `kernel/src/main.rs:387` | `ui text engine ready` | en |
| `kernel/src/main.rs:412` | `Desktop::new (fs load from ATA) done` | en |
| `kernel/src/main.rs:420` | `wallpaper painted` | en |
| `kernel/src/main.rs:732` | `the kernel panicked` | en |
| `kernel/src/netstack.rs:708` | `tcp stream error` | en |
| `kernel/src/trace.rs:658` | `The quick brown fox jumps over the lazy dog 0123` | en |
| `kernel/src/trace.rs:736` | `The quick brown fox jumps over the lazy dog 0123` | en |
| `kernel/src/wasm/mod.rs:396` | `link host.draw_text` | en |
| `kernel/src/wasm/mod.rs:417` | `link host.time_ms` | en |
| `kernel/src/wasm/mod.rs:453` | `trap in entry` | en |
| `kernel/src/wasm/wasi.rs:381` | `link env.system` | en |
| `kitsune_core/src/appfs/mod.rs:94` | `not found` | en |
| `kitsune_core/src/appfs/mod.rs:95` | `already exists` | en |
| `kitsune_core/src/appfs/mod.rs:96` | `not a directory` | en |
| `kitsune_core/src/appfs/mod.rs:97` | `is a directory` | en |
| `kitsune_core/src/appfs/mod.rs:98` | `directory not empty` | en |
| `kitsune_core/src/appfs/mod.rs:100` | `invalid argument` | en |
| `kitsune_core/src/appfs/mod.rs:101` | `permission denied` | en |
| `kitsune_core/src/appfs/mod.rs:102` | `bad file descriptor` | en |
| `kitsune_core/src/appfs/mod.rs:103` | `too many open files` | en |
| `kitsune_core/src/appfs/mod.rs:104` | `i/o error` | en |
| `kitsune_core/src/appfs/volume_tests.rs:350` | `id={id}\nname={id}\nversion=1.0.0\n{extra}` | en |
| `kitsune_core/src/appinstall.rs:78` | `package is larger than 4 MiB` | en |
| `kitsune_core/src/appinstall.rs:80` | `an app with this id is already installed` | en |
| `kitsune_core/src/appinstall.rs:81` | `app is not installed` | en |
| `kitsune_core/src/appinstall.rs:82` | `invalid app id` | en |
| `kitsune_core/src/appinstall.rs:83` | `too many installed apps` | en |
| `kitsune_core/src/appinstall.rs:84` | `file system: {e}` | en |
| `kitsune_core/src/appmanifest.rs:173` | `manifest is too large` | en |
| `kitsune_core/src/appmanifest.rs:174` | `manifest is not valid UTF-8` | en |
| `kitsune_core/src/appmanifest.rs:175` | `manifest has too many lines` | en |
| `kitsune_core/src/appmanifest.rs:176` | `manifest syntax error` | en |
| `kitsune_core/src/appmanifest.rs:178` | `unknown manifest key` | en |
| `kitsune_core/src/appmanifest.rs:179` | `manifest key '{k}' is required` | en |
| `kitsune_core/src/appmanifest.rs:180` | `invalid value for '{k}'` | en |
| `kitsune_core/src/appmanifest.rs:181` | `'{k}' is above the system limit` | en |
| `kitsune_core/src/appmanifest.rs:691` | `icon is larger than 64 KiB` | en |
| `kitsune_core/src/appmanifest.rs:692` | `icon is not a PNG` | en |
| `kitsune_core/src/appmanifest.rs:693` | `icon is larger than 64x64` | en |
| `kitsune_core/src/appmanifest.rs:694` | `icon PNG is corrupt` | en |
| `kitsune_core/src/appmanifest.rs:747` | `package has no kitsune.manifest section` | misto |
| `kitsune_core/src/appmanifest.rs:748` | `package has two manifests` | en |
| `kitsune_core/src/appmanifest.rs:749` | `package has two icons` | en |
| `kitsune_core/src/base64.rs:29` | `invalid base64 character` | en |
| `kitsune_core/src/base64.rs:30` | `invalid base64 length` | en |
| `kitsune_core/src/base64.rs:31` | `base64 padding before the end` | en |
| `kitsune_core/src/base64.rs:32` | `base64 data too large` | misto |
| `kitsune_core/src/bmp.rs:72` | `bmp data is truncated` | misto |
| `kitsune_core/src/bmp.rs:73` | `not a bmp (missing BM)` | en |
| `kitsune_core/src/bmp.rs:74` | `unsupported bmp header size` | en |
| `kitsune_core/src/bmp.rs:76` | `invalid bmp dimensions` | en |
| `kitsune_core/src/bmp.rs:79` | `invalid bmp colour masks` | en |
| `kitsune_core/src/bmp.rs:80` | `bmp image: {e}` | en |
| `kitsune_core/src/brand.rs:593` | `light face` | en |
| `kitsune_core/src/brand.rs:603` | `tail (dark)` | en |
| `kitsune_core/src/brand.rs:617` | `  <title>{title}</title>` | en |
| `kitsune_core/src/brand.rs:637` | `  <path fill="{}" fill-rule="evenodd" d="{d}"/> <!-- silhouette; eyes and nose are holes -->` | en |
| `kitsune_core/src/compositor/sim/paint.rs:77` | `layer {:?} wrote ({x},{y}) outside its footprint {:?}` | en |
| `kitsune_core/src/editor2/mod.rs:353` | `line index out of sync with the text` | en |
| `kitsune_core/src/editor2/mod.rs:356` | `cursor past the end` | en |
| `kitsune_core/src/editor2/mod.rs:359` | `cursor not at a valid position` | en |
| `kitsune_core/src/editor2/mod.rs:362` | `selection anchor past the end` | en |
| `kitsune_core/src/editor2/mod.rs:365` | `scroll position past the last line` | en |
| `kitsune_core/src/editor2/mod.rs:368` | `empty viewport` | en |
| `kitsune_core/src/editor2/mod.rs:374` | `row wider than the window` | en |
| `kitsune_core/src/editor2/mod.rs:378` | `more rows than the window holds` | en |
| `kitsune_core/src/editor2/mod.rs:383` | `cursor drawn outside the window` | en |
| `kitsune_core/src/fileman.rs:940` | `Enter` | en |
| `kitsune_core/src/fs3/dir.rs:56` | `hole in a directory` | en |
| `kitsune_core/src/fs3/extent.rs:117` | `extent chain too long or cyclic` | en |
| `kitsune_core/src/fs3/extent.rs:166` | `invalid extent` | en |
| `kitsune_core/src/fs3/fsck.rs:118` | `in-memory bitmaps differ from the medium` | en |
| `kitsune_core/src/fs3/fsck.rs:121` | `free block counter` | en |
| `kitsune_core/src/fs3/fsck.rs:124` | `free inode counter` | en |
| `kitsune_core/src/fs3/fsck.rs:157` | `directory size != blocks` | en |
| `kitsune_core/src/fs3/fsck.rs:175` | `invalid name in directory` | en |
| `kitsune_core/src/fs3/fsck.rs:178` | `duplicate name in directory` | en |
| `kitsune_core/src/fs3/fsck.rs:182` | `entry points at an unallocated inode` | en |
| `kitsune_core/src/fs3/fsck.rs:205` | `entry kind differs from inode kind` | en |
| `kitsune_core/src/fs3/fsck.rs:216` | `trash flag disagrees with the location` | en |
| `kitsune_core/src/fs3/fsck.rs:219` | `trashed entry lacks its original name` | en |
| `kitsune_core/src/fs3/fsck.rs:230` | `root must hold exactly one .trash entry` | en |
| `kitsune_core/src/fs3/fsck.rs:245` | `extent list invalid` | en |
| `kitsune_core/src/fs3/fsck.rs:253` | `extent chain invalid` | en |
| `kitsune_core/src/fs3/fsck.rs:263` | `block owned by two structures` | en |
| `kitsune_core/src/fs3/fsck.rs:272` | `block owned by two structures` | en |
| `kitsune_core/src/fs3/fsck.rs:279` | `nblocks != sum of extent lengths` | en |
| `kitsune_core/src/fs3/fsck.rs:293` | `extent beyond end of file` | en |
| `kitsune_core/src/fs3/fsck.rs:303` | `non-zero bytes past end of file` | en |
| `kitsune_core/src/fs3/fsck.rs:314` | `allocated inode is unreachable` | en |
| `kitsune_core/src/fs3/fsck.rs:326` | `block in use but marked free` | en |
| `kitsune_core/src/fs3/fsck.rs:328` | `block marked used but unowned (leak)` | en |
| `kitsune_core/src/fs3/mod.rs:500` | `fsck found problems` | en |
| `kitsune_core/src/fs3/mod.rs:535` | `bitmap leaves the metadata uncovered` | en |
| `kitsune_core/src/fs3/mod.rs:538` | `root/trash inode not allocated` | en |
| `kitsune_core/src/fs3/mod.rs:581` | `journal target out of range` | en |
| `kitsune_core/src/fs3/mod.rs:599` | `bad root inode` | en |
| `kitsune_core/src/fs3/mod.rs:603` | `bad trash inode` | en |
| `kitsune_core/src/fs3/mod.rs:607` | `root has no .trash entry` | misto |
| `kitsune_core/src/fs3/mod.rs:728` | `metadata block out of range` | en |
| `kitsune_core/src/fs3/mod.rs:803` | `double free of an inode` | en |
| `kitsune_core/src/fs3/mod.rs:827` | `block range outside the data region` | misto |
| `kitsune_core/src/fs3/mod.rs:831` | `block allocated or freed twice` | en |
| `kitsune_core/src/fs3/ops.rs:121` | `directory entry points at a free inode` | en |
| `kitsune_core/src/fs3/ops.rs:190` | `inode missing from its parent` | en |
| `kitsune_core/src/fs3/ops.rs:321` | `directory entry changed under us` | en |
| `kitsune_core/src/i18n/audit.rs:845` | `{file}:{line}: {w:?} should be {r} in {s:?}` | en |
| `kitsune_core/src/icmp.rs:247` | `destination unreachable (code {c})` | en |
| `kitsune_core/src/icmp.rs:248` | `time exceeded` | en |
| `kitsune_core/src/icmp.rs:249` | `no route to host` | misto |
| `kitsune_core/src/icmp.rs:250` | `host did not answer ARP` | en |
| `kitsune_core/src/icmp.rs:251` | `invalid target address` | en |
| `kitsune_core/src/icmp.rs:252` | `network unavailable` | en |
| `kitsune_core/src/icmp.rs:253` | `network busy` | en |
| `kitsune_core/src/image.rs:56` | `image has a zero dimension` | en |
| `kitsune_core/src/image.rs:57` | `image is larger than the pixel limit` | en |
| `kitsune_core/src/image.rs:58` | `buffer length is wrong for the dimensions` | en |
| `kitsune_core/src/image.rs:59` | `rectangle is outside the image` | en |
| `kitsune_core/src/image.rs:60` | `out of memory` | en |
| `kitsune_core/src/image.rs:172` | `Image({}x{})` | en |
| `kitsune_core/src/image.rs:799` | `unknown image format` | en |
| `kitsune_core/src/inflate.rs:62` | `compressed data is truncated` | misto |
| `kitsune_core/src/inflate.rs:63` | `reserved deflate block type` | en |
| `kitsune_core/src/inflate.rs:64` | `stored block length check failed` | en |
| `kitsune_core/src/inflate.rs:65` | `invalid huffman code lengths` | en |
| `kitsune_core/src/inflate.rs:66` | `block has no end-of-block code` | misto |
| `kitsune_core/src/inflate.rs:68` | `invalid huffman code` | en |
| `kitsune_core/src/inflate.rs:69` | `back-reference too far` | en |
| `kitsune_core/src/inflate.rs:70` | `decompressed data exceeds the limit` | misto |
| `kitsune_core/src/inflate.rs:71` | `bad zlib header` | en |
| `kitsune_core/src/inflate.rs:74` | `out of memory` | en |
| `kitsune_core/src/netstats.rs:353` | `no address` | misto |
| `kitsune_core/src/png.rs:145` | `not a png (bad signature)` | en |
| `kitsune_core/src/png.rs:146` | `png is truncated` | en |
| `kitsune_core/src/png.rs:149` | `png has no IHDR first` | misto |
| `kitsune_core/src/png.rs:150` | `invalid png IHDR` | en |
| `kitsune_core/src/png.rs:151` | `invalid png dimensions` | en |
| `kitsune_core/src/png.rs:152` | `invalid png palette or transparency` | en |
| `kitsune_core/src/png.rs:154` | `png has no IDAT` | misto |
| `kitsune_core/src/png.rs:155` | `png chunks out of order` | en |
| `kitsune_core/src/png.rs:156` | `unknown critical png chunk` | en |
| `kitsune_core/src/png.rs:158` | `png image data too short` | misto |
| `kitsune_core/src/png.rs:159` | `png image data too long` | misto |
| `kitsune_core/src/png.rs:160` | `invalid png filter type` | en |
| `kitsune_core/src/png.rs:161` | `png palette index out of range` | en |
| `kitsune_core/src/png.rs:162` | `png image: {e}` | en |
| `kitsune_core/src/ppm.rs:43` | `not a P3/P6 ppm` | en |
| `kitsune_core/src/ppm.rs:44` | `ppm data is truncated` | misto |
| `kitsune_core/src/ppm.rs:45` | `invalid ppm header` | en |
| `kitsune_core/src/ppm.rs:46` | `invalid ppm sample` | en |
| `kitsune_core/src/ppm.rs:47` | `ppm image: {e}` | en |
| `kitsune_core/src/shell/exec.rs:984` | `{name}.sh` | en |
| `kitsune_core/src/sntp.rs:145` | `not a server reply` | en |
| `kitsune_core/src/sntp.rs:148` | `bad stratum` | en |
| `kitsune_core/src/sntp.rs:150` | `bad server timestamps` | en |
| `kitsune_core/src/sntp.rs:152` | `implausible date` | en |
| `kitsune_core/src/tlsverify.rs:83` | `certificate too large` | en |
| `kitsune_core/src/tlsverify.rs:84` | `chain too long` | en |
| `kitsune_core/src/tlsverify.rs:86` | `not yet valid` | en |
| `kitsune_core/src/tlsverify.rs:87` | `name mismatch` | en |
| `kitsune_core/src/tlsverify.rs:90` | `bad signature` | en |
| `kitsune_core/src/tlsverify.rs:91` | `invalid CA in chain` | en |
| `kitsune_core/src/tlsverify.rs:94` | `system clock wrong` | en |
| `kitsune_core/src/wallpaper.rs:384` | `file too big` | en |
| `kitsune_core/src/wallpaper.rs:385` | `not PNG/BMP/PPM` | en |
| `kitsune_core/src/wallpaper.rs:386` | `image too big` | en |
| `kitsune_core/src/wasmsec.rs:64` | `not a wasm module (too short)` | en |
| `kitsune_core/src/wasmsec.rs:65` | `not a wasm module (bad magic)` | en |
| `kitsune_core/src/wasmsec.rs:68` | `section extends past the end of the file` | en |
| `kitsune_core/src/wasmsec.rs:69` | `unknown section id` | en |
| `kitsune_core/src/wasmsec.rs:70` | `too many sections` | en |
| `kitsune_core/src/web/style.rs:460` | `courier new` | en |
| `kitsune_core/src/web/style.rs:468` | `source code pro` | en |

## Todos os textos, por arquivo, com a chave proposta

A chave é `<app>.<resumo>`; ao migrar, ajuste o resumo, mantenha o prefixo do app e
use `.one`/`.other` para plurais. Textos neutros (nomes próprios, unidades) também aparecem:
decida caso a caso se vão para o catálogo.

### Shell (migrado nesta onda)

**`kernel/src/desktop/overlays.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 64 | `app:{}` | neutro | `shell.app` |
| 134 | `app:{id}` | neutro | `shell.app_id` |

**`kernel/src/desktop/panel.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 481 | `Alt+↑` | neutro | `shell.alt` |
| 488 | `Alt+←` | neutro | `shell.alt_2` |
| 494 | `Alt+→` | neutro | `shell.alt_3` |

**`kernel/src/desktop/taskbar.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 87 | `sys:{}` | neutro | `shell.sys` |

### Ajustes

**`kernel/src/desktop/settings_ui.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 814 | `UTC {:02}:{:02}:{:02}` | neutro | `settings.utc_02_02_02` |
| 1276 | `Kitsune` | neutro | `settings.kitsune` |

**`kitsune_core/src/settings.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 139 | `UTC{}{:02}:{:02}` | neutro | `settings.utc_02_02` |

**`kitsune_core/src/wallpaper.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 384 | `file too big` | en | `settings.file_too_big` |
| 385 | `not PNG/BMP/PPM` | en | `settings.not_png_bmp_ppm` |
| 386 | `image too big` | en | `settings.image_too_big` |

### Arquivos

**`kitsune_core/src/fileman.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 940 | `Enter` | en | `files.enter` |

**`kitsune_core/src/fs3/dir.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 56 | `hole in a directory` | en | `files.hole_in_directory` |

**`kitsune_core/src/fs3/extent.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 117 | `extent chain too long or cyclic` | en | `files.extent_chain_too_long` |
| 166 | `invalid extent` | en | `files.invalid_extent` |

**`kitsune_core/src/fs3/fsck.rs`** (21)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 118 | `in-memory bitmaps differ from the medium` | en | `files.in_memory_bitmaps_differ` |
| 121 | `free block counter` | en | `files.free_block_counter` |
| 124 | `free inode counter` | en | `files.free_inode_counter` |
| 157 | `directory size != blocks` | en | `files.directory_size_blocks` |
| 175 | `invalid name in directory` | en | `files.invalid_name_in_directory` |
| 178 | `duplicate name in directory` | en | `files.duplicate_name_in_directory` |
| 182 | `entry points at an unallocated inode` | en | `files.entry_points_at_an` |
| 205 | `entry kind differs from inode kind` | en | `files.entry_kind_differs_from` |
| 216 | `trash flag disagrees with the location` | en | `files.trash_flag_disagrees_with` |
| 219 | `trashed entry lacks its original name` | en | `files.trashed_entry_lacks_its` |
| 230 | `root must hold exactly one .trash entry` | en | `files.root_must_hold_exactly` |
| 245 | `extent list invalid` | en | `files.extent_list_invalid` |
| 253 | `extent chain invalid` | en | `files.extent_chain_invalid` |
| 263 | `block owned by two structures` | en | `files.block_owned_by_two` |
| 272 | `block owned by two structures` | en | `files.block_owned_by_two_2` |
| 279 | `nblocks != sum of extent lengths` | en | `files.nblocks_sum_extent_lengths` |
| 293 | `extent beyond end of file` | en | `files.extent_beyond_end_file` |
| 303 | `non-zero bytes past end of file` | en | `files.non_zero_bytes_past` |
| 314 | `allocated inode is unreachable` | en | `files.allocated_inode_is_unreachable` |
| 326 | `block in use but marked free` | en | `files.block_in_use_but` |
| 328 | `block marked used but unowned (leak)` | en | `files.block_marked_used_but` |

**`kitsune_core/src/fs3/mod.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 500 | `fsck found problems` | en | `files.fsck_found_problems` |
| 535 | `bitmap leaves the metadata uncovered` | en | `files.bitmap_leaves_metadata_uncovered` |
| 538 | `root/trash inode not allocated` | en | `files.root_trash_inode_not` |
| 581 | `journal target out of range` | en | `files.journal_target_out_range` |
| 599 | `bad root inode` | en | `files.bad_root_inode` |
| 603 | `bad trash inode` | en | `files.bad_trash_inode` |
| 607 | `root has no .trash entry` | misto | `files.root_has_no_trash` |
| 728 | `metadata block out of range` | en | `files.metadata_block_out_range` |
| 803 | `double free of an inode` | en | `files.double_free_an_inode` |
| 827 | `block range outside the data region` | misto | `files.block_range_outside_data` |
| 831 | `block allocated or freed twice` | en | `files.block_allocated_or_freed` |

**`kitsune_core/src/fs3/ops.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 121 | `directory entry points at a free inode` | en | `files.directory_entry_points_at` |
| 190 | `inode missing from its parent` | en | `files.inode_missing_from_its` |
| 321 | `directory entry changed under us` | en | `files.directory_entry_changed_under` |

### Editor

**`kitsune_core/src/editor2/mod.rs`** (9)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 353 | `line index out of sync with the text` | en | `editor.line_index_out_sync` |
| 356 | `cursor past the end` | en | `editor.cursor_past_end` |
| 359 | `cursor not at a valid position` | en | `editor.cursor_not_at_valid` |
| 362 | `selection anchor past the end` | en | `editor.selection_anchor_past_end` |
| 365 | `scroll position past the last line` | en | `editor.scroll_position_past_last` |
| 368 | `empty viewport` | en | `editor.empty_viewport` |
| 374 | `row wider than the window` | en | `editor.row_wider_than_window` |
| 378 | `more rows than the window holds` | en | `editor.more_rows_than_window` |
| 383 | `cursor drawn outside the window` | en | `editor.cursor_drawn_outside_window` |

### Terminal

**`kernel/src/desktop/shellhost.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 302 | `hd{}` | neutro | `term.hd` |
| 645 | `sh: {}\n` | neutro | `term.sh` |

**`kernel/src/desktop/term.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 268 | `sh: {problem}\n` | neutro | `term.sh_problem` |
| 396 | `sh: {}\n` | neutro | `term.sh` |

**`kitsune_core/src/shell/builtins.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 1037 | `export {n}="{v}"\n` | neutro | `term.export_n_v` |
| 1109 | `alias {k}='{v}'\n` | neutro | `term.alias_k_v` |
| 1183 | `{n}.sh` | neutro | `term.n_sh` |

**`kitsune_core/src/shell/exec.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 458 | `sh: {}\n` | neutro | `term.sh` |
| 477 | `sh: {}\n` | neutro | `term.sh_2` |
| 984 | `{name}.sh` | en | `term.name_sh` |

**`kitsune_core/src/shell/netcmds.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 263 | `  inet {ip}/{}` | neutro | `term.inet_ip` |
| 265 | `  gateway {g}` | neutro | `term.gateway_g` |

### Tarefas

**`kitsune_core/src/netstats.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 339 | `net: {} link={} tx={}/{}B err={} drop={} rx={}/{}B err={} drop={} \| ` | neutro | `tasks.net_link_tx_b` |
| 353 | `no address` | misto | `tasks.no_address` |
| 356 | ` lease={}s` | neutro | `tasks.lease_s` |
| 362 | ` dhcp={} renew={} rebind={} lost={} \| dns q={} hit={} failover={} fail={} \| ping {}/{}` | neutro | `tasks.dhcp_renew_rebind_lost` |

### Imagens

**`kitsune_core/src/bmp.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 72 | `bmp data is truncated` | misto | `viewer.bmp_data_is_truncated` |
| 73 | `not a bmp (missing BM)` | en | `viewer.not_bmp_missing_bm` |
| 74 | `unsupported bmp header size` | en | `viewer.unsupported_bmp_header_size` |
| 76 | `invalid bmp dimensions` | en | `viewer.invalid_bmp_dimensions` |
| 79 | `invalid bmp colour masks` | en | `viewer.invalid_bmp_colour_masks` |
| 80 | `bmp image: {e}` | en | `viewer.bmp_image` |

**`kitsune_core/src/image.rs`** (10)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 56 | `image has a zero dimension` | en | `viewer.image_has_zero_dimension` |
| 57 | `image is larger than the pixel limit` | en | `viewer.image_is_larger_than` |
| 58 | `buffer length is wrong for the dimensions` | en | `viewer.buffer_length_is_wrong` |
| 59 | `rectangle is outside the image` | en | `viewer.rectangle_is_outside_image` |
| 60 | `out of memory` | en | `viewer.out_memory` |
| 172 | `Image({}x{})` | en | `viewer.image_x` |
| 799 | `unknown image format` | en | `viewer.unknown_image_format` |
| 800 | `png: {e}` | neutro | `viewer.png` |
| 801 | `bmp: {e}` | neutro | `viewer.bmp` |
| 802 | `ppm: {e}` | neutro | `viewer.ppm` |

**`kitsune_core/src/inflate.rs`** (10)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 62 | `compressed data is truncated` | misto | `viewer.compressed_data_is_truncated` |
| 63 | `reserved deflate block type` | en | `viewer.reserved_deflate_block_type` |
| 64 | `stored block length check failed` | en | `viewer.stored_block_length_check` |
| 65 | `invalid huffman code lengths` | en | `viewer.invalid_huffman_code_lengths` |
| 66 | `block has no end-of-block code` | misto | `viewer.block_has_no_end` |
| 68 | `invalid huffman code` | en | `viewer.invalid_huffman_code` |
| 69 | `back-reference too far` | en | `viewer.back_reference_too_far` |
| 70 | `decompressed data exceeds the limit` | misto | `viewer.decompressed_data_exceeds_limit` |
| 71 | `bad zlib header` | en | `viewer.bad_zlib_header` |
| 74 | `out of memory` | en | `viewer.out_memory` |

**`kitsune_core/src/png.rs`** (15)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 145 | `not a png (bad signature)` | en | `viewer.not_png_bad_signature` |
| 146 | `png is truncated` | en | `viewer.png_is_truncated` |
| 149 | `png has no IHDR first` | misto | `viewer.png_has_no_ihdr` |
| 150 | `invalid png IHDR` | en | `viewer.invalid_png_ihdr` |
| 151 | `invalid png dimensions` | en | `viewer.invalid_png_dimensions` |
| 152 | `invalid png palette or transparency` | en | `viewer.invalid_png_palette_or` |
| 154 | `png has no IDAT` | misto | `viewer.png_has_no_idat` |
| 155 | `png chunks out of order` | en | `viewer.png_chunks_out_order` |
| 156 | `unknown critical png chunk` | en | `viewer.unknown_critical_png_chunk` |
| 157 | `png data: {e}` | pt | `viewer.png_data` |
| 158 | `png image data too short` | misto | `viewer.png_image_data_too` |
| 159 | `png image data too long` | misto | `viewer.png_image_data_too_2` |
| 160 | `invalid png filter type` | en | `viewer.invalid_png_filter_type` |
| 161 | `png palette index out of range` | en | `viewer.png_palette_index_out` |
| 162 | `png image: {e}` | en | `viewer.png_image` |

**`kitsune_core/src/ppm.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 43 | `not a P3/P6 ppm` | en | `viewer.not_p3_p6_ppm` |
| 44 | `ppm data is truncated` | misto | `viewer.ppm_data_is_truncated` |
| 45 | `invalid ppm header` | en | `viewer.invalid_ppm_header` |
| 46 | `invalid ppm sample` | en | `viewer.invalid_ppm_sample` |
| 47 | `ppm image: {e}` | en | `viewer.ppm_image` |

### Navegador

**`kernel/src/netd.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 373 | `no DHCP ack` | pt | `web.no_dhcp_ack` |
| 374 | `no DHCP offer` | pt | `web.no_dhcp_offer` |

**`kernel/src/netstack.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 708 | `tcp stream error` | en | `web.tcp_stream_error` |

**`kitsune_core/src/browser.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 953 | `&amp;` | neutro | `web.amp` |
| 954 | `&lt;` | neutro | `web.lt` |
| 955 | `&gt;` | neutro | `web.gt` |
| 956 | `&quot;` | neutro | `web.quot` |

**`kitsune_core/src/browser/body_tests.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 39 | `HTTP/1.1 200 OKr\n{headers}r\n` | neutro | `web.http_1_1_200` |

**`kitsune_core/src/icmp.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 247 | `destination unreachable (code {c})` | en | `web.destination_unreachable_code_c` |
| 248 | `time exceeded` | en | `web.time_exceeded` |
| 249 | `no route to host` | misto | `web.no_route_host` |
| 250 | `host did not answer ARP` | en | `web.host_did_not_answer` |
| 251 | `invalid target address` | en | `web.invalid_target_address` |
| 252 | `network unavailable` | en | `web.network_unavailable` |
| 253 | `network busy` | en | `web.network_busy` |

**`kitsune_core/src/net.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 788 | `{}/{} gw ` | neutro | `web.gw` |
| 793 | ` dns {}` | neutro | `web.dns` |

**`kitsune_core/src/sntp.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 145 | `not a server reply` | en | `web.not_server_reply` |
| 148 | `bad stratum` | en | `web.bad_stratum` |
| 150 | `bad server timestamps` | en | `web.bad_server_timestamps` |
| 152 | `implausible date` | en | `web.implausible_date` |

**`kitsune_core/src/tlsverify.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 83 | `certificate too large` | en | `web.certificate_too_large` |
| 84 | `chain too long` | en | `web.chain_too_long` |
| 86 | `not yet valid` | en | `web.not_yet_valid` |
| 87 | `name mismatch` | en | `web.name_mismatch` |
| 90 | `bad signature` | en | `web.bad_signature` |
| 91 | `invalid CA in chain` | en | `web.invalid_ca_in_chain` |
| 94 | `system clock wrong` | en | `web.system_clock_wrong` |

**`kitsune_core/src/web/form.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 186 | `AÁEÉIÍOÓUÚCÇYÝ` | pt | `web.aaeeiioouuccyy` |
| 190 | `AÀEÈIÌOÒUÙ` | pt | `web.aaeeiioouu` |
| 192 | `AÃOÕNÑ` | pt | `web.aaoonn` |
| 195 | `AÂEÊIÎOÔUÛ` | pt | `web.aaeeiioouu_2` |
| 199 | `AÄEËIÏOÖUÜ` | pt | `web.aaeeiioouu_3` |

**`kitsune_core/src/web/imgcache.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 248 | `data:#{:016x}-{}` | pt | `web.data_016x` |

**`kitsune_core/src/web/style.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 460 | `courier new` | en | `web.courier_new` |
| 468 | `source code pro` | en | `web.source_code_pro` |

### Apps de terceiros (WASM)

**`kernel/src/desktop/wasmwin.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 95 | `{}/{}.wasm` | neutro | `apps.wasm` |

**`kernel/src/wasm/mod.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 396 | `link host.draw_text` | en | `apps.link_host_draw_text` |
| 417 | `link host.time_ms` | en | `apps.link_host_time_ms` |
| 452 | `no entry export` | pt | `apps.no_entry_export` |
| 453 | `trap in entry` | en | `apps.trap_in_entry` |

**`kernel/src/wasm/wasi.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 381 | `link env.system` | en | `apps.link_env_system` |

**`kitsune_core/src/appfs/mod.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 94 | `not found` | en | `apps.not_found` |
| 95 | `already exists` | en | `apps.already_exists` |
| 96 | `not a directory` | en | `apps.not_directory` |
| 97 | `is a directory` | en | `apps.is_directory` |
| 98 | `directory not empty` | en | `apps.directory_not_empty` |
| 99 | `no space left (quota)` | pt | `apps.no_space_left_quota` |
| 100 | `invalid argument` | en | `apps.invalid_argument` |
| 101 | `permission denied` | en | `apps.permission_denied` |
| 102 | `bad file descriptor` | en | `apps.bad_file_descriptor` |
| 103 | `too many open files` | en | `apps.too_many_open_files` |
| 104 | `i/o error` | en | `apps.i_error` |

**`kitsune_core/src/appfs/volume_tests.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 113 | `readdir {i} => {:?}` | neutro | `apps.readdir_i` |
| 350 | `id={id}\nname={id}\nversion=1.0.0\n{extra}` | en | `apps.id_id_name_id` |

**`kitsune_core/src/appinstall.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 78 | `package is larger than 4 MiB` | en | `apps.package_is_larger_than` |
| 80 | `an app with this id is already installed` | en | `apps.an_app_with_this` |
| 81 | `app is not installed` | en | `apps.app_is_not_installed` |
| 82 | `invalid app id` | en | `apps.invalid_app_id` |
| 83 | `too many installed apps` | en | `apps.too_many_installed_apps` |
| 84 | `file system: {e}` | en | `apps.file_system` |

**`kitsune_core/src/appmanifest.rs`** (16)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 173 | `manifest is too large` | en | `apps.manifest_is_too_large` |
| 174 | `manifest is not valid UTF-8` | en | `apps.manifest_is_not_valid` |
| 175 | `manifest has too many lines` | en | `apps.manifest_has_too_many` |
| 176 | `manifest syntax error` | en | `apps.manifest_syntax_error` |
| 177 | `duplicate manifest key '{k}'` | neutro | `apps.duplicate_manifest_key_k` |
| 178 | `unknown manifest key` | en | `apps.unknown_manifest_key` |
| 179 | `manifest key '{k}' is required` | en | `apps.manifest_key_k_is` |
| 180 | `invalid value for '{k}'` | en | `apps.invalid_value_for_k` |
| 181 | `'{k}' is above the system limit` | en | `apps.k_is_above_system` |
| 691 | `icon is larger than 64 KiB` | en | `apps.icon_is_larger_than` |
| 692 | `icon is not a PNG` | en | `apps.icon_is_not_png` |
| 693 | `icon is larger than 64x64` | en | `apps.icon_is_larger_than_2` |
| 694 | `icon PNG is corrupt` | en | `apps.icon_png_is_corrupt` |
| 747 | `package has no kitsune.manifest section` | misto | `apps.package_has_no_kitsune` |
| 748 | `package has two manifests` | en | `apps.package_has_two_manifests` |
| 749 | `package has two icons` | en | `apps.package_has_two_icons` |

**`kitsune_core/src/wasmsec.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 64 | `not a wasm module (too short)` | en | `apps.not_wasm_module_too` |
| 65 | `not a wasm module (bad magic)` | en | `apps.not_wasm_module_bad` |
| 68 | `section extends past the end of the file` | en | `apps.section_extends_past_end` |
| 69 | `unknown section id` | en | `apps.unknown_section_id` |
| 70 | `too many sections` | en | `apps.too_many_sections` |

### Sistema (logs e tela de falha: ficam em inglês)

**`kernel/src/boot.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 83 | `Kitsune` | neutro | `sys.kitsune` |
| 93 | `Sistema operacional` | pt | `sys.sistema_operacional` |

**`kernel/src/crash.rs`** (8)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 234 | `KITSUNE - UNSUPPORTED SCREEN` | en | `sys.kitsune_unsupported_screen` |
| 257 | `thread : {}` | neutro | `sys.thread` |
| 265 | `RIP    : {:#018x}` | neutro | `sys.rip_018x` |
| 267 | `RSP    : {rsp:#018x}` | neutro | `sys.rsp_rsp_018x` |
| 269 | `RFLAGS : {:#018x}` | neutro | `sys.rflags_018x` |
| 271 | `ERROR  : {code:#018x}` | en | `sys.error_code_018x` |
| 274 | `CR2    : {cr2:#018x}` | neutro | `sys.cr2_cr2_018x` |
| 280 | `The system is halted. Reset or power-cycle the machine to restart.\nThe same report was written to the seri...` | misto | `sys.system_is_halted_reset` |

**`kernel/src/desktop/browser_ui.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 139 | `load to first paint` | en | `sys.load_first_paint` |

**`kernel/src/interrupts.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 314 | `stack overflow in thread '{owner}': guard page hit at {cr2:#x} ({code:?})` | en | `sys.stack_overflow_in_thread` |
| 321 | `page fault accessing {cr2:#x}: {code:?}` | en | `sys.page_fault_accessing_cr2` |

**`kernel/src/io.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 12 | `in al, dx` | en | `sys.in_al_dx` |
| 22 | `out dx, al` | en | `sys.out_dx_al` |
| 30 | `out dx, ax` | en | `sys.out_dx_ax` |
| 39 | `in ax, dx` | en | `sys.in_ax_dx` |
| 48 | `out dx, eax` | en | `sys.out_dx_eax` |
| 57 | `in eax, dx` | en | `sys.in_eax_dx` |

**`kernel/src/main.rs`** (12)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 145 | `this screen resolution is not supported` | en | `sys.this_screen_resolution_is` |
| 147 | `Detected {}x{} (stride {}, {} bytes/pixel): the screen needs {} bytes, the bootloader provided a framebuffe...` | en | `sys.detected_x_stride_bytes` |
| 195 | `wasm demo done` | en | `sys.wasm_demo_done` |
| 273 | `pci scan + virtio-gpu probe done` | en | `sys.pci_scan_virtio_gpu` |
| 283 | `ata detect done` | en | `sys.ata_detect_done` |
| 321 | `nic init done` | en | `sys.nic_init_done` |
| 326 | `dhcp done` | en | `sys.dhcp_done` |
| 364 | `storage init done` | en | `sys.storage_init_done` |
| 387 | `ui text engine ready` | en | `sys.ui_text_engine_ready` |
| 412 | `Desktop::new (fs load from ATA) done` | en | `sys.desktop_new_fs_load` |
| 420 | `wallpaper painted` | en | `sys.wallpaper_painted` |
| 732 | `the kernel panicked` | en | `sys.kernel_panicked` |

**`kernel/src/trace.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 658 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps` |
| 736 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps_2` |

### Outros

**`kitsune_core/src/base64.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 29 | `invalid base64 character` | en | `misc.invalid_base64_character` |
| 30 | `invalid base64 length` | en | `misc.invalid_base64_length` |
| 31 | `base64 padding before the end` | en | `misc.base64_padding_before_end` |
| 32 | `base64 data too large` | misto | `misc.base64_data_too_large` |

**`kitsune_core/src/brand.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 593 | `light face` | en | `misc.light_face` |
| 603 | `tail (dark)` | en | `misc.tail_dark` |
| 617 | `  <title>{title}</title>` | en | `misc.title_title_title` |
| 621 | `  <rect width="{GRID}" height="{GRID}" rx="{}" fill="{}"/> <!-- tile -->` | neutro | `misc.rect_width_grid_height` |
| 637 | `  <path fill="{}" fill-rule="evenodd" d="{d}"/> <!-- silhouette; eyes and nose are holes -->` | en | `misc.path_fill_fill_rule` |
| 644 | `  <polygon fill="{}" points="{}"/> <!-- {} -->` | neutro | `misc.polygon_fill_points` |

**`kitsune_core/src/compositor/sim/paint.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 77 | `layer {:?} wrote ({x},{y}) outside its footprint {:?}` | en | `misc.layer_wrote_x_y` |

**`kitsune_core/src/i18n/audit.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 845 | `{file}:{line}: {w:?} should be {r} in {s:?}` | en | `misc.file_line_w_should` |

**`kitsune_core/src/i18n/template.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 52 | `Str({s:?})` | neutro | `misc.str_s` |
| 53 | `Int({n})` | neutro | `misc.int_n` |
| 54 | `Num({n})` | neutro | `misc.num_n` |
| 55 | `Pad({n}, {w})` | neutro | `misc.pad_n_w` |
| 56 | `Dec({n}, {p})` | neutro | `misc.dec_n_p` |
| 57 | `Bytes({n})` | neutro | `misc.bytes_n` |

