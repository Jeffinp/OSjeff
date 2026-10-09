# Auditoria de textos do OSjeff (i18n)

Gerado por `python3 -I tools/i18n-audit.py` (não edite à mão; `--check` confere se está em dia).
Lista os literais de texto visíveis ao usuário em `kernel/src` e `osjeff_core/src`, por app, para que a
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
| Ajustes | 3 | 129 | 0 | 4 | 0 | 22 |
| Arquivos | 6 | 39 | 0 | 37 | 2 | 218 |
| Editor | 1 | 9 | 0 | 9 | 0 | 49 |
| Terminal | 11 | 203 | 0 | 177 | 5 | 0 |
| Tarefas | 3 | 89 | 0 | 3 | 1 | 0 |
| Registro | 1 | 19 | 0 | 0 | 0 | 4 |
| Calculadora | 2 | 2 | 0 | 0 | 0 | 0 |
| Imagens | 5 | 46 | 0 | 33 | 9 | 46 |
| Navegador | 14 | 134 | 17 | 14 | 3 | 0 |
| Apps de terceiros (WASM) | 11 | 77 | 16 | 40 | 1 | 0 |
| Kit de componentes | 1 | 32 | 0 | 0 | 0 | 12 |
| Sistema (logs e tela de falha: ficam em inglês) | 9 | 75 | 0 | 25 | 1 | 0 |
| Outros | 5 | 13 | 0 | 6 | 1 | 63 |
| **Total** | 75 | 873 | 33 | 348 | 23 | 520 |

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
| `kernel/src/desktop/settings_ui.rs` | 109 | 0 | 1 | 0 |
| `osjeff_core/src/settings.rs` | 14 | 0 | 0 | 0 |
| `osjeff_core/src/wallpaper.rs` | 6 | 0 | 3 | 0 |

### Arquivos

gerenciador de arquivos, lixeira, VFS.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/fileman.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/fs3/dir.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/fs3/extent.rs` | 2 | 0 | 2 | 0 |
| `osjeff_core/src/fs3/fsck.rs` | 21 | 0 | 21 | 0 |
| `osjeff_core/src/fs3/mod.rs` | 11 | 0 | 9 | 2 |
| `osjeff_core/src/fs3/ops.rs` | 3 | 0 | 3 | 0 |

### Editor

editor de texto e diálogos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/editor2/mod.rs` | 9 | 0 | 9 | 0 |

### Terminal

terminal, interpretador e comandos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/shellhost.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/desktop/term.rs` | 2 | 0 | 0 | 0 |
| `osjeff_core/src/shell/builtins.rs` | 3 | 0 | 0 | 0 |
| `osjeff_core/src/shell/exec.rs` | 3 | 0 | 1 | 0 |
| `osjeff_core/src/shell/netcmds.rs` | 2 | 0 | 0 | 0 |

### Tarefas

monitor de atividade.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/netstats.rs` | 4 | 0 | 0 | 1 |

### Imagens

visualizador de imagens e decodificadores.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/bmp.rs` | 6 | 0 | 5 | 1 |
| `osjeff_core/src/image.rs` | 10 | 0 | 7 | 0 |
| `osjeff_core/src/inflate.rs` | 10 | 0 | 7 | 3 |
| `osjeff_core/src/png.rs` | 15 | 0 | 10 | 4 |
| `osjeff_core/src/ppm.rs` | 5 | 0 | 4 | 1 |

### Navegador

navegador, páginas internas, erros de rede e TLS (outro agente está editando).

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/netd.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/netstack.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/browser.rs` | 20 | 0 | 0 | 0 |
| `osjeff_core/src/browser/body_tests.rs` | 1 | 0 | 0 | 0 |
| `osjeff_core/src/browser/errors.rs` | 29 | 0 | 0 | 0 |
| `osjeff_core/src/browser/pages.rs` | 24 | 0 | 1 | 1 |
| `osjeff_core/src/browser/tabs.rs` | 5 | 0 | 0 | 0 |
| `osjeff_core/src/icmp.rs` | 7 | 0 | 6 | 1 |
| `osjeff_core/src/net.rs` | 2 | 0 | 0 | 0 |
| `osjeff_core/src/sntp.rs` | 4 | 0 | 4 | 0 |
| `osjeff_core/src/tlsverify.rs` | 23 | 16 | 0 | 0 |
| `osjeff_core/src/web/form.rs` | 8 | 0 | 0 | 0 |
| `osjeff_core/src/web/imgcache.rs` | 5 | 0 | 0 | 0 |
| `osjeff_core/src/web/style.rs` | 3 | 1 | 2 | 1 |

### Apps de terceiros (WASM)

janela de app, manifesto, instalação, SDK.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/wasmwin.rs` | 2 | 1 | 0 | 0 |
| `kernel/src/wasm/abi2.rs` | 9 | 6 | 0 | 0 |
| `kernel/src/wasm/manager.rs` | 5 | 2 | 0 | 0 |
| `kernel/src/wasm/manager/runtime.rs` | 7 | 4 | 0 | 0 |
| `kernel/src/wasm/mod.rs` | 13 | 3 | 3 | 0 |
| `kernel/src/wasm/wasi.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/appfs/mod.rs` | 11 | 0 | 10 | 0 |
| `osjeff_core/src/appfs/volume_tests.rs` | 2 | 0 | 1 | 0 |
| `osjeff_core/src/appinstall.rs` | 6 | 0 | 6 | 0 |
| `osjeff_core/src/appmanifest.rs` | 16 | 0 | 14 | 1 |
| `osjeff_core/src/wasmsec.rs` | 5 | 0 | 5 | 0 |

### Kit de componentes

widgets, galeria e primitivas.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/gallery.rs` | 32 | 0 | 0 | 0 |

### Sistema (logs e tela de falha: ficam em inglês)

boot, falha grave, drivers.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/boot.rs` | 1 | 0 | 0 | 0 |
| `kernel/src/crash.rs` | 8 | 0 | 2 | 1 |
| `kernel/src/desktop/browser.rs` | 7 | 0 | 0 | 0 |
| `kernel/src/desktop/browser_input.rs` | 11 | 0 | 0 | 0 |
| `kernel/src/desktop/browser_ui.rs` | 26 | 0 | 1 | 0 |
| `kernel/src/interrupts.rs` | 2 | 0 | 2 | 0 |
| `kernel/src/io.rs` | 6 | 0 | 6 | 0 |
| `kernel/src/main.rs` | 12 | 0 | 12 | 0 |
| `kernel/src/trace.rs` | 2 | 0 | 2 | 0 |

### Outros

núcleo sem dono claro.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/base64.rs` | 4 | 0 | 3 | 1 |
| `osjeff_core/src/compositor/sim/mod.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/compositor/sim/paint.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/i18n/audit.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/i18n/template.rs` | 6 | 0 | 0 | 0 |

## Textos sem acento (a corrigir ao migrar)

| Arquivo:linha | Texto | Correção |
|---|---|---|
| `kernel/src/desktop/wasmwin.rs:172` | `pacote nao encontrado` | nao → não |
| `kernel/src/wasm/abi2.rs:94` | `sem memoria exportada` | memoria → memória |
| `kernel/src/wasm/abi2.rs:95` | `ponteiro invalido` | invalido → inválido |
| `kernel/src/wasm/abi2.rs:101` | `sem memoria exportada` | memoria → memória |
| `kernel/src/wasm/abi2.rs:103` | `ponteiro invalido` | invalido → inválido |
| `kernel/src/wasm/abi2.rs:316` | `ponteiro invalido` | invalido → inválido |
| `kernel/src/wasm/abi2.rs:335` | `ponteiro invalido` | invalido → inválido |
| `kernel/src/wasm/manager.rs:511` | `App nao encontrado` | nao → não |
| `kernel/src/wasm/manager.rs:878` | `sem memoria para a janela` | memoria → memória |
| `kernel/src/wasm/manager/runtime.rs:19` | `instancia inexistente` | instancia → (acento) |
| `kernel/src/wasm/manager/runtime.rs:22` | `modulo invalido: {e}` | modulo → módulo, invalido → inválido |
| `kernel/src/wasm/manager/runtime.rs:25` | `id de app invalido` | invalido → inválido |
| `kernel/src/wasm/manager/runtime.rs:50` | `instanciacao falhou: {}` | instanciacao → (acento) |
| `kernel/src/wasm/mod.rs:411` | `saiu com codigo {code}` | codigo → código |
| `kernel/src/wasm/mod.rs:417` | `acesso fora da memoria` | memoria → memória |
| `kernel/src/wasm/mod.rs:422` | `assinatura de chamada invalida` | invalida → inválida |
| `osjeff_core/src/tlsverify.rs:86` | `ainda nao valido` | nao → não, valido → válido |
| `osjeff_core/src/tlsverify.rs:87` | `nome nao confere` | nao → não |
| `osjeff_core/src/tlsverify.rs:88` | `autoassinado, cadeia nao confiavel` | nao → não |
| `osjeff_core/src/tlsverify.rs:89` | `cadeia nao confiavel` | nao → não |
| `osjeff_core/src/tlsverify.rs:90` | `assinatura invalida` | invalida → inválida |
| `osjeff_core/src/tlsverify.rs:91` | `autoridade invalida na cadeia` | invalida → inválida |
| `osjeff_core/src/tlsverify.rs:92` | `restricao da cadeia violada` | restricao → (acento) |
| `osjeff_core/src/tlsverify.rs:93` | `algoritmo nao suportado` | nao → não |
| `osjeff_core/src/tlsverify.rs:106` | `Certificado inválido: ainda nao valido` | nao → não, valido → válido |
| `osjeff_core/src/tlsverify.rs:107` | `Certificado inválido: nome nao confere com o site` | nao → não |
| `osjeff_core/src/tlsverify.rs:108` | `Certificado inválido: autoassinado, cadeia nao confiavel` | nao → não |
| `osjeff_core/src/tlsverify.rs:109` | `Certificado inválido: cadeia nao confiavel` | nao → não |
| `osjeff_core/src/tlsverify.rs:110` | `Certificado inválido: assinatura invalida` | invalida → inválida |
| `osjeff_core/src/tlsverify.rs:111` | `Certificado inválido: autoridade invalida na cadeia` | invalida → inválida |
| `osjeff_core/src/tlsverify.rs:112` | `Certificado inválido: restricao da cadeia violada` | restricao → (acento) |
| `osjeff_core/src/tlsverify.rs:113` | `Certificado inválido: algoritmo nao suportado` | nao → não |
| `osjeff_core/src/web/style.rs:17` | `\nhtml,body,div,p,h1,h2,h3,h4,h5,h6,ul,ol,dl,dt,dd,header,footer,article,section,nav,main,aside,blockquote,...` | area → área |

## Inglês e textos mistos numa interface em português

| Arquivo:linha | Texto | Tipo |
|---|---|---|
| `kernel/src/crash.rs:234` | `UNSUPPORTED SCREEN` | en |
| `kernel/src/crash.rs:271` | `ERROR  : {code:#018x}` | en |
| `kernel/src/crash.rs:280` | `The system is halted. Reset or power-cycle the machine to restart.\nThe same report was written to the seri...` | misto |
| `kernel/src/desktop/browser_ui.rs:133` | `load to first paint` | en |
| `kernel/src/desktop/settings_ui.rs:1233` | `Total` | en |
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
| `kernel/src/main.rs:402` | `Desktop::new (fs load from ATA) done` | en |
| `kernel/src/main.rs:410` | `wallpaper painted` | en |
| `kernel/src/main.rs:722` | `the kernel panicked` | en |
| `kernel/src/netstack.rs:721` | `tcp stream error` | en |
| `kernel/src/trace.rs:658` | `The quick brown fox jumps over the lazy dog 0123` | en |
| `kernel/src/trace.rs:736` | `The quick brown fox jumps over the lazy dog 0123` | en |
| `kernel/src/wasm/mod.rs:343` | `link host.draw_text` | en |
| `kernel/src/wasm/mod.rs:364` | `link host.time_ms` | en |
| `kernel/src/wasm/mod.rs:400` | `trap in entry` | en |
| `kernel/src/wasm/wasi.rs:381` | `link env.system` | en |
| `osjeff_core/src/appfs/mod.rs:75` | `not found` | en |
| `osjeff_core/src/appfs/mod.rs:76` | `already exists` | en |
| `osjeff_core/src/appfs/mod.rs:77` | `not a directory` | en |
| `osjeff_core/src/appfs/mod.rs:78` | `is a directory` | en |
| `osjeff_core/src/appfs/mod.rs:79` | `directory not empty` | en |
| `osjeff_core/src/appfs/mod.rs:81` | `invalid argument` | en |
| `osjeff_core/src/appfs/mod.rs:82` | `permission denied` | en |
| `osjeff_core/src/appfs/mod.rs:83` | `bad file descriptor` | en |
| `osjeff_core/src/appfs/mod.rs:84` | `too many open files` | en |
| `osjeff_core/src/appfs/mod.rs:85` | `i/o error` | en |
| `osjeff_core/src/appfs/volume_tests.rs:347` | `id={id}\nname={id}\nversion=1.0.0\n{extra}` | en |
| `osjeff_core/src/appinstall.rs:48` | `package is larger than 4 MiB` | en |
| `osjeff_core/src/appinstall.rs:50` | `an app with this id is already installed` | en |
| `osjeff_core/src/appinstall.rs:51` | `app is not installed` | en |
| `osjeff_core/src/appinstall.rs:52` | `invalid app id` | en |
| `osjeff_core/src/appinstall.rs:53` | `too many installed apps` | en |
| `osjeff_core/src/appinstall.rs:54` | `file system: {e}` | en |
| `osjeff_core/src/appmanifest.rs:162` | `manifest is too large` | en |
| `osjeff_core/src/appmanifest.rs:163` | `manifest is not valid UTF-8` | en |
| `osjeff_core/src/appmanifest.rs:164` | `manifest has too many lines` | en |
| `osjeff_core/src/appmanifest.rs:165` | `manifest syntax error` | en |
| `osjeff_core/src/appmanifest.rs:167` | `unknown manifest key` | en |
| `osjeff_core/src/appmanifest.rs:168` | `manifest key '{k}' is required` | en |
| `osjeff_core/src/appmanifest.rs:169` | `invalid value for '{k}'` | en |
| `osjeff_core/src/appmanifest.rs:170` | `'{k}' is above the system limit` | en |
| `osjeff_core/src/appmanifest.rs:577` | `icon is larger than 64 KiB` | en |
| `osjeff_core/src/appmanifest.rs:578` | `icon is not a PNG` | en |
| `osjeff_core/src/appmanifest.rs:579` | `icon is larger than 64x64` | en |
| `osjeff_core/src/appmanifest.rs:580` | `icon PNG is corrupt` | en |
| `osjeff_core/src/appmanifest.rs:613` | `package has no osjeff.manifest section` | misto |
| `osjeff_core/src/appmanifest.rs:614` | `package has two manifests` | en |
| `osjeff_core/src/appmanifest.rs:615` | `package has two icons` | en |
| `osjeff_core/src/base64.rs:29` | `invalid base64 character` | en |
| `osjeff_core/src/base64.rs:30` | `invalid base64 length` | en |
| `osjeff_core/src/base64.rs:31` | `base64 padding before the end` | en |
| `osjeff_core/src/base64.rs:32` | `base64 data too large` | misto |
| `osjeff_core/src/bmp.rs:72` | `bmp data is truncated` | misto |
| `osjeff_core/src/bmp.rs:73` | `not a bmp (missing BM)` | en |
| `osjeff_core/src/bmp.rs:74` | `unsupported bmp header size` | en |
| `osjeff_core/src/bmp.rs:76` | `invalid bmp dimensions` | en |
| `osjeff_core/src/bmp.rs:79` | `invalid bmp colour masks` | en |
| `osjeff_core/src/bmp.rs:80` | `bmp image: {e}` | en |
| `osjeff_core/src/browser/pages.rs:14` | `body{margin:0;background:#ffffff;color:#1d1d1f;font-size:15px;line-height:1.5}.w{max-width:680px;margin:0 a...` | en |
| `osjeff_core/src/browser/pages.rs:160` | `Espaço, PgDn, Home, End` | misto |
| `osjeff_core/src/compositor/sim/mod.rs:170` | `{diff} pixels differ; first at ({x},{y}): incremental {:06X}, reference {:06X}; layers there (bottom to top...` | en |
| `osjeff_core/src/compositor/sim/paint.rs:77` | `layer {:?} wrote ({x},{y}) outside its footprint {:?}` | en |
| `osjeff_core/src/editor2/mod.rs:353` | `line index out of sync with the text` | en |
| `osjeff_core/src/editor2/mod.rs:356` | `cursor past the end` | en |
| `osjeff_core/src/editor2/mod.rs:359` | `cursor not at a valid position` | en |
| `osjeff_core/src/editor2/mod.rs:362` | `selection anchor past the end` | en |
| `osjeff_core/src/editor2/mod.rs:365` | `scroll position past the last line` | en |
| `osjeff_core/src/editor2/mod.rs:368` | `empty viewport` | en |
| `osjeff_core/src/editor2/mod.rs:374` | `row wider than the window` | en |
| `osjeff_core/src/editor2/mod.rs:378` | `more rows than the window holds` | en |
| `osjeff_core/src/editor2/mod.rs:383` | `cursor drawn outside the window` | en |
| `osjeff_core/src/fileman.rs:940` | `Enter` | en |
| `osjeff_core/src/fs3/dir.rs:56` | `hole in a directory` | en |
| `osjeff_core/src/fs3/extent.rs:117` | `extent chain too long or cyclic` | en |
| `osjeff_core/src/fs3/extent.rs:166` | `invalid extent` | en |
| `osjeff_core/src/fs3/fsck.rs:118` | `in-memory bitmaps differ from the medium` | en |
| `osjeff_core/src/fs3/fsck.rs:121` | `free block counter` | en |
| `osjeff_core/src/fs3/fsck.rs:124` | `free inode counter` | en |
| `osjeff_core/src/fs3/fsck.rs:157` | `directory size != blocks` | en |
| `osjeff_core/src/fs3/fsck.rs:175` | `invalid name in directory` | en |
| `osjeff_core/src/fs3/fsck.rs:178` | `duplicate name in directory` | en |
| `osjeff_core/src/fs3/fsck.rs:182` | `entry points at an unallocated inode` | en |
| `osjeff_core/src/fs3/fsck.rs:205` | `entry kind differs from inode kind` | en |
| `osjeff_core/src/fs3/fsck.rs:216` | `trash flag disagrees with the location` | en |
| `osjeff_core/src/fs3/fsck.rs:219` | `trashed entry lacks its original name` | en |
| `osjeff_core/src/fs3/fsck.rs:230` | `root must hold exactly one .trash entry` | en |
| `osjeff_core/src/fs3/fsck.rs:245` | `extent list invalid` | en |
| `osjeff_core/src/fs3/fsck.rs:253` | `extent chain invalid` | en |
| `osjeff_core/src/fs3/fsck.rs:263` | `block owned by two structures` | en |
| `osjeff_core/src/fs3/fsck.rs:272` | `block owned by two structures` | en |
| `osjeff_core/src/fs3/fsck.rs:279` | `nblocks != sum of extent lengths` | en |
| `osjeff_core/src/fs3/fsck.rs:293` | `extent beyond end of file` | en |
| `osjeff_core/src/fs3/fsck.rs:303` | `non-zero bytes past end of file` | en |
| `osjeff_core/src/fs3/fsck.rs:314` | `allocated inode is unreachable` | en |
| `osjeff_core/src/fs3/fsck.rs:326` | `block in use but marked free` | en |
| `osjeff_core/src/fs3/fsck.rs:328` | `block marked used but unowned (leak)` | en |
| `osjeff_core/src/fs3/mod.rs:500` | `fsck found problems` | en |
| `osjeff_core/src/fs3/mod.rs:535` | `bitmap leaves the metadata uncovered` | en |
| `osjeff_core/src/fs3/mod.rs:538` | `root/trash inode not allocated` | en |
| `osjeff_core/src/fs3/mod.rs:581` | `journal target out of range` | en |
| `osjeff_core/src/fs3/mod.rs:599` | `bad root inode` | en |
| `osjeff_core/src/fs3/mod.rs:603` | `bad trash inode` | en |
| `osjeff_core/src/fs3/mod.rs:607` | `root has no .trash entry` | misto |
| `osjeff_core/src/fs3/mod.rs:728` | `metadata block out of range` | en |
| `osjeff_core/src/fs3/mod.rs:803` | `double free of an inode` | en |
| `osjeff_core/src/fs3/mod.rs:827` | `block range outside the data region` | misto |
| `osjeff_core/src/fs3/mod.rs:831` | `block allocated or freed twice` | en |
| `osjeff_core/src/fs3/ops.rs:121` | `directory entry points at a free inode` | en |
| `osjeff_core/src/fs3/ops.rs:190` | `inode missing from its parent` | en |
| `osjeff_core/src/fs3/ops.rs:321` | `directory entry changed under us` | en |
| `osjeff_core/src/i18n/audit.rs:795` | `{file}:{line}: {w:?} should be {r} in {s:?}` | en |
| `osjeff_core/src/icmp.rs:247` | `destination unreachable (code {c})` | en |
| `osjeff_core/src/icmp.rs:248` | `time exceeded` | en |
| `osjeff_core/src/icmp.rs:249` | `no route to host` | misto |
| `osjeff_core/src/icmp.rs:250` | `host did not answer ARP` | en |
| `osjeff_core/src/icmp.rs:251` | `invalid target address` | en |
| `osjeff_core/src/icmp.rs:252` | `network unavailable` | en |
| `osjeff_core/src/icmp.rs:253` | `network busy` | en |
| `osjeff_core/src/image.rs:56` | `image has a zero dimension` | en |
| `osjeff_core/src/image.rs:57` | `image is larger than the pixel limit` | en |
| `osjeff_core/src/image.rs:58` | `buffer length is wrong for the dimensions` | en |
| `osjeff_core/src/image.rs:59` | `rectangle is outside the image` | en |
| `osjeff_core/src/image.rs:60` | `out of memory` | en |
| `osjeff_core/src/image.rs:172` | `Image({}x{})` | en |
| `osjeff_core/src/image.rs:799` | `unknown image format` | en |
| `osjeff_core/src/inflate.rs:62` | `compressed data is truncated` | misto |
| `osjeff_core/src/inflate.rs:63` | `reserved deflate block type` | en |
| `osjeff_core/src/inflate.rs:64` | `stored block length check failed` | en |
| `osjeff_core/src/inflate.rs:65` | `invalid huffman code lengths` | en |
| `osjeff_core/src/inflate.rs:66` | `block has no end-of-block code` | misto |
| `osjeff_core/src/inflate.rs:68` | `invalid huffman code` | en |
| `osjeff_core/src/inflate.rs:69` | `back-reference too far` | en |
| `osjeff_core/src/inflate.rs:70` | `decompressed data exceeds the limit` | misto |
| `osjeff_core/src/inflate.rs:71` | `bad zlib header` | en |
| `osjeff_core/src/inflate.rs:74` | `out of memory` | en |
| `osjeff_core/src/netstats.rs:353` | `no address` | misto |
| `osjeff_core/src/png.rs:145` | `not a png (bad signature)` | en |
| `osjeff_core/src/png.rs:146` | `png is truncated` | en |
| `osjeff_core/src/png.rs:149` | `png has no IHDR first` | misto |
| `osjeff_core/src/png.rs:150` | `invalid png IHDR` | en |
| `osjeff_core/src/png.rs:151` | `invalid png dimensions` | en |
| `osjeff_core/src/png.rs:152` | `invalid png palette or transparency` | en |
| `osjeff_core/src/png.rs:154` | `png has no IDAT` | misto |
| `osjeff_core/src/png.rs:155` | `png chunks out of order` | en |
| `osjeff_core/src/png.rs:156` | `unknown critical png chunk` | en |
| `osjeff_core/src/png.rs:158` | `png image data too short` | misto |
| `osjeff_core/src/png.rs:159` | `png image data too long` | misto |
| `osjeff_core/src/png.rs:160` | `invalid png filter type` | en |
| `osjeff_core/src/png.rs:161` | `png palette index out of range` | en |
| `osjeff_core/src/png.rs:162` | `png image: {e}` | en |
| `osjeff_core/src/ppm.rs:43` | `not a P3/P6 ppm` | en |
| `osjeff_core/src/ppm.rs:44` | `ppm data is truncated` | misto |
| `osjeff_core/src/ppm.rs:45` | `invalid ppm header` | en |
| `osjeff_core/src/ppm.rs:46` | `invalid ppm sample` | en |
| `osjeff_core/src/ppm.rs:47` | `ppm image: {e}` | en |
| `osjeff_core/src/shell/exec.rs:984` | `{name}.sh` | en |
| `osjeff_core/src/sntp.rs:145` | `not a server reply` | en |
| `osjeff_core/src/sntp.rs:148` | `bad stratum` | en |
| `osjeff_core/src/sntp.rs:150` | `bad server timestamps` | en |
| `osjeff_core/src/sntp.rs:152` | `implausible date` | en |
| `osjeff_core/src/wallpaper.rs:359` | `file too big` | en |
| `osjeff_core/src/wallpaper.rs:360` | `not PNG/BMP/PPM` | en |
| `osjeff_core/src/wallpaper.rs:361` | `image too big` | en |
| `osjeff_core/src/wasmsec.rs:49` | `not a wasm module (too short)` | en |
| `osjeff_core/src/wasmsec.rs:50` | `not a wasm module (bad magic)` | en |
| `osjeff_core/src/wasmsec.rs:53` | `section extends past the end of the file` | en |
| `osjeff_core/src/wasmsec.rs:54` | `unknown section id` | en |
| `osjeff_core/src/wasmsec.rs:55` | `too many sections` | en |
| `osjeff_core/src/web/style.rs:17` | `\nhtml,body,div,p,h1,h2,h3,h4,h5,h6,ul,ol,dl,dt,dd,header,footer,article,section,nav,main,aside,blockquote,...` | misto |
| `osjeff_core/src/web/style.rs:460` | `courier new` | en |
| `osjeff_core/src/web/style.rs:468` | `source code pro` | en |

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

**`kernel/src/desktop/settings_ui.rs`** (109)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 454 | `Aparência` | pt | `settings.aparencia` |
| 455 | `Tema` | pt | `settings.tema` |
| 458 | `Tema` | pt | `settings.tema_2` |
| 465 | `Automático` | pt | `settings.automatico` |
| 465 | `Claro` | pt | `settings.claro` |
| 465 | `Escuro` | pt | `settings.escuro` |
| 467 | `Cor de destaque` | pt | `settings.cor_destaque` |
| 509 | `Movimento` | neutro | `settings.movimento` |
| 515 | `Reduzir movimento` | neutro | `settings.reduzir_movimento` |
| 516 | `As transições terminam na hora` | pt | `settings.as_transicoes_terminam_na` |
| 521 | `Notificações` | pt | `settings.notificacoes` |
| 524 | `Mostrar notificações` | pt | `settings.mostrar_notificacoes` |
| 529 | `Duração` | pt | `settings.duracao` |
| 540 | `Papel de parede` | pt | `settings.papel_parede` |
| 584 | `Sua imagem` | pt | `settings.sua_imagem` |
| 604 | `Imagem do usuário` | pt | `settings.imagem_usuario` |
| 609 | `ex.: /Imagens/praia.png` | pt | `settings.ex_imagens_praia_png` |
| 612 | `Aplicar` | pt | `settings.aplicar` |
| 619 | `Escolher imagem…` | pt | `settings.escolher_imagem` |
| 650 | `Barra de apps` | pt | `settings.barra_apps` |
| 651 | `Uso` | neutro | `settings.uso` |
| 654 | `Arraste um ícone fixado para outra posição` | pt | `settings.arraste_icone_fixado_para` |
| 657 | `Botão direito no ícone, em Fixar ou Desafixar` | pt | `settings.botao_direito_no_icone` |
| 659 | `Nova janela` | pt | `settings.nova_janela` |
| 659 | `Shift + clique no ícone do app` | pt | `settings.shift_clique_no_icone` |
| 740 | `Teclado` | pt | `settings.teclado` |
| 741 | `Disposição` | pt | `settings.disposicao` |
| 744 | `Padrão internacional` | pt | `settings.padrao_internacional` |
| 745 | `Português do Brasil: ç e acentos` | pt | `settings.portugues_brasil_c_acentos` |
| 759 | `Teste` | neutro | `settings.teste` |
| 764 | `Digite aqui para experimentar` | pt | `settings.digite_aqui_para_experimentar` |
| 769 | `Data e hora` | pt | `settings.data_hora` |
| 772 | `Agora` | pt | `settings.agora` |
| 825 | `{}, {} de {} de {}` | pt | `settings.text` |
| 840 | `UTC {:02}:{:02}:{:02}` | neutro | `settings.utc_02_02_02` |
| 850 | `Formato` | neutro | `settings.formato` |
| 853 | `Relógio de 24 horas` | pt | `settings.relogio_24_horas` |
| 856 | `Fuso horário` | pt | `settings.fuso_horario` |
| 863 | `Buscar cidade` | pt | `settings.buscar_cidade` |
| 919 | `Nenhuma cidade encontrada.` | pt | `settings.nenhuma_cidade_encontrada` |
| 931 | `Ajustar data e hora` | pt | `settings.ajustar_data_hora` |
| 934 | `Dia` | pt | `settings.dia` |
| 935 | `Mês` | pt | `settings.mes` |
| 936 | `Ano` | pt | `settings.ano` |
| 937 | `Hora` | pt | `settings.hora` |
| 1000 | `Ler do relógio` | pt | `settings.ler_relogio` |
| 1007 | `Ajustar` | neutro | `settings.ajustar` |
| 1039 | `Rede` | pt | `settings.rede` |
| 1043 | `Sem placa de rede` | pt | `settings.sem_placa_rede` |
| 1045 | `Sem sinal` | pt | `settings.sem_sinal` |
| 1047 | `Procurando endereço` | pt | `settings.procurando_endereco` |
| 1049 | `Conectado` | pt | `settings.conectado` |
| 1051 | `Conexão` | pt | `settings.conexao` |
| 1069 | `Endereços` | pt | `settings.enderecos` |
| 1116 | `Estático` | pt | `settings.estatico` |
| 1118 | `{} restantes` | neutro | `settings.restantes` |
| 1120 | `Sem expiração` | pt | `settings.sem_expiracao` |
| 1123 | `Endereço IP` | pt | `settings.endereco_ip` |
| 1124 | `Máscara` | pt | `settings.mascara` |
| 1125 | `Roteador` | neutro | `settings.roteador` |
| 1127 | `Concessão` | pt | `settings.concessao` |
| 1128 | `Dispositivo e tráfego` | pt | `settings.dispositivo_trafego` |
| 1134 | `Endereço físico` | pt | `settings.endereco_fisico` |
| 1147 | `Recebido` | pt | `settings.recebido` |
| 1148 | `Enviado` | pt | `settings.enviado` |
| 1150 | `{} recebidos · {} enviados` | neutro | `settings.recebidos_enviados` |
| 1154 | `Pacotes` | neutro | `settings.pacotes` |
| 1158 | `Disco` | pt | `settings.disco` |
| 1163 | `Disco principal` | pt | `settings.disco_principal` |
| 1164 | `Sistema de arquivos OJFS v3` | pt | `settings.sistema_arquivos_ojfs_v3` |
| 1167 | `Memória (sem disco)` | pt | `settings.memoria_sem_disco` |
| 1168 | `Os arquivos somem ao desligar` | pt | `settings.os_arquivos_somem_ao` |
| 1172 | `Volume` | neutro | `settings.volume` |
| 1219 | `Usado` | neutro | `settings.usado` |
| 1226 | `Livre` | neutro | `settings.livre` |
| 1233 | `Total` | en | `settings.total` |
| 1241 | `Arquivos e pastas` | pt | `settings.arquivos_pastas` |
| 1242 | `Dispositivos` | neutro | `settings.dispositivos` |
| 1244 | `Disco de inicialização` | pt | `settings.disco_inicializacao` |
| 1244 | `Disco de arquivos` | pt | `settings.disco_arquivos` |
| 1261 | `Energia` | neutro | `settings.energia` |
| 1264 | `Reiniciar` | pt | `settings.reiniciar` |
| 1264 | `Reiniciar…` | pt | `settings.reiniciar_2` |
| 1265 | `Desligar` | pt | `settings.desligar` |
| 1265 | `Desligar…` | pt | `settings.desligar_2` |
| 1287 | `Sobre` | pt | `settings.sobre` |
| 1302 | `Versão {} · compilação {}` | pt | `settings.versao_compilacao` |
| 1305 | `de depuração` | pt | `settings.depuracao` |
| 1334 | `Processador` | neutro | `settings.processador` |
| 1338 | `Memória` | pt | `settings.memoria` |
| 1340 | `Tempo ligado` | pt | `settings.tempo_ligado` |
| 1342 | `{} × {} pixels` | neutro | `settings.pixels` |
| 1344 | `Tela` | pt | `settings.tela` |
| 1349 | ` · máquina virtual` | pt | `settings.maquina_virtual` |
| 1353 | `Inicialização` | pt | `settings.inicializacao` |
| 1355 | `Fonte forte de números aleatórios` | pt | `settings.fonte_forte_numeros_aleatorios` |
| 1356 | `Números aleatórios de fonte mista` | pt | `settings.numeros_aleatorios_fonte_mista` |
| 1357 | `Números aleatórios de fonte fraca` | pt | `settings.numeros_aleatorios_fonte_fraca` |
| 1359 | `Segurança` | pt | `settings.seguranca` |
| 1476 | `Caminho inválido para papel de parede (use letras, números . _ - /).` | pt | `settings.caminho_invalido_para_papel` |
| 1480 | `Arquivo não encontrado.` | pt | `settings.arquivo_nao_encontrado` |
| 1483 | `Imagem recusada: {e}` | pt | `settings.imagem_recusada` |
| 1490 | `Papel de parede aplicado, mas não foi salvo.` | pt | `settings.papel_parede_aplicado_mas` |
| 1517 | `Papel de parede aplicado.` | pt | `settings.papel_parede_aplicado` |
| 1708 | `Escolha uma imagem: digite o caminho abaixo ou use o Arquivos.` | pt | `settings.escolha_imagem_digite_caminho` |
| 1727 | `No Arquivos, clique com o botão direito na imagem e escolha “Definir como papel de parede”.` | pt | `settings.no_arquivos_clique_com` |
| 1786 | `Lido do relógio.` | pt | `settings.lido_relogio` |
| 1793 | `Relógio ajustado.` | pt | `settings.relogio_ajustado` |
| 1796 | `Data inválida.` | pt | `settings.data_invalida` |

**`osjeff_core/src/settings.rs`** (14)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 65 | `Cidade do México` | pt | `settings.cidade_mexico` |
| 67 | `Nova York` | pt | `settings.nova_york` |
| 68 | `Bogotá` | pt | `settings.bogota` |
| 75 | `São Paulo` | pt | `settings.sao_paulo` |
| 76 | `Brasília` | pt | `settings.brasilia` |
| 77 | `Fernando de Noronha` | pt | `settings.fernando_noronha` |
| 78 | `Açores` | pt | `settings.acores` |
| 91 | `Nairóbi` | pt | `settings.nairobi` |
| 92 | `Teerã` | pt | `settings.teera` |
| 96 | `Nova Délhi` | pt | `settings.nova_delhi` |
| 102 | `Tóquio` | pt | `settings.toquio` |
| 106 | `Ilhas Salomão` | pt | `settings.ilhas_salomao` |
| 121 | `UTC{}{:02}:{:02}` | neutro | `settings.utc_02_02` |
| 198 | `Âmbar` | pt | `settings.ambar` |

**`osjeff_core/src/wallpaper.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 247 | `Crepúsculo` | pt | `settings.crepusculo` |
| 290 | `Papel` | pt | `settings.papel` |
| 308 | `Pôr do sol` | pt | `settings.por_sol` |
| 359 | `file too big` | en | `settings.file_too_big` |
| 360 | `not PNG/BMP/PPM` | en | `settings.not_png_bmp_ppm` |
| 361 | `image too big` | en | `settings.image_too_big` |

### Arquivos

**`osjeff_core/src/fileman.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 940 | `Enter` | en | `files.enter` |

**`osjeff_core/src/fs3/dir.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 56 | `hole in a directory` | en | `files.hole_in_directory` |

**`osjeff_core/src/fs3/extent.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 117 | `extent chain too long or cyclic` | en | `files.extent_chain_too_long` |
| 166 | `invalid extent` | en | `files.invalid_extent` |

**`osjeff_core/src/fs3/fsck.rs`** (21)

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

**`osjeff_core/src/fs3/mod.rs`** (11)

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

**`osjeff_core/src/fs3/ops.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 121 | `directory entry points at a free inode` | en | `files.directory_entry_points_at` |
| 190 | `inode missing from its parent` | en | `files.inode_missing_from_its` |
| 321 | `directory entry changed under us` | en | `files.directory_entry_changed_under` |

### Editor

**`osjeff_core/src/editor2/mod.rs`** (9)

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

**`osjeff_core/src/shell/builtins.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 1037 | `export {n}="{v}"\n` | neutro | `term.export_n_v` |
| 1109 | `alias {k}='{v}'\n` | neutro | `term.alias_k_v` |
| 1183 | `{n}.sh` | neutro | `term.n_sh` |

**`osjeff_core/src/shell/exec.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 458 | `sh: {}\n` | neutro | `term.sh` |
| 477 | `sh: {}\n` | neutro | `term.sh_2` |
| 984 | `{name}.sh` | en | `term.name_sh` |

**`osjeff_core/src/shell/netcmds.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 263 | `  inet {ip}/{}` | neutro | `term.inet_ip` |
| 265 | `  gateway {g}` | neutro | `term.gateway_g` |

### Tarefas

**`osjeff_core/src/netstats.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 339 | `net: {} link={} tx={}/{}B err={} drop={} rx={}/{}B err={} drop={} \| ` | neutro | `tasks.net_link_tx_b` |
| 353 | `no address` | misto | `tasks.no_address` |
| 356 | ` lease={}s` | neutro | `tasks.lease_s` |
| 362 | ` dhcp={} renew={} rebind={} lost={} \| dns q={} hit={} failover={} fail={} \| ping {}/{}` | neutro | `tasks.dhcp_renew_rebind_lost` |

### Imagens

**`osjeff_core/src/bmp.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 72 | `bmp data is truncated` | misto | `viewer.bmp_data_is_truncated` |
| 73 | `not a bmp (missing BM)` | en | `viewer.not_bmp_missing_bm` |
| 74 | `unsupported bmp header size` | en | `viewer.unsupported_bmp_header_size` |
| 76 | `invalid bmp dimensions` | en | `viewer.invalid_bmp_dimensions` |
| 79 | `invalid bmp colour masks` | en | `viewer.invalid_bmp_colour_masks` |
| 80 | `bmp image: {e}` | en | `viewer.bmp_image` |

**`osjeff_core/src/image.rs`** (10)

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

**`osjeff_core/src/inflate.rs`** (10)

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

**`osjeff_core/src/png.rs`** (15)

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

**`osjeff_core/src/ppm.rs`** (5)

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
| 721 | `tcp stream error` | en | `web.tcp_stream_error` |

**`osjeff_core/src/browser.rs`** (20)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 77 | `Não seguro` | pt | `web.nao_seguro` |
| 78 | `Conexão segura` | pt | `web.conexao_segura` |
| 79 | `Certificado inválido` | pt | `web.certificado_invalido` |
| 126 | `Falha ao carregar a página.` | pt | `web.falha_ao_carregar_pagina` |
| 127 | `Nome não encontrado: confira o endereço (DNS).` | pt | `web.nome_nao_encontrado_confira` |
| 128 | `Conexão recusada pelo servidor.` | pt | `web.conexao_recusada_pelo_servidor` |
| 129 | `Tempo esgotado: o servidor não respondeu.` | pt | `web.tempo_esgotado_servidor_nao` |
| 130 | `Falha na negociação TLS (conexão segura).` | pt | `web.falha_na_negociacao_tls` |
| 132 | `Bloqueado: redirecionamento de HTTPS para HTTP.` | pt | `web.bloqueado_redirecionamento_https_para` |
| 133 | `Redirecionamento inválido.` | pt | `web.redirecionamento_invalido` |
| 134 | `Redirecionamento em ciclo.` | pt | `web.redirecionamento_em_ciclo` |
| 136 | `O carregador de páginas falhou (thread encerrada).` | pt | `web.carregador_paginas_falhou_thread` |
| 458 | `Página cortada no limite de tamanho` | pt | `web.pagina_cortada_no_limite` |
| 459 | `Página incompleta: a conexão foi interrompida` | pt | `web.pagina_incompleta_conexao_foi` |
| 460 | `Página parcial: os dados recebidos estão corrompidos` | pt | `web.pagina_parcial_os_dados` |
| 461 | `A verificação da página falhou` | pt | `web.verificacao_pagina_falhou` |
| 939 | `&amp;` | neutro | `web.amp` |
| 940 | `&lt;` | neutro | `web.lt` |
| 941 | `&gt;` | neutro | `web.gt` |
| 942 | `&quot;` | neutro | `web.quot` |

**`osjeff_core/src/browser/body_tests.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 39 | `HTTP/1.1 200 OKr\n{headers}r\n` | neutro | `web.http_1_1_200` |

**`osjeff_core/src/browser/errors.rs`** (29)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 34 | `O certificado do site expirou.` | pt | `web.certificado_site_expirou` |
| 35 | `O certificado do site ainda não é válido.` | pt | `web.certificado_site_ainda_nao` |
| 36 | `O certificado não vale para este endereço.` | pt | `web.certificado_nao_vale_para` |
| 37 | `O certificado foi emitido pelo próprio site.` | pt | `web.certificado_foi_emitido_pelo` |
| 38 | `Quem emitiu o certificado não é confiável.` | pt | `web.quem_emitiu_certificado_nao` |
| 39 | `A assinatura do certificado não confere.` | pt | `web.assinatura_certificado_nao_confere` |
| 40 | `A hora do sistema ainda não foi confirmada.` | pt | `web.hora_sistema_ainda_nao` |
| 41 | `O certificado do site está malformado.` | pt | `web.certificado_site_esta_malformado` |
| 43 | `A cadeia de certificados do site não é aceitável.` | pt | `web.cadeia_certificados_site_nao` |
| 45 | `O certificado usa um recurso que não é suportado.` | pt | `web.certificado_usa_recurso_que` |
| 46 | `Não foi possível verificar o certificado do site.` | pt | `web.nao_foi_possivel_verificar` |
| 55 | `Sem conexão` | pt | `web.sem_conexao` |
| 56 | `Confira a rede e tente de novo.` | pt | `web.confira_rede_tente_novo` |
| 60 | `Site não encontrado` | pt | `web.site_nao_encontrado` |
| 61 | `Não achamos o servidor. Confira o endereço digitado.` | pt | `web.nao_achamos_servidor_confira` |
| 65 | `Conexão recusada` | pt | `web.conexao_recusada` |
| 66 | `O servidor não aceitou a conexão.` | pt | `web.servidor_nao_aceitou_conexao` |
| 70 | `Tempo esgotado` | pt | `web.tempo_esgotado` |
| 71 | `O servidor demorou demais para responder.` | pt | `web.servidor_demorou_demais_para` |
| 75 | `Conexão segura recusada` | pt | `web.conexao_segura_recusada` |
| 76 | `Não foi possível abrir uma conexão segura com o site.` | pt | `web.nao_foi_possivel_abrir` |
| 80 | `Esta conexão não é segura` | pt | `web.esta_conexao_nao_segura` |
| 86 | `O site tentou sair de uma conexão segura para uma sem proteção.` | pt | `web.site_tentou_sair_conexao` |
| 90 | `Redirecionamento inválido` | pt | `web.redirecionamento_invalido` |
| 91 | `O endereço para onde o site envia você não é válido.` | pt | `web.endereco_para_onde_site` |
| 95 | `Redirecionamento em ciclo` | pt | `web.redirecionamento_em_ciclo` |
| 96 | `O site volta sempre para um endereço já visitado.` | pt | `web.site_volta_sempre_para` |
| 101 | `O site redirecionou mais vezes do que o permitido.` | pt | `web.site_redirecionou_mais_vezes` |
| 106 | `O carregador de páginas parou de responder.` | pt | `web.carregador_paginas_parou_responder` |

**`osjeff_core/src/browser/pages.rs`** (24)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 14 | `body{margin:0;background:#ffffff;color:#1d1d1f;font-size:15px;line-height:1.5}.w{max-width:680px;margin:0 a...` | en | `web.body_margin_0_background` |
| 45 | `Início` | pt | `web.inicio` |
| 46 | `Favoritos` | pt | `web.favoritos` |
| 47 | `Histórico` | pt | `web.historico` |
| 48 | `Sobre` | pt | `web.sobre` |
| 76 | `{n} favoritos` | pt | `web.n_favoritos` |
| 78 | `Favoritos` | pt | `web.favoritos_2` |
| 78 | `Favoritos` | pt | `web.favoritos_3` |
| 115 | `1 página` | pt | `web.1_pagina` |
| 116 | `{n} páginas` | pt | `web.n_paginas` |
| 118 | `Histórico` | pt | `web.historico_2` |
| 118 | `Histórico` | pt | `web.historico_3` |
| 138 | `Sobre o Navegador` | pt | `web.sobre_navegador` |
| 141 | `O navegador do OSjeff.` | pt | `web.navegador_osjeff` |
| 149 | `Nova aba` | pt | `web.nova_aba` |
| 150 | `Fechar aba` | pt | `web.fechar_aba` |
| 151 | `Próxima aba` | pt | `web.proxima_aba` |
| 152 | `Ir para a aba (9 é a última)` | pt | `web.ir_para_aba_9` |
| 153 | `Endereço` | pt | `web.endereco` |
| 154 | `Adicionar aos favoritos` | pt | `web.adicionar_aos_favoritos` |
| 155 | `Buscar na página` | pt | `web.buscar_na_pagina` |
| 157 | `Voltar e avançar` | pt | `web.voltar_avancar` |
| 158 | `Recarregar` | pt | `web.recarregar` |
| 160 | `Espaço, PgDn, Home, End` | misto | `web.espaco_pgdn_home_end` |

**`osjeff_core/src/browser/tabs.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 181 | `Favoritos` | pt | `web.favoritos` |
| 182 | `Histórico` | pt | `web.historico` |
| 183 | `Sobre o Navegador` | pt | `web.sobre_navegador` |
| 184 | `Nova aba` | pt | `web.nova_aba` |
| 189 | `Nova aba` | pt | `web.nova_aba_2` |

**`osjeff_core/src/icmp.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 247 | `destination unreachable (code {c})` | en | `web.destination_unreachable_code_c` |
| 248 | `time exceeded` | en | `web.time_exceeded` |
| 249 | `no route to host` | misto | `web.no_route_host` |
| 250 | `host did not answer ARP` | en | `web.host_did_not_answer` |
| 251 | `invalid target address` | en | `web.invalid_target_address` |
| 252 | `network unavailable` | en | `web.network_unavailable` |
| 253 | `network busy` | en | `web.network_busy` |

**`osjeff_core/src/net.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 788 | `{}/{} gw ` | neutro | `web.gw` |
| 793 | ` dns {}` | neutro | `web.dns` |

**`osjeff_core/src/sntp.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 145 | `not a server reply` | en | `web.not_server_reply` |
| 148 | `bad stratum` | en | `web.bad_stratum` |
| 150 | `bad server timestamps` | en | `web.bad_server_timestamps` |
| 152 | `implausible date` | en | `web.implausible_date` |

**`osjeff_core/src/tlsverify.rs`** (23)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 86 | `ainda nao valido` | pt sem-acento | `web.ainda_nao_valido` |
| 87 | `nome nao confere` | pt sem-acento | `web.nome_nao_confere` |
| 88 | `autoassinado, cadeia nao confiavel` | pt sem-acento | `web.autoassinado_cadeia_nao_confiavel` |
| 89 | `cadeia nao confiavel` | pt sem-acento | `web.cadeia_nao_confiavel` |
| 90 | `assinatura invalida` | pt sem-acento | `web.assinatura_invalida` |
| 91 | `autoridade invalida na cadeia` | pt sem-acento | `web.autoridade_invalida_na_cadeia` |
| 92 | `restricao da cadeia violada` | pt sem-acento | `web.restricao_cadeia_violada` |
| 93 | `algoritmo nao suportado` | pt sem-acento | `web.algoritmo_nao_suportado` |
| 94 | `hora do sistema incorreta` | pt | `web.hora_sistema_incorreta` |
| 102 | `Certificado inválido: certificado malformado` | pt | `web.certificado_invalido_certificado_malformado` |
| 103 | `Certificado inválido: certificado grande demais` | pt | `web.certificado_invalido_certificado_grande` |
| 104 | `Certificado inválido: cadeia longa demais` | pt | `web.certificado_invalido_cadeia_longa` |
| 105 | `Certificado inválido: expirado` | pt | `web.certificado_invalido_expirado` |
| 106 | `Certificado inválido: ainda nao valido` | pt sem-acento | `web.certificado_invalido_ainda_nao` |
| 107 | `Certificado inválido: nome nao confere com o site` | pt sem-acento | `web.certificado_invalido_nome_nao` |
| 108 | `Certificado inválido: autoassinado, cadeia nao confiavel` | pt sem-acento | `web.certificado_invalido_autoassinado_cadeia` |
| 109 | `Certificado inválido: cadeia nao confiavel` | pt sem-acento | `web.certificado_invalido_cadeia_nao` |
| 110 | `Certificado inválido: assinatura invalida` | pt sem-acento | `web.certificado_invalido_assinatura_invalida` |
| 111 | `Certificado inválido: autoridade invalida na cadeia` | pt sem-acento | `web.certificado_invalido_autoridade_invalida` |
| 112 | `Certificado inválido: restricao da cadeia violada` | pt sem-acento | `web.certificado_invalido_restricao_cadeia` |
| 113 | `Certificado inválido: algoritmo nao suportado` | pt sem-acento | `web.certificado_invalido_algoritmo_nao` |
| 114 | `Certificado inválido: hora do sistema incorreta` | pt | `web.certificado_invalido_hora_sistema` |
| 115 | `Certificado inválido: cadeia rejeitada` | pt | `web.certificado_invalido_cadeia_rejeitada` |

**`osjeff_core/src/web/form.rs`** (8)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 106 | `Formulários POST não são suportados.` | pt | `web.formularios_post_nao_sao` |
| 107 | `Formulário grande demais para enviar.` | pt | `web.formulario_grande_demais_para` |
| 108 | `Formulário inválido.` | pt | `web.formulario_invalido` |
| 180 | `AÁEÉIÍOÓUÚCÇYÝ` | pt | `web.aaeeiioouuccyy` |
| 184 | `AÀEÈIÌOÒUÙ` | pt | `web.aaeeiioouu` |
| 186 | `AÃOÕNÑ` | pt | `web.aaoonn` |
| 189 | `AÂEÊIÎOÔUÛ` | pt | `web.aaeeiioouu_2` |
| 193 | `AÄEËIÏOÖUÜ` | pt | `web.aaeeiioouu_3` |

**`osjeff_core/src/web/imgcache.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 72 | `formato não suportado` | pt | `web.formato_nao_suportado` |
| 73 | `falha ao carregar` | pt | `web.falha_ao_carregar` |
| 74 | `imagem grande demais` | pt | `web.imagem_grande_demais` |
| 75 | `limite de imagens` | pt | `web.limite_imagens` |
| 241 | `data:#{:016x}-{}` | pt | `web.data_016x` |

**`osjeff_core/src/web/style.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 17 | `\nhtml,body,div,p,h1,h2,h3,h4,h5,h6,ul,ol,dl,dt,dd,header,footer,article,section,nav,main,aside,blockquote,...` | misto sem-acento | `web.html_body_div_p` |
| 460 | `courier new` | en | `web.courier_new` |
| 468 | `source code pro` | en | `web.source_code_pro` |

### Apps de terceiros (WASM)

**`kernel/src/desktop/wasmwin.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 94 | `{}/{}.wasm` | neutro | `apps.wasm` |
| 172 | `pacote nao encontrado` | pt sem-acento | `apps.pacote_nao_encontrado` |

**`kernel/src/wasm/abi2.rs`** (9)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 89 | `sem ABI v2` | pt | `apps.sem_abi_v2` |
| 94 | `sem memoria exportada` | pt sem-acento | `apps.sem_memoria_exportada` |
| 95 | `ponteiro invalido` | pt sem-acento | `apps.ponteiro_invalido` |
| 101 | `sem memoria exportada` | pt sem-acento | `apps.sem_memoria_exportada_2` |
| 103 | `ponteiro invalido` | pt sem-acento | `apps.ponteiro_invalido_2` |
| 316 | `ponteiro invalido` | pt sem-acento | `apps.ponteiro_invalido_3` |
| 322 | `sem ABI v2` | pt | `apps.sem_abi_v2_2` |
| 335 | `ponteiro invalido` | pt sem-acento | `apps.ponteiro_invalido_4` |
| 341 | `sem ABI v2` | pt | `apps.sem_abi_v2_3` |

**`kernel/src/wasm/manager.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 511 | `App nao encontrado` | pt sem-acento | `apps.app_nao_encontrado` |
| 515 | `O app encerrou: {}` | neutro | `apps.app_encerrou` |
| 518 | `O app encerrou: {}` | neutro | `apps.app_encerrou_2` |
| 523 | `Carregando app...` | pt | `apps.carregando_app` |
| 878 | `sem memoria para a janela` | pt sem-acento | `apps.sem_memoria_para_janela` |

**`kernel/src/wasm/manager/runtime.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 19 | `instancia inexistente` | neutro sem-acento | `apps.instancia_inexistente` |
| 22 | `modulo invalido: {e}` | pt sem-acento | `apps.modulo_invalido` |
| 25 | `id de app invalido` | pt sem-acento | `apps.id_app_invalido` |
| 50 | `instanciacao falhou: {}` | pt sem-acento | `apps.instanciacao_falhou` |
| 56 | `_initialize: {}` | neutro | `apps.initialize` |
| 156 | `sem runtime` | pt | `apps.sem_runtime` |
| 187 | `sem framebuffer` | pt | `apps.sem_framebuffer` |

**`kernel/src/wasm/mod.rs`** (13)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 343 | `link host.draw_text` | en | `apps.link_host_draw_text` |
| 364 | `link host.time_ms` | en | `apps.link_host_time_ms` |
| 399 | `no entry export` | pt | `apps.no_entry_export` |
| 400 | `trap in entry` | en | `apps.trap_in_entry` |
| 408 | `falta de combustivel (laco infinito?)` | pt | `apps.falta_combustivel_laco_infinito` |
| 411 | `saiu com codigo {code}` | pt sem-acento | `apps.saiu_com_codigo_code` |
| 417 | `acesso fora da memoria` | pt sem-acento | `apps.acesso_fora_memoria` |
| 418 | `panico (unreachable)` | neutro | `apps.panico_unreachable` |
| 419 | `estouro de pilha` | pt | `apps.estouro_pilha` |
| 420 | `divisao por zero` | pt | `apps.divisao_por_zero` |
| 421 | `chamada indireta nula` | neutro | `apps.chamada_indireta_nula` |
| 422 | `assinatura de chamada invalida` | pt sem-acento | `apps.assinatura_chamada_invalida` |
| 424 | `erro do guest` | pt | `apps.erro_guest` |

**`kernel/src/wasm/wasi.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 381 | `link env.system` | en | `apps.link_env_system` |

**`osjeff_core/src/appfs/mod.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 75 | `not found` | en | `apps.not_found` |
| 76 | `already exists` | en | `apps.already_exists` |
| 77 | `not a directory` | en | `apps.not_directory` |
| 78 | `is a directory` | en | `apps.is_directory` |
| 79 | `directory not empty` | en | `apps.directory_not_empty` |
| 80 | `no space left (quota)` | pt | `apps.no_space_left_quota` |
| 81 | `invalid argument` | en | `apps.invalid_argument` |
| 82 | `permission denied` | en | `apps.permission_denied` |
| 83 | `bad file descriptor` | en | `apps.bad_file_descriptor` |
| 84 | `too many open files` | en | `apps.too_many_open_files` |
| 85 | `i/o error` | en | `apps.i_error` |

**`osjeff_core/src/appfs/volume_tests.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 113 | `readdir {i} => {:?}` | neutro | `apps.readdir_i` |
| 347 | `id={id}\nname={id}\nversion=1.0.0\n{extra}` | en | `apps.id_id_name_id` |

**`osjeff_core/src/appinstall.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 48 | `package is larger than 4 MiB` | en | `apps.package_is_larger_than` |
| 50 | `an app with this id is already installed` | en | `apps.an_app_with_this` |
| 51 | `app is not installed` | en | `apps.app_is_not_installed` |
| 52 | `invalid app id` | en | `apps.invalid_app_id` |
| 53 | `too many installed apps` | en | `apps.too_many_installed_apps` |
| 54 | `file system: {e}` | en | `apps.file_system` |

**`osjeff_core/src/appmanifest.rs`** (16)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 162 | `manifest is too large` | en | `apps.manifest_is_too_large` |
| 163 | `manifest is not valid UTF-8` | en | `apps.manifest_is_not_valid` |
| 164 | `manifest has too many lines` | en | `apps.manifest_has_too_many` |
| 165 | `manifest syntax error` | en | `apps.manifest_syntax_error` |
| 166 | `duplicate manifest key '{k}'` | neutro | `apps.duplicate_manifest_key_k` |
| 167 | `unknown manifest key` | en | `apps.unknown_manifest_key` |
| 168 | `manifest key '{k}' is required` | en | `apps.manifest_key_k_is` |
| 169 | `invalid value for '{k}'` | en | `apps.invalid_value_for_k` |
| 170 | `'{k}' is above the system limit` | en | `apps.k_is_above_system` |
| 577 | `icon is larger than 64 KiB` | en | `apps.icon_is_larger_than` |
| 578 | `icon is not a PNG` | en | `apps.icon_is_not_png` |
| 579 | `icon is larger than 64x64` | en | `apps.icon_is_larger_than_2` |
| 580 | `icon PNG is corrupt` | en | `apps.icon_png_is_corrupt` |
| 613 | `package has no osjeff.manifest section` | misto | `apps.package_has_no_osjeff` |
| 614 | `package has two manifests` | en | `apps.package_has_two_manifests` |
| 615 | `package has two icons` | en | `apps.package_has_two_icons` |

**`osjeff_core/src/wasmsec.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 49 | `not a wasm module (too short)` | en | `apps.not_wasm_module_too` |
| 50 | `not a wasm module (bad magic)` | en | `apps.not_wasm_module_bad` |
| 53 | `section extends past the end of the file` | en | `apps.section_extends_past_end` |
| 54 | `unknown section id` | en | `apps.unknown_section_id` |
| 55 | `too many sections` | en | `apps.too_many_sections` |

### Kit de componentes

**`kernel/src/desktop/gallery.rs`** (32)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 12 | `Ícones` | pt | `kit.icones` |
| 125 | `Padrão` | pt | `kit.padrao` |
| 127 | `Excluir` | pt | `kit.excluir` |
| 140 | `Dia` | pt | `kit.dia` |
| 140 | `Semana` | neutro | `kit.semana` |
| 140 | `Mês` | pt | `kit.mes` |
| 163 | `Três` | pt | `kit.tres` |
| 178 | `Campo de texto` | pt | `kit.campo_texto` |
| 187 | `Imagens` | pt | `kit.imagens` |
| 211 | `Nova janela` | pt | `kit.nova_janela` |
| 220 | `Mostrar barra` | pt | `kit.mostrar_barra` |
| 229 | `Desativado` | neutro | `kit.desativado` |
| 235 | `Dica de ferramenta` | pt | `kit.dica_ferramenta` |
| 318 | `Botões da janela: normal, ao passar, fechar ao passar, restaurar` | pt | `kit.botoes_janela_normal_ao` |
| 337 | `Configurações rápidas: desligado e ligado` | pt | `kit.configuracoes_rapidas_desligado_ligado` |
| 343 | `Não perturbe` | pt | `kit.nao_perturbe` |
| 344 | `Desligado` | neutro | `kit.desligado` |
| 352 | `Movimento` | neutro | `kit.movimento` |
| 353 | `Reduzido` | neutro | `kit.reduzido` |
| 359 | `Indicadores da barra de tarefas` | pt | `kit.indicadores_barra_tarefas` |
| 385 | `Pré-visualização do encaixe` | pt | `kit.pre_visualizacao_encaixe` |
| 391 | `Ponteiros` | neutro | `kit.ponteiros` |
| 409 | `Rápida raposa marrom pula sobre o cão` | pt | `kit.rapida_raposa_marrom_pula` |
| 411 | `Título 1 · 28` | pt | `kit.titulo_1_28` |
| 412 | `Título 2 · 22` | pt | `kit.titulo_2_22` |
| 413 | `Título 3 · 17` | pt | `kit.titulo_3_17` |
| 416 | `Corpo médio · 13` | pt | `kit.corpo_medio_13` |
| 438 | `fn main() { println!("olá, mundo"); }` | pt | `kit.fn_main_println_ola` |
| 446 | `Janela` | pt | `kit.janela` |
| 447 | `Conteúdo` | pt | `kit.conteudo` |
| 500 | `Cores de destaque` | pt | `kit.cores_destaque` |
| 545 | `Glifos` | neutro | `kit.glifos` |

### Sistema (logs e tela de falha: ficam em inglês)

**`kernel/src/boot.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 57 | `Sistema operacional` | pt | `sys.sistema_operacional` |

**`kernel/src/crash.rs`** (8)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 234 | `UNSUPPORTED SCREEN` | en | `sys.unsupported_screen` |
| 257 | `thread : {}` | neutro | `sys.thread` |
| 265 | `RIP    : {:#018x}` | neutro | `sys.rip_018x` |
| 267 | `RSP    : {rsp:#018x}` | neutro | `sys.rsp_rsp_018x` |
| 269 | `RFLAGS : {:#018x}` | neutro | `sys.rflags_018x` |
| 271 | `ERROR  : {code:#018x}` | en | `sys.error_code_018x` |
| 274 | `CR2    : {cr2:#018x}` | neutro | `sys.cr2_cr2_018x` |
| 280 | `The system is halted. Reset or power-cycle the machine to restart.\nThe same report was written to the seri...` | misto | `sys.system_is_halted_reset` |

**`kernel/src/desktop/browser.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 55 | `Conexão segura` | pt | `sys.conexao_segura` |
| 56 | `Não seguro` | pt | `sys.nao_seguro` |
| 58 | `Certificado inválido` | pt | `sys.certificado_invalido` |
| 379 | `Navegador` | neutro | `sys.navegador` |
| 438 | `Navegador` | neutro | `sys.navegador_2` |
| 600 | `Limite de 8 abas` | pt | `sys.limite_8_abas` |
| 747 | `Navegador` | neutro | `sys.navegador_3` |

**`kernel/src/desktop/browser_input.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 291 | `Nada para guardar aqui` | pt | `sys.nada_para_guardar_aqui` |
| 328 | `O endereço do formulário não é válido` | pt | `sys.endereco_formulario_nao_valido` |
| 521 | `Esse link não pode ser aberto` | pt | `sys.esse_link_nao_pode` |
| 575 | `Copiar` | pt | `sys.copiar` |
| 576 | `Abrir link` | pt | `sys.abrir_link` |
| 577 | `Copiar endereço do link` | pt | `sys.copiar_endereco_link` |
| 578 | `Voltar` | pt | `sys.voltar` |
| 579 | `Recarregar` | pt | `sys.recarregar` |
| 583 | `Remover dos favoritos` | pt | `sys.remover_dos_favoritos` |
| 585 | `Adicionar aos favoritos` | pt | `sys.adicionar_aos_favoritos` |
| 614 | `Endereço copiado` | pt | `sys.endereco_copiado` |

**`kernel/src/desktop/browser_ui.rs`** (26)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 133 | `load to first paint` | en | `sys.load_first_paint` |
| 156 | `Carregando…` | pt | `sys.carregando` |
| 338 | `Pesquisar ou digitar um endereço` | pt | `sys.pesquisar_ou_digitar_endereco` |
| 489 | `Nova aba` | pt | `sys.nova_aba` |
| 595 | `Pesquisar ou digitar um endereço` | pt | `sys.pesquisar_ou_digitar_endereco_2` |
| 639 | `Favoritos` | pt | `sys.favoritos` |
| 639 | `Sugestões` | pt | `sys.sugestoes` |
| 685 | `Visitados recentemente` | neutro | `sys.visitados_recentemente` |
| 806 | `Tentar novamente` | neutro | `sys.tentar_novamente` |
| 822 | `A hora do sistema não foi confirmada: confira o relógio.` | pt | `sys.hora_sistema_nao_foi` |
| 831 | `Continuar mesmo assim (inseguro)` | neutro | `sys.continuar_mesmo_assim_inseguro` |
| 842 | `Vale só para este site, nesta sessão.` | pt | `sys.vale_so_para_este` |
| 957 | `Buscar na página` | pt | `sys.buscar_na_pagina` |
| 985 | `Nenhum` | pt | `sys.nenhum` |
| 987 | `{} de {}` | pt | `sys.text` |
| 1163 | `Válido de` | pt | `sys.valido` |
| 1167 | `Válido até` | pt | `sys.valido_ate` |
| 1173 | `Cadeia, nome e assinatura conferidos` | pt | `sys.cadeia_nome_assinatura_conferidos` |
| 1174 | `A conexão é criptografada e a identidade do site foi comprovada.` | pt | `sys.conexao_criptografada_identidade_site` |
| 1177 | `Não verificado` | pt | `sys.nao_verificado` |
| 1178 | `Você escolheu continuar com este site nesta sessão. Não digite senhas nem dados pessoais.` | pt | `sys.voce_escolheu_continuar_com` |
| 1182 | `Esta conexão não é criptografada: outras pessoas na rede podem ver o que você envia e recebe.` | pt | `sys.esta_conexao_nao_criptografada` |
| 1186 | `Verificação` | pt | `sys.verificacao` |
| 1228 | `Conexão segura` | pt | `sys.conexao_segura` |
| 1229 | `Certificado inválido` | pt | `sys.certificado_invalido` |
| 1230 | `Conexão não segura` | pt | `sys.conexao_nao_segura` |

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
| 402 | `Desktop::new (fs load from ATA) done` | en | `sys.desktop_new_fs_load` |
| 410 | `wallpaper painted` | en | `sys.wallpaper_painted` |
| 722 | `the kernel panicked` | en | `sys.kernel_panicked` |

**`kernel/src/trace.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 658 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps` |
| 736 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps_2` |

### Outros

**`osjeff_core/src/base64.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 29 | `invalid base64 character` | en | `misc.invalid_base64_character` |
| 30 | `invalid base64 length` | en | `misc.invalid_base64_length` |
| 31 | `base64 padding before the end` | en | `misc.base64_padding_before_end` |
| 32 | `base64 data too large` | misto | `misc.base64_data_too_large` |

**`osjeff_core/src/compositor/sim/mod.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 170 | `{diff} pixels differ; first at ({x},{y}): incremental {:06X}, reference {:06X}; layers there (bottom to top...` | en | `misc.diff_pixels_differ_first` |

**`osjeff_core/src/compositor/sim/paint.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 77 | `layer {:?} wrote ({x},{y}) outside its footprint {:?}` | en | `misc.layer_wrote_x_y` |

**`osjeff_core/src/i18n/audit.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 795 | `{file}:{line}: {w:?} should be {r} in {s:?}` | en | `misc.file_line_w_should` |

**`osjeff_core/src/i18n/template.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 52 | `Str({s:?})` | neutro | `misc.str_s` |
| 53 | `Int({n})` | neutro | `misc.int_n` |
| 54 | `Num({n})` | neutro | `misc.num_n` |
| 55 | `Pad({n}, {w})` | neutro | `misc.pad_n_w` |
| 56 | `Dec({n}, {p})` | neutro | `misc.dec_n_p` |
| 57 | `Bytes({n})` | neutro | `misc.bytes_n` |

