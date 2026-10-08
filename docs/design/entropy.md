# Entropia e números aleatórios (W21)

Estado: **gerador real no kernel** (`kernel/src/rng.rs`) sobre um módulo puro e testado no host
(`osjeff_core::entropy`). Substitui o `WeakMixer` (um hash de 64 bits de TSC e ticks) que o TLS
usava quando a CPU não tinha `RDRAND`, o caso do QEMU com WHPX no Windows.

## 1. O problema

Tudo o que o sistema precisa de imprevisível vinha de `RDRAND` ou, sem ele, de um misturador de
64 bits alimentado por TSC e ticks do PIT: o *client random* e a chave efêmera do TLS, o
número de sequência inicial do TCP (que era zero: `smoltcp` com `random_seed = 0`), o `xid` do
DHCP, o id e a porta de origem do DNS, o *nonce* do SNTP e a porta local de cada conexão. O QEMU
com WHPX esconde `RDRAND` do guest, então o kernel mostrava o toast `RNG: weak fallback` e a
confidencialidade do HTTPS não era garantida.

## 2. Arquitetura

```
 RDSEED / RDRAND / virtio-rng ──┐  classe "hardware", crédito pleno
 timer, teclado, mouse, NIC,    ├─► Pool (SHA-256, crédito por fonte) ─ drain() ─► Drbg (ChaCha20,
 jitter ativo de CPU ───────────┤   classe "timing", crédito mínimo                 apagamento rápido
 RTC, TSC de boot ──────────────┘   (RTC e boot: mistura, crédito 0)                de chave) ─► fill()
```

| Peça | Onde | O que faz |
|---|---|---|
| `entropy::chacha` | core | ChaCha20 (RFC 8439): bloco e fluxo. ~60 linhas, sem dependência nova |
| `entropy::Drbg` | core | gerador de apagamento rápido de chave (Bernstein): cada `fill` expande a chave, usa os 32 primeiros bytes como **próxima chave** e entrega o resto; reseed = `SHA256(rótulo ‖ chave ‖ semente)`, então uma semente ruim nunca enfraquece o estado; contadores de requisições e de bytes pedem reseed |
| `entropy::Pool` | core | acumulador SHA-256 (fonte e tamanho entram no hash); crédito em **milibits**, limitado a 8 bits por byte; saúde por fonte (eventos, bytes, crédito, rejeitados); `drain()` entrega uma semente de 256 bits e guarda o crédito que sobrou |
| `entropy::TimingEstimator` | core | testes de "preso", variação mínima e repetição sobre a 1ª, 2ª e 3ª diferenças dos timestamps; decide entre "credita 0,5 bit" e "credita nada" |
| `entropy::Entropy` | core | pool + DRBG + política de reseed + `Quality` |
| `rng.rs` | kernel | cola: CPUID, `RDSEED`/`RDRAND`, anel de amostras sem trava, thread de dobra, `fill`, `wait_ready`, adaptador `rand_core` |
| `virtio_rng.rs` | kernel | driver do dispositivo de entropia virtio 1.x (`1af4:1005` / `1af4:1044`), uma fila, um descritor, sem bloqueio |

### 2.1 Fontes e quanto cada uma vale

| Fonte | Quando | Crédito | Por quê |
|---|---|---|---|
| `RDSEED` | CPUID `07H:EBX[18]`; 4 palavras de 64 bits (até 64 tentativas cada), no boot e a cada reseed | 256 bits por 256 bits lidos | sai direto do condicionador de entropia da CPU |
| `RDRAND` | CPUID `01H:ECX[30]`; 8 palavras (até 10 tentativas cada), dobradas por XOR em 32 bytes | 256 bits por 512 bits lidos | `RDRAND` é um DRBG que a CPU realimenta; ler o dobro do que se credita segue a recomendação da Intel |
| virtio-rng | dispositivo presente (`-device virtio-rng-pci`); 32 bytes por pedido, no boot e a cada reseed | 8 bits por byte (zero se o bloco for constante ou tiver palavra sabidamente ruim) | é a entropia do **hospedeiro** (`/dev/urandom` no QEMU): confiar nela é confiar no hipervisor |
| Timer (IRQ0) | TSC a cada tick, 250 Hz | 0,5 bit por amostra aceita | o ruído é o atraso de entrega da interrupção (agendamento do hospedeiro, cache, SMI) |
| Teclado, mouse | TSC a cada byte de IRQ | 0,5 bit por amostra aceita | tempo humano, mais o mesmo ruído de entrega |
| NIC | TSC a cada quadro recebido (a NIC é *polled*, a amostra sai da thread dona dela) | 0,5 bit por amostra aceita | tempo de chegada de pacotes |
| Jitter ativo de CPU | só enquanto uma conexão HTTPS espera (§4): laços curtos com dependência de memória, TSC depois de cada um | 0,1 bit por amostra aceita | o tempo de execução também varia com o hospedeiro, mas é o mesmo ruído que as outras fontes veem, por isso vale menos |
| RTC, TSC de boot, ticks, endereço do estado | uma vez | **0** | valores conhecidos ou adivinháveis; entram no hash só para que dois boots difiram |

Os números são **afirmações**, não medições. O estimador só garante que uma amostra com tempo
perfeitamente regular, preso, alternando ou com variação de poucos ciclos não recebe crédito
(testes `estimator_*`); o valor de 0,5 / 0,1 bit é uma escolha conservadora, não derivada de um
modelo de ruído.

### 2.2 Qualidade (`Quality`)

* **Strong**: uma fonte de hardware (`RDSEED`, `RDRAND` ou virtio-rng) pôs pelo menos 128 bits
  de crédito na chave do DRBG.
* **Mixed**: nenhuma fonte de hardware, mas pelo menos 128 bits de crédito de *timing*. Bom contra
  quem não enxerga os relógios da máquina; **não é promessa** contra quem enxerga. Numa máquina
  virtual totalmente determinística (por exemplo `-icount` com tudo gravado e repetido) a entropia
  real pode ser zero e o estimador, que só vê timestamps, não tem como saber. **Por isso timing
  sozinho nunca chega a Strong, por mais crédito que acumule.** O pedido original dizia "Strong
  se o pool tiver 256 bits"; deliberadamente não foi feito assim.
* **Weak**: menos que isso. O DRBG ainda responde (ids de DHCP e DNS não precisam de segredo),
  mas nada secreto pode depender dele.

## 3. Sem alocação e sem trava nas interrupções

`rng::sample(fonte)` é a única função que uma ISR chama: um `rdtsc`, um `fetch_add` atômico e
um *store* atômico num vetor estático de 256 `u64`. Não aloca, não toma lock (o `lock xadd` é
uma instrução, não um *spin lock*), não loga, não toca o gerador. Cada produtor (ISR do timer,
do teclado, do mouse, a thread da NIC) ganha o seu slot do contador; um produtor preemptado entre
pegar o slot e gravar deixa-o vazio e o consumidor pula slots vazios (`0`); um pico maior que o anel
sobrescreve amostras antigas e conta em `LOST`. O estimador e o SHA-256 rodam só em contexto de
thread (`rng::service`, `rng::fill`), com interrupções desligadas por seções curtas (a mesma
exclusão mútua do `klog` num kernel de um núcleo), então uma thread preemptada nunca deixa o
gerador trancado. Os quatro pontos de chamada têm um comentário dizendo isso:

| Ponto | Contexto | Chamada |
|---|---|---|
| `interrupts::timer_schedule` | ISR IRQ0 | `rng::sample(TIMER)` |
| `interrupts::keyboard` | ISR IRQ1 | `rng::sample(KEYBOARD)` |
| `interrupts::mouse` | ISR IRQ12 | `rng::sample(MOUSE)` |
| `nic::Port::poll` | thread `fetcher`, ao chegar um quadro | `rng::sample(NIC)` |

## 4. Política

* **Strong ou Mixed:** tudo segue como antes. A mudança de classificação é anunciada **uma vez**
  como linha INFO, sem toast: `RNG: strong (...)` ou
  `RNG: pool seeded from timing jitter (N bits credited) at T ms`.
* **Weak:** `rng::fill` continua respondendo, mas **o handshake HTTPS espera** (`rng::wait_ready`,
  até 5 s) antes de abrir a conexão: enquanto espera coleta jitter de CPU, dobra o anel, deixa as
  outras threads rodarem e sai assim que há 128 bits creditados. Se não vierem, o HTTPS é recusado
  (a página mostra a falha genérica de TLS) e o log ganha **um** aviso (o único toast do RNG):
  `RNG: weak, only N of 128 bits credited; HTTPS refused until entropy arrives`, no máximo um a
  cada 30 s. Não existe mais caminho "usa o gerador fraco mesmo assim".
* **Reseed:** assim que a chave tem 128 bits de crédito pendente (a primeira vez), depois a cada
  60 s se houver 128 bits novos, e sempre que o DRBG pedir (65 536 requisições ou 1 MiB), mesmo
  com pouco crédito (misturar nunca piora o estado). Uma fonte de hardware que aparece tarde sobe
  a nota na hora. A cada reseed periódico as fontes de hardware são lidas de novo.
* **Consumidores:** `Net::new` (semente do `smoltcp`: ISN do TCP), `Netd::boot` (semente do
  DHCP), id e porta do DNS, nonce do SNTP, porta local de cada conexão e o `CryptoRng` do TLS
  passam todos por `rng::fill`. O `CryptoRng` só é construído depois de `wait_ready`.

## 5. Provas

* **Core:** 40 testes em `entropy` (vetores da RFC 8439, respostas conhecidas do DRBG e do pool
  calculadas à parte em Python, determinismo, reseed, nenhuma repetição, balanço de bits,
  qui-quadrado e transições em 1 MiB, tetos de crédito, timer regular sem crédito, varredura
  aleatória de invariantes) e mais os de `rng` (CPUID, tentativas, valores sabidamente ruins).
  Alvo de fuzz `entropy_api` (§6): 575 275 execuções em 91 s, sem crash (e `cargo fuzz build` completo).
* **QEMU headless** (BIOS, `tools/qemu-headless.sh`, `tools/qemu-browse.sh` contra um
  `openssl s_server` na máquina hospedeira em `10.0.2.2:4443`):

  | Cenário (QEMU headless, BIOS, SLIRP) | O que a serial mostra |
  |---|---|
  | padrão (`-device virtio-rng-pci`, CPU `qemu64`) | `virtio-rng @ pci 00:03.0 id 1af4:1005`, `RNG: sources rdseed=no rdrand=no virtio-rng=yes`, `RNG: strong (256 bits from hardware, 0 from timing credited) at 112 ms` (4 boots: 112 a 116 ms) |
  | `-cpu max`, com ou sem virtio-rng | `RNG: sources rdseed=yes rdrand=yes ...`, `RNG: strong ... at 112 ms` |
  | `QEMU_RNG=none`, `-cpu qemu64` (o caso do dono sem o dispositivo) | `RNG: sources rdseed=no rdrand=no virtio-rng=no`, `RNG: no hardware generator; collecting timing jitter (128 bits credited needed)`, depois **uma** linha `RNG: pool seeded from timing jitter (N bits credited) at T ms` com N = 128 a 139 e T = 1,1 s (depois que as amostras de antes do gerador existir passaram a contar; antes disso, N = 128, 129, 251 ou 252 e T <= 2,1 s em 6 boots). O limite de 128 bits cai em ~1 s de ticks a 250 Hz; N maior só mostra que ninguém chamou `service` até então. Nenhum `RNG: weak`, nenhum toast |
  | `QEMU_RNG=none` com `-icount shift=0,sleep=off` (timer determinístico) | só 11 bits creditados quando o HTTPS pediu: `RNG: waiting up to 5000 ms for 128 credited bits (11 so far)`, e logo depois `RNG: pool seeded from timing jitter (128 bits credited)`: o jitter de CPU coletado durante a espera completou o crédito. **Aqui o crédito é otimista**: numa VM determinística o "jitter" vem de interrupções que caem em pontos fixos do laço (§2.2, §7) |
  | `-icount shift=0,sleep=off`, `QEMU_RNG=none`, **espera encurtida a 1 ms num build temporário** (`TLS_WAIT_MS = 1`, não commitado) | caminho de recusa: `RNG: waiting up to 1 ms for 128 credited bits (11 so far)`, `RNG: weak, only 11 of 128 bits credited; HTTPS refused until entropy arrives` (este é o único aviso/toast do RNG), a página mostra "Falha na negociacao TLS (conexao segura)" e o `net.pcap` não tem **nenhum** ClientHello: nada foi enviado com o gerador fraco |

  HTTPS contra `openssl s_server -accept 4443` (certificado autoassinado, TLS 1.3) em `10.0.2.2`,
  com um segundo handshake pelo botão "Continuar mesmo assim": o handshake inteiro (ClientHello com
  chave efêmera, ServerHello, `CertificateVerify`) roda com o gerador novo, e a serial mostra
  `tls: certificate check FAILED (autoassinado, ...)` e `tls: UNVERIFIED connection ... handshake 8-32 ms`.
  Os *client randoms* extraídos do `net.pcap` (`python3 -I` sobre o TCP, handshake tipo 1) são
  **todos diferentes**, entre conexões do mesmo boot e entre boots:

  | Cenário | Boot | Conexão | `client random` (16 primeiros hex de 64) |
  |---|---|---|---|
  | sem virtio-rng | 1 | 1 | `4149b2b0ac7069e9` |
  | sem virtio-rng | 2 | 1 | `955bb4d8bba1c2e5` |
  | sem virtio-rng | 2 | 2 | `b11415f8824bd1b7` |
  | sem virtio-rng | 3 | 1 | `6bf92e075a0a83fa` |
  | sem virtio-rng | 3 | 2 | `f442346e5e340d0d` |
  | com virtio-rng | 4 | 1 | `0a699e09e1f50084` |
  | com virtio-rng | 4 | 2 | `48b6a640b2466b21` |
  | `-icount shift=0` | 5 | 1 | `ad76f2043da1cf44` |

  A cópia usada para o servidor está em §8.

## 6. Fuzz

`fuzz/fuzz_targets/entropy_api.rs`: uma sequência de operações (`add` com qualquer id e qualquer
número de bits declarado, timestamps, `fill` de qualquer tamanho, `maybe_reseed`, relógio) sobre um
`Entropy`; depois de cada passo exige: nenhum pânico, crédito por fonte nunca maior que 8 bits por
byte recebido e nunca decrescente, nota nunca decrescente, timing sozinho nunca Strong, crédito
dentro da chave nunca acima de 256 por classe, `fill` preenche tudo e nenhuma saída de 32 bytes
se repete.

## 7. O que continua fraco

* **Timing em VM determinística:** ver §2.2. Sem `RDRAND`, sem virtio-rng e sem ruído do
  hospedeiro a nota "Mixed" é um palpite, não uma garantia. A saída honesta é pôr o
  `-device virtio-rng-pci` (os scripts de `tools/` e o `run.ps1` já o fazem por padrão).
* **Confiança no hospedeiro:** virtio-rng e a disponibilidade de `RDRAND` dentro de uma VM
  são tão confiáveis quanto o hipervisor; ele já vê toda a memória do guest, então isto não amplia
  o que ele pode fazer.
* **`RDRAND` e `RDSEED` são caixas pretas.** Nunca são usados direto: tudo passa pelo pool e
  pelo DRBG, junto com as fontes de timing, de modo que uma delas falha sem derrubar as outras.
  Valores sabidamente ruins (0, todos os bits, `0xFFFF_FFFF` do bug da AMD, bloco constante)
  são rejeitados (o bloco inteiro é descartado e sorteado de novo, até 3 vezes); depois de 3 blocos
  ruins seguidos a instrução deixa de ser consultada (uma falha isolada de `RDSEED`, que existe sob carga, não a desliga).
* **Apagamento de memória** (`wipe`) é o melhor que Rust seguro permite (`fill(0)` +
  `black_box`); não há garantia contra o otimizador nem contra cópias na pilha.
* **Sem persistência de semente** entre boots (o disco do OJFS não guarda estado do RNG).
  Cada boot recomeça do zero, o que torna a janela de "Weak" logo depois do boot a parte
  mais frágil em máquinas sem fonte de hardware: por isso o HTTPS espera.
* Núcleo único: o estado é protegido por interrupções desligadas, não por um lock. Se o
  kernel ganhar SMP isto precisa de um *spin lock* de verdade (e do anel por CPU).

## 8. Como reproduzir

```bash
cargo build --release -p os
# (a) com virtio-rng (padrão) e (b) sem ele, CPU qemu64 (sem RDRAND/RDSEED)
tools/qemu-headless.sh bios /tmp/a 22 -- -cpu qemu64;  grep 'RNG:' /tmp/a/serial.log
QEMU_RNG=none tools/qemu-headless.sh bios /tmp/b 25 -- -cpu qemu64;  grep 'RNG:' /tmp/b/serial.log
# -cpu max liga RDSEED/RDRAND
QEMU_RNG=none tools/qemu-headless.sh bios /tmp/c 22 -- -cpu max;  grep 'RNG:' /tmp/c/serial.log
# client random de cada handshake: servidor TLS na máquina hospedeira + navegador dirigido pelo monitor
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout k.pem -out c.pem \
  -days 2 -subj /CN=10.0.2.2 -addext subjectAltName=IP:10.0.2.2
openssl s_server -accept 4443 -cert c.pem -key k.pem -tls1_3 -quiet &
QEMU_RNG=none BOOT_WAIT=14 CLICK_AFTER=380,275 CLICK_WAIT=25 \
  tools/qemu-browse.sh /tmp/d https://10.0.2.2:4443/ 25 page
python3 -I tools/tls-hello.py /tmp/d/net.pcap      # um "client random" por linha
```
