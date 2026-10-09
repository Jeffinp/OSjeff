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
| Shell (migrado nesta onda) | 3 | 6 | 0 | 0 | 0 | 104 |
| Ajustes | 3 | 129 | 0 | 4 | 0 | 22 |
| Arquivos | 13 | 220 | 14 | 43 | 5 | 0 |
| Editor | 6 | 54 | 0 | 11 | 0 | 0 |
| Terminal | 11 | 203 | 0 | 177 | 5 | 0 |
| Tarefas | 3 | 89 | 0 | 3 | 1 | 0 |
| Registro | 1 | 19 | 0 | 0 | 0 | 4 |
| Calculadora | 2 | 2 | 0 | 0 | 0 | 0 |
| Imagens | 7 | 90 | 0 | 33 | 9 | 0 |
| Navegador | 11 | 81 | 31 | 11 | 1 | 0 |
| Apps de terceiros (WASM) | 11 | 77 | 16 | 40 | 1 | 0 |
| Kit de componentes | 2 | 40 | 1 | 0 | 0 | 12 |
| Sistema (logs e tela de falha: ficam em inglês) | 6 | 31 | 0 | 24 | 1 | 0 |
| Outros | 3 | 11 | 0 | 4 | 1 | 63 |
| **Total** | 82 | 1052 | 62 | 350 | 24 | 205 |

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
| `kernel/src/desktop/files.rs` | 76 | 0 | 4 | 2 |
| `kernel/src/desktop/files_ui.rs` | 29 | 0 | 1 | 0 |
| `kernel/src/desktop/sysstore.rs` | 1 | 1 | 0 | 0 |
| `kernel/src/desktop/vfs.rs` | 4 | 4 | 0 | 0 |
| `osjeff_core/src/fileman.rs` | 28 | 0 | 1 | 0 |
| `osjeff_core/src/fileman/apps.rs` | 16 | 0 | 0 | 1 |
| `osjeff_core/src/fileman/ui.rs` | 9 | 0 | 0 | 0 |
| `osjeff_core/src/fs3/dir.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/fs3/extent.rs` | 2 | 0 | 2 | 0 |
| `osjeff_core/src/fs3/fsck.rs` | 21 | 0 | 21 | 0 |
| `osjeff_core/src/fs3/mod.rs` | 11 | 1 | 9 | 2 |
| `osjeff_core/src/fs3/ops.rs` | 4 | 0 | 4 | 0 |
| `osjeff_core/src/vfs.rs` | 18 | 8 | 0 | 0 |

### Editor

editor de texto e diálogos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/edit.rs` | 10 | 0 | 0 | 0 |
| `kernel/src/desktop/edit_ui.rs` | 19 | 0 | 0 | 0 |
| `osjeff_core/src/editor2/dialog.rs` | 11 | 0 | 0 | 0 |
| `osjeff_core/src/editor2/mod.rs` | 9 | 0 | 9 | 0 |
| `osjeff_core/src/editor2/search.rs` | 2 | 0 | 2 | 0 |
| `osjeff_core/src/editor2/ui.rs` | 3 | 0 | 0 | 0 |

### Terminal

terminal, interpretador e comandos.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/shellhost.rs` | 10 | 0 | 9 | 0 |
| `kernel/src/desktop/term.rs` | 3 | 0 | 2 | 0 |
| `osjeff_core/src/shell/builtins.rs` | 107 | 0 | 102 | 1 |
| `osjeff_core/src/shell/exec.rs` | 32 | 0 | 22 | 0 |
| `osjeff_core/src/shell/fs.rs` | 11 | 0 | 9 | 2 |
| `osjeff_core/src/shell/glob.rs` | 3 | 0 | 3 | 0 |
| `osjeff_core/src/shell/line.rs` | 1 | 0 | 0 | 0 |
| `osjeff_core/src/shell/netcmds.rs` | 17 | 0 | 13 | 1 |
| `osjeff_core/src/shell/parse.rs` | 10 | 0 | 9 | 1 |
| `osjeff_core/src/shell/regex.rs` | 2 | 0 | 2 | 0 |
| `osjeff_core/src/shell/sys.rs` | 7 | 0 | 6 | 0 |

### Tarefas

monitor de atividade.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/tarefas.rs` | 70 | 0 | 3 | 0 |
| `osjeff_core/src/activity.rs` | 15 | 0 | 0 | 0 |
| `osjeff_core/src/netstats.rs` | 4 | 0 | 0 | 1 |

### Registro

visualizador do registro.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/logview.rs` | 19 | 0 | 0 | 0 |

### Calculadora

calculadora.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/calc_ui.rs` | 1 | 0 | 0 | 0 |
| `osjeff_core/src/calc.rs` | 1 | 0 | 0 | 0 |

### Imagens

visualizador de imagens e decodificadores.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/viewer.rs` | 22 | 0 | 0 | 0 |
| `osjeff_core/src/bmp.rs` | 6 | 0 | 5 | 1 |
| `osjeff_core/src/image.rs` | 10 | 0 | 7 | 0 |
| `osjeff_core/src/inflate.rs` | 10 | 0 | 7 | 3 |
| `osjeff_core/src/png.rs` | 15 | 0 | 10 | 4 |
| `osjeff_core/src/ppm.rs` | 5 | 0 | 4 | 1 |
| `osjeff_core/src/viewer.rs` | 22 | 0 | 0 | 0 |

### Navegador

navegador, páginas internas, erros de rede e TLS (outro agente está editando).

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/desktop/apps.rs` | 8 | 0 | 0 | 0 |
| `kernel/src/netd.rs` | 2 | 0 | 0 | 0 |
| `kernel/src/netstack.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/browser.rs` | 20 | 11 | 0 | 0 |
| `osjeff_core/src/browser/body_tests.rs` | 1 | 0 | 0 | 0 |
| `osjeff_core/src/icmp.rs` | 7 | 0 | 6 | 1 |
| `osjeff_core/src/net.rs` | 2 | 0 | 0 | 0 |
| `osjeff_core/src/sntp.rs` | 4 | 0 | 4 | 0 |
| `osjeff_core/src/tlsverify.rs` | 23 | 16 | 0 | 0 |
| `osjeff_core/src/web/form.rs` | 8 | 3 | 0 | 0 |
| `osjeff_core/src/web/imgcache.rs` | 5 | 1 | 0 | 0 |

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
| `kernel/src/desktop/input.rs` | 8 | 1 | 0 | 0 |

### Sistema (logs e tela de falha: ficam em inglês)

boot, falha grave, drivers.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `kernel/src/boot.rs` | 1 | 0 | 0 | 0 |
| `kernel/src/crash.rs` | 8 | 0 | 2 | 1 |
| `kernel/src/interrupts.rs` | 2 | 0 | 2 | 0 |
| `kernel/src/io.rs` | 6 | 0 | 6 | 0 |
| `kernel/src/main.rs` | 12 | 0 | 12 | 0 |
| `kernel/src/trace.rs` | 2 | 0 | 2 | 0 |

### Outros

núcleo sem dono claro.

| Arquivo | Textos | Sem acento | Inglês | Misto |
|---|---:|---:|---:|---:|
| `osjeff_core/src/base64.rs` | 4 | 0 | 3 | 1 |
| `osjeff_core/src/i18n/audit.rs` | 1 | 0 | 1 | 0 |
| `osjeff_core/src/i18n/template.rs` | 6 | 0 | 0 | 0 |

## Textos sem acento (a corrigir ao migrar)

| Arquivo:linha | Texto | Correção |
|---|---|---|
| `kernel/src/desktop/input.rs:507` | `endereco do formulario invalido` | endereco → endereço, formulario → (acento), invalido → inválido |
| `kernel/src/desktop/sysstore.rs:65` | `Memoria (sem disco v3)` | Memoria → memória |
| `kernel/src/desktop/vfs.rs:94` | `Disco pequeno demais: arquivos so na memoria` | memoria → memória |
| `kernel/src/desktop/vfs.rs:95` | `Sem disco: arquivos so na memoria` | memoria → memória |
| `kernel/src/desktop/vfs.rs:96` | `Disco desconhecido (intocado): arquivos so na memoria` | memoria → memória |
| `kernel/src/desktop/vfs.rs:97` | `Falha no disco: arquivos so na memoria` | memoria → memória |
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
| `osjeff_core/src/browser.rs:67` | `Nao seguro` | Nao → não |
| `osjeff_core/src/browser.rs:116` | `Falha ao carregar a pagina.` | pagina → página |
| `osjeff_core/src/browser.rs:117` | `Nome nao encontrado: confira o endereco (DNS).` | nao → não, endereco → endereço |
| `osjeff_core/src/browser.rs:119` | `Tempo esgotado: o servidor nao respondeu.` | nao → não |
| `osjeff_core/src/browser.rs:120` | `Falha na negociacao TLS (conexao segura).` | negociacao → (acento), conexao → conexão |
| `osjeff_core/src/browser.rs:123` | `Redirecionamento invalido.` | invalido → inválido |
| `osjeff_core/src/browser.rs:126` | `O carregador de paginas falhou (thread encerrada).` | paginas → páginas |
| `osjeff_core/src/browser.rs:448` | `Pagina cortada no limite de tamanho` | Pagina → página |
| `osjeff_core/src/browser.rs:449` | `Pagina incompleta (conexao interrompida)` | Pagina → página, conexao → conexão |
| `osjeff_core/src/browser.rs:450` | `Pagina com dados compactados corrompidos (parcial)` | Pagina → página |
| `osjeff_core/src/browser.rs:451` | `Pagina com falha de verificacao (checksum)` | Pagina → página, verificacao → (acento) |
| `osjeff_core/src/fs3/mod.rs:827` | `block range outside the data area` | area → área |
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
| `osjeff_core/src/vfs.rs:73` | `Item nao encontrado` | nao → não |
| `osjeff_core/src/vfs.rs:75` | `O destino nao e uma pasta` | nao → não |
| `osjeff_core/src/vfs.rs:77` | `A pasta nao esta vazia` | nao → não |
| `osjeff_core/src/vfs.rs:78` | `Nome invalido` | invalido → inválido |
| `osjeff_core/src/vfs.rs:79` | `Nome longo demais (maximo 255 bytes)` | maximo → máximo |
| `osjeff_core/src/vfs.rs:80` | `Caminho invalido` | invalido → inválido |
| `osjeff_core/src/vfs.rs:82` | `Nao e possivel mover uma pasta para dentro dela mesma` | Nao → não, possivel → possível |
| `osjeff_core/src/vfs.rs:90` | `Operacao cancelada` | Operacao → operação |
| `osjeff_core/src/web/form.rs:95` | `formularios POST nao suportados` | nao → não |
| `osjeff_core/src/web/form.rs:96` | `formulario grande demais para enviar` | formulario → (acento) |
| `osjeff_core/src/web/form.rs:97` | `formulario invalido` | formulario → (acento), invalido → inválido |
| `osjeff_core/src/web/imgcache.rs:72` | `formato nao suportado` | nao → não |

## Inglês e textos mistos numa interface em português

| Arquivo:linha | Texto | Tipo |
|---|---|---|
| `kernel/src/crash.rs:234` | `UNSUPPORTED SCREEN` | en |
| `kernel/src/crash.rs:271` | `ERROR  : {code:#018x}` | en |
| `kernel/src/crash.rs:280` | `The system is halted. Reset or power-cycle the machine to restart.\nThe same report was written to the seri...` | misto |
| `kernel/src/desktop/files.rs:350` | `{name} aberto` | en |
| `kernel/src/desktop/files.rs:354` | `{name} instalado e aberto` | en |
| `kernel/src/desktop/files.rs:358` | `{name} instalado` | en |
| `kernel/src/desktop/files.rs:361` | `{name} removido` | en |
| `kernel/src/desktop/files.rs:1818` | `Tamanho total: {}` | misto |
| `kernel/src/desktop/files.rs:1887` | `Estado: não instalado (Enter instala e abre)` | misto |
| `kernel/src/desktop/files_ui.rs:1008` | `Enter abre  ·  I instala  ·  Del remove` | en |
| `kernel/src/desktop/settings_ui.rs:1233` | `Total` | en |
| `kernel/src/desktop/shellhost.rs:302` | `boot disk` | en |
| `kernel/src/desktop/shellhost.rs:484` | `{p}: Is a directory` | en |
| `kernel/src/desktop/shellhost.rs:490` | `usage: edit [FILE]` | en |
| `kernel/src/desktop/shellhost.rs:528` | `edit [FILE]: open the text editor` | en |
| `kernel/src/desktop/shellhost.rs:529` | `files: open the file manager` | en |
| `kernel/src/desktop/shellhost.rs:530` | `tasks: open the task manager` | en |
| `kernel/src/desktop/shellhost.rs:531` | `calc: open the calculator` | en |
| `kernel/src/desktop/shellhost.rs:532` | `reboot: restart the machine` | en |
| `kernel/src/desktop/shellhost.rs:533` | `shutdown: power the machine off` | en |
| `kernel/src/desktop/tarefas.rs:1419` | `Total` | en |
| `kernel/src/desktop/tarefas.rs:1578` | `Total` | en |
| `kernel/src/desktop/tarefas.rs:2264` | `Encerrar “{name}”?` | en |
| `kernel/src/desktop/term.rs:254` | `sh: too many commands waiting` | en |
| `kernel/src/desktop/term.rs:259` | `sh: the command thread stopped` | en |
| `kernel/src/interrupts.rs:314` | `stack overflow in thread '{owner}': guard page hit at {cr2:#x} ({code:?})` | en |
| `kernel/src/interrupts.rs:321` | `page fault accessing {cr2:#x}: {code:?}` | en |
| `kernel/src/io.rs:12` | `in al, dx` | en |
| `kernel/src/io.rs:22` | `out dx, al` | en |
| `kernel/src/io.rs:30` | `out dx, ax` | en |
| `kernel/src/io.rs:39` | `in ax, dx` | en |
| `kernel/src/io.rs:48` | `out dx, eax` | en |
| `kernel/src/io.rs:57` | `in eax, dx` | en |
| `kernel/src/main.rs:146` | `this screen resolution is not supported` | en |
| `kernel/src/main.rs:148` | `Detected {}x{} (stride {}, {} bytes/pixel): the screen needs {} bytes, the bootloader provided a framebuffe...` | en |
| `kernel/src/main.rs:196` | `wasm demo done` | en |
| `kernel/src/main.rs:274` | `pci scan + virtio-gpu probe done` | en |
| `kernel/src/main.rs:284` | `ata detect done` | en |
| `kernel/src/main.rs:322` | `nic init done` | en |
| `kernel/src/main.rs:327` | `dhcp done` | en |
| `kernel/src/main.rs:365` | `storage init done` | en |
| `kernel/src/main.rs:388` | `ui text engine ready` | en |
| `kernel/src/main.rs:403` | `Desktop::new (fs load from ATA) done` | en |
| `kernel/src/main.rs:411` | `wallpaper painted` | en |
| `kernel/src/main.rs:1101` | `the kernel panicked` | en |
| `kernel/src/netstack.rs:708` | `tcp stream error` | en |
| `kernel/src/trace.rs:650` | `The quick brown fox jumps over the lazy dog 0123` | en |
| `kernel/src/trace.rs:728` | `The quick brown fox jumps over the lazy dog 0123` | en |
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
| `osjeff_core/src/editor2/mod.rs:353` | `line index out of sync with the text` | en |
| `osjeff_core/src/editor2/mod.rs:356` | `cursor past the end` | en |
| `osjeff_core/src/editor2/mod.rs:359` | `cursor not at a valid position` | en |
| `osjeff_core/src/editor2/mod.rs:362` | `selection anchor past the end` | en |
| `osjeff_core/src/editor2/mod.rs:365` | `scroll position past the last line` | en |
| `osjeff_core/src/editor2/mod.rs:368` | `empty viewport` | en |
| `osjeff_core/src/editor2/mod.rs:374` | `row wider than the window` | en |
| `osjeff_core/src/editor2/mod.rs:378` | `more rows than the window holds` | en |
| `osjeff_core/src/editor2/mod.rs:383` | `cursor drawn outside the window` | en |
| `osjeff_core/src/editor2/search.rs:368` | `Replace with: ` | en |
| `osjeff_core/src/editor2/search.rs:371` | `Go to line: ` | en |
| `osjeff_core/src/fileman.rs:941` | `Enter` | en |
| `osjeff_core/src/fileman/apps.rs:98` | `pasta do usuário (/home)` | misto |
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
| `osjeff_core/src/fs3/fsck.rs:216` | `trash flag does not match location` | en |
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
| `osjeff_core/src/fs3/mod.rs:535` | `bitmap does not cover the metadata` | en |
| `osjeff_core/src/fs3/mod.rs:538` | `root/trash inode not allocated` | en |
| `osjeff_core/src/fs3/mod.rs:581` | `journal target out of range` | en |
| `osjeff_core/src/fs3/mod.rs:599` | `bad root inode` | en |
| `osjeff_core/src/fs3/mod.rs:603` | `bad trash inode` | en |
| `osjeff_core/src/fs3/mod.rs:607` | `root has no .trash entry` | misto |
| `osjeff_core/src/fs3/mod.rs:728` | `metadata block out of range` | en |
| `osjeff_core/src/fs3/mod.rs:803` | `double free of an inode` | en |
| `osjeff_core/src/fs3/mod.rs:827` | `block range outside the data area` | misto |
| `osjeff_core/src/fs3/mod.rs:831` | `block allocated or freed twice` | en |
| `osjeff_core/src/fs3/ops.rs:121` | `directory entry points at a free inode` | en |
| `osjeff_core/src/fs3/ops.rs:190` | `inode missing from its parent` | en |
| `osjeff_core/src/fs3/ops.rs:321` | `directory entry changed under us` | en |
| `osjeff_core/src/fs3/ops.rs:376` | `directory tree does not terminate` | en |
| `osjeff_core/src/i18n/audit.rs:786` | `{file}:{line}: {w:?} should be {r} in {s:?}` | en |
| `osjeff_core/src/icmp.rs:247` | `destination unreachable (code {c})` | en |
| `osjeff_core/src/icmp.rs:248` | `time exceeded` | en |
| `osjeff_core/src/icmp.rs:249` | `no route to host` | misto |
| `osjeff_core/src/icmp.rs:250` | `host did not answer ARP` | en |
| `osjeff_core/src/icmp.rs:251` | `invalid target address` | en |
| `osjeff_core/src/icmp.rs:252` | `network unavailable` | en |
| `osjeff_core/src/icmp.rs:253` | `network busy` | en |
| `osjeff_core/src/image.rs:56` | `image has a zero dimension` | en |
| `osjeff_core/src/image.rs:57` | `image is larger than the pixel limit` | en |
| `osjeff_core/src/image.rs:58` | `buffer size does not match the dimensions` | en |
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
| `osjeff_core/src/shell/builtins.rs:19` | `help [CMD]: list commands or describe one` | en |
| `osjeff_core/src/shell/builtins.rs:20` | `ls [-aF1l] [PATH...]: list directory contents` | en |
| `osjeff_core/src/shell/builtins.rs:22` | `pwd: print the working directory` | en |
| `osjeff_core/src/shell/builtins.rs:23` | `cat [-n] [FILE...]: print files or stdin` | en |
| `osjeff_core/src/shell/builtins.rs:26` | `rm [-rf] PATH...: remove files or directories` | en |
| `osjeff_core/src/shell/builtins.rs:27` | `rmdir DIR...: remove empty directories` | en |
| `osjeff_core/src/shell/builtins.rs:28` | `mv SRC... DEST: move or rename` | en |
| `osjeff_core/src/shell/builtins.rs:29` | `cp [-r] SRC... DEST: copy files or directories` | en |
| `osjeff_core/src/shell/builtins.rs:30` | `touch FILE...: create empty files` | en |
| `osjeff_core/src/shell/builtins.rs:31` | `head [-n N] [FILE...]: first lines` | en |
| `osjeff_core/src/shell/builtins.rs:32` | `tail [-n N] [FILE...]: last lines` | en |
| `osjeff_core/src/shell/builtins.rs:33` | `wc [-lwcm] [FILE...]: count lines, words, bytes` | en |
| `osjeff_core/src/shell/builtins.rs:36` | `grep [-ivncFlqHh] PATTERN [FILE...]: search text` | en |
| `osjeff_core/src/shell/builtins.rs:39` | `sort [-rnu] [FILE...]: sort lines` | en |
| `osjeff_core/src/shell/builtins.rs:47` | `tee [-a] FILE...: copy stdin to stdout and files` | en |
| `osjeff_core/src/shell/builtins.rs:50` | `clear: clear the terminal` | en |
| `osjeff_core/src/shell/builtins.rs:54` | `export [NAME[=VALUE]...]: mark variables exported` | en |
| `osjeff_core/src/shell/builtins.rs:57` | `unset NAME...: remove variables` | en |
| `osjeff_core/src/shell/builtins.rs:60` | `history [N\|-c]: show or clear the command history` | en |
| `osjeff_core/src/shell/builtins.rs:65` | `alias [NAME=VALUE...]: define or list aliases` | en |
| `osjeff_core/src/shell/builtins.rs:68` | `unalias [-a] NAME...: remove aliases` | en |
| `osjeff_core/src/shell/builtins.rs:69` | `which CMD...: show how a command resolves` | en |
| `osjeff_core/src/shell/builtins.rs:70` | `date [+FORMAT]: print the date and time` | en |
| `osjeff_core/src/shell/builtins.rs:71` | `uptime: time since boot` | en |
| `osjeff_core/src/shell/builtins.rs:72` | `free [-bkm]: memory usage` | en |
| `osjeff_core/src/shell/builtins.rs:73` | `df: disk usage` | en |
| `osjeff_core/src/shell/builtins.rs:76` | `ping [-c N] HOST: test network reachability` | en |
| `osjeff_core/src/shell/builtins.rs:81` | `sleep SECONDS: wait` | en |
| `osjeff_core/src/shell/builtins.rs:82` | `seq [FIRST] LAST: print a sequence of numbers` | en |
| `osjeff_core/src/shell/builtins.rs:85` | `basename PATH [SUFFIX]: last path component` | en |
| `osjeff_core/src/shell/builtins.rs:90` | `dirname PATH: directory part of a path` | en |
| `osjeff_core/src/shell/builtins.rs:93` | `stat PATH...: show kind and size` | en |
| `osjeff_core/src/shell/builtins.rs:98` | `cut -d C -f N[,M]: select fields (stdin or files)` | en |
| `osjeff_core/src/shell/builtins.rs:101` | `nl: number lines (stdin or files)` | en |
| `osjeff_core/src/shell/builtins.rs:104` | `yes [WORD]: repeat a word (bounded by the output limit)` | en |
| `osjeff_core/src/shell/builtins.rs:180` | `option requires an argument -- '{c}'` | en |
| `osjeff_core/src/shell/builtins.rs:187` | `invalid option -- '{c}'` | en |
| `osjeff_core/src/shell/builtins.rs:246` | `no help for '{name}'` | misto |
| `osjeff_core/src/shell/builtins.rs:293` | `cannot access '{t}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:346` | `too many arguments` | en |
| `osjeff_core/src/shell/builtins.rs:359` | `OLDPWD not set` | en |
| `osjeff_core/src/shell/builtins.rs:465` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:493` | `cannot create directory '{d}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:524` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:534` | `refusing to remove '{p}'` | en |
| `osjeff_core/src/shell/builtins.rs:541` | `cannot remove '{p}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:546` | `cannot remove '{p}': Is a directory` | en |
| `osjeff_core/src/shell/builtins.rs:551` | `cannot remove '{at}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:563` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:571` | `failed to remove '{d}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:577` | `failed to remove '{d}': Not a directory` | en |
| `osjeff_core/src/shell/builtins.rs:582` | `failed to remove '{d}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:604` | `usage: mv SRC... DEST` | en |
| `osjeff_core/src/shell/builtins.rs:610` | `target '{dst}' is not a directory` | en |
| `osjeff_core/src/shell/builtins.rs:617` | `cannot move '{s}' to '{to}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:635` | `cannot stat '{src}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:640` | `cannot read '{src}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:644` | `cannot create '{dst}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:647` | `-r not specified; omitting directory '{src}'` | en |
| `osjeff_core/src/shell/builtins.rs:650` | `'{src}': nesting too deep` | en |
| `osjeff_core/src/shell/builtins.rs:655` | `cannot copy '{src}' into itself` | en |
| `osjeff_core/src/shell/builtins.rs:659` | `cannot create directory '{dst}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:664` | `cannot read '{src}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:682` | `usage: cp [-r] SRC... DEST` | en |
| `osjeff_core/src/shell/builtins.rs:688` | `target '{dst}' is not a directory` | en |
| `osjeff_core/src/shell/builtins.rs:705` | `missing file operand` | en |
| `osjeff_core/src/shell/builtins.rs:713` | `cannot touch '{f}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:730` | `invalid number of lines: '{v}'` | en |
| `osjeff_core/src/shell/builtins.rs:823` | `usage: grep [OPTIONS] PATTERN [FILE...]` | en |
| `osjeff_core/src/shell/builtins.rs:837` | `invalid pattern: {}` | en |
| `osjeff_core/src/shell/builtins.rs:1063` | `'{a}': not a valid identifier` | en |
| `osjeff_core/src/shell/builtins.rs:1072` | `cannot set '{name}'` | en |
| `osjeff_core/src/shell/builtins.rs:1133` | `too many aliases` | en |
| `osjeff_core/src/shell/builtins.rs:1140` | `invalid alias name in '{a}'` | en |
| `osjeff_core/src/shell/builtins.rs:1146` | `{a}: not found` | en |
| `osjeff_core/src/shell/builtins.rs:1161` | `usage: unalias [-a] NAME...` | en |
| `osjeff_core/src/shell/builtins.rs:1167` | `{a}: not found` | en |
| `osjeff_core/src/shell/builtins.rs:1176` | `usage: which CMD...` | en |
| `osjeff_core/src/shell/builtins.rs:1208` | `{n} not found\n` | en |
| `osjeff_core/src/shell/builtins.rs:1228` | `invalid date '{f}'` | en |
| `osjeff_core/src/shell/builtins.rs:1272` | `up {d} {unit}, {h:02}:{m:02}:{s:02}` | en |
| `osjeff_core/src/shell/builtins.rs:1274` | `up {h:02}:{m:02}:{s:02}` | en |
| `osjeff_core/src/shell/builtins.rs:1385` | `invalid signal '{s}'` | en |
| `osjeff_core/src/shell/builtins.rs:1394` | `usage: kill [-SIGNAL] PID...` | en |
| `osjeff_core/src/shell/builtins.rs:1407` | `invalid pid '{p}'` | en |
| `osjeff_core/src/shell/builtins.rs:1424` | `invalid count '{v}'` | en |
| `osjeff_core/src/shell/builtins.rs:1430` | `usage: ping [-c N] HOST` | en |
| `osjeff_core/src/shell/builtins.rs:1526` | `argument expected` | en |
| `osjeff_core/src/shell/builtins.rs:1532` | `missing ')'` | en |
| `osjeff_core/src/shell/builtins.rs:1560` | `integer expression expected` | en |
| `osjeff_core/src/shell/builtins.rs:1611` | `too many arguments` | en |
| `osjeff_core/src/shell/builtins.rs:1629` | `missing ']'` | en |
| `osjeff_core/src/shell/builtins.rs:1638` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:1656` | `invalid time interval '{arg}'` | en |
| `osjeff_core/src/shell/builtins.rs:1675` | `usage: seq [FIRST [INCR]] LAST` | en |
| `osjeff_core/src/shell/builtins.rs:1686` | `increment must not be zero` | en |
| `osjeff_core/src/shell/builtins.rs:1707` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:1729` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:1750` | `missing operand` | en |
| `osjeff_core/src/shell/builtins.rs:1765` | `cannot stat '{p}': {}` | en |
| `osjeff_core/src/shell/builtins.rs:1838` | `usage: tr SET1 SET2 \| tr -d SET1` | en |
| `osjeff_core/src/shell/builtins.rs:1866` | `you must specify a list of fields (-f)` | en |
| `osjeff_core/src/shell/builtins.rs:1874` | `invalid field '{part}'` | en |
| `osjeff_core/src/shell/exec.rs:326` | `exit [N]: leave the shell or script with status N` | en |
| `osjeff_core/src/shell/exec.rs:327` | `return [N]: leave a function with status N` | en |
| `osjeff_core/src/shell/exec.rs:329` | `continue [N]: next iteration of the Nth loop` | en |
| `osjeff_core/src/shell/exec.rs:330` | `shift [N]: drop the first N positional arguments` | en |
| `osjeff_core/src/shell/exec.rs:331` | `source FILE [ARGS]: run a script in this shell` | en |
| `osjeff_core/src/shell/exec.rs:333` | `sh FILE [ARGS]: run a script file` | en |
| `osjeff_core/src/shell/exec.rs:336` | `set [NAME=VALUE \| -- ARGS]: list variables or set them` | en |
| `osjeff_core/src/shell/exec.rs:560` | `{name}: syntax error at line {l}, column {c}: {}\n` | en |
| `osjeff_core/src/shell/exec.rs:761` | `too many functions` | en |
| `osjeff_core/src/shell/exec.rs:863` | `cannot set variable` | en |
| `osjeff_core/src/shell/exec.rs:953` | `{name}: Is a directory` | en |
| `osjeff_core/src/shell/exec.rs:956` | `{name}: command not found` | en |
| `osjeff_core/src/shell/exec.rs:968` | `{name}.sh` | en |
| `osjeff_core/src/shell/exec.rs:1058` | `{path}: script too large` | en |
| `osjeff_core/src/shell/exec.rs:1110` | `exit: numeric argument required` | en |
| `osjeff_core/src/shell/exec.rs:1118` | `return: only valid in a function or sourced script` | en |
| `osjeff_core/src/shell/exec.rs:1141` | `{}: bad loop count` | en |
| `osjeff_core/src/shell/exec.rs:1157` | `shift: bad count` | en |
| `osjeff_core/src/shell/exec.rs:1163` | `{}: file name required` | en |
| `osjeff_core/src/shell/exec.rs:1198` | `set: invalid argument '{a}'` | en |
| `osjeff_core/src/shell/exec.rs:1230` | `command substitution nested too deeply` | en |
| `osjeff_core/src/shell/exec.rs:1248` | `command substitution output truncated` | en |
| `osjeff_core/src/shell/fs.rs:32` | `No such file or directory` | misto |
| `osjeff_core/src/shell/fs.rs:33` | `Not a directory` | en |
| `osjeff_core/src/shell/fs.rs:34` | `Is a directory` | en |
| `osjeff_core/src/shell/fs.rs:35` | `File exists` | en |
| `osjeff_core/src/shell/fs.rs:36` | `Directory not empty` | en |
| `osjeff_core/src/shell/fs.rs:37` | `No space left on device` | misto |
| `osjeff_core/src/shell/fs.rs:38` | `File too large` | en |
| `osjeff_core/src/shell/fs.rs:39` | `File name too long` | en |
| `osjeff_core/src/shell/fs.rs:40` | `Invalid path` | en |
| `osjeff_core/src/shell/fs.rs:41` | `Read-only file system` | en |
| `osjeff_core/src/shell/fs.rs:42` | `Input/output error` | en |
| `osjeff_core/src/shell/glob.rs:143` | `division by zero` | en |
| `osjeff_core/src/shell/glob.rs:144` | `arithmetic syntax error` | en |
| `osjeff_core/src/shell/glob.rs:145` | `arithmetic expression too deep` | en |
| `osjeff_core/src/shell/netcmds.rs:24` | `nslookup HOST: look up the IPv4 addresses of a name` | en |
| `osjeff_core/src/shell/netcmds.rs:29` | `curl [-sSfiL] [-o FILE\|-O] URL: fetch a http(s) URL` | en |
| `osjeff_core/src/shell/netcmds.rs:34` | `wget [-q] [-O FILE\|-] URL: download a http(s) URL to a file` | en |
| `osjeff_core/src/shell/netcmds.rs:37` | `ifconfig: show the network interface` | en |
| `osjeff_core/src/shell/netcmds.rs:67` | `usage: nslookup HOST` | en |
| `osjeff_core/src/shell/netcmds.rs:85` | `can't find {host}: NXDOMAIN` | en |
| `osjeff_core/src/shell/netcmds.rs:139` | `usage: curl [-sSfiL] [-o FILE\|-O] URL` | en |
| `osjeff_core/src/shell/netcmds.rs:168` | `Could not resolve host` | en |
| `osjeff_core/src/shell/netcmds.rs:180` | `(22) The requested URL returned error: {}` | en |
| `osjeff_core/src/shell/netcmds.rs:203` | `(23) body cut at {} bytes` | en |
| `osjeff_core/src/shell/netcmds.rs:214` | `usage: wget [-q] [-O FILE\|-] URL` | en |
| `osjeff_core/src/shell/netcmds.rs:241` | `{url}: server returned error {}` | en |
| `osjeff_core/src/shell/netcmds.rs:253` | `warning: body cut at {} bytes` | en |
| `osjeff_core/src/shell/netcmds.rs:264` | `no network interface` | misto |
| `osjeff_core/src/shell/parse.rs:68` | `bad ${ } substitution` | en |
| `osjeff_core/src/shell/parse.rs:69` | `unexpected token` | en |
| `osjeff_core/src/shell/parse.rs:70` | `unexpected end of input` | en |
| `osjeff_core/src/shell/parse.rs:71` | `expected '{what}'` | en |
| `osjeff_core/src/shell/parse.rs:72` | `redirection needs a file name` | en |
| `osjeff_core/src/shell/parse.rs:73` | `'&' (background jobs) is not supported` | en |
| `osjeff_core/src/shell/parse.rs:74` | `'<<' (here-documents) is not supported` | en |
| `osjeff_core/src/shell/parse.rs:76` | `descriptor redirections such as '2>' are not supported` | misto |
| `osjeff_core/src/shell/parse.rs:78` | `invalid function name` | en |
| `osjeff_core/src/shell/parse.rs:79` | `nesting is too deep` | en |
| `osjeff_core/src/shell/regex.rs:27` | `nothing to repeat` | en |
| `osjeff_core/src/shell/regex.rs:29` | `pattern too long` | en |
| `osjeff_core/src/shell/sys.rs:90` | `not supported on this system` | en |
| `osjeff_core/src/shell/sys.rs:92` | `operation not permitted` | en |
| `osjeff_core/src/shell/sys.rs:93` | `network is unreachable` | en |
| `osjeff_core/src/shell/sys.rs:94` | `host not found` | en |
| `osjeff_core/src/shell/sys.rs:95` | `timed out` | en |
| `osjeff_core/src/shell/sys.rs:97` | `transfer failed` | en |
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
| 483 | `Alt+↑` | neutro | `shell.alt` |
| 490 | `Alt+←` | neutro | `shell.alt_2` |
| 496 | `Alt+→` | neutro | `shell.alt_3` |

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

**`kernel/src/desktop/files.rs`** (76)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 54 | `1 item` | neutro | `files.1_item` |
| 56 | `{n} itens` | neutro | `files.n_itens` |
| 69 | `Início` | pt | `files.inicio` |
| 350 | `{name} aberto` | en | `files.name_aberto` |
| 354 | `{name} instalado e aberto` | en | `files.name_instalado_aberto` |
| 358 | `{name} instalado` | en | `files.name_instalado` |
| 361 | `{name} removido` | en | `files.name_removido` |
| 382 | `Formato não suportado` | pt | `files.formato_nao_suportado` |
| 441 | `Nome` | pt | `files.nome` |
| 442 | `Tamanho` | pt | `files.tamanho` |
| 446 | `Data da exclusão` | pt | `files.data_exclusao` |
| 450 | `Última modificação` | pt | `files.ultima_modificacao` |
| 921 | `Disco` | pt | `files.disco` |
| 942 | `{} na lixeira` | pt | `files.na_lixeira` |
| 958 | `{} {} para {}` | pt | `files.para` |
| 983 | `Já existe uma cópia em andamento` | pt | `files.ja_existe_copia_em` |
| 991 | `Copiando` | pt | `files.copiando` |
| 1097 | `Cancelado` | neutro | `files.cancelado` |
| 1380 | `Novo arquivo.txt` | pt | `files.novo_arquivo_txt` |
| 1382 | `Nova pasta` | pt | `files.nova_pasta` |
| 1459 | `{} na lixeira` | pt | `files.na_lixeira_2` |
| 1516 | `Papel de parede aplicado` | pt | `files.papel_parede_aplicado` |
| 1550 | `Não é possível colar na lixeira` | pt | `files.nao_possivel_colar_na` |
| 1558 | `Nada para colar` | pt | `files.nada_para_colar` |
| 1606 | `Cópia cancelada` | pt | `files.copia_cancelada` |
| 1633 | `Cópia concluída ({})` | pt | `files.copia_concluida` |
| 1635 | `1 arquivo` | pt | `files.1_arquivo` |
| 1637 | `{n} arquivos` | pt | `files.n_arquivos` |
| 1727 | `Excluído` | pt | `files.excluido` |
| 1752 | `Cancelado` | neutro | `files.cancelado_2` |
| 1766 | `Local: Lixeira` | pt | `files.local_lixeira` |
| 1768 | `Itens: {}` | neutro | `files.itens` |
| 1772 | `Nome: {}` | pt | `files.nome_2` |
| 1773 | `Local: {}` | pt | `files.local` |
| 1778 | `Tipo: Pasta` | pt | `files.tipo_pasta` |
| 1781 | `Conteúdo: {} arquivos, {} pastas` | pt | `files.conteudo_arquivos_pastas` |
| 1786 | `Tamanho: {}` | pt | `files.tamanho_2` |
| 1790 | `Tipo: {}` | pt | `files.tipo` |
| 1794 | `Tamanho: {} ({} bytes)` | pt | `files.tamanho_bytes` |
| 1799 | `Criado: {}` | neutro | `files.criado` |
| 1800 | `Modificado: {}` | pt | `files.modificado` |
| 1807 | `Erro: {}` | pt | `files.erro` |
| 1810 | `Seleção: {} itens` | pt | `files.selecao_itens` |
| 1818 | `Tamanho total: {}` | misto | `files.tamanho_total` |
| 1822 | `Pasta: {}` | pt | `files.pasta` |
| 1824 | `Itens: {}` | neutro | `files.itens_2` |
| 1828 | `Livre: {} de {}` | pt | `files.livre` |
| 1833 | `Volume: memória (não persiste)` | pt | `files.volume_memoria_nao_persiste` |
| 1854 | `Cancelar` | pt | `files.cancelar` |
| 1854 | `Excluir` | pt | `files.excluir` |
| 1855 | `Concluído` | pt | `files.concluido` |
| 1856 | `Cancelar` | pt | `files.cancelar_2` |
| 1878 | `Erro: {}` | pt | `files.erro_2` |
| 1882 | `Pacote: pacote de app válido` | pt | `files.pacote_pacote_app_valido` |
| 1887 | `Estado: não instalado (Enter instala e abre)` | misto | `files.estado_nao_instalado_enter` |
| 1892 | `Pacote: inválido ({e})` | pt | `files.pacote_invalido` |
| 1907 | `Manifesto: indisponível` | pt | `files.manifesto_indisponivel` |
| 1910 | `Estado: {}` | neutro | `files.estado` |
| 1913 | `Pacote: {}` | neutro | `files.pacote` |
| 1915 | `Arquivo: /apps/{app_id}.wasm` | pt | `files.arquivo_apps_app_id` |
| 1917 | `Origem: embutido no sistema (I instala)` | pt | `files.origem_embutido_no_sistema` |
| 2043 | `{} itens` | neutro | `files.itens_3` |
| 2045 | `Seleção` | pt | `files.selecao` |
| 2047 | `Tamanho` | pt | `files.tamanho_3` |
| 2059 | `Aplicativo` | neutro | `files.aplicativo` |
| 2061 | `Estado` | neutro | `files.estado_2` |
| 2067 | `Pacote` | neutro | `files.pacote_2` |
| 2067 | `Tamanho` | pt | `files.tamanho_4` |
| 2073 | `Excluído` | pt | `files.excluido_2` |
| 2073 | `Modificado` | pt | `files.modificado_2` |
| 2084 | `Imagem grande demais para pré-visualizar` | pt | `files.imagem_grande_demais_para` |
| 2094 | `Dimensões` | pt | `files.dimensoes` |
| 2095 | `{} × {} px` | neutro | `files.px` |
| 2100 | `Não foi possível abrir a imagem` | pt | `files.nao_foi_possivel_abrir` |
| 2109 | `Sem pré-visualização` | pt | `files.sem_pre_visualizacao` |
| 2118 | `Itens` | neutro | `files.itens_4` |

**`kernel/src/desktop/files_ui.rs`** (29)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 111 | `Favoritos` | pt | `files.favoritos` |
| 172 | `Início` | pt | `files.inicio` |
| 174 | `Imagens` | pt | `files.imagens` |
| 176 | `Lixeira` | pt | `files.lixeira` |
| 179 | `Memória` | pt | `files.memoria` |
| 181 | `Disco` | pt | `files.disco` |
| 230 | `{} livres` | neutro | `files.livres` |
| 372 | `Buscar` | pt | `files.buscar` |
| 410 | `Data da exclusão` | pt | `files.data_exclusao` |
| 414 | `Última modificação` | pt | `files.ultima_modificacao` |
| 463 | `Nome` | pt | `files.nome` |
| 471 | `Tamanho` | pt | `files.tamanho` |
| 564 | `Nenhum resultado` | pt | `files.nenhum_resultado` |
| 565 | `Nada encontrado para “{}”` | pt | `files.nada_encontrado_para` |
| 568 | `Lixeira vazia` | pt | `files.lixeira_vazia` |
| 570 | `Nenhum app` | pt | `files.nenhum_app` |
| 574 | `Pasta vazia` | pt | `files.pasta_vazia` |
| 575 | `Arraste itens para cá` | pt | `files.arraste_itens_para_ca` |
| 858 | `Selecione um item` | pt | `files.selecione_item` |
| 983 | `{} de {} itens` | pt | `files.itens` |
| 1008 | `Enter abre  ·  I instala  ·  Del remove` | en | `files.enter_abre_i_instala` |
| 1046 | `O item será apagado de vez. Isso não pode ser desfeito.` | pt | `files.item_sera_apagado_vez` |
| 1049 | `Os {n} itens serão apagados de vez. Isso não pode ser desfeito.` | pt | `files.os_n_itens_serao` |
| 1054 | `Excluir permanentemente?` | pt | `files.excluir_permanentemente` |
| 1055 | `Excluir da lixeira?` | pt | `files.excluir_lixeira` |
| 1057 | `Esvaziar a lixeira?` | pt | `files.esvaziar_lixeira` |
| 1058 | `Tudo o que está na lixeira será apagado de vez.` | pt | `files.tudo_que_esta_na` |
| 1087 | `Informações` | pt | `files.informacoes` |
| 1140 | `{} {} arquivos` | pt | `files.arquivos` |

**`kernel/src/desktop/sysstore.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 65 | `Memoria (sem disco v3)` | pt sem-acento | `files.memoria_sem_disco_v3` |

**`kernel/src/desktop/vfs.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 94 | `Disco pequeno demais: arquivos so na memoria` | pt sem-acento | `files.disco_pequeno_demais_arquivos` |
| 95 | `Sem disco: arquivos so na memoria` | pt sem-acento | `files.sem_disco_arquivos_so` |
| 96 | `Disco desconhecido (intocado): arquivos so na memoria` | pt sem-acento | `files.disco_desconhecido_intocado_arquivos` |
| 97 | `Falha no disco: arquivos so na memoria` | pt sem-acento | `files.falha_no_disco_arquivos` |

**`osjeff_core/src/fileman.rs`** (28)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 941 | `Enter` | en | `files.enter` |
| 949 | `Espaço` | pt | `files.espaco` |
| 977 | `Abrir` | pt | `files.abrir` |
| 980 | `Instalar e abrir` | pt | `files.instalar_abrir` |
| 983 | `Informações` | pt | `files.informacoes` |
| 990 | `Restaurar` | pt | `files.restaurar` |
| 991 | `Excluir permanentemente` | pt | `files.excluir_permanentemente` |
| 993 | `Esvaziar a lixeira` | pt | `files.esvaziar_lixeira` |
| 995 | `Informações` | pt | `files.informacoes_2` |
| 997 | `Selecionar tudo` | pt | `files.selecionar_tudo` |
| 1002 | `Abrir` | pt | `files.abrir_2` |
| 1003 | `Pré-visualizar` | pt | `files.pre_visualizar` |
| 1006 | `Definir como papel de parede` | pt | `files.definir_como_papel_parede` |
| 1008 | `Recortar` | pt | `files.recortar` |
| 1009 | `Copiar` | pt | `files.copiar` |
| 1011 | `Renomear` | pt | `files.renomear` |
| 1013 | `Excluir` | pt | `files.excluir` |
| 1014 | `Excluir permanentemente` | pt | `files.excluir_permanentemente_2` |
| 1015 | `Informações` | pt | `files.informacoes_3` |
| 1017 | `Novo arquivo` | pt | `files.novo_arquivo` |
| 1018 | `Nova pasta` | pt | `files.nova_pasta` |
| 1020 | `Colar` | pt | `files.colar` |
| 1022 | `Selecionar tudo` | pt | `files.selecionar_tudo_2` |
| 1024 | `Informações` | pt | `files.informacoes_4` |
| 1477 | `1 item` | neutro | `files.1_item` |
| 1479 | `{n} itens` | neutro | `files.n_itens` |
| 1489 | `1 selecionado ({})` | neutro | `files.1_selecionado` |
| 1491 | `{k} selecionados ({})` | neutro | `files.k_selecionados` |

**`osjeff_core/src/fileman/apps.rs`** (16)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 49 | `não instalado` | pt | `files.nao_instalado` |
| 83 | `Já instalado` | pt | `files.ja_instalado` |
| 85 | `Não instalado` | pt | `files.nao_instalado_2` |
| 92 | `1 (desenho contínuo)` | pt | `files.1_desenho_continuo` |
| 93 | `2 (por eventos)` | pt | `files.2_por_eventos` |
| 97 | `só /data/{}` | pt | `files.so_data` |
| 98 | `pasta do usuário (/home)` | misto | `files.pasta_usuario_home` |
| 102 | `HTTP e HTTPS (endereços públicos)` | pt | `files.http_https_enderecos_publicos` |
| 107 | `ler e escrever` | pt | `files.ler_escrever` |
| 110 | `App: {} ({})` | neutro | `files.app` |
| 111 | `Versão: {}   ABI {}` | pt | `files.versao_abi` |
| 112 | `Arquivos: {fs}` | pt | `files.arquivos_fs` |
| 113 | `Rede: {net}` | pt | `files.rede_net` |
| 114 | `Área de transferência: {clip}` | pt | `files.area_transferencia_clip` |
| 115 | `Memória: {} MiB   Disco: {} KiB` | pt | `files.memoria_mib_disco_kib` |
| 117 | `Arquivos abertos: {}   Janela {}x{}{}` | pt | `files.arquivos_abertos_janela_x` |

**`osjeff_core/src/fileman/ui.rs`** (9)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 951 | `Pasta` | pt | `files.pasta` |
| 959 | `Imagem {upper}` | pt | `files.imagem_upper` |
| 960 | `Aplicativo` | neutro | `files.aplicativo` |
| 961 | `Texto` | neutro | `files.texto` |
| 962 | `Texto {upper}` | neutro | `files.texto_upper` |
| 963 | `Arquivo` | pt | `files.arquivo` |
| 964 | `Arquivo {upper}` | pt | `files.arquivo_upper` |
| 1025 | `Hoje, {hm}` | pt | `files.hoje_hm` |
| 1026 | `Ontem, {hm}` | pt | `files.ontem_hm` |

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
| 216 | `trash flag does not match location` | en | `files.trash_flag_does_not` |
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
| 535 | `bitmap does not cover the metadata` | en | `files.bitmap_does_not_cover` |
| 538 | `root/trash inode not allocated` | en | `files.root_trash_inode_not` |
| 581 | `journal target out of range` | en | `files.journal_target_out_range` |
| 599 | `bad root inode` | en | `files.bad_root_inode` |
| 603 | `bad trash inode` | en | `files.bad_trash_inode` |
| 607 | `root has no .trash entry` | misto | `files.root_has_no_trash` |
| 728 | `metadata block out of range` | en | `files.metadata_block_out_range` |
| 803 | `double free of an inode` | en | `files.double_free_an_inode` |
| 827 | `block range outside the data area` | misto sem-acento | `files.block_range_outside_data` |
| 831 | `block allocated or freed twice` | en | `files.block_allocated_or_freed` |

**`osjeff_core/src/fs3/ops.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 121 | `directory entry points at a free inode` | en | `files.directory_entry_points_at` |
| 190 | `inode missing from its parent` | en | `files.inode_missing_from_its` |
| 321 | `directory entry changed under us` | en | `files.directory_entry_changed_under` |
| 376 | `directory tree does not terminate` | en | `files.directory_tree_does_not` |

**`osjeff_core/src/vfs.rs`** (18)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 73 | `Item nao encontrado` | pt sem-acento | `files.item_nao_encontrado` |
| 74 | `Ja existe um item com esse nome` | pt | `files.ja_existe_item_com` |
| 75 | `O destino nao e uma pasta` | pt sem-acento | `files.destino_nao_pasta` |
| 76 | `O item e uma pasta` | pt | `files.item_pasta` |
| 77 | `A pasta nao esta vazia` | pt sem-acento | `files.pasta_nao_esta_vazia` |
| 78 | `Nome invalido` | pt sem-acento | `files.nome_invalido` |
| 79 | `Nome longo demais (maximo 255 bytes)` | pt sem-acento | `files.nome_longo_demais_maximo` |
| 80 | `Caminho invalido` | pt sem-acento | `files.caminho_invalido` |
| 81 | `Item reservado do sistema` | pt | `files.item_reservado_sistema` |
| 82 | `Nao e possivel mover uma pasta para dentro dela mesma` | pt sem-acento | `files.nao_possivel_mover_pasta` |
| 83 | `Disco cheio` | pt | `files.disco_cheio` |
| 84 | `Limite de arquivos do disco atingido` | pt | `files.limite_arquivos_disco_atingido` |
| 85 | `Arquivo grande demais` | pt | `files.arquivo_grande_demais` |
| 86 | `Sistema de arquivos ocupado` | pt | `files.sistema_arquivos_ocupado` |
| 87 | `Sem sistema de arquivos` | pt | `files.sem_sistema_arquivos` |
| 88 | `Erro de leitura/escrita no disco` | pt | `files.erro_leitura_escrita_no` |
| 89 | `Sistema de arquivos danificado` | pt | `files.sistema_arquivos_danificado` |
| 90 | `Operacao cancelada` | pt sem-acento | `files.operacao_cancelada` |

### Editor

**`kernel/src/desktop/edit.rs`** (10)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 150 | `sem nome` | pt | `editor.sem_nome` |
| 196 | `Todos` | pt | `editor.todos` |
| 210 | `Cancelar` | pt | `editor.cancelar` |
| 210 | `Salvar` | pt | `editor.salvar` |
| 221 | `Cancelar` | pt | `editor.cancelar_2` |
| 225 | `Abrir` | pt | `editor.abrir` |
| 227 | `Salvar` | pt | `editor.salvar_2` |
| 596 | `Novo arquivo` | pt | `editor.novo_arquivo` |
| 602 | `Editor: {}` | neutro | `editor.editor` |
| 638 | `Salvo: {}` | neutro | `editor.salvo` |

**`kernel/src/desktop/edit_ui.rs`** (19)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 51 | `Abrir` | pt | `editor.abrir` |
| 53 | `Salvar como` | pt | `editor.salvar_como` |
| 245 | `Ir para a linha` | pt | `editor.ir_para_linha` |
| 246 | `Buscar` | pt | `editor.buscar` |
| 272 | `Substituir por` | pt | `editor.substituir_por` |
| 308 | `Todos` | pt | `editor.todos` |
| 323 | `Nenhum resultado` | pt | `editor.nenhum_resultado` |
| 324 | `Recomeçou do início` | pt | `editor.recomecou_inicio` |
| 325 | `1 substituição` | pt | `editor.1_substituicao` |
| 326 | `{n} substituições` | pt | `editor.n_substituicoes` |
| 327 | `Linha inválida` | pt | `editor.linha_invalida` |
| 373 | `Deseja salvar as alterações?` | pt | `editor.deseja_salvar_as_alteracoes` |
| 379 | `As alterações em “{}” serão perdidas se você não as salvar.` | pt | `editor.as_alteracoes_em_serao` |
| 399 | `Cancelar` | pt | `editor.cancelar` |
| 400 | `Salvar` | pt | `editor.salvar` |
| 449 | `Favoritos` | pt | `editor.favoritos` |
| 583 | `Pasta vazia` | pt | `editor.pasta_vazia` |
| 601 | `Nome do arquivo` | pt | `editor.nome_arquivo` |
| 612 | `Já existe “{}”. Substituir?` | pt | `editor.ja_existe_substituir` |

**`osjeff_core/src/editor2/dialog.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 463 | `Salvar` | pt | `editor.salvar` |
| 465 | `Cancelar` | pt | `editor.cancelar` |
| 521 | `{}.{} KB` | neutro | `editor.kb` |
| 524 | `{}.{} MB` | neutro | `editor.mb` |
| 534 | `Ln {}, Col {}   {} linhas   {}   UTF-8   {}` | neutro | `editor.ln_col_linhas_utf` |
| 545 | `   sel {}` | neutro | `editor.sel` |
| 548 | `   SO LEITURA` | neutro | `editor.so_leitura` |
| 550 | `   * modificado` | pt | `editor.modificado` |
| 569 | `Ln {}, Col {}` | neutro | `editor.ln_col` |
| 591 | `1 linha` | neutro | `editor.1_linha` |
| 593 | `{} linhas` | neutro | `editor.linhas` |

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

**`osjeff_core/src/editor2/search.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 368 | `Replace with: ` | en | `editor.replace_with` |
| 371 | `Go to line: ` | en | `editor.go_line` |

**`osjeff_core/src/editor2/ui.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 268 | `Início` | pt | `editor.inicio` |
| 270 | `Imagens` | pt | `editor.imagens` |
| 271 | `Disco` | pt | `editor.disco` |

### Terminal

**`kernel/src/desktop/shellhost.rs`** (10)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 301 | `hd{}` | neutro | `term.hd` |
| 302 | `boot disk` | en | `term.boot_disk` |
| 484 | `{p}: Is a directory` | en | `term.p_is_directory` |
| 490 | `usage: edit [FILE]` | en | `term.usage_edit_file` |
| 528 | `edit [FILE]: open the text editor` | en | `term.edit_file_open_text` |
| 529 | `files: open the file manager` | en | `term.files_open_file_manager` |
| 530 | `tasks: open the task manager` | en | `term.tasks_open_task_manager` |
| 531 | `calc: open the calculator` | en | `term.calc_open_calculator` |
| 532 | `reboot: restart the machine` | en | `term.reboot_restart_machine` |
| 533 | `shutdown: power the machine off` | en | `term.shutdown_power_machine_off` |

**`kernel/src/desktop/term.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 254 | `sh: too many commands waiting` | en | `term.sh_too_many_commands` |
| 259 | `sh: the command thread stopped` | en | `term.sh_command_thread_stopped` |
| 261 | `sh: no shell` | pt | `term.sh_no_shell` |

**`osjeff_core/src/shell/builtins.rs`** (107)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 19 | `help [CMD]: list commands or describe one` | en | `term.help_cmd_list_commands` |
| 20 | `ls [-aF1l] [PATH...]: list directory contents` | en | `term.ls_af1l_path_list` |
| 22 | `pwd: print the working directory` | en | `term.pwd_print_working_directory` |
| 23 | `cat [-n] [FILE...]: print files or stdin` | en | `term.cat_n_file_print` |
| 26 | `rm [-rf] PATH...: remove files or directories` | en | `term.rm_rf_path_remove` |
| 27 | `rmdir DIR...: remove empty directories` | en | `term.rmdir_dir_remove_empty` |
| 28 | `mv SRC... DEST: move or rename` | en | `term.mv_src_dest_move` |
| 29 | `cp [-r] SRC... DEST: copy files or directories` | en | `term.cp_r_src_dest` |
| 30 | `touch FILE...: create empty files` | en | `term.touch_file_create_empty` |
| 31 | `head [-n N] [FILE...]: first lines` | en | `term.head_n_n_file` |
| 32 | `tail [-n N] [FILE...]: last lines` | en | `term.tail_n_n_file` |
| 33 | `wc [-lwcm] [FILE...]: count lines, words, bytes` | en | `term.wc_lwcm_file_count` |
| 36 | `grep [-ivncFlqHh] PATTERN [FILE...]: search text` | en | `term.grep_ivncflqhh_pattern_file` |
| 39 | `sort [-rnu] [FILE...]: sort lines` | en | `term.sort_rnu_file_sort` |
| 47 | `tee [-a] FILE...: copy stdin to stdout and files` | en | `term.tee_file_copy_stdin` |
| 50 | `clear: clear the terminal` | en | `term.clear_clear_terminal` |
| 54 | `export [NAME[=VALUE]...]: mark variables exported` | en | `term.export_name_value_mark` |
| 57 | `unset NAME...: remove variables` | en | `term.unset_name_remove_variables` |
| 60 | `history [N\|-c]: show or clear the command history` | en | `term.history_n_c_show` |
| 65 | `alias [NAME=VALUE...]: define or list aliases` | en | `term.alias_name_value_define` |
| 68 | `unalias [-a] NAME...: remove aliases` | en | `term.unalias_name_remove_aliases` |
| 69 | `which CMD...: show how a command resolves` | en | `term.which_cmd_show_how` |
| 70 | `date [+FORMAT]: print the date and time` | en | `term.date_format_print_date` |
| 71 | `uptime: time since boot` | en | `term.uptime_time_since_boot` |
| 72 | `free [-bkm]: memory usage` | en | `term.free_bkm_memory_usage` |
| 73 | `df: disk usage` | en | `term.df_disk_usage` |
| 76 | `ping [-c N] HOST: test network reachability` | en | `term.ping_c_n_host` |
| 81 | `sleep SECONDS: wait` | en | `term.sleep_seconds_wait` |
| 82 | `seq [FIRST] LAST: print a sequence of numbers` | en | `term.seq_first_last_print` |
| 85 | `basename PATH [SUFFIX]: last path component` | en | `term.basename_path_suffix_last` |
| 90 | `dirname PATH: directory part of a path` | en | `term.dirname_path_directory_part` |
| 93 | `stat PATH...: show kind and size` | en | `term.stat_path_show_kind` |
| 98 | `cut -d C -f N[,M]: select fields (stdin or files)` | en | `term.cut_d_c_f` |
| 101 | `nl: number lines (stdin or files)` | en | `term.nl_number_lines_stdin` |
| 104 | `yes [WORD]: repeat a word (bounded by the output limit)` | en | `term.yes_word_repeat_word` |
| 180 | `option requires an argument -- '{c}'` | en | `term.option_requires_an_argument` |
| 187 | `invalid option -- '{c}'` | en | `term.invalid_option_c` |
| 246 | `no help for '{name}'` | misto | `term.no_help_for_name` |
| 293 | `cannot access '{t}': {}` | en | `term.cannot_access_t` |
| 346 | `too many arguments` | en | `term.too_many_arguments` |
| 359 | `OLDPWD not set` | en | `term.oldpwd_not_set` |
| 465 | `missing operand` | en | `term.missing_operand` |
| 493 | `cannot create directory '{d}': {}` | en | `term.cannot_create_directory_d` |
| 524 | `missing operand` | en | `term.missing_operand_2` |
| 534 | `refusing to remove '{p}'` | en | `term.refusing_remove_p` |
| 541 | `cannot remove '{p}': {}` | en | `term.cannot_remove_p` |
| 546 | `cannot remove '{p}': Is a directory` | en | `term.cannot_remove_p_is` |
| 551 | `cannot remove '{at}': {}` | en | `term.cannot_remove_at` |
| 563 | `missing operand` | en | `term.missing_operand_3` |
| 571 | `failed to remove '{d}': {}` | en | `term.failed_remove_d` |
| 577 | `failed to remove '{d}': Not a directory` | en | `term.failed_remove_d_not` |
| 582 | `failed to remove '{d}': {}` | en | `term.failed_remove_d_2` |
| 604 | `usage: mv SRC... DEST` | en | `term.usage_mv_src_dest` |
| 610 | `target '{dst}' is not a directory` | en | `term.target_dst_is_not` |
| 617 | `cannot move '{s}' to '{to}': {}` | en | `term.cannot_move_s` |
| 635 | `cannot stat '{src}': {}` | en | `term.cannot_stat_src` |
| 640 | `cannot read '{src}': {}` | en | `term.cannot_read_src` |
| 644 | `cannot create '{dst}': {}` | en | `term.cannot_create_dst` |
| 647 | `-r not specified; omitting directory '{src}'` | en | `term.r_not_specified_omitting` |
| 650 | `'{src}': nesting too deep` | en | `term.src_nesting_too_deep` |
| 655 | `cannot copy '{src}' into itself` | en | `term.cannot_copy_src_into` |
| 659 | `cannot create directory '{dst}': {}` | en | `term.cannot_create_directory_dst` |
| 664 | `cannot read '{src}': {}` | en | `term.cannot_read_src_2` |
| 682 | `usage: cp [-r] SRC... DEST` | en | `term.usage_cp_r_src` |
| 688 | `target '{dst}' is not a directory` | en | `term.target_dst_is_not_2` |
| 705 | `missing file operand` | en | `term.missing_file_operand` |
| 713 | `cannot touch '{f}': {}` | en | `term.cannot_touch_f` |
| 730 | `invalid number of lines: '{v}'` | en | `term.invalid_number_lines_v` |
| 823 | `usage: grep [OPTIONS] PATTERN [FILE...]` | en | `term.usage_grep_options_pattern` |
| 837 | `invalid pattern: {}` | en | `term.invalid_pattern` |
| 1049 | `export {n}="{v}"\n` | neutro | `term.export_n_v` |
| 1063 | `'{a}': not a valid identifier` | en | `term.not_valid_identifier` |
| 1072 | `cannot set '{name}'` | en | `term.cannot_set_name` |
| 1099 | `{a}: numeric argument required` | neutro | `term.numeric_argument_required` |
| 1121 | `alias {k}='{v}'\n` | neutro | `term.alias_k_v` |
| 1133 | `too many aliases` | en | `term.too_many_aliases` |
| 1140 | `invalid alias name in '{a}'` | en | `term.invalid_alias_name_in` |
| 1146 | `{a}: not found` | en | `term.not_found` |
| 1161 | `usage: unalias [-a] NAME...` | en | `term.usage_unalias_name` |
| 1167 | `{a}: not found` | en | `term.not_found_2` |
| 1176 | `usage: which CMD...` | en | `term.usage_which_cmd` |
| 1195 | `{n}.sh` | neutro | `term.n_sh` |
| 1208 | `{n} not found\n` | en | `term.n_not_found` |
| 1228 | `invalid date '{f}'` | en | `term.invalid_date_f` |
| 1272 | `up {d} {unit}, {h:02}:{m:02}:{s:02}` | en | `term.up_d_unit_h` |
| 1274 | `up {h:02}:{m:02}:{s:02}` | en | `term.up_h_02_m` |
| 1385 | `invalid signal '{s}'` | en | `term.invalid_signal_s` |
| 1394 | `usage: kill [-SIGNAL] PID...` | en | `term.usage_kill_signal_pid` |
| 1407 | `invalid pid '{p}'` | en | `term.invalid_pid_p` |
| 1424 | `invalid count '{v}'` | en | `term.invalid_count_v` |
| 1430 | `usage: ping [-c N] HOST` | en | `term.usage_ping_c_n` |
| 1526 | `argument expected` | en | `term.argument_expected` |
| 1532 | `missing ')'` | en | `term.missing` |
| 1560 | `integer expression expected` | en | `term.integer_expression_expected` |
| 1611 | `too many arguments` | en | `term.too_many_arguments_2` |
| 1629 | `missing ']'` | en | `term.missing_2` |
| 1638 | `missing operand` | en | `term.missing_operand_4` |
| 1656 | `invalid time interval '{arg}'` | en | `term.invalid_time_interval_arg` |
| 1675 | `usage: seq [FIRST [INCR]] LAST` | en | `term.usage_seq_first_incr` |
| 1686 | `increment must not be zero` | en | `term.increment_must_not_be` |
| 1707 | `missing operand` | en | `term.missing_operand_5` |
| 1729 | `missing operand` | en | `term.missing_operand_6` |
| 1750 | `missing operand` | en | `term.missing_operand_7` |
| 1765 | `cannot stat '{p}': {}` | en | `term.cannot_stat_p` |
| 1838 | `usage: tr SET1 SET2 \| tr -d SET1` | en | `term.usage_tr_set1_set2` |
| 1866 | `you must specify a list of fields (-f)` | en | `term.you_must_specify_list` |
| 1874 | `invalid field '{part}'` | en | `term.invalid_field_part` |

**`osjeff_core/src/shell/exec.rs`** (32)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 326 | `exit [N]: leave the shell or script with status N` | en | `term.exit_n_leave_shell` |
| 327 | `return [N]: leave a function with status N` | en | `term.return_n_leave_function` |
| 329 | `continue [N]: next iteration of the Nth loop` | en | `term.continue_n_next_iteration` |
| 330 | `shift [N]: drop the first N positional arguments` | en | `term.shift_n_drop_first` |
| 331 | `source FILE [ARGS]: run a script in this shell` | en | `term.source_file_args_run` |
| 333 | `sh FILE [ARGS]: run a script file` | en | `term.sh_file_args_run` |
| 336 | `set [NAME=VALUE \| -- ARGS]: list variables or set them` | en | `term.set_name_value_args` |
| 338 | `:: do nothing, successfully` | pt | `term.nothing_successfully` |
| 560 | `{name}: syntax error at line {l}, column {c}: {}\n` | en | `term.name_syntax_error_at` |
| 587 | `step limit exceeded, aborting` | neutro | `term.step_limit_exceeded_aborting` |
| 606 | `pipe buffer limit reached, data dropped` | pt | `term.pipe_buffer_limit_reached` |
| 675 | `pipe buffer limit reached, data dropped` | pt | `term.pipe_buffer_limit_reached_2` |
| 717 | `loop iteration limit reached` | neutro | `term.loop_iteration_limit_reached` |
| 737 | `loop iteration limit reached` | neutro | `term.loop_iteration_limit_reached_2` |
| 761 | `too many functions` | en | `term.too_many_functions` |
| 824 | `ambiguous redirect` | neutro | `term.ambiguous_redirect` |
| 863 | `cannot set variable` | en | `term.cannot_set_variable` |
| 953 | `{name}: Is a directory` | en | `term.name_is_directory` |
| 956 | `{name}: command not found` | en | `term.name_command_not_found` |
| 968 | `{name}.sh` | en | `term.name_sh` |
| 1023 | `function call depth limit exceeded` | neutro | `term.function_call_depth_limit` |
| 1058 | `{path}: script too large` | en | `term.path_script_too_large` |
| 1070 | `script nesting limit exceeded` | neutro | `term.script_nesting_limit_exceeded` |
| 1110 | `exit: numeric argument required` | en | `term.exit_numeric_argument_required` |
| 1118 | `return: only valid in a function or sourced script` | en | `term.return_only_valid_in` |
| 1126 | `return: numeric argument required` | neutro | `term.return_numeric_argument_required` |
| 1141 | `{}: bad loop count` | en | `term.bad_loop_count` |
| 1157 | `shift: bad count` | en | `term.shift_bad_count` |
| 1163 | `{}: file name required` | en | `term.file_name_required` |
| 1198 | `set: invalid argument '{a}'` | en | `term.set_invalid_argument` |
| 1230 | `command substitution nested too deeply` | en | `term.command_substitution_nested_too` |
| 1248 | `command substitution output truncated` | en | `term.command_substitution_output_truncated` |

**`osjeff_core/src/shell/fs.rs`** (11)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 32 | `No such file or directory` | misto | `term.no_such_file_or` |
| 33 | `Not a directory` | en | `term.not_directory` |
| 34 | `Is a directory` | en | `term.is_directory` |
| 35 | `File exists` | en | `term.file_exists` |
| 36 | `Directory not empty` | en | `term.directory_not_empty` |
| 37 | `No space left on device` | misto | `term.no_space_left_on` |
| 38 | `File too large` | en | `term.file_too_large` |
| 39 | `File name too long` | en | `term.file_name_too_long` |
| 40 | `Invalid path` | en | `term.invalid_path` |
| 41 | `Read-only file system` | en | `term.read_only_file_system` |
| 42 | `Input/output error` | en | `term.input_output_error` |

**`osjeff_core/src/shell/glob.rs`** (3)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 143 | `division by zero` | en | `term.division_by_zero` |
| 144 | `arithmetic syntax error` | en | `term.arithmetic_syntax_error` |
| 145 | `arithmetic expression too deep` | en | `term.arithmetic_expression_too_deep` |

**`osjeff_core/src/shell/line.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 232 | `(reverse-i-search)'{}': ` | neutro | `term.reverse_i_search` |

**`osjeff_core/src/shell/netcmds.rs`** (17)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 24 | `nslookup HOST: look up the IPv4 addresses of a name` | en | `term.nslookup_host_look_up` |
| 29 | `curl [-sSfiL] [-o FILE\|-O] URL: fetch a http(s) URL` | en | `term.curl_ssfil_file_url` |
| 34 | `wget [-q] [-O FILE\|-] URL: download a http(s) URL to a file` | en | `term.wget_q_file_url` |
| 37 | `ifconfig: show the network interface` | en | `term.ifconfig_show_network_interface` |
| 67 | `usage: nslookup HOST` | en | `term.usage_nslookup_host` |
| 85 | `can't find {host}: NXDOMAIN` | en | `term.can_t_find_host` |
| 139 | `usage: curl [-sSfiL] [-o FILE\|-O] URL` | en | `term.usage_curl_ssfil_file` |
| 168 | `Could not resolve host` | en | `term.could_not_resolve_host` |
| 180 | `(22) The requested URL returned error: {}` | en | `term.22_requested_url_returned` |
| 203 | `(23) body cut at {} bytes` | en | `term.23_body_cut_at` |
| 214 | `usage: wget [-q] [-O FILE\|-] URL` | en | `term.usage_wget_q_file` |
| 241 | `{url}: server returned error {}` | en | `term.url_server_returned_error` |
| 253 | `warning: body cut at {} bytes` | en | `term.warning_body_cut_at` |
| 259 | `{}.{} KiB` | neutro | `term.kib` |
| 264 | `no network interface` | misto | `term.no_network_interface` |
| 274 | `  inet {ip}/{}` | neutro | `term.inet_ip` |
| 276 | `  gateway {g}` | neutro | `term.gateway_g` |

**`osjeff_core/src/shell/parse.rs`** (10)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 68 | `bad ${ } substitution` | en | `term.bad_substitution` |
| 69 | `unexpected token` | en | `term.unexpected_token` |
| 70 | `unexpected end of input` | en | `term.unexpected_end_input` |
| 71 | `expected '{what}'` | en | `term.expected_what` |
| 72 | `redirection needs a file name` | en | `term.redirection_needs_file_name` |
| 73 | `'&' (background jobs) is not supported` | en | `term.background_jobs_is_not` |
| 74 | `'<<' (here-documents) is not supported` | en | `term.here_documents_is_not` |
| 76 | `descriptor redirections such as '2>' are not supported` | misto | `term.descriptor_redirections_such_as` |
| 78 | `invalid function name` | en | `term.invalid_function_name` |
| 79 | `nesting is too deep` | en | `term.nesting_is_too_deep` |

**`osjeff_core/src/shell/regex.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 27 | `nothing to repeat` | en | `term.nothing_repeat` |
| 29 | `pattern too long` | en | `term.pattern_too_long` |

**`osjeff_core/src/shell/sys.rs`** (7)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 90 | `not supported on this system` | en | `term.not_supported_on_this` |
| 91 | `no such process` | pt | `term.no_such_process` |
| 92 | `operation not permitted` | en | `term.operation_not_permitted` |
| 93 | `network is unreachable` | en | `term.network_is_unreachable` |
| 94 | `host not found` | en | `term.host_not_found` |
| 95 | `timed out` | en | `term.timed_out` |
| 97 | `transfer failed` | en | `term.transfer_failed` |

### Tarefas

**`kernel/src/desktop/tarefas.rs`** (70)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 203 | `Memória` | pt | `tasks.memoria` |
| 203 | `Disco` | pt | `tasks.disco` |
| 203 | `Rede` | pt | `tasks.rede` |
| 211 | `Memória` | pt | `tasks.memoria_2` |
| 212 | `Disco` | pt | `tasks.disco_2` |
| 213 | `Rede` | pt | `tasks.rede_2` |
| 672 | `{} encerrado.` | neutro | `tasks.encerrado` |
| 687 | `{n} aplicativo(s) encerrado(s).` | neutro | `tasks.n_aplicativo_s_encerrado` |
| 700 | `{n} comando(s) interrompido(s).` | pt | `tasks.n_comando_s_interrompido` |
| 732 | `{} reiniciado.` | neutro | `tasks.reiniciado` |
| 1124 | `Uso do processador` | pt | `tasks.uso_processador` |
| 1168 | `Tempo ligado` | pt | `tasks.tempo_ligado` |
| 1185 | `Carga média` | pt | `tasks.carga_media` |
| 1193 | `{} processos` | neutro | `tasks.processos` |
| 1216 | `Processador` | neutro | `tasks.processador` |
| 1237 | `{vm}{feats} recursos` | neutro | `tasks.vm_feats_recursos` |
| 1263 | `Por processo` | pt | `tasks.por_processo` |
| 1322 | `Memória em uso` | pt | `tasks.memoria_em_uso` |
| 1377 | `Pressão da memória` | pt | `tasks.pressao_memoria` |
| 1406 | `do espaço do sistema` | pt | `tasks.espaco_sistema` |
| 1417 | `Em uso` | pt | `tasks.em_uso` |
| 1419 | `Total` | en | `tasks.total` |
| 1422 | `Memória física` | pt | `tasks.memoria_fisica` |
| 1448 | `Por aplicativo` | pt | `tasks.por_aplicativo` |
| 1456 | `valores aproximados` | neutro | `tasks.valores_aproximados` |
| 1478 | `Nenhum aplicativo aberto.` | pt | `tasks.nenhum_aplicativo_aberto` |
| 1530 | `Disco principal` | pt | `tasks.disco_principal` |
| 1531 | `Memória (sem disco)` | pt | `tasks.memoria_sem_disco` |
| 1531 | `Os arquivos somem ao desligar` | pt | `tasks.os_arquivos_somem_ao` |
| 1578 | `Total` | en | `tasks.total_2` |
| 1580 | `Arquivos e pastas` | pt | `tasks.arquivos_pastas` |
| 1603 | `Leitura e gravação` | pt | `tasks.leitura_gravacao` |
| 1605 | `Gravação` | pt | `tasks.gravacao` |
| 1639 | `Leitura {} · Gravação {} · {}` | pt | `tasks.leitura_gravacao_2` |
| 1672 | `{} desde a inicialização` | pt | `tasks.desde_inicializacao` |
| 1681 | `{} desde a inicialização` | pt | `tasks.desde_inicializacao_2` |
| 1685 | `Gravação` | pt | `tasks.gravacao_2` |
| 1702 | `Sem placa de rede` | pt | `tasks.sem_placa_rede` |
| 1704 | `Sem sinal` | pt | `tasks.sem_sinal` |
| 1706 | `Procurando endereço` | pt | `tasks.procurando_endereco` |
| 1708 | `Conectado` | pt | `tasks.conectado` |
| 1771 | `Estático` | pt | `tasks.estatico` |
| 1773 | `{} restantes` | neutro | `tasks.restantes` |
| 1775 | `Sem expiração` | pt | `tasks.sem_expiracao` |
| 1787 | `Endereço IP` | pt | `tasks.endereco_ip` |
| 1787 | `Máscara` | pt | `tasks.mascara` |
| 1811 | `Concessão` | pt | `tasks.concessao` |
| 1819 | `Tráfego` | pt | `tasks.trafego` |
| 1821 | `Recebido` | pt | `tasks.recebido` |
| 1821 | `Enviado` | pt | `tasks.enviado` |
| 1855 | `Recebido {} · Enviado {} · {}` | pt | `tasks.recebido_enviado` |
| 1886 | `{} · {} pacotes` | neutro | `tasks.pacotes` |
| 1893 | `Recebido` | pt | `tasks.recebido_2` |
| 1899 | `{} · {} pacotes` | neutro | `tasks.pacotes_2` |
| 1906 | `Enviado` | pt | `tasks.enviado_2` |
| 1916 | `Erros e descartes` | pt | `tasks.erros_descartes` |
| 1933 | `Buscar` | pt | `tasks.buscar` |
| 2142 | `Nada para mostrar.` | pt | `tasks.nada_para_mostrar` |
| 2144 | `Nenhum processo corresponde à busca.` | pt | `tasks.nenhum_processo_corresponde_busca` |
| 2176 | `{} processos · {} threads · CPU {} · Memória {} · Disco {}` | pt | `tasks.processos_threads_cpu_memoria` |
| 2196 | `Nome interno: {} · {}` | pt | `tasks.nome_interno` |
| 2200 | `serviço do sistema` | pt | `tasks.servico_sistema` |
| 2219 | `Reiniciar` | pt | `tasks.reiniciar` |
| 2226 | `Encerrar` | neutro | `tasks.encerrar` |
| 2264 | `Encerrar “{name}”?` | en | `tasks.encerrar_name` |
| 2274 | `Todos os aplicativos instalados serão fechados.` | pt | `tasks.todos_os_aplicativos_instalados` |
| 2275 | `Os comandos em execução nos terminais serão interrompidos.` | pt | `tasks.os_comandos_em_execucao` |
| 2278 | `É um serviço do sistema. {msg}` | pt | `tasks.servico_sistema_msg` |
| 2296 | `Cancelar` | pt | `tasks.cancelar` |
| 2303 | `Encerrar` | neutro | `tasks.encerrar_2` |

**`osjeff_core/src/activity.rs`** (15)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 23 | `Rede (busca)` | pt | `tasks.rede_busca` |
| 25 | `Terminal (execução)` | pt | `tasks.terminal_execucao` |
| 27 | `Sistema` | pt | `tasks.sistema` |
| 35 | `Arquivos` | pt | `tasks.arquivos` |
| 36 | `Ajustes` | pt | `tasks.ajustes` |
| 38 | `Imagens` | pt | `tasks.imagens` |
| 143 | `{h} h {m:02} min` | neutro | `tasks.h_h_m_02` |
| 145 | `{m} min {s:02} s` | neutro | `tasks.m_min_s_02` |
| 170 | `há {secs} s` | pt | `tasks.ha_secs_s` |
| 463 | `Atenção` | pt | `tasks.atencao` |
| 464 | `Crítica` | pt | `tasks.critica` |
| 504 | `Em espera` | pt | `tasks.em_espera` |
| 536 | `Nome` | pt | `tasks.nome` |
| 539 | `Memória` | pt | `tasks.memoria` |
| 540 | `Tempo ativo` | pt | `tasks.tempo_ativo` |

**`osjeff_core/src/netstats.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 339 | `net: {} link={} tx={}/{}B err={} drop={} rx={}/{}B err={} drop={} \| ` | neutro | `tasks.net_link_tx_b` |
| 353 | `no address` | misto | `tasks.no_address` |
| 356 | ` lease={}s` | neutro | `tasks.lease_s` |
| 362 | ` dhcp={} renew={} rebind={} lost={} \| dns q={} hit={} failover={} fail={} \| ping {}/{}` | neutro | `tasks.dhcp_renew_rebind_lost` |

### Registro

**`kernel/src/desktop/logview.rs`** (19)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 28 | `Tudo` | pt | `log.tudo` |
| 28 | `Aviso` | pt | `log.aviso` |
| 28 | `Erro` | pt | `log.erro` |
| 322 | `Registro limpo.` | neutro | `log.registro_limpo` |
| 332 | `Salvo em /var/log/syslog.txt.` | pt | `log.salvo_em_var_log` |
| 334 | `Salvo em /var/log/syslog.txt (só o final).` | pt | `log.salvo_em_var_log_2` |
| 336 | `Disco cheio: não foi possível salvar.` | pt | `log.disco_cheio_nao_foi` |
| 337 | `Não foi possível salvar.` | pt | `log.nao_foi_possivel_salvar` |
| 453 | `Buscar no registro` | pt | `log.buscar_no_registro` |
| 461 | `Seguir` | neutro | `log.seguir` |
| 471 | `Limpar` | pt | `log.limpar` |
| 479 | `Salvar` | pt | `log.salvar` |
| 486 | `Hora` | pt | `log.hora` |
| 486 | `Nível` | pt | `log.nivel` |
| 486 | `Origem` | pt | `log.origem` |
| 591 | `O registro está vazio.` | pt | `log.registro_esta_vazio` |
| 593 | `Nenhuma linha corresponde ao filtro.` | pt | `log.nenhuma_linha_corresponde_ao` |
| 617 | `{} linhas` | neutro | `log.linhas` |
| 619 | `{} de {} linhas` | pt | `log.linhas_2` |

### Calculadora

**`kernel/src/desktop/calc_ui.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 280 | `Copiado` | neutro | `calc.copiado` |

**`osjeff_core/src/calc.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 422 | `Erro` | pt | `calc.erro` |

### Imagens

**`kernel/src/desktop/viewer.rs`** (22)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 69 | `Janelas demais abertas` | pt | `viewer.janelas_demais_abertas` |
| 105 | `Não foi possível abrir a imagem` | pt | `viewer.nao_foi_possivel_abrir` |
| 112 | `Arquivo grande demais` | pt | `viewer.arquivo_grande_demais` |
| 113 | `O Imagens abre arquivos de até 24 MiB.` | pt | `viewer.imagens_abre_arquivos_ate` |
| 130 | `Imagens — {}` | pt | `viewer.imagens` |
| 294 | `Já existe um arquivo com esse nome` | pt | `viewer.ja_existe_arquivo_com` |
| 304 | `Salvo como {}` | pt | `viewer.salvo_como` |
| 463 | `Papel de parede aplicado` | pt | `viewer.papel_parede_aplicado` |
| 648 | `Cancelar` | pt | `viewer.cancelar` |
| 648 | `Salvar` | pt | `viewer.salvar` |
| 828 | `Ajustar` | neutro | `viewer.ajustar` |
| 828 | `Preencher` | neutro | `viewer.preencher` |
| 871 | `Nenhuma imagem` | pt | `viewer.nenhuma_imagem` |
| 872 | `Abra uma imagem pelo Arquivos.` | pt | `viewer.abra_imagem_pelo_arquivos` |
| 1073 | `  ·  {} de {}` | pt | `viewer.text` |
| 1110 | `Salvar como` | pt | `viewer.salvar_como` |
| 1130 | `Nome do arquivo` | pt | `viewer.nome_arquivo` |
| 1138 | `PNG, BMP ou PPM` | pt | `viewer.png_bmp_ou_ppm` |
| 1153 | `Cancelar` | pt | `viewer.cancelar_2` |
| 1153 | `Salvar` | pt | `viewer.salvar_2` |
| 1159 | `Cancelar` | pt | `viewer.cancelar_3` |
| 1167 | `Salvar` | pt | `viewer.salvar_3` |

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
| 58 | `buffer size does not match the dimensions` | en | `viewer.buffer_size_does_not` |
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

**`osjeff_core/src/viewer.rs`** (22)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 382 | `Nome` | pt | `viewer.nome` |
| 385 | `Dimensões` | pt | `viewer.dimensoes` |
| 385 | `{w} × {h} px` | neutro | `viewer.w_h_px` |
| 388 | `Resolução` | pt | `viewer.resolucao` |
| 389 | `{},{} Mpx` | neutro | `viewer.mpx` |
| 392 | `Formato` | neutro | `viewer.formato` |
| 400 | `Tamanho` | pt | `viewer.tamanho` |
| 402 | `Transparência` | pt | `viewer.transparencia` |
| 403 | `Sim` | neutro | `viewer.sim` |
| 403 | `Não` | pt | `viewer.nao` |
| 405 | `Zoom` | neutro | `viewer.zoom` |
| 408 | `Posição` | pt | `viewer.posicao` |
| 409 | `{} de {}` | pt | `viewer.text` |
| 418 | `O arquivo {f} está danificado ou usa um recurso que o Imagens não suporta.` | pt | `viewer.arquivo_f_esta_danificado` |
| 422 | `Formato não reconhecido` | pt | `viewer.formato_nao_reconhecido` |
| 423 | `O Imagens abre arquivos PNG, BMP e PPM.` | pt | `viewer.imagens_abre_arquivos_png` |
| 425 | `Não foi possível abrir a imagem` | pt | `viewer.nao_foi_possivel_abrir` |
| 426 | `Não foi possível abrir a imagem` | pt | `viewer.nao_foi_possivel_abrir_2` |
| 427 | `Não foi possível abrir a imagem` | pt | `viewer.nao_foi_possivel_abrir_3` |
| 435 | `Imagem grande demais (limite de 16 Mpx)` | pt | `viewer.imagem_grande_demais_limite` |
| 436 | `Memória insuficiente` | pt | `viewer.memoria_insuficiente` |
| 437 | `Imagem inválida` | pt | `viewer.imagem_invalida` |

### Navegador

**`kernel/src/desktop/apps.rs`** (8)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 52 | `Buscar: ` | pt | `web.buscar` |
| 197 | `Pesquisar ou digitar um endereço` | pt | `web.pesquisar_ou_digitar_endereco` |
| 498 | `A identidade do servidor não foi comprovada: a conexão pode ser interceptada.` | pt | `web.identidade_servidor_nao_foi` |
| 508 | `Hora do sistema não confirmada: confira o relógio (sem resposta de servidor de hora).` | pt | `web.hora_sistema_nao_confirmada` |
| 523 | `Continuar mesmo assim (inseguro)` | neutro | `web.continuar_mesmo_assim_inseguro` |
| 531 | `Vale só para este site, nesta sessão.` | pt | `web.vale_so_para_este` |
| 559 | `Navegador` | neutro | `web.navegador` |
| 567 | `Pesquise ou digite um endereço na barra acima` | pt | `web.pesquise_ou_digite_endereco` |

**`kernel/src/netd.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 373 | `no DHCP ack` | pt | `web.no_dhcp_ack` |
| 374 | `no DHCP offer` | pt | `web.no_dhcp_offer` |

**`kernel/src/netstack.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 708 | `tcp stream error` | en | `web.tcp_stream_error` |

**`osjeff_core/src/browser.rs`** (20)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 67 | `Nao seguro` | pt sem-acento | `web.nao_seguro` |
| 68 | `Conexão segura` | pt | `web.conexao_segura` |
| 69 | `Certificado inválido` | pt | `web.certificado_invalido` |
| 116 | `Falha ao carregar a pagina.` | pt sem-acento | `web.falha_ao_carregar_pagina` |
| 117 | `Nome nao encontrado: confira o endereco (DNS).` | pt sem-acento | `web.nome_nao_encontrado_confira` |
| 118 | `Conexão recusada pelo servidor.` | pt | `web.conexao_recusada_pelo_servidor` |
| 119 | `Tempo esgotado: o servidor nao respondeu.` | pt sem-acento | `web.tempo_esgotado_servidor_nao` |
| 120 | `Falha na negociacao TLS (conexao segura).` | pt sem-acento | `web.falha_na_negociacao_tls` |
| 122 | `Bloqueado: redirecionamento de HTTPS para HTTP.` | pt | `web.bloqueado_redirecionamento_https_para` |
| 123 | `Redirecionamento invalido.` | pt sem-acento | `web.redirecionamento_invalido` |
| 124 | `Redirecionamento em ciclo.` | pt | `web.redirecionamento_em_ciclo` |
| 126 | `O carregador de paginas falhou (thread encerrada).` | pt sem-acento | `web.carregador_paginas_falhou_thread` |
| 448 | `Pagina cortada no limite de tamanho` | pt sem-acento | `web.pagina_cortada_no_limite` |
| 449 | `Pagina incompleta (conexao interrompida)` | pt sem-acento | `web.pagina_incompleta_conexao_interrompida` |
| 450 | `Pagina com dados compactados corrompidos (parcial)` | pt sem-acento | `web.pagina_com_dados_compactados` |
| 451 | `Pagina com falha de verificacao (checksum)` | pt sem-acento | `web.pagina_com_falha_verificacao` |
| 928 | `&amp;` | neutro | `web.amp` |
| 929 | `&lt;` | neutro | `web.lt` |
| 930 | `&gt;` | neutro | `web.gt` |
| 931 | `&quot;` | neutro | `web.quot` |

**`osjeff_core/src/browser/body_tests.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 39 | `HTTP/1.1 200 OKr\n{headers}r\n` | neutro | `web.http_1_1_200` |

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
| 95 | `formularios POST nao suportados` | pt sem-acento | `web.formularios_post_nao_suportados` |
| 96 | `formulario grande demais para enviar` | pt sem-acento | `web.formulario_grande_demais_para` |
| 97 | `formulario invalido` | pt sem-acento | `web.formulario_invalido` |
| 169 | `AÁEÉIÍOÓUÚCÇYÝ` | pt | `web.aaeeiioouuccyy` |
| 173 | `AÀEÈIÌOÒUÙ` | pt | `web.aaeeiioouu` |
| 175 | `AÃOÕNÑ` | pt | `web.aaoonn` |
| 178 | `AÂEÊIÎOÔUÛ` | pt | `web.aaeeiioouu_2` |
| 182 | `AÄEËIÏOÖUÜ` | pt | `web.aaeeiioouu_3` |

**`osjeff_core/src/web/imgcache.rs`** (5)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 72 | `formato nao suportado` | pt sem-acento | `web.formato_nao_suportado` |
| 73 | `falha ao carregar` | pt | `web.falha_ao_carregar` |
| 74 | `imagem grande demais` | pt | `web.imagem_grande_demais` |
| 75 | `limite de imagens` | pt | `web.limite_imagens` |
| 241 | `data:#{:016x}-{}` | pt | `web.data_016x` |

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

**`kernel/src/desktop/input.rs`** (8)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 458 | `Favorito adicionado` | neutro | `kit.favorito_adicionado` |
| 459 | `Favorito removido` | neutro | `kit.favorito_removido` |
| 460 | `Nada para guardar aqui` | pt | `kit.nada_para_guardar_aqui` |
| 483 | `Zoom {}%` | neutro | `kit.zoom` |
| 507 | `endereco do formulario invalido` | pt sem-acento | `kit.endereco_formulario_invalido` |
| 567 | `Favorito adicionado` | neutro | `kit.favorito_adicionado_2` |
| 568 | `Favorito removido` | neutro | `kit.favorito_removido_2` |
| 569 | `Nada para guardar aqui` | pt | `kit.nada_para_guardar_aqui_2` |

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
| 146 | `this screen resolution is not supported` | en | `sys.this_screen_resolution_is` |
| 148 | `Detected {}x{} (stride {}, {} bytes/pixel): the screen needs {} bytes, the bootloader provided a framebuffe...` | en | `sys.detected_x_stride_bytes` |
| 196 | `wasm demo done` | en | `sys.wasm_demo_done` |
| 274 | `pci scan + virtio-gpu probe done` | en | `sys.pci_scan_virtio_gpu` |
| 284 | `ata detect done` | en | `sys.ata_detect_done` |
| 322 | `nic init done` | en | `sys.nic_init_done` |
| 327 | `dhcp done` | en | `sys.dhcp_done` |
| 365 | `storage init done` | en | `sys.storage_init_done` |
| 388 | `ui text engine ready` | en | `sys.ui_text_engine_ready` |
| 403 | `Desktop::new (fs load from ATA) done` | en | `sys.desktop_new_fs_load` |
| 411 | `wallpaper painted` | en | `sys.wallpaper_painted` |
| 1101 | `the kernel panicked` | en | `sys.kernel_panicked` |

**`kernel/src/trace.rs`** (2)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 650 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps` |
| 728 | `The quick brown fox jumps over the lazy dog 0123` | en | `sys.quick_brown_fox_jumps_2` |

### Outros

**`osjeff_core/src/base64.rs`** (4)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 29 | `invalid base64 character` | en | `misc.invalid_base64_character` |
| 30 | `invalid base64 length` | en | `misc.invalid_base64_length` |
| 31 | `base64 padding before the end` | en | `misc.base64_padding_before_end` |
| 32 | `base64 data too large` | misto | `misc.base64_data_too_large` |

**`osjeff_core/src/i18n/audit.rs`** (1)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 786 | `{file}:{line}: {w:?} should be {r} in {s:?}` | en | `misc.file_line_w_should` |

**`osjeff_core/src/i18n/template.rs`** (6)

| Linha | Texto | Idioma | Chave proposta |
|---:|---|---|---|
| 52 | `Str({s:?})` | neutro | `misc.str_s` |
| 53 | `Int({n})` | neutro | `misc.int_n` |
| 54 | `Num({n})` | neutro | `misc.num_n` |
| 55 | `Pad({n}, {w})` | neutro | `misc.pad_n_w` |
| 56 | `Dec({n}, {p})` | neutro | `misc.dec_n_p` |
| 57 | `Bytes({n})` | neutro | `misc.bytes_n` |

