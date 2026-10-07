# Testes e verificação

O OSjeff tem cinco camadas de verificação. Cada uma responde a uma pergunta
diferente, e a lista abaixo diz **o que cada uma não cobre**.

| Camada | Pergunta que responde | Comando | Cobre | Não cobre |
|---|---|---|---|---|
| Testes unitários | A lógica pura está certa? | `cargo test-core` | `osjeff_core` (shell, editor2, calc, janelas, heap, FS v2/v3, blockdev/blockcache, rede, HTML/CSS, browser) | `kernel/` (hardware) |
| Fuzzing | Dado hostil derruba o parser? | `cd fuzz && cargo fuzz run <alvo>` | `net` (+ lease DHCP, DNS, ICMP), `fs` (v2 e v3), `web`, `shell` (com sessão de terminal), `editor2` (com os diálogos), `image`, `x509` (cadeia de certificados) | TCP/TLS/DNS (`smoltcp`, `embedded-tls`), drivers |

| Fuzzing | Dado hostil derruba o parser? | `cd fuzz && cargo fuzz run <alvo>` | `net`, `fs` (v2 e v3), `web`, `shell`, `editor2`, manifesto de app (`app_manifest`), sandbox de arquivos (`app_sandbox`) | TCP/TLS/DNS (`smoltcp`, `embedded-tls`), drivers |
| Fuzzing | Dado hostil derruba o parser? | `cd fuzz && cargo fuzz run <alvo>` | `net` (+ lease DHCP, DNS, ICMP), `fs` (v2 e v3), `web`, `html_img_form` (imagens, formulários, busca, favoritos), `shell`, `editor2`, `image`, `x509` (cadeia de certificados) | TCP/TLS/DNS (`smoltcp`, `embedded-tls`), drivers |
| Boot em QEMU | O kernel sobe e o desktop é o mesmo? | `tools/verify-boot.sh` | BIOS e UEFI, panic/exceção na serial, imagem do desktop | Hardware real, rede real |
| Lint | Há `unsafe` sem justificativa, avisos? | `cargo lint-kernel`, `cargo lint-host` | Todo o código | Corretude |
| Supply chain | Dependência vulnerável ou de licença ruim? | `cargo deny check`, `cargo audit` | `Cargo.lock` | Código das dependências |

Tudo roda no CI (`.github/workflows/ci.yml`), exceto os boots em QEMU e o fuzzing,
que precisam de mais tempo e de OVMF.

## 1. Testes unitários (`osjeff_core`)

```bash
cargo test-core          # alias de `cargo test -p osjeff_core`
```

Por que só o core: um binário `no_std`/`no_main` não tem harness de teste. Por isso
toda decisão que não precisa tocar hardware mora em `osjeff_core`, que compila
com `std` sob teste e `no_std` em produção (`#![forbid(unsafe_code)]`).
Regra do projeto: **lógica nova vai para o core com teste; o kernel só liga o
hardware a ela.**

### Cobertura

```bash
cargo install cargo-llvm-cov
cargo llvm-cov -p osjeff_core --summary-only            # linhas, funções, regiões
cargo llvm-cov -p osjeff_core --branch --summary-only   # + branches (precisa nightly)
```

Leia a tabela com cuidado: a coluna **Cover** à esquerda é de *regiões*; a de
*linhas* é a segunda. Os números de linhas incluem os próprios módulos `#[cfg(test)]`,
então a cobertura de **código de produção** é menor (~88% na auditoria). O CI
falha abaixo de 90% de linhas.

## 2. Fuzzing

Alvos em `fuzz/fuzz_targets/` (crate independente, fora do workspace):

| Alvo | Entrada | Exercita |
|---|---|---|
| `net_parse` | bytes como frame Ethernet; os mesmos bytes como mensagem DNS e como programa da máquina de lease | `osjeff_core::net` (ARP, IPv4, ICMP, UDP, DHCP, `respond`) com buffers de saída de vários tamanhos; `lease` (ticks e respostas em qualquer ordem: configuração só com lease em mãos); `dns` (resposta, cache, `Resolve` terminando dentro do limite); `icmp` (eventos, ping, `next_hop`, checksums do pedido) |
| `ojfs_parse` | bytes como imagem de disco | todas as operações do OJFS v2 (`list/read/write/remove/mkdir/trash/purge`) |
| `ojfs3_parse` | remendos sobre um OJFS v3 válido (com todos os CRC refeitos, para passar do checksum), bytes crus, ou dispositivo de tamanho qualquer | `detect`, `mount`, caminhada (`readdir/stat/read_at/path_of/trash_list`), `fsck`, 16 operações, `fsck` de novo (um FS são continua são) |
| `ojfs3_ops` | sequência de operações, com queda de energia opcional (em ordem ou cache volátil) | escrita lida de volta, `fsck` limpo, remount idêntico, estado exatamente antes/depois da operação cortada |
| `web_parse` | bytes como HTML/CSS/URL/resposta HTTP | parser, CSS, layout, `dechunk`, URL |
| `shell_parse` | bytes como linha/script de shell | lexer/parser, executor (FS em memória com limites, `SysInfo` mock com DNS, página e Ctrl+C), editor de linha com Tab, e uma **sessão de terminal** inteira (`Term`: histórico, rolagem, Ctrl+C, colar) numa janela de tamanho aleatório: o que seria desenhado sempre cabe na janela |
| `editor_ops` | documento + sequência de operações (`arbitrary`) | `editor2`: teclas, mouse, busca/substituição, undo/redo, wrap; invariantes depois de cada operação |
| `editor_dialog` | listagem de pasta arbitrária (nomes com `..`, `/`, controles) + teclas, cliques, roda, tamanhos de janela, erros e a pergunta de substituir | `editor2::dialog`: seleção, rolagem e cursor dentro dos limites, campo até `MAX_FIELD`, e todo caminho que o seletor entrega ao kernel é absoluto e normalizado; `CloseAsk` com teclas quaisquer |
| `image_decode` | bytes como PNG/BMP/PPM/zlib (cru, com CRCs reparados, PNG sintetizado ou BMP com offset ajustado) | `image::decode`, `inflate` (com `max_output` pequeno, também em fluxo), e as operações sobre a imagem decodificada (resize, fit, rotação, composição) mais a ida e volta exata dos codificadores PNG/BMP |
| `html_img_form` | `[modo, zoom, largura, ...bytes]`: os bytes como HTML cru, dentro de `<img src/alt/width>`, de um `<form>` (action, name, value, size, método) ou como payload `data:image/png;base64,` | `web::Doc` com zoom 50-300% e qualquer estado de imagem (invariantes de geometria, índices de links, imagens e campos), `FormState` (teclas, Tab, foco, colagem, query, `target`, teclas mortas), `Page::find`/`select`, `base64`, `decode_for_page`/`decode_data_uri`/`image_key`, `ImageCache` (sequências de operações dentro do teto de bytes) e o `Browser` (barra, histórico, favoritos, sugestões, páginas `osjeff://`); roda numa thread de pilha pequena |
| `x509_parse` | `[modo, bytes]`: o leitor DER/X.509 estrito, o casamento de nomes (SAN/curinga), o parser de datas, a validação de cadeia (`rustls-webpki` com âncora real) com os bytes como cadeia de 1 a 4 certificados, como folha ou intermediária substituindo as de uma cadeia de teste válida (chega à checagem de assinatura), e o `CertificateVerify` do TLS 1.3 com os bytes como assinatura | `osjeff_core::x509`, `osjeff_core::tlsverify` (nunca pânico nem travamento; semente: os certificados de `tools/gen-test-certs.py`) |

| `app_manifest` | bytes como `.wasm` inteiro, como payload de `osjeff.manifest` ou de `osjeff.icon` (embrulhado numa seção válida), ou como manifesto/ícone soltos | `wasmsec` (cabeçalho, seções, LEB128), `appmanifest` (chaves, quotas, ícone PNG até 64x64) e as invariantes do manifesto aceito |
| `app_sandbox` | sequência de operações com caminhos em bytes crus sobre dois apps que dividem um `MemFs` (mais URLs) | `appfs` (normalização, `Sandbox`, cota, descritores) e `appnet`: nada existe fora de `/data/<id>`, o arquivo do sistema fica intacto, a cota vale, URL aceita nunca é local |

```bash
cargo install cargo-fuzz
cd fuzz
cargo fuzz run net_parse -- -max_total_time=600 -print_final_stats=1 -dict=dict/net.dict
cargo fuzz run ojfs_parse -- -max_total_time=600
cargo fuzz run ojfs3_parse -- -max_total_time=600
cargo fuzz run ojfs3_ops   -- -max_total_time=600
cargo fuzz run web_parse  -- -max_total_time=600 -dict=dict/web.dict
cargo fuzz run shell_parse -- -max_total_time=600 -print_final_stats=1 -dict=dict/shell.dict
cargo fuzz run editor_ops  -- -max_total_time=600 -print_final_stats=1
cargo fuzz run editor_dialog -- -max_total_time=600 -print_final_stats=1
cargo fuzz run image_decode -- -max_total_time=600 -print_final_stats=1 -dict=dict/image.dict
cargo fuzz run x509_parse -- -max_total_time=600 -print_final_stats=1

cargo fuzz run app_manifest -- -max_total_time=600 -print_final_stats=1
cargo fuzz run app_sandbox -- -max_total_time=600 -print_final_stats=1
cargo fuzz run html_img_form -- -max_total_time=600 -print_final_stats=1
```

O perfil de release do `fuzz/` liga `overflow-checks` e `debug-assertions`: um
estouro aritmético que no kernel (release, sem checagem) daria a volta em silêncio
aqui vira crash.

**Regressões.** Cada bug achado ficou como arquivo em `fuzz/regressions/<alvo>/`
(entrada mínima) **e** como teste unitário em `osjeff_core`. Para repetir:

```bash
cd fuzz
cargo fuzz run web_parse regressions/web_parse/html-nesting-stack-overflow
```

Resultado da auditoria (10 minutos por alvo, em paralelo, QEMU não envolvido):

| Alvo | Execuções | execs/s | Cobertura do código-alvo |
|---|---|---|---|
| `net_parse` | 38,2 M | 63 mil | `net.rs` 99,35% das linhas |
| `ojfs_parse` | 1,0 M | 1,7 mil | `fs.rs` 97,25% |
| `web_parse` | 0,69 M | 0,4–0,6 mil | layout/style 100%, dom 99%, css 98,6%, `browser.rs` 63,6% |
| `image_decode` | 0,30 M (10 min, 4 núcleos compartilhados) | 0,5 mil | `bmp.rs` 94,9% das linhas, `png.rs` 93,9%, `ppm.rs` 88,6%, `image.rs` 83,6%, `inflate.rs` 82,6%, `deflate.rs` 82,0% (o restante é `Display`, ramos de alocação falha e erros só alcançáveis por teste unitário) |

`web_parse` ainda ganhava cobertura no fim: rode por horas antes de confiar nele.

Alvos do editor v2 e do shell (10 minutos cada, rodando em paralelo, corpus vazio no
início, nenhum crash; cobertura de regiões/linhas por `cargo fuzz coverage` sobre o
corpus final):

| Alvo | Execuções | execs/s | Cobertura libFuzzer (arestas) | Cobertura do código-alvo |
|---|---|---|---|---|
| `shell_parse` | 570 mil | 950 | 6382 | `parse.rs` 96% das linhas, `line.rs` 93%, `glob.rs` 88%, `fs.rs` (MemFs) 87%, `exec.rs` 77%, `builtins.rs` 65% |
| `editor_ops` | 1,62 M | 2690 | 2095 | `editor2` regiões: `view.rs` 97%, `keys.rs` 95%, `buffer.rs` 92%, `mod.rs` 90%, `search.rs` 84%, `undo.rs` 72% |

O primeiro `editor_ops` achou um erro **da harness** (assumia que refazer tudo devolvia o
texto final mesmo com refazeres pendentes de operações `Undo` anteriores), corrigido na
harness: não houve bug na biblioteca. Os trechos que o fuzz não alcança (limite de
memória do desfazer, comandos que dependem de `SysInfo` real) são cobertos pelos testes
unitários.

**OJFS v3** (10 minutos por alvo, rodados em paralelo numa máquina de 4 núcleos, sem
crash em nenhum dos dois; nenhuma regressão a registrar em `fuzz/regressions/ojfs3_*`):

| Alvo | Execuções | execs/s | Cobertura (libFuzzer) | Linhas de `fs3` cobertas pelo corpus (`cargo fuzz coverage`) |
|---|---|---|---|---|
| `ojfs3_parse` | 114.724 | 190 | 3259 arestas, 763 entradas no corpus | `dir.rs` 93,7%, `inode.rs` 98,3%, `layout.rs` 93,2%, `bits.rs` 94,0%, `extent.rs` 83,2%, `mod.rs` 79,9%, `ops.rs` 77,5%, `fsck.rs` 76,4% (`migrate.rs` fora do alvo: coberto pelos testes) |
| `ojfs3_ops` | 196.201 | 326 | 3764 arestas, 1359 entradas | (alvo de sequência e de queda de energia; não medido por linha) |

O `ojfs3_parse` refaz os CRC depois dos remendos (`fix_crc`): sem isso o fuzzer não
passa do checksum e só exercita a rejeição. Como o `fsck` é a prova de consistência,
ambos terminam com `fsck`.

### OJFS v3: queda de energia, desempenho

```bash
cargo test -p osjeff_core fs3::tests::crash         # corta a energia em cada setor/flush
cargo test -p osjeff_core -- --ignored many_seeds   # 60 sementes x 6000 operações vs. modelo
cargo run --release -p osjeff_core --example ojfs3_bench   # MiB/s e setores por operação
```

`cargo test -p osjeff_core` compila o core com `opt-level = 2` (perfil `test` no
`Cargo.toml` raiz): as varreduras de queda de energia rodam em segundos em vez de minutos.
O kernel e os perfis `dev`/`release` não mudam.

### Injetar arquivos num disco v3 (host)

`osjeff_core/examples/fs3_inject.rs` abre (ou formata, se estiver em branco, ou migra, se for
v2) a imagem do disco do sistema de arquivos e copia arquivos para ela, para testar o desktop
com dados grandes sem digitá-los:

```bash
truncate -s 64M fs.img
cargo run --release -p osjeff_core --example fs3_inject -- fs.img foto.png /Imagens/foto.png
cargo run --release -p osjeff_core --example fs3_inject -- fs.img --files 2000 /many   # 2000 arquivos
cargo run --release -p osjeff_core --example fs3_inject -- fs.img --ls /
KEEP_FS=1 tools/qemu-headless.sh bios out 25      # usa o <out>/fs.img existente
```

Cria as pastas que faltam, recusa uma imagem com conteúdo desconhecido (nunca reformata) e
copia em blocos de 1 MiB (arquivos maiores que o limite de 64 MiB de `read_file` também
entram). Teste de ponta a ponta do gerenciador: copiar/mover/renomear/apagar/restaurar um
arquivo de 3 MB, reiniciar com `KEEP_FS=1` e conferir que tudo persistiu e que o log do
`storage` diz `fsck clean`.

Cenários do gerenciador de arquivos e do visualizador (`tools/perf/scen/w15a-*.sh`; precisam de
um disco preparado com o `fs3_inject`: `/big.bin` de 3 MB, `/Imagens/{foto.png,foto.bmp,foto.ppm,
transparente.png,corrompida.png,pequena.png}` e `--files 2000 /many`):

| Cenário | O que faz |
|---|---|
| `w15a-viewer.sh` | abre o Arquivos, entra em `/Imagens`, abre a imagem corrompida (erro na janela), BMP, zoom, 100 %, girar, painel de informações, PNG, PPM, PNG transparente |
| `w15a-files-ops.sh` | copia o arquivo de 3 MB entre pastas (barra de progresso), renomeia (F2), recorta/cola, apaga para a lixeira, restaura, exclusão permanente com confirmação |
| `w15a-many.sh` | abre a pasta de 2000 arquivos e rola (PageUp/PageDown/setas/Home/End); com `perf-trace`, os tempos de quadro saem no serial (medido: quadro estável 14 a 31 ms no TCG) |
| `w15a-soak.sh` | 100x abre/fecha o Arquivos e 100x abre/fecha o Visualizador; `tools/perf/w8-heap.sh <serial.log>` mede a deriva do heap (medido: +1544 B em 200 ciclos) |

Cenários do Editor e do Terminal (W15b, `tools/perf/scen/w15b-*.sh`; disco em branco basta, os
arquivos grandes são criados pelo próprio terminal; `typestr "texto"` em `lib.sh` digita tecla a tecla
no layout US; rodam em ~2 a 4 minutos no TCG, `QEMU_MEM=256M tools/perf/run.sh <img> bios <saida> 300 <cen>`):

| Cenário | O que faz |
|---|---|
| `w15b-probe.sh` | fumaça: o terminal no boot, `echo`, `ls`, Tab (`cd Doc` completa), `pwd`, `free` |
| `w15b-term.sh` | histórico (↑), Tab em caminho, Ctrl+L, `sleep 20` (a janela mostra "executando" e um segundo terminal responde), Ctrl+C, `ping`, `nslookup` (DNS real do QEMU), `ifconfig`, `curl`, `seq 10000` com PageUp e Ctrl+Home, recursão de função, `yes` (1 MiB de saída) sem travar |
| `w15b-edit.sh` | digitar, Ctrl+S (Salvar como: digitar substitui o nome sugerido), Ctrl+F/Ctrl+H e Alt+A, Ctrl+Z, fechar com alterações (Cancelar, depois Descartar), reabrir (`edit`), Ctrl+O, Ctrl+Q, um arquivo de 1 MB com 165 mil linhas (Ctrl+End/Home, PageDown) e outro de 60 mil linhas de 16 caracteres |
| `w15b-ui.sh` | roda do mouse no terminal, Ctrl+Shift+C/Ctrl+V, maximizar (a grade acompanha), clique, duplo clique e arrastar no editor, abrir um texto pelo Arquivos e a pergunta ao fechar com alterações |

Persistência: depois do cenário, `fs3_inject <disco.img> --ls /` lista o que ficou, e um novo
boot com o mesmo disco monta com `fsck clean`. Queda de energia no meio de uma cópia grande
(`kill -9` do QEMU): o próximo boot monta limpo; o arquivo copiado pela metade fica no disco
(menor que o original) e nada mais é afetado.

## 3. Boot em QEMU

Sem tela e sem KVM (TCG), BIOS e UEFI:

```bash
tools/qemu-headless.sh bios /tmp/osj 25      # serial.log + screen.png
QEMU_MEM=256M tools/qemu-headless.sh uefi /tmp/osj-uefi 40
KEEP_FS=1 tools/qemu-headless.sh bios /tmp/osj 25   # reaproveita fs.img (persistência, disco corrompido)
FS_SIZE=64K tools/qemu-headless.sh bios /tmp/osj 25 # disco antigo de 64 KiB (fica no OJFS v2)
```

O disco do filesystem do runner (`<outdir>/fs.img`) é um arquivo esparso de **64 MiB** por
padrão (o OJFS v3 exige >= 1 MiB; `tools/run.sh` e `tools/perf/run.sh` seguem a mesma
regra e aceitam `FS_SIZE`). Em um disco novo em branco o boot formata o v3 e semeia
`leiame.txt`, `notas.txt` e `Documentos/projeto.txt`; a serial mostra as linhas
`storage: ...` (montagem, migração, `TooSmall`, `Unknown`). Com `FS_SIZE=64K` o boot loga
`storage: disk too small for OJFS v3 (64 KiB), staying on v2` e o desktop funciona como antes.

**Migração v2 -> v3 no QEMU:** `cp <fs.img v2 de 64 KiB> disco.img && truncate -s 64M disco.img`
e `KEEP_FS=1` com esse arquivo copiado para `<outdir>/fs.img`: o primeiro boot loga
`storage: migrated OJFS v2 -> v3: ...`, o segundo só `OJFS v3 mounted` (sem remigrar), e
`cmp -n 65536` entre antes e depois prova que os 64 KiB iniciais não mudaram.

`tools/verify-boot.sh <outdir> [baseline]` constrói, sobe os dois modos e falha se
o boot não completar, se aparecer `KERNEL PANIC`/`FATAL` na serial, ou se o desktop
diferir da baseline (o HUD e o relógio são mascarados porque mudam a cada execução).
Para tirar uma baseline, rode o script uma vez num commit bom e passe o `outdir`
como segundo argumento nos seguintes.
Uma baseline tirada com o disco antigo de 64 KiB continua valendo com o disco de 64 MiB:
montar o v3 não muda um pixel do desktop (provado: 0 pixels em BIOS e UEFI).

### Rede em QEMU

`tools/qemu-headless.sh` captura o tráfego da NIC em `<outdir>/net.pcap` e aceita duas
variáveis (documentadas no cabeçalho do script):

```bash
QEMU_NIC=virtio tools/qemu-headless.sh bios /tmp/osj 25     # virtio-net em vez do NE2000 (ne2k, padrão; none = sem NIC)
QEMU_NIC=virtio QEMU_NETDEV="user,id=n0,net=192.168.77.0/24,host=192.168.77.2,dhcpstart=192.168.77.15,dns=192.168.77.3" \
  tools/qemu-headless.sh bios /tmp/osj 25                   # outra sub-rede; o convidado vê o host em 192.168.77.2
python3 -I tools/pcapsum.py /tmp/osj/net.pcap               # resumo: ARP, DHCP (tipo, unicast/broadcast), DNS, TCP, ICMP
```

O que cada prova mostra no resumo do pcap e na serial (todas feitas [M] com o commit que as
introduziu; a serial tem `net:` e `dns:`):

| Prova | Como | O que aparece |
|---|---|---|
| virtio-net, SLIRP padrão | `QEMU_NIC=virtio` | `net: using virtio-net`, `DHCP lease 10.0.2.15/24 ...`; pcap: DISCOVER, OFFER, REQUEST, ACK, ARP gratuito |
| página por virtio-net | `python3 -m http.server 8077 --bind 127.0.0.1` no host + navegador em `http://192.168.77.2:8077/` | `fetch: 279 bytes (status 200)` e a página na tela |
| NE2000 não regride | `tools/verify-boot.sh` (0 pixels diferentes) e a mesma página com `QEMU_NIC` padrão | idem |
| sem NIC | `QEMU_NIC=none` | `net: no network interface found`; o navegador mostra a falha na hora |
| RENEW / REBIND / expiração | gancho **temporário** que força `lease_secs = Some(20)` e, nas variantes, descarta as respostas durante RENEWING (e REBINDING) | pcap: REQUEST unicast `10.0.2.15 -> 10.0.2.2` em T1 (~10 s); com a resposta descartada, REQUEST broadcast em T2 (~17,5 s); descartando também essa, na expiração (20 s) DISCOVER e novo lease |
| ping | gancho **temporário** no boot chamando `netd::ping_us` | `ping gateway (10.0.2.2): reply rtt 525 us = 1 ms`; pcap: ICMP tipo 8 e 0; alvo sem ARP: `host did not answer ARP`; o próprio IP: `invalid target address` |
| failover do DNS | gancho temporário com DNS `10.0.2.250` (morto) e `10.0.2.3`, navegador em `http://example.com/` | `dns: no answer for example.com, trying 10.0.2.3`, `dns: example.com -> ... (2 attempts)`; pcap: ARP sem resposta para `.250`, depois a consulta para `.3` |

Os ganchos são do padrão acima (remova antes de commitar); o lease de 20 s existe só para
a prova (o mínimo de retransmissão do RFC é 60 s, mas é limitado pelo fim da etapa).

**Autoteste de armazenamento** (build `perf-trace` com `OSJ_STORAGE_SELFTEST=1`):

```bash
OSJ_STORAGE_SELFTEST=1 cargo build --release -p os --features perf-trace
```

depois do mount o boot cria `/selftest`, escreve 2 MiB com um padrão, relê e compara,
roda `fsck`, apaga e loga na serial `storage: selftest ...` com os tempos e MiB/s
(em TCG, PIO de verdade). É a única forma de exercitar o v3 no kernel enquanto o desktop
ainda usa o v2. Medido num host compartilhado (varia 30%): escrita de 2 MiB em 0,4 a 0,6 s
(3,4 a 5,3 MiB/s), leitura de 2 MiB de 0,4 a 0,6 s (3,2 a 5,3 MiB/s), `fsck` de um volume
pequeno abaixo de 5 ms, 13 a 23 ms por arquivo pequeno (4 barreiras de flush cada).
O gargalo é o PIO emulado (um setor por trânsito de porta), não o ceder a CPU entre
setores: sem o `yield` deu o mesmo.

### Provando falhas (padrão usado na auditoria)

Para provar que uma falha é reportada, use um gancho **temporário** de build, rode,
e remova antes de commitar:

```rust
match option_env!("OSJ_FAULT") {
    Some("ud2")   => unsafe { core::arch::asm!("ud2") },
    Some("pf")    => unsafe { core::ptr::read_volatile(0xdead_0000usize as *const u8); },
    Some("panic") => panic!("test panic"),
    Some("oom")   => { let v = alloc::vec![0u8; 200 << 20]; core::hint::black_box(&v); }
    _ => {}
}
```

```bash
OSJ_FAULT=ud2 cargo build --release -p os && tools/qemu-headless.sh bios /tmp/f 14
grep -E "FATAL|PANIC" /tmp/f/serial.log
```

Para confirmar que **não** houve triple fault, passe `-- -d cpu_reset -D /tmp/f.qlog`
ao script e procure `Triple fault` no log (duas linhas `CPU Reset` são o reset
normal de ligar a máquina).

## 4. Desempenho

QEMU sem aceleração **não** representa hardware real. Compare proporções.

| Ferramenta | Uso |
|---|---|
| HUD do desktop (canto superior direito) | ms/frame, fps, draws/s, heap, threads |
| `cargo build --release -p os --features perf-trace` | estatísticas por segundo na serial (`[trace]`): custo por etapa de render, ISR, alocações, latência de entrada |
| `tools/perf/run.sh`, `ab.sh`, `cmp.sh` | cenários scriptados (mouse/teclas pelo monitor do QEMU), A/B intercalado, `-icount` para razões estáveis |
| `tools/perf/scen/w8-*.sh`, `w8-heap.sh` | window manager: várias instâncias (`w8-multi`), maximizar/minimizar/Alt+Tab/redimensionar (`w8-wm`), 30+ janelas (`w8-stress`), soak de abrir/fechar 100x com a ocupação exata do heap (`w8-soak` + `w8-heap.sh`, build `perf-trace`) |
| `tools/perf/scen/w13-*.sh` | plataforma de apps: quatro apps ao mesmo tempo (`w13-apps`), app hostil (`w13-hostile`, só com o gancho temporário descrito abaixo), instalar/remover pelo Files (`w13-install`), CPU por app no Gerenciador de tarefas (`w13-cpu`), soak de abrir/fechar apps 100x (`w13-soak` + `w8-heap.sh`, build `perf-trace`) |
| `cd bench && cargo bench` | microbenchmarks no host (criterion), crate fora do workspace |

Os marcos de boot (`[trace] boot + N ms`) saem na serial em qualquer build.

## 5. Lint e supply chain

```bash
cargo lint-kernel      # clippy no alvo bare-metal, -D warnings
cargo lint-host        # clippy do core e do builder, -D warnings
cargo fmt --all -- --check
cargo deny check       # advisories, licenças, bans, fontes (deny.toml)
cargo audit
```

O kernel liga `#![warn(clippy::undocumented_unsafe_blocks)]`: com `-D warnings`,
**todo `unsafe` precisa de um comentário `// SAFETY:`** dizendo a invariante.

## 6. Plataforma de apps WASM (W13)

Projeto em [`docs/design/apps.md`](design/apps.md). O que é testado e como:

| O quê | Como | Resultado |
|---|---|---|
| Leitor de seções, manifesto, ícone, quotas | `cargo test-core` (`wasmsec` 24, `appmanifest` 43 testes) | tabela de chaves válidas/inválidas, duplicadas, acima do teto, `x-`, CRLF, LEB128 hostil, todos os prefixos e todas as trocas de um byte de um pacote sem pânico |
| Sandbox de arquivos | `appfs` (40 testes), incluindo a **tabela de caminhos hostis** (`..`, `../..`, `/../`, `a/../../b`, `//`, `\`, NUL, controle, não ASCII, `%2e%2e`, nomes longos, profundidade), um teste de propriedade (30 000 caminhos gerados: nenhum resolve fora do prefixo) e uma sequência de 20 000 operações hostis sobre dois apps que dividem um `MemFs` | nada existe fora de `/data/<id>`; um app não vê os arquivos do outro; cota e teto de descritores valem |
| Rede, ABI, instalador | `appnet` (13), `appabi` (7), `appinstall` (18) | filtro de destinos (loopback, privados, IPv6, IPs disfarçados), `check_range` sem estouro, duplicado/inválido/acima do teto/semeadura sem sobrescrever |
| Fuzz `app_manifest` | `cargo fuzz run app_manifest -- -max_total_time=660` | 57,3 M execuções, 0 falhas |
| Fuzz `app_sandbox` | `cargo fuzz run app_sandbox -- -max_total_time=660` | 2,7 M execuções (operações com caminhos em bytes crus sobre dois apps), 0 falhas |
| Quatro apps ao mesmo tempo | `w13-apps.sh` (BIOS e UEFI) | `docs/img/apps-*.png` |
| App hostil | `w13-hostile.sh` | log abaixo; `docs/img/apps-hostile-*.png` |
| Instalar/remover | `w13-install.sh` | `docs/img/apps-files.png`, `apps-start.png` |
| Heap estável | `w13-soak.sh` + `w8-heap.sh` (100 rodadas de Ola + Pintura) | BIOS: primeira amostra = última = 1 415 248 B (`last-first = 0 B`), pico de 4 797 408 B com os apps abertos; UEFI: primeira amostra = última = 1 415 248 B (`last-first = 0 B`), pico de 5 169 408 B com os apps abertos |
| CPU por app | `w13-cpu.sh` | `docs/img/apps-taskmgr.png` |

**App hostil.** Não está no repositório: é um módulo WAT montado por um gancho **temporário**
em `kernel/build.rs` (`apps/hostile.wasm`, `fs=own`, `max_fds=4`, `mem_mib=2`) e o gancho é
revertido depois (`git checkout kernel/build.rs`). A tecla 1 a 7 dispara um ataque cada.
Serial do `w13-hostile.sh` (um app morre por vez; clock e notes seguem rodando):

```text
apps: `hostile` crashed: falta de combustivel (laco infinito?)
apps: `hostile` crashed: ponteiro invalido
[app hostile] memory.grow: -1
[app hostile] memory.grow: 1
[app hostile] sandbox: path escape refused (attempt 1)
[app hostile] open ../../etc/x: -1
[app hostile] open /data/notes/nota-1.txt: -2
[app hostile] sandbox: path escape refused (attempt 2)
[app hostile] open ../notes/nota-1.txt: -1
[app hostile] fds opened / last code: 4
[app hostile] fds opened / last code: -7
[app hostile] net_http_get without permission: -1
```

Os ataques que matam o app (laço infinito, ponteiro inválido) deixam na janela dele
"O app encerrou: <motivo>"; os demais devolvem um código de erro e o app continua.

## O que ainda não é testado

- O `kernel/` não tem testes automatizados. A migração de lógica pura para o core
  é contínua (ver [`ROADMAP.md`](ROADMAP.md)).
- As funções `osj.*` (`kernel/src/wasm/abi2.rs`), o `AppManager` e a cola do desktop não são
  fuzzados nem têm teste unitário (são `kernel/`): a decisão que elas executam (caminhos,
  cotas, URL, manifesto, ponteiros) está em `osjeff_core` e é testada, e o conjunto é exercitado
  por boot (`w13-*.sh`, app hostil). `net_http_get` valida mas não transporta.
- Nenhum teste em hardware real.
- TLS real: o sandbox de QEMU não tem internet útil; os caminhos de rede são
  testados por unidade e por boot. A resolução DNS pela SLIRP funcionou neste ambiente
  (`example.com`), mas o servidor DHCP/DNS testado é sempre o da SLIRP.
- O CI (`ci.yml`) foi escrito mas ainda não rodou no GitHub; cada comando dele
  foi executado localmente.
