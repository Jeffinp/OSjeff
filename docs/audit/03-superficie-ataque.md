# 03 — Superfície de ataque: rede, disco, parsers web e fuzzing

Auditoria do OSjeff (Agente C). Escopo: dado que entra no kernel vindo de fora — frames de
rede (`osjeff_core/src/net.rs`, `kernel/src/ne2000.rs`, `netstack.rs`, `fetch.rs`), imagem de
disco OJFS (`osjeff_core/src/fs.rs`, `kernel/src/ata.rs`, `kernel/src/desktop/files.rs`) e
parsers de conteúdo remoto (`osjeff_core/src/browser.rs`, `web/*`, `kernel/src/wasm`).
Tudo roda em ring 0 com `panic = abort` e o `panic_handler` do kernel faz `hlt` em laço: **um
panic ao processar dado externo equivale a travar a máquina**.

Premissas: README/ARCHITECTURE.md não foram usados; tudo abaixo vem do código e de execução.
Toolchain `nightly-2026-10-05`, host x86_64 Linux. Branch: `worktree-agent-a2f662387f61f8e00`.

## 1. Resumo

Três das nove falhas corrigidas são **CRÍTICAS no sentido do enunciado (travam o kernel e
são acionáveis de fora)**: qualquer servidor HTTP/página que o usuário abra (ou qualquer
atacante no caminho, já que o TLS não verifica certificado — ver achado K1) consegue parar a
máquina. Outras duas (ALTAS) travam o kernel a partir de um disco corrompido ou construído de
propósito. Nenhuma delas corrompe memória (Rust seguro, `forbid(unsafe_code)` em `osjeff_core`):
o dano é negação de serviço (panic/estouro de pilha) ou dado errado, nunca escrita fora dos
limites. O fuzzer **não achou nenhum bug de memória**, e depois das correções os três alvos
rodaram sem crash (tabela na seção 5).

| # | Sev. | Achado | Estado | Commit |
|---|------|--------|--------|--------|
| 1 | CRÍTICA | `dechunk`: tamanho de chunk gigante → panic (corpo HTTP) | corrigido | `a4a11e6` |
| 2 | CRÍTICA | HTML aninhado sem limite → estouro da pilha de 80 KiB | corrigido | `1da81dc` |
| 3 | CRÍTICA | `parse_color("#é1")` → panic (CSS da página) | corrigido | `9d380d3` |
| 4 | ALTA | OJFS: `parent` em ciclo → recursão infinita em `trash_slot`/`purge_slot` | corrigido | `f7a1701` |
| 5 | ALTA | OJFS: `size` > 1024 → leitura além do registro / panic em `fs_load` | corrigido | `a27840b` |
| 6 | MÉDIA | CSS `margin:2147483647` → overflow i32 (coordenadas negativas em release) | corrigido | `a701f71` |
| 7 | MÉDIA | `parse_url`: porta com muitos dígitos estoura u32 (porta errada em release) | corrigido | `894d12a` |
| 8 | BAIXA | `respond` (ARP) panica se `out` < 42 bytes; ARP com hlen/plen estranhos respondido | corrigido | `0ba3fa8` |
| 9 | BAIXA | OJFS: imagem menor que `IMAGE_SIZE` → panic em `is_used` | corrigido | `8261c5e` |
| K1 | ALTA | TLS sem verificação de certificado + RNG xorshift (kernel) | só reportado | — |
| K2 | MÉDIA | DHCP ignorado pela pilha TCP (IP fixo 10.0.2.15) (kernel) | só reportado | — |
| K3 | MÉDIA | Gerenciador de arquivos abre/salva sempre o arquivo da *raiz* (kernel) | só reportado | — |
| K4 | MÉDIA | Falha de leitura (ou disco alheio) → disco é formatado e regravado (kernel) | só reportado | — |
| K5 | MÉDIA | NE2000: frame que cruza o fim do anel é lido errado (kernel) | só reportado (suposição) | — |
| K6 | MÉDIA | HTTP puro sem limite de corpo; heap de 64 MiB (kernel) | só reportado (suposição) | — |
| K7 | BAIXA | NE2000/ATA: laço de poll sem orçamento, `curr - 1`, `send` sem teto, `mib()` (kernel) | só reportado | — |

## 2. Achados corrigidos (`osjeff_core`)

Cada correção é um commit separado com teste de regressão que **falha antes e passa depois**
(trechos de saída na seção "Como provar"). A numeração segue a tabela. Linhas citadas são do
código **antes** da correção (`fc89615`).

### [CRÍTICA] 1. Tamanho de chunk gigante derruba o kernel (`dechunk`)
Onde: `osjeff_core/src/browser.rs:266-303` (`dechunk`, chamado por `page_body`, chamado em
`kernel/src/desktop/mod.rs:565` com a resposta crua do servidor)

O que acontece: o tamanho do chunk é acumulado sem checagem e depois usado para fatiar.
```rust
size = size * 16 + d as usize;          // :276  overflow com 17+ dígitos hex
...
let end = (i + size).min(body.len());   // :294  i + size dá a volta (wrap) em release
out.extend_from_slice(&body[i..end]);   //       -> "slice index starts at 19 but ends at 18"
```
Com `overflow-checks` ligado o `*` já entra em panic; no build `--release` do kernel (checks
desligados) o `*` dá a volta e, com 16 dígitos `F`, `i + size` fica menor que `i` e o fatiamento
panica do mesmo jeito.

Por que importa: o corpo vem do servidor remoto. Basta responder `Transfer-Encoding: chunked`
com `FFFFFFFFFFFFFFFFF` para o navegador do OSjeff parar a máquina (o pedido é HTTP/1.0 mas o
código decodifica chunked independentemente do que foi pedido). Em HTTPS, qualquer um no caminho
consegue o mesmo (K1).

Como provar: **PROVADO**. Teste `browser::tests::dechunk_huge_chunk_size_does_not_panic`: falhava
em debug (`attempt to multiply with overflow`, `browser.rs:276`) e em `--release`
(`slice index starts at 19 but ends at 18`, `browser.rs:295`); passa nos dois depois. O
fuzzer `web_parse` reencontrou sozinho (entrada de **4 bytes**, `fuzz/regressions/web_parse/dechunk-size-overflow`).

Correção proposta (aplicada, `a4a11e6`): `checked_mul/checked_add` (estouro encerra a
decodificação mantendo os chunks já lidos) e `end = i + size.min(body.len() - i)`.
Esforço: baixo

### [CRÍTICA] 2. Aninhamento HTML sem limite estoura a pilha do kernel
Onde: `osjeff_core/src/web/dom.rs:126-155` (`parse_nodes` ⇄ `parse_element`), mais a recursão
de `layout.rs` (`layout_children` ⇄ `layout_block`, `collect_inline`), `style.rs::compute` e o
`Drop` da árvore. Executado em `kernel/src/desktop/mod.rs:566` (`web::render`), na thread do
compositor.

O que acontece: cada elemento aninhado consome pilha nativa e a profundidade é controlada pela
página.
```rust
open.push(tag.clone());
let children = self.parse_nodes(open);   // recursão sem teto
```
Medido num build `--release` (LTO, opt 3) com thread de 80 KiB — o tamanho padrão da pilha do
kernel no `bootloader_api` 0.11 (`config.rs:54`, `kernel_stack_size: 80 * 1024`; o `BOOT_CONFIG` do
kernel não altera): **~550 bytes por nível, sobrevivem no máximo 148 elementos aninhados**
(`<b>` ou `<div>`; 244 com 128 KiB). O que passa disso bate na guard page e o kernel para.

Por que importa: uma página com algumas centenas de `<div>` (ou `<b>` repetido) para a máquina;
o conteúdo é acionável de fora. Um experimento em QEMU de outro agente (scratchpad compartilhado,
`exp_web_nest/serial.log`) mostra profundidade 50 e 100 renderizando e o log serial terminando em
"render nested depth 200" sem o "ok" — compatível com o limite medido aqui (não o reproduzi).

Como provar: **PROVADO no host** (medição acima + teste
`web::dom::html_tests::deeply_nested_markup_does_not_overflow_the_stack`, que roda numa thread de
256 KiB com 20 000 níveis: antes, `thread has overflowed its stack / SIGABRT`; depois, passa). O
fuzzer reencontrou sozinho (ASAN `stack-overflow`, entrada de **7 bytes**,
`fuzz/regressions/web_parse/html-nesting-stack-overflow`).

Correção proposta (aplicada, `1da81dc`): `MAX_DEPTH = 40` no parser; acima disso o elemento
"wrapper" é descartado mas o conteúdo continua no pai (nada visível se perde; ~22 KiB de pilha no
pior caso). Alternativa maior: parser/layout iterativos.
Esforço: baixo (feito)

### [CRÍTICA] 3. `parse_color` panica com hex não-ASCII (CSS da página)
Onde: `osjeff_core/src/web/mod.rs:41-47` (chamado por `style.rs::compute` para `color`,
`background`, vindos de `<style>` e `style="..."`)

O que acontece: checa só o tamanho em **bytes** e fatia em offsets fixos.
```rust
return match hex.len() {            // 3 ou 6 *bytes*
    3 => { let r = u8::from_str_radix(&hex[0..1], 16).ok()?;   // :41
```
`color:#é1` (3 bytes) ou `#ééé` (6) coloca o corte no meio de um caractere UTF-8:
"end byte index 1 is not a char boundary". (Bônus: `from_str_radix` aceita `+`, então `#+f+f+f`
virava uma cor.)

Por que importa: CSS vem do servidor; uma única declaração para o navegador e a máquina.

Como provar: **PROVADO**. Teste `web::tests::non_ascii_hex_color_is_rejected_not_panicking`
(falha antes com o panic acima). Fuzzer `web_parse` achou em segundos (entrada de 7 bytes,
`fuzz/regressions/web_parse/color-non-ascii-hex`).

Correção proposta (aplicada, `9d380d3`): exigir só dígitos hex ASCII antes de fatiar.
Esforço: baixo

### [ALTA] 4. OJFS: ciclo de `parent` causa recursão infinita
Onde: `osjeff_core/src/fs.rs:228-240` (`trash_slot`) e `:258-270` (`purge_slot`); disparado por
`kernel/src/desktop/mod.rs:676-678` (Delete no gerenciador) e por `fs::remove`/`purge`.

O que acontece: a função recursa nos filhos **antes** de mudar o próprio estado.
```rust
if is_dir(img, i) {
    for c in 0..MAX_FILES {
        if is_used(img, c) && parent_at(img, c) == i as u8 { purge_slot(img, c); }
    }
}
img[rec_off(i)] = ST_FREE;      // só depois
```
Um diretório cujo byte `parent` aponta para si mesmo (ou para um descendente) é filho de si
mesmo → recursão sem fim → estouro de pilha → o kernel para. O byte `parent` vem do disco e o
único teste de validade da imagem é o magic `OJF2`.

Por que importa: disco corrompido ou imagem construída de propósito (USB/disco secundário) mais
um único Delete (ou `remove`/`purge` no terminal) numa pasta afetada para a máquina; o mesmo disco
trava de novo toda vez que a pasta for apagada.

Como provar: **PROVADO**. Testes `parent_self_cycle_does_not_recurse_forever` e
`parent_two_cycle_does_not_recurse_forever`: antes `has overflowed its stack ... SIGABRT`; depois
passam. Fuzzer `ojfs_parse` achou sozinho (ASAN stack-overflow em `purge_slot`, 22 bytes,
`fuzz/regressions/ojfs_parse/purge-parent-cycle-stack-overflow`).

Correção proposta (aplicada, `f7a1701`): marcar o slot (lixeira/livre) **antes** de recursar —
cada slot é visitado uma vez, profundidade ≤ `MAX_FILES`. `restore_slot` já fazia isso. Formato
em disco inalterado.
Esforço: baixo

### [ALTA] 5. OJFS: campo `size` > 1024 lê além do registro
Onde: `osjeff_core/src/fs.rs:94-100` (`size_at`) e `:133-140` (`read_slot`:
`&img[o + HEADER..o + HEADER + s]`); consumidor `kernel/src/desktop/files.rs:48-62` (`fs_load`:
`buf[..n].copy_from_slice(data)` com `buf = [0; 1024]`).

O que acontece: o `size` (u16 LE, até 65535) vem do disco sem limite. `read_slot` devolve até
64 KiB: para slots intermediários isso **vaza os bytes dos registros seguintes** (outros
arquivos) para quem lê; para o último slot passa do fim da imagem e panica; em `fs_load`, `n > 1024`
panica em `buf[..n]`. Também aparece na lista de arquivos (tamanho absurdo).

Por que importa: abrir/`cat` de um arquivo de disco corrompido trava a máquina e o vazamento
entre registros quebra o isolamento por arquivo (dentro do próprio disco).

Como provar: **PROVADO**. Teste `corrupted_size_field_is_clamped` (slots 0, 1 e 47, tamanho
0xFFFF). Fuzzer: asserção `size_at <= MAX_FILE_SIZE` falhou em segundos
(`fuzz/regressions/ojfs_parse/size-field-over-max`).

Correção proposta (aplicada, `a27840b`): `size_at` aplica `.min(MAX_FILE_SIZE)`; todos os
consumidores ficam seguros. Formato inalterado.
Esforço: baixo

### [MÉDIA] 6. Comprimentos CSS sem limite estouram a aritmética do layout
Onde: `osjeff_core/src/web/style.rs:92` (`parse_px`) → `layout.rs:128-130` (`y += c.margin`,
`width - 2 * c.padding`, `x + c.padding`); rasterização em `kernel/src/desktop/apps.rs:244-275`.

O que acontece: `parse_px` devolve qualquer `i32`. `margin:2147483647` faz `y += margin`
estourar: panic com `overflow-checks`; no `--release` do kernel dá a volta e produz
coordenadas **negativas**, que `paint_web_page` converte com `as usize`.
Por que importa: no kernel (release) não há panic: `fill_rect` e `draw_char` fazem bounds-check
contra a largura/altura do framebuffer, então o efeito é desenho lixo/aliasing, não escrita fora
dos limites (verificado lendo `fb.rs:148-160`). Por isso MÉDIA e não CRÍTICA; em build com
checks vira panic.

Como provar: **PROVADO** no host. `huge_css_lengths_do_not_overflow_layout` falhava em debug
(`attempt to add with overflow`, `layout.rs:128`) e em `--release` (`assertion failed: *x >= 0 &&
*y >= 0`). `fuzz/regressions/web_parse/css-length-overflow` é a entrada mínima (fabricada à mão: o
fuzzer não chegou aqui sozinho, ver seção 5).

Correção proposta (aplicada, `a701f71`): limitar todo comprimento a 4096 px em `parse_px`.
Esforço: baixo

### [MÉDIA] 7. Porta da URL: acumulador u32 estoura
Onde: `osjeff_core/src/browser.rs:93` (`p = p * 10 + digit`), usado por `parse_url` para a URL
digitada **e** para o `Location` de redirects (`kernel/src/fetch.rs:121`).

O que acontece: `https://host:4294967376/` dá a volta no u32 e vira porta 80 (checks desligados)
em vez de cair no padrão 443; com `overflow-checks` é panic. O servidor controla o `Location`.
Por que importa: no kernel conecta na porta errada silenciosamente (um redirect consegue apontar
a conexão para uma porta que o usuário não escolheu); em debug trava.

Como provar: **PROVADO**. `parse_url_oversized_port_falls_back_to_default` falha em debug (panic) e
em `--release` (`left: 80, right: 443`). O fuzzer achou (entrada de 29 bytes);
`fuzz/regressions/web_parse/url-port-overflow` é a versão mínima feita à mão (24 bytes).

Correção proposta (aplicada, `894d12a`): `saturating_mul/saturating_add`.
Esforço: baixo

### [BAIXA] 8. `respond` panica com buffer de saída pequeno; ARP com hlen/plen estranhos
Onde: `osjeff_core/src/net.rs:79-92` (`build_arp_reply`), `:164-183` (ramo ARP de `respond`)

O que acontece: `respond` promete `None` quando não há resposta, mas o ramo ARP escrevia 42 bytes
em `out` sem checar (`range end index 6 out of range for slice of length 0`); o ramo ICMP já
checava `total > out.len()`. Além disso o ARP não validava `htype/ptype/hlen/plen`: com tamanhos
diferentes de 6/4 os campos eram lidos em offsets errados e a resposta saía com endereços lixo.
Por que importa: o kernel passa um buffer de 1600 bytes (`main.rs:626`), então hoje não dispara;
é contrato quebrado de uma API pública, e o ARP malformado de qualquer vizinho da LAN gerava
resposta com MAC/IP errados.

Como provar: **PROVADO** (testes `arp_reply_with_small_out_buffer_returns_none`,
`arp_with_unexpected_hw_or_proto_sizes_ignored`; fuzzer: entrada de **1 byte**,
`fuzz/regressions/net_parse/arp-small-out-buffer`).
Correção proposta (aplicada, `0ba3fa8`): `build_arp_reply` devolve `Option` e checa o tamanho;
só responde a ARP Ethernet/IPv4.
Esforço: baixo

### [BAIXA] 9. OJFS: imagem menor que `IMAGE_SIZE` panica
Onde: `osjeff_core/src/fs.rs:79,209,214` (`is_used`/`is_active`/`is_trashed`)

O que acontece: toda a API de leitura indexa `img[rec_off(i)]` direto; um buffer curto (leitura
parcial/falha de disco entregue ao FS) dá `index out of bounds: the len is 0 but the index is 4`.
Por que importa: o kernel hoje usa um buffer estático de tamanho certo (`desktop/mod.rs:47`), então
é robustez de API. O fuzzer achou em segundos (7 bytes).
Como provar: **PROVADO** (`short_image_never_panics`, falha antes).
Correção proposta (aplicada, `8261c5e`): os três "portões" exigem `img.len() >= IMAGE_SIZE`.
Esforço: baixo

## 3. Achados no `kernel/` (somente reportados — exigem verificação em QEMU)

### [ALTA] K1. TLS sem verificação de certificado e RNG não criptográfico
Onde: `kernel/src/netstack.rs:223-247` (`UnsecureProvider`), `:385-424` (`Rdtsc`)
O que acontece: o handshake usa `UnsecureProvider` (aceita qualquer certificado) e as chaves
efêmeras vêm de um xorshift semeado pelo `rdtsc`.
```rust
UnsecureProvider::new::<Aes128GcmSha256>(rng)   // sem verificar cadeia/nome
struct Rdtsc { state: u64 }                      // xorshift + rdtsc, "NOT cryptographically secure"
```
Por que importa: qualquer atacante no caminho se passa por qualquer site HTTPS e entrega o HTML/CSS
que dispara os achados 1–3 (e conteúdo falso). O código já avisa que é "demo-grade". Como o
navegador não tem segredos a proteger, o dano direto é integridade do conteúdo e a ponte para os
travamentos, não vazamento.
Como provar: **PROVADO por leitura** (comentário e código no próprio arquivo).
Correção proposta: verificar a cadeia (conjunto mínimo de CAs embutido) ou, no mínimo, rotular a
página como "não autenticada" na UI; trocar o RNG por `rdrand`/entropia de várias fontes.
Esforço: alto

### [MÉDIA] K2. DHCP é ignorado pela pilha TCP
Onde: `kernel/src/netstack.rs:19-21` (`IP`, `GATEWAY`, `DNS_SERVER` constantes) e
`kernel/src/main.rs:230-232` (o resultado do DHCP vai só para `net::respond`)
O que acontece: `dhcp_acquire` devolve o IP leasado, mas `netstack::Net::new` sempre configura
10.0.2.15/24, gateway 10.0.2.2 e DNS 10.0.2.3 (SLIRP do QEMU). Router/DNS do DHCP são
parseados e descartados. Em qualquer rede que não seja o SLIRP padrão o navegador não funciona, e
o ARP/ping respondem com um IP diferente do usado pelo TCP.
Como provar: **PROVADO por leitura**; efeito em rede real é SUPOSIÇÃO.
Correção proposta: passar `DhcpReply` (ip, máscara, router, dns) para `Net::new`.
Esforço: médio

### [MÉDIA] K3. Gerenciador de arquivos abre e salva sempre o arquivo da raiz
Onde: `kernel/src/desktop/mod.rs:696-708` (`files_primary`) → `desktop/files.rs:48-62`
(`fs_load` usa `fs::read(disk(), name)`, que só procura na raiz); `fs_save` usa `fs::write` (raiz).
O que acontece: Enter num arquivo dentro de uma pasta carrega o arquivo da **raiz** de mesmo nome
(ou "file not found" e editor vazio); Ctrl+S grava na raiz, podendo sobrescrever o arquivo errado.
Como provar: **PROVADO por leitura** (`fs::read` = `find_in(img, ROOT, ..)`); não executado no QEMU.
Correção proposta: guardar o slot (ou o `parent`) do arquivo aberto e usar `read_slot`/`write_in`.
Esforço: médio

### [MÉDIA] K4. Disco "não reconhecido" ou leitura com erro é formatado e regravado
Onde: `kernel/src/desktop/mod.rs:202-216`
```rust
if !crate::ata::read_image(disk()) || !fs::is_formatted(disk()) {
    fs::format(disk());  ... let _ = crate::ata::write_image(disk());
}
```
O que acontece: qualquer falha de `read_image` (timeout, `SR_ERR` transitório) **ou** qualquer
disco sem o magic `OJF2` (inclusive um disco com dados de outro sistema no master secundário, ou
uma imagem `OJFS` antiga) é tratado como "em branco": o kernel formata e escreve por cima do LBA 0.
Por que importa: perda silenciosa de dados em uso normal (erro de leitura passageiro apaga o FS
inteiro).
Como provar: **PROVADO por leitura**; não executado.
Correção proposta: só formatar quando a leitura teve sucesso **e** os primeiros setores estão
zerados (ou após confirmação do usuário); falha de leitura deve mantê-lo RAM-only sem gravar.
Esforço: baixo/médio

### [MÉDIA] K5. NE2000: frame que cruza o fim do anel é lido de endereço inexistente
Onde: `kernel/src/ne2000.rs:148-175` (`poll`), `:121-131` (`dma_read`)
```rust
if !(4..=1518 + 4).contains(&total) || !(RX_START..=RX_STOP).contains(&next_page) { ... }
let n = data_len.min(buf.len());
dma_read(((next as u16) << 8) + 4, &mut buf[..n]);   // leitura linear, sem wrap
```
O que acontece: o anel vai de página 0x46 a 0x80. Um frame que começa perto do fim continua em
0x46, mas o DMA remoto lê linearmente e passa de 0x8000 (fora da RAM do chip): bytes lixo.
Com frames de 1514 bytes (6 páginas) ~9% deles cruzam o fim. O checksum TCP do smoltcp descarta o
frame (retransmissão), então o sintoma é perda de pacotes/lentidão, não corrupção. O limite
superior `RX_STOP` é inclusivo (um `next_page == 0x80` é aceito, mas um chip real nunca o envia).
Por que importa: throughput e falhas intermitentes de fetch.
Como provar: **SUPOSIÇÃO** (leitura de código + datasheet do DP8390; não reproduzido em QEMU).
Correção proposta: dividir a leitura em duas quando `addr + n > RX_STOP << 8`, continuando em
`RX_START << 8`; validar `next_page` em `RX_START..RX_STOP` (exclusivo).
Esforço: médio

### [MÉDIA] K6. HTTP puro sem limite de corpo; heap de 64 MiB
Onde: `kernel/src/netstack.rs:171-190` (`http_get`: `out.extend_from_slice(data)` sem teto) —
o caminho HTTPS limita a 256 KiB (`:277`).
O que acontece: um servidor HTTP que não fecha a conexão pode entregar dados por até 10 s
(`deadline(10000)`); `out` cresce por dobra e `fetch_url` ainda duplica com `page_body`. O heap é
estático de 64 MiB (`main.rs:79`); sem memória o alocador aborta. Além disso o DOM/lista de
desenho amplifica o HTML ~70× em memória (um `<a>` de 3 bytes vira dezenas/centenas de bytes), logo
um HTML de 256 KiB já ocupa dezenas de MiB no pior caso.
Como provar: **SUPOSIÇÃO** (não medido no kernel; velocidade do NE2000 por PIO limita o volume).
Correção proposta: mesmo teto de 256 KiB no `http_get` e teto de nós/comandos no `web::render`.
Esforço: baixo

### [BAIXA] K7. Detalhes do NE2000, ATA e do laço de rede
Onde e o quê:
- `kernel/src/main.rs:625` — `while let Some(len) = ne2000::poll(&mut rx)` não tem orçamento: uma
  inundação de frames na LAN mantém o laço do compositor ocupado e congela a UI (DoS por flood).
  Sugestão: processar no máximo N (ex. 16) frames por iteração. SUPOSIÇÃO.
- `kernel/src/ne2000.rs:159` — `curr - 1` com `curr == 0` (valor lido do hardware) dá underflow u8
  (wrap em release; escreve BNRY errado). O hardware é confiável em QEMU; em hardware real/errado
  é só ressincronização ruim.
- `kernel/src/ne2000.rs:182` — `send` não limita `frame.len()`: acima de 1536 bytes (6 páginas TX)
  o DMA invade o anel de recepção. Hoje os chamadores respeitam (smoltcp ≤ 1514; `respond` ≤ 1534);
  falta um `if len > 1536 { return }`.
- `kernel/src/ata.rs:59` — `sectors * 512` com `sectors` vindo do IDENTIFY (até 2^64) estoura u64
  (wrap em release; só mostra capacidade errada).
- `osjeff_core/src/web/layout.rs`/`style.rs` — cascata CSS é O(regras × elementos): 5000 regras ×
  5000 elementos = ~0,39 s no host (medido); no kernel e com mais regras congela a UI por segundos.
  Sugestão: limitar nº de regras.
- `kernel/src/netstack.rs:140-153` — a URL/redirect vai para a linha de requisição e o `Host:` sem
  filtrar controles; o `Location` já vem quebrado por `\n`, então só um `\r` solto passa
  (efeito prático nulo, pois o pedido vai ao mesmo servidor). SUPOSIÇÃO.
Esforço: baixo cada

## 4. Verificado e sem problema (não são achados)

- `net::respond` (IPv4/ICMP): `total_len` é limitado a `payload.len()`, `ihl >= 20` e
  `total_len >= ihl` garantem `payload[ihl..total_len]`; ICMP gigante é truncado a 1500 bytes e
  `build_icmp_reply` checa `total > out.len()`. 38 M execuções fuzz sem crash.
- `net::parse_dhcp`: o laço de opções trata pad (`0`), fim (`255`), `len` 0 e `len` que passa do fim
  (`val + len > dhcp.len()` → `break`); `i` sempre avança (sem laço infinito); opções de IP exigem
  `len >= 4`.
- `net::checksum`: soma em `u32`; só estoura com ≥ 128 KiB de dados, inalcançável (frames ≤ 1600).
- Builders (`arp_announce`, `dhcp_discover`, `dhcp_request`): panicam se `out` for menor que o
  frame (42 / ~300 bytes) por contrato; os chamadores usam 64 e 600 bytes. Não alterei a assinatura
  porque o kernel as chama diretamente.
- OJFS: `name_len` é limitado a `MAX_NAME` em `name_at`; nomes são com tamanho, não terminados em
  NUL, então "sem terminador" não existe; `alloc` valida nome vazio/longo; `write_in` valida o
  tamanho antes de tocar o registro. Profundidade do diretório não é recursiva além de
  `trash/restore/purge` (agora limitadas a 48).
- Parser HTML/CSS: todo laço avança `i` (revisado: `<`, `<>`, `</`, comentário sem fim, `@media`
  aberto, valores sem `;`; e os alvos fuzz rodaram sem timeout/laço infinito); `decode_entity`/`parse_radix` usam `checked_*`; `decode_utf8` só é
  chamado com entrada não vazia.
- `kernel/src/wasm`: os módulos `.wasm` são **embutidos** em tempo de build (`DEMO_WASM`,
  `APP_WASM`, WAD), não há carga de módulo externo; as chamadas do guest usam `guest_bytes`
  (`get(ptr..ptr.saturating_add(len))`) e `Memory::read/write` com checagem; desenho é recortado
  ao retângulo da janela. `host_blit` e `host_fill` usam `i32` sem checagem (um guest malicioso
  gera coordenadas que dão a volta), mas `fill_rect` recorta contra o framebuffer; não vi escrita
  fora dos limites. Fora do escopo aprofundar (guest embutido = confiável hoje).

## 5. Fuzzing

Infraestrutura: `fuzz/` (crate próprio com `[workspace]` vazio, **não** é membro do workspace raiz),
`cargo-fuzz` 0.13 + `libfuzzer-sys` 0.4, sanitizer padrão (AddressSanitizer), toolchain nightly do
repo. Alvos em `fuzz/fuzz_targets/`, dicionários em `fuzz/dict/`, entradas mínimas em
`fuzz/regressions/`.

**Opção para pegar overflow aritmético que em release passaria calado:** `fuzz/Cargo.toml` define
```toml
[profile.release]      # o `cargo fuzz run` compila com --release
debug = 1
debug-assertions = true
overflow-checks = true
```
logo todo `+ - *` que daria a volta no `--release` do kernel vira panic (crash) no fuzzer. Os
testes unitários foram conferidos também com `cargo test --release` (overflow-checks desligados,
como o kernel) — as falhas 1, 6 e 7 aparecem nos dois modos.

Alvos:
- `net_parse`: bytes arbitrários como frame → `respond` com 12 tamanhos de `out` (0…1600),
  `parse_dhcp` (também com o `chaddr` do próprio frame), `checksum`; modo "moldado" (bit alto do
  1º byte) força envelope ARP/ICMP/DHCP válido para passar dos checks de ethertype/protocolo/magic;
  afirma `n <= out.len()` e checksums válidos; builders ARP/DHCP.
- `ojfs_parse`: imagem crua, curta, ou *estruturada* (magic `OJF2` + até 48 registros com
  state/flags/parent/name_len/size fuzzados via `arbitrary`) → todos os acessores em todos os slots
  + sequência de até 64 operações (`write_in`, `mkdir`, `remove`, `trash`, `restore`, `purge`,
  `*_slot`, `empty_trash`, `format`), com asserções dos contratos (nome ≤ 16, tamanho ≤ 1024) e
  percurso de cadeia de `parent`.
- `web_parse`: status/headers/chunked, URL, entidades, CSS, cor, e HTML→layout numa thread de
  512 KiB (para a recursão ilimitada aparecer como estouro de pilha); modos de repetição
  (`mode >> 2` × 8 repetições, gera aninhamento profundo), CSS embutido e resposta chunked.

Resultados na árvore corrigida (HEAD), 1 processo por alvo, máquina compartilhada com outros
agentes (os execs/s variam com a carga; os três rodaram em paralelo):

| Alvo | Tempo | Execuções | execs/s | Cobertura libFuzzer (cov / ft / corpus) | Cobertura de linhas do código-alvo (`cargo fuzz coverage`) | Crashes |
|------|-------|-----------|---------|------------------------------------------|---------------------------------------------------------|---------|
| `net_parse` | 600 s | 38 152 713 | 63 482 | 440 / 768 / 171 | `net.rs` 99,35 % (regiões 99,58 %) | 0 |
| `ojfs_parse` | 600 s | 1 019 087 | 1 695 | 687 / 3 712 / 980 | `fs.rs` 97,25 % (regiões 97,04 %) | 0 |
| `web_parse` | 600 s + 900 s (continuação) | 353 167 + 332 769 | 587 / 369 | 2 398 / 13 041 / 2 978 | `web/layout.rs` 100 %, `web/style.rs` 100 %, `web/dom.rs` 99,2 %, `web/css.rs` 98,6 %, `web/mod.rs` 78,1 %, `browser.rs` 63,6 % | 0 |

(`browser.rs` 63,6 %: o modelo da barra de endereços `Browser::on_key`/`submit` não é exercitado
pelo alvo; li o código à mão — `url_len < URL_CAP` e `caret <= url_len` guardam todos os índices.)

Bugs que o fuzzer **encontrou sozinho** na árvore original (`fc89615`, com os mesmos alvos, em
modo `-fork=1 -ignore_crashes=1` para continuar após cada crash; entrada mínima por `cargo fuzz tmin`):

| Alvo | Bug | Entrada mínima | Tempo até achar |
|------|-----|----------------|-----------------|
| `net_parse` | #8 ARP com `out` pequeno | 1 byte | segundos |
| `ojfs_parse` | #9 imagem curta | 7 bytes | segundos |
| `ojfs_parse` | #5 `size_at > MAX_FILE_SIZE` (contrato) | 27 bytes | segundos |
| `ojfs_parse` | #4 `purge_slot` recursão infinita (ASAN stack-overflow) | 22 bytes | segundos |
| `web_parse` | #1 `dechunk` overflow | 4 bytes | segundos |
| `web_parse` | #3 `parse_color` char boundary | 7 bytes | segundos |
| `web_parse` | #7 porta overflow | 29 bytes | segundos |
| `web_parse` | #2 aninhamento (ASAN stack-overflow) | 7 bytes | < 35 s (rodada com #1/#3/#7 já corrigidos) |

O fuzzer **não** achou sozinho o #6 (margin/padding gigantes): ele fica atrás dos outros crashes
na mesma entrada e nunca gerou um valor próximo de 2147483647; achei por leitura e provei com
teste; `fuzz/regressions/web_parse/css-length-overflow` é fabricada à mão (confirmada: trava na
árvore antiga, passa na nova). Com a pilha do ASAN o #2 só aparece em threads pequenas; sem a
thread de 512 KiB o alvo não o detectaria (8 MiB de pilha principal).

Reprodução (todas as entradas de `fuzz/regressions/` travam a árvore anterior às correções e passam
na atual; conferido com `cargo fuzz run <alvo> -- -runs=1 <arquivo>` nos dois binários):
```
cargo install cargo-fuzz --locked
cargo fuzz run net_parse   -- -max_total_time=600 -max_len=2048  -dict=fuzz/dict/net.dict
cargo fuzz run ojfs_parse  -- -max_total_time=600 -max_len=16384
cargo fuzz run web_parse   -- -max_total_time=600 -max_len=8192  -dict=fuzz/dict/web.dict
cargo fuzz run web_parse   -- -runs=1 fuzz/regressions/web_parse/dechunk-size-overflow
cargo fuzz coverage ojfs_parse    # depois: llvm-cov report <bin> -instr-profile=fuzz/coverage/ojfs_parse/coverage.profdata
```

## 6. Limites desta auditoria

- Nada do `kernel/` foi alterado nem executado em QEMU por mim; K1–K7 são leitura de código
  (indicado em cada um como PROVADO por leitura ou SUPOSIÇÃO). O kernel não foi recompilado
  (nenhuma assinatura pública de `osjeff_core` mudou, só funções internas e comportamento).
- O fuzz de 10 min por alvo é o mínimo pedido: `web_parse` ainda ganhava cobertura no fim (2 273 →
  2 398 edges em 15 min extras), então rodar horas/overnight é recomendável. `ojfs_parse` também
  ainda subia devagar (684 → 687 nos últimos 5 % da rodada).
- Não há fuzz de TCP/TLS/DNS: `smoltcp` e `embedded-tls` são dependências externas, fora do código
  próprio; K1 trata do uso inseguro.
- O formato OJFS em disco não foi alterado por nenhuma correção.
