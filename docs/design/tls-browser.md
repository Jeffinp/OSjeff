# HTTPS verificado e navegador (W16, W17)

Estado: **certificado verificado, hora confirmada por SNTP, gzip/deflate, links
clicáveis e histórico** (W16); **imagens PNG/BMP/PPM, formulários GET, favoritos,
sugestões na barra, busca na página, zoom, seleção e cópia de texto, páginas internas
`osjeff://` e roda do mouse** (W17, §8). O que continua de fora está em §7.

## 1. Modelo de confiança

O navegador só mostra **"Conexao segura"** quando as três coisas aconteceram, nesta
conexão:

1. a **cadeia** que o servidor mandou foi montada até uma raiz da trust store embutida,
   com assinaturas, validade, `basicConstraints`/`keyUsage`, EKU `serverAuth`,
   restrições de nome e `pathLen` conferidos;
2. o **nome do site** (o host digitado, sem ponto final) casa com um `subjectAltName`
   da folha (curinga só como rótulo inteiro à esquerda, casa um rótulo);
3. a assinatura do `CertificateVerify` do TLS 1.3 sobre o transcript confere com a chave
   da folha (prova que o servidor tem a chave privada).

Quem faz o quê:

| Parte | Onde | Origem |
|---|---|---|
| Construção/validação de caminho, nomes, datas, restrições | `rustls-webpki 0.103.13` (ISC) | biblioteca revisada, `no_std` + `alloc` |
| RSA PKCS#1 v1.5 e PSS (SHA-256/384/512, chaves de 2048 a 8192 bits), ECDSA P-256/P-384 | `rsa 0.9`, `p256 0.13`, `p384 0.13`, `sha2` (RustCrypto, Rust puro) | idem |
| Tabela de algoritmos, limites, mapeamento de erros, trust store | `osjeff_core::tlsverify` | nosso, com 53 testes sobre certificados reais |
| Leitor DER/X.509 estrito (diagnóstico, limites, casamento de nomes independente) | `osjeff_core::x509` | nosso, sem pânico, fuzzado |
| Adaptador para o `embedded-tls` | `kernel/src/tlsv.rs` | nosso, ~200 linhas |

Por que não a feature `webpki` do `embedded-tls`: ela depende do `ring` (C e assembly,
não compila em `x86_64-unknown-none`), aceita **uma** CA e não suporta intermediárias.
Por que não criptografia própria: preferimos código revisado; escrevemos só a cola.

Importante: o `embedded-tls` trata um provedor **sem verificador** como "pule a
verificação" (`UnsecureProvider` é isso). Nosso `Provider` sempre tem verificador, então
um erro de configuração não desliga a checagem em silêncio.

Não há revogação (CRL, OCSP, grampeamento), nem *pinning*, nem HSTS, nem CT. Ed25519 e
RSA-PKCS#1 em `CertificateVerify` (proibido no TLS 1.3) não são aceitos.

## 2. Trust store

46 raízes públicas do pacote `ca-certificates` (bundle Mozilla): ISRG X1/X2, DigiCert
(Global G2/G3, Assured ID G2/G3, Trusted G4, TLS ECC P384 G5, TLS RSA4096 G5), GlobalSign
(R3, R6, R46, E46, ECC R4/R5), Google Trust Services R1/R3/R4, Amazon 1 a 4, USERTrust
RSA/ECC, COMODO RSA/ECC, Sectigo R46/E46, Microsoft RSA/ECC 2017, GoDaddy G2, Starfield
G2 e Services G2, IdenTrust, Entrust, QuoVadis 2/3 G3, SSL.com (4), Certum, HARICA (2),
T-TeleSec Class 2, Telekom Security RSA 2023. GTS Root R2, Baltimore CyberTrust e DigiCert
Global Root CA **não estão** no bundle atual do Mozilla e por isso não entram.

Arquivos:

- `osjeff_core/data/trust-store.bin`: `"OJTS1\0"`, contagem `u16`, depois `(u16 tamanho, DER)`.
  Cerca de 45 KiB no kernel.
- `osjeff_core/data/trust-store.sha256`: o SHA-256 (do DER) e o nome de cada raiz, na ordem
  do `.bin`. Um teste refaz o hash de todas as raízes e falha se o dado mudar sem o
  manifesto (e se algum root vencer, deixar de ser CA ou de carregar como âncora).
- `tools/trust-store.list`: a lista, por nome de arquivo do bundle.

**Como atualizar:** `tools/gen-trust-store.sh [dir]` (padrão
`/usr/share/ca-certificates/mozilla`, precisa de `openssl` e `python3`). A saída é
reprodutível (mesma entrada, mesmos bytes). Depois: `cargo test -p osjeff_core`, conferir o
diff do manifesto e a versão do `ca-certificates` registrada no cabeçalho dele.

`EXTRA_PEM="a.pem b.pem" tools/gen-trust-store.sh` acrescenta raízes marcadas `(EXTRA)`: é
**só para desenvolvimento** (as provas deste documento rodam atrás de um gateway que
reassina todo HTTPS) e o resultado nunca deve ser commitado.

## 3. Hora confiável

Validade de certificado depende de data, e o RTC pode estar errado. `kernel/src/clock.rs`
lê o RTC (em UTC) **uma vez** no boot, antes de existir outra thread (o par de portas do
CMOS é compartilhado com o relógio da tela) e o avança pelo timer. O `fetcher`:

1. ao subir, confirma a hora por SNTP (`osjeff_core::sntp`): `time.cloudflare.com`,
   `pool.ntp.org`, `time.google.com` (DNS próprio), e por último o **gateway**;
2. antes de uma carga HTTPS, se ainda não confirmou, tenta de novo (no máximo uma vez por
   minuto).

O pacote só vale se: modo 4, versão 3 ou 4, leap diferente de 3, stratum de 1 a 15
(stratum 0 é *kiss-o'-death* e é reportado), o *originate* repete o *transmit* que
enviamos (16 bits aleatórios), timestamps não nulos e em ordem, atraso de 0 a 5 s e data
entre 2024-01-01 e 2100. O deslocamento vale **só para a checagem de certificado**: o RTC
não é gravado e o relógio da tela não muda.

Sem resposta, usa-se o RTC; um RTC fora de 2024..2100 não é usado (erro "hora do sistema
incorreta"). Quando uma checagem de data falha e a hora não foi confirmada, a página diz
"Hora do sistema nao confirmada: confira o relogio".

Em redes que bloqueiam UDP/123 o gateway pode servir hora (`tools/sntpd.py` faz isso nas
provas).

## 4. Limites

| Limite | Valor |
|---|---|
| Certificados na cadeia | 8 |
| Tamanho de um certificado | 16 KiB |
| Handshake completo | 30 s (leitura individual: 12 s) |
| Chaves RSA | 2048 a 8192 bits |
| Corpo HTTP bruto | 256 KiB (inalterado) |
| Corpo descompactado (gzip/deflate) | 1 MiB |
| Hosts liberados com "continuar mesmo assim" | 8, só na memória, perdem-se ao reiniciar |

`Accept-Encoding: gzip, deflate`; `Transfer-Encoding: chunked` e `Content-Encoding` são
tratados por `browser::page_body` (primeiro `dechunk`, depois descompactar). O pedido agora
é HTTP/1.1 com `Connection: close` (gateways respondem 426 a HTTP/1.0). Uma codificação
desconhecida (`br`) ou um fluxo corrompido vira uma página curta explicando o problema.

## 5. Erros na tela

A barra de endereço mostra o estado da **conexão atual**:

| Estado | Barra |
|---|---|
| `https://` com cadeia válida | "Conexao segura" (verde) |
| `https://` aberto com "continuar mesmo assim" | "Certificado invalido" (vermelho) |
| `http://` | "Nao seguro" (vermelho claro) |
| carregando `https://` ou página inicial | nada |

`Security` não tem variante "seguro" sem verificação: `HttpsVerified` só sai de
`Browser::loaded_with(Conn::Verified, ..)`, e o `fetcher` só devolve `Conn::Verified` com o
`Verifier` concluído (cadeia **e** assinatura).

Mensagens (todas começam com "Certificado invalido: "): expirado; ainda nao valido; nome
nao confere com o site; autoassinado, cadeia nao confiavel; cadeia nao confiavel;
assinatura invalida; autoridade invalida na cadeia; restricao da cadeia violada; algoritmo
nao suportado; hora do sistema incorreta; certificado malformado/grande demais; cadeia
longa demais. Outras falhas têm mensagem própria: DNS, conexão recusada, tempo esgotado,
falha TLS, redirecionamento de HTTPS para HTTP bloqueado.

Só o erro de certificado oferece **"Continuar mesmo assim (inseguro)"**. O clique
guarda o host (comparação sem diferenciar maiúsculas) numa lista em memória, recarrega, e o
`fetcher` pula a validação **só para esse host**, em todos os saltos da navegação. A página
fica marcada "Certificado invalido". Nada é gravado em disco.

## 5.1 Navegador

- **Links.** O motor `web` registra cada `<a href>` (até 2000 por página) em `Page::links` e uma
  caixa por palavra de texto do link em `Page::hits`; `Page::link_at(x, y)` devolve o `href`.
  O clique na janela converte para coordenadas da página (com a rolagem) e chama
  `Browser::open_link`, que resolve o `href` contra a URL da página com
  `redirect::resolve_redirect` (relativo, absoluto e `//host`; `javascript:`, `data:` e o
  rebaixamento https para http são recusados).
- **Histórico.** `Browser` guarda até 64 URLs absolutas em memória (nada em disco), com cursor:
  carregar uma página nova apaga o "avançar", recarregar a mesma não duplica, uma falha não
  entra. Alt+← e Alt+→ voltam e avançam na janela do navegador em foco.
- **gzip/deflate** (`osjeff_core::gzip`): cabeçalho completo do gzip, CRC-32 e tamanho
  conferidos, `deflate` com ou sem envoltório zlib, limite de 1 MiB descompactado.

## 6. Provas (QEMU) da W16

Neste ambiente de testes todo HTTPS de saída passa por um gateway que **reassina** os
sites com a CA "sandbox-egress-gateway-production": com a trust store de produção
`https://example.com` falha, corretamente, com "cadeia nao confiavel". As provas
`https://example.com` e `https://www.wikipedia.org` usam por isso uma trust store de
desenvolvimento com essa CA como raiz extra (`EXTRA_PEM`), e provam o caminho de
verificação de uma cadeia de 3 certificados real; **não provam** as raízes públicas
contra as cadeias públicas reais. As raízes são validadas por hash, parse e carga como
âncora nos testes. Os erros foram provados com servidores locais (`openssl s_server`, cadeias
de `tools/gen-tls-proof-pki.py`, o guest alcança o host em `10.0.2.2:porta`).

| Cenário | Serial | Tela |
|---|---|---|
| `example.com` (cadeia de 3, raiz extra) | `tls: chain verified for example.com (3 certs, root ...); handshake 60 ms` | `docs/img/browser-https-example.png` |
| `www.wikipedia.org` | `... handshake 144 ms`, 24 KB | `docs/img/browser-https-wikipedia.png` |
| cadeia local válida | `tls: chain verified for 10.0.2.2 (2 certs, root OSjeff Proof Root)` | |
| expirada | `certificate check FAILED (expirado)` | `docs/img/browser-cert-expired.png` |
| nome errado | `FAILED (nome nao confere)` | |
| autoassinada | `FAILED (autoassinado, cadeia nao confiavel)` | |
| raiz fora da store | `FAILED (cadeia nao confiavel)` | |
| continuar mesmo assim | `tls: UNVERIFIED connection ... user override`; badge vermelho | `docs/img/browser-cert-override.png` |
| RTC em 2025, sem servidor de hora | `FAILED (ainda nao valido)` e "Hora do sistema nao confirmada" | `docs/img/browser-cert-clock.png` |
| RTC em 2020, SNTP no gateway | `sntp: offset 200419735139 ms ... time now 2026-10-07 ... (confirmed)`, cadeia verificada | |
| RTC em 2020, sem SNTP | `FAILED (hora do sistema incorreta)` | |
| página local com links, `Content-Encoding: gzip` e `chunked` (servidor Python) | `fetch: GET .../gz.html`, 308 bytes gzip descompactados; clique no link; Alt+← volta ao índice | `docs/img/browser-links.png`, `browser-gzip.png`, `browser-back.png` |

Custo, medido no QEMU (TCG, ne2k, SLIRP) contra a árvore anterior (`d3b7166`,
`UnsecureProvider`):

| | antes | depois |
|---|---|---|
| ELF do kernel | 2 322 008 B | 2 694 200 B (+372 KB, dos quais ~45 KB são a trust store) |
| `.text` / `.bss` | 1 847 792 / 95 657 920 B | 2 196 482 / 95 658 456 B (BSS +536 B) |
| Fetch HTTPS a um `openssl s_server` local (SYN até o fim) | 79 ms | 74 ms |
| Handshake com verificação (2 a 3 certificados) | n/d | 60 a 144 ms |

O custo de CPU da verificação é pequeno aqui (ECDSA P-256 e RSA com e = 65537); o de
binário é o de `rustls-webpki` + `rsa` + `p256` + `p384`. O desktop ocioso segue
idêntico à baseline (`tools/verify-boot.sh`: 0 pixels, BIOS e UEFI).

## 7. O que ficou de fora

- **Navegador** (depois da W17): formulários `POST`, `<select>`, `<textarea>`, caixas de
  seleção e botões de rádio; JPEG, GIF, WebP e SVG (a imagem vira uma caixa com o `alt` e
  "formato nao suportado"); JavaScript, cookies, `float`/flexbox; reuso de conexão (cada
  imagem abre uma conexão e, em HTTPS, um handshake); persistência dos favoritos (a
  interface `BookmarkStore` e o ponto único de troca existem, o armazenamento é em
  memória); seleção por caractere (é por palavra); a área de transferência tem 256 bytes
  (uma seleção longa é cortada); acentos só no campo de formulário (a fonte é ASCII: a tela
  mostra a letra sem acento, o valor enviado é UTF-8 correto).
- Revogação (CRL/OCSP), *pinning*, HSTS, Certificate Transparency.
- O RNG do handshake (W21) é um DRBG ChaCha20 sobre um pool de entropia
  ([`entropy.md`](entropy.md)). Sem `RDRAND`/`RDSEED` e sem virtio-rng ele vive de jitter de
  temporização (nota "Mixed"): o handshake espera 128 bits creditados e **recusa** se não vierem;
  numa VM totalmente determinística isso continua sendo uma estimativa, não uma garantia.
- Ed25519 em certificados de servidor.
- Reuso de conexão/sessão TLS (cada recurso abre um handshake novo).

## 8. Navegador na W17: imagens, formulários, UI e roda do mouse

### 8.1 Imagens

`<img src alt width height>` (até 200 por página no layout, **8 baixadas** por página).
O fluxo, sem travar a interface:

1. o layout reserva uma caixa cinza do tamanho declarado (`width`/`height`; só um dos
   dois usa a proporção quando a imagem já é conhecida, senão 4:3; sem atributos, 160x120);
2. o desktop pede as imagens uma por vez à thread `fetcher` (`fetch::try_post_image`):
   mesma pilha, mesmas regras de redirect, corpo de **no máximo 512 KiB**; o `fetcher`
   confere as dimensões no cabeçalho (**no máximo 2 Mpx**, antes de reservar memória),
   decodifica (`osjeff_core::image`), reduz à largura da coluna (`Image::fit`) e achata sobre
   o fundo da página, ali mesmo, fora da thread do compositor;
3. a imagem entra num cache LRU (`web::imgcache::ImageCache`, **6 MiB** de pixels, 24 entradas,
   mantido entre páginas), a página é diagramada de novo (sem reanalisar o HTML: `Doc`
   guarda o DOM) e a rolagem é preservada.

JPEG/GIF/WebP/SVG, falha de rede, status diferente de 200, imagem grande demais e a nona
imagem viram uma caixa com o texto `alt` e o motivo ("formato nao suportado", "falha ao
carregar", "imagem grande demais", "limite de imagens"). `data:image/...;base64,` é decodificada
no próprio navegador (decodificador base64 puro, 64 KiB), sem rede. `<a><img></a>` é
clicável. `src` relativo resolve contra a URL da página; **https para http (conteúdo misto)
é recusado**. Memória: a prova de 100 navegações com 4 imagens novas cada (`perf-trace`)
estabiliza o heap em ~8,4 MiB (6 MiB de cache + o resto do sistema), com picos de ~15 MiB só
durante a decodificação da imagem de 1,26 Mpx.

### 8.2 Formulários GET

`<form method=get action>` com `input` de texto (`text`, `search`, `url`, `email`, `tel`,
`number`, sem tipo), `password` (bolinhas), `hidden`, `submit`/`<button>`. Foco por clique ou
Tab/Shift+Tab, caret, Backspace/Delete/setas/Home/End, Ctrl+V; Enter ou botão envia:
`action?nome=valor&...` com `application/x-www-form-urlencoded` sobre os bytes UTF-8 (a query
do `action` é trocada, o botão só entra se foi o apertado, no máximo 380 bytes). **`method=post`
mostra "formularios POST nao suportados"** e não navega. `textarea`, `select`, caixas de seleção
e rádios não são desenhados. Como o teclado é US, as teclas `'` `` ` `` `~` `^` `"` são
*teclas mortas* nos campos (estilo US-Internacional): `'` e `c` dão `ç`, `~` e `a` dão `ã`; `'`
e espaço dão o apóstrofo; antes de outra letra saem as duas. Prova: digitar `a'c~ao` e `Jos'e`
envia `q=caf%C3%A9+a%C3%A7%C3%A3o&nome=Jos%C3%A9` e o servidor decodifica `café ação`/`José`.

### 8.3 Barra, atalhos e páginas internas

| Item | Comportamento |
|---|---|
| Botões | voltar, avançar (cinza sem histórico), recarregar, início, buscar; estrela de favorito |
| Cursor | mão sobre link, botão de formulário, botão da barra e sugestão |
| Endereço | Ctrl+L ou clique seleciona tudo (digitar substitui); sugestões (favoritos, depois histórico; prefixo antes de substring; até 6; ↑/↓/Enter/clique; Esc fecha) |
| Favoritos | Ctrl+D ou a estrela; `BookmarkStore` (trait) com `MemoryBookmarks` (até 64); o ponto único de troca é `new_bookmark_store()` em `kernel/src/desktop/instance.rs` (não existe `desktop/vfs.rs` na base desta frente, então nada é gravado em `/home/.bookmarks`) |
| Páginas internas | `osjeff://inicio` (a tela inicial), `favoritos` (com "[remover]"), `historico`, `sobre`: HTML gerado e diagramado pelo mesmo motor, sem rede |
| Rolagem | setas, PageUp/PageDown, Home/End e Espaço/Shift+Espaço (com o foco na página; Home/End movem o caret quando o foco é a barra), roda do mouse |
| Busca na página | Ctrl+F, destaca todas as ocorrências (a atual em laranja), Enter/Shift+Enter navega, Esc fecha |
| Zoom | Ctrl+`+`/`-`/`0`: 50, 75, 100, 125, 150, 200, 250, 300% (aritmética inteira; escala da fonte arredondada, medidas proporcionais) |
| Seleção | arrastar o mouse sobre o texto (por palavra), Ctrl+C copia (256 bytes) |
| Título | o `<title>` vai para a barra de título da janela (`NAVEGADOR - ...`) |

### 8.4 Roda do mouse (sistema todo)

O PS/2 negocia o IntelliMouse (taxas 200, 100, 80 e `0xF2`: id 3 liga pacotes de 4 bytes
com eixo Z); qualquer outra resposta mantém o pacote de 3 bytes (`ps2: mouse id 0 (no wheel)`
na serial). `Event::Mouse` ganhou `dz` (positivo = roda para o usuário = rolar para baixo; o
`mouse_move 0 0 <dz>` do monitor do QEMU tem o **sinal invertido**: `-1` rola para baixo). O
desktop entrega a rolagem à **janela sob o ponteiro**, focada ou não, sem mudar o foco:
Navegador rola 3 linhas por passo, Gerenciador de tarefas e Arquivos movem a seleção, Editor
move o cursor 3 linhas; Terminal e Calculadora não têm o que rolar.

### 8.5 Provas (QEMU, BIOS e UEFI)

| Cenário | Evidência |
|---|---|
| página local com PNG pequeno e grande (1400x900, reduzido para 864x555), PNG sem atributos, BMP, JPEG (caixa "formato nao suportado"), `data:`, imagem como link e uma quebrada (404) | serial `img: ... -> 1400x900 (shown 864x555)`, `img: ... failed: Unsupported`; `docs/img/browser-images.png`, `browser-images-errors.png` |
| formulário GET com acentos, campo oculto e um formulário POST | o servidor recebe `q=caf%C3%A9+a%C3%A7%C3%A3o&nome=Jos%C3%A9&origem=osjeff%2F%C3%A7%C3%A3o` e responde `q = [café ação] (12 bytes UTF-8)`; `browser-form.png`, `browser-form-result.png` |
| roda: `mouse_move 0 0 -1` repetido | o navegador rola; sobre uma janela **não focada** rola ela e o foco fica onde estava; Task Manager e Arquivos movem a seleção; com a negociação desligada (gancho temporário) a serial diz `id 0` e o mouse continua movendo e clicando, sem roda |
| favoritos, sugestões, busca na página, zoom, seleção e cópia | `browser-suggest.png`, `browser-find.png` |
| 100 navegações com imagens novas (`perf-trace`) | heap: primeiro 327 KiB, platô de 8,4 MiB, sem deriva (`tools/perf/w8-heap.sh`) |
| desktop ocioso | `tools/verify-boot.sh`: 0 pixels de diferença contra a baseline, BIOS e UEFI |

### 8.6 Fuzz e limites

`fuzz/fuzz_targets/html_img_form.rs` (img com `src`/`alt`/tamanhos hostis, `data:`/base64, formulários
e edição, layout com zoom, busca, seleção, cache de imagens, modelo do navegador com favoritos e
páginas internas): 661 s e 640 s sem falha restante. Achou **um** bug, corrigido com teste e
arquivo em `fuzz/regressions/html_img_form/`: `set_page_title` cortava em 80 bytes dentro de um
caractere de vários bytes (pânico). Nenhum teto anterior mudou (corpo HTML 256 KiB, 8000 nós,
1000 regras, profundidade 40); a URL do navegador subiu de 220 para 480 bytes para caber a query
de um formulário.
