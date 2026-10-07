# HTTPS verificado e navegador (W16)

Estado: **certificado verificado, hora confirmada por SNTP, gzip/deflate**. Links
clicáveis, histórico, imagens, formulários e rolagem do navegador **não** foram feitos
nesta frente (ver "O que ficou de fora").

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

## 6. Provas (QEMU)

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

- **Navegador**: links clicáveis, voltar/avançar e histórico, favoritos, formulários GET,
  `<img>` (PNG/BMP/PPM), rolagem por teclado/roda/barra além do que já existia. A base
  existe (decodificadores de imagem, `redirect` para URLs relativas, `inflate`), mas o motor
  `web` ainda não emite regiões de link/imagem/campo no `Page`.
- Revogação (CRL/OCSP), *pinning*, HSTS, Certificate Transparency.
- Sem `RDRAND` o RNG do handshake continua sendo o fallback fraco (o servidor é
  autenticado, mas a confidencialidade da sessão não é garantida nessa CPU).
- Ed25519 em certificados de servidor.
- Reuso de conexão/sessão TLS (cada recurso abriria um handshake novo).
