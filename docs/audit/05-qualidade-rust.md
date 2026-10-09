> *Written when the project was called OSjeff (renamed Kitsune in 2026-10; names and paths below are the historical ones).*

# Auditoria 05 — Rust idiomático, qualidade, dependências e cobertura (Agente E)

Branch `audit/performance-security`, HEAD `1c14b3d`. Toolchain `nightly-2026-10-05`
(`rustc 1.101.0-nightly 2026-10-04`). Todas as medições foram feitas num `git worktree`
(`.../scratchpad/wtE`) com `CARGO_TARGET_DIR` próprio; nenhum arquivo de `kernel/`,
`osjeff_core/` ou `os/` do checkout principal foi alterado. Os testes de reprodução
(fuzz/regressão) estão em `.../scratchpad/saved_tests/` (`fuzz_smoke.rs`, `repro.rs`,
`depth.rs`, `color_render.rs`) e rodam como `osjeff_core/tests/*.rs`.

Legenda de prova: **PROVADO** = reproduzi com comando/saída; **SUPOSIÇÃO** = inferido da leitura.

---

## 0. Resumo executivo

| Métrica | README/ARCHITECTURE | Medido |
|---|---|---|
| Testes | 152 | **189** (todos passam) |
| Cobertura de linhas (`cargo llvm-cov`, bruto) | ~98% | **92,33%** (4122 linhas, 316 perdidas) |
| Cobertura de linhas só do código de produção (sem os módulos `#[cfg(test)]`) | — | **87,98%** (2233/2538) |
| Cobertura de funções / regiões | — | 96,95% / 93,52% |
| Cobertura de **branches** (`--branch`) | — | **72,53%** (892 branches, 245 perdidos) |
| Cobertura do **kernel** (6 682 linhas não-comentário) | — | **0%** (`[[bin]] test = false`, nenhum teste) |
| `unsafe` sem `// SAFETY:` (kernel) | — | **96 blocos + 4 `unsafe impl` = 100** (1 bloco documentado em 101) |
| `cargo audit` | — | 0 vulnerabilidades; 4 avisos (2 unmaintained, 1 unsound, 1 yanked) |
| `cargo deny check` | — | advisories: 1 erro (`bincode` unmaintained) + 1 yanked (`spin 0.9.8`); licenses: 3 crates do workspace sem `license`; bans: 3 duplicados; sources: ok |
| `clippy -D warnings` | "falha de propósito só o puro" | `osjeff_core`: passa. `kernel`: **1 erro** (`render.rs:74`). `cargo lint-host`: **também falha** (ver A-09) |
| Edição | — | 2024 nos 3 crates do workspace (os 2 `wasm-apps` Rust usam 2021) |

**Top-5 achados** (detalhes abaixo):

1. **[ALTA]** O navegador derruba o kernel com HTML/CSS remoto: `parse_color("#é1")` entra em pânico (slice fora de fronteira de char) e ~150 `<div>` aninhados estouram a pilha de 80 KiB (parser recursivo sem limite). Ambos PROVADOS.
2. **[ALTA]** HTTPS "fake-secure": `UnsecureProvider` (sem verificação de certificado) + RNG xorshift semeado por `rdtsc` declarado `CryptoRng`; o app trata `https://` como padrão e não mostra aviso.
3. **[MÉDIA]** `fs`: pasta-pai não validada permite ciclo (`mkdir(parent=0)` num FS vazio → `purge_slot` recursa até estourar a pilha) e campo `size` do disco sem teto (`read_slot` panica/over-read). O disco é dado externo.
4. **[MÉDIA]** Cobertura real é bem menor que a divulgada e concentrada no lugar errado: o código de maior risco (kernel, `web/mod.rs` 25%, `web/style.rs` 82%, branches de erro de `net.rs`/`fs.rs`) é justamente o menos testado; o único módulo sem teste algum (`parse_color`) contém o bug nº 1.
5. **[MÉDIA]** 100 `unsafe` sem justificativa + funções *seguras* que entregam `&'static mut` (`disk()`, `scheduler()`, `scratch_slice()`, `app_mut()`) e `io::outb` público seguro: a invariante "single-core + IF" está só em comentário.

---

## 1. Achados (por severidade)

### [ALTA] A-01 `parse_color` panica com hex multibyte vindo de CSS remoto

Onde: `osjeff_core/src/web/mod.rs:41-49` (chamado de `web/style.rs:152,157` para `color`/`background`)
O que acontece:
```rust
if let Some(hex) = s.strip_prefix('#') {
    return match hex.len() {            // len() em BYTES
        3 => { let r = u8::from_str_radix(&hex[0..1], 16).ok()?; ... }   // fatia por byte
```
`"#é1"` tem `hex.len() == 3` (é = 2 bytes), então `&hex[0..1]` corta no meio do caractere → `panic: byte index 1 is not a char boundary`. O renderizador (`web::render`) chama isso com qualquer valor de `color:`/`background:` de uma página (ou de `style=""`).
Por que importa: um servidor (ou MITM — ver A-02) derruba o compositor inteiro; no kernel o `panic_handler` apenas faz `hlt` (tela congelada). É também o único trecho de `web/mod.rs` e é 100% sem teste (cobertura do arquivo: 25,7%).
Como provar (**PROVADO**):
```
cd osjeff_core && cargo test --test color_render -- --nocapture
→ thread panicked at osjeff_core/src/web/mod.rs:41:48
→ render(<p style=color:#é1>) panicked = true
→ render(<style>p{background:#€}</style>) panicked = true
```
(também achado por fuzz: `FUZZ parse_color: 14/20000 panics`).
Correção proposta: operar em bytes (`hex.as_bytes()`, `to_digit(16)`), ou checar `hex.is_ascii()` antes de fatiar; somar testes (`#fff`, `#FFF`, `#12345`, `#é1`, `rgb(300,0,0)`, `rgb(1,2)`).
Esforço: baixo
Severidade: ALTA (derruba o SO a partir de dado remoto).

---

### [ALTA] A-02 HTML aninhado ≥ ~150 níveis estoura a pilha de 80 KiB do kernel

Onde: `osjeff_core/src/web/dom.rs:89-177` (`parse_nodes` ↔ `parse_element`, recursão mútua) e `web/layout.rs:89-166` (`layout_children`/`layout_block`/`collect_inline`); chamado de `kernel/src/desktop/mod.rs:566` (thread do compositor = pilha de boot).
O que acontece: nenhum limite de profundidade (`grep -n "MAX_DEPTH" osjeff_core/src/web` → vazio). `bootloader_api` 0.11 usa `kernel_stack_size = 80 KiB` por padrão e `kernel/src/main.rs:49` não altera (`BootloaderConfig::new_default()`).
Por que importa: um site (ou MITM) com `<div>` × N aninhados derruba o OS; páginas reais passam de 50 níveis facilmente. O `drop` da árvore também é recursivo. O estouro cai em page fault → `halt()` (sem IST/GDT próprio ⇒ pode virar double/triple fault).
Como provar (**PROVADO**, release, thread com 80 KiB — mesma pilha do kernel):
```
cargo test --release --test depth --no-run   # depth.rs em saved_tests
DEPTH=100 <bin> --nocapture  → ok=true
DEPTH=150 <bin> --nocapture  → thread has overflowed its stack (SIGABRT)
# só parse_html (sem layout), N=1000 → também estoura
```
Correção proposta: (a) `MAX_DEPTH = 32..64` em `parse_element` (acima disso, tratar como texto/ignorar nós extras) e em `layout_*`; (b) transformar o parser em iterativo com pilha explícita (`Vec<Element>`); (c) limitar nº de nós/tamanho (hoje só há o teto de 256 KiB em `netstack.rs:277`); (d) rodar `render` numa thread com pilha dedicada de 128 KiB+ e/ou aumentar `kernel_stack_size`. Adicionar teste de profundidade + alvo de fuzz.
Esforço: médio
Severidade: ALTA.

---

### [ALTA] A-03 HTTPS sem verificação de certificado e RNG previsível marcado `CryptoRng`

Onde: `kernel/src/netstack.rs:226-262` (`UnsecureProvider::new::<Aes128GcmSha256>(rng)`), `netstack.rs:385-424` (`struct Rdtsc`, `impl rand_core::CryptoRng`), `kernel/Cargo.toml:23`, `osjeff_core/src/browser.rs:54-62,574` (sem esquema ⇒ `https://`), `browser.rs:177` (busca via `https://www.bing.com`).
O que acontece:
```rust
tls.open(TlsContext::new(&config, UnsecureProvider::new::<Aes128GcmSha256>(rng)))
...
struct Rdtsc { state: u64 }   // xorshift64, semente = rdtsc()|1
impl rand_core::CryptoRng for Rdtsc {}
```
Por que importa (classificação: **segurança real, ALTA**, ainda que o comentário diga "demo-grade"):
1. Sem verificação de cadeia/hostname/validade, qualquer MITM (rede local, DNS spoof — `resolve` usa o DNS do DHCP sem DNSSEC) apresenta certificado qualquer e lê/altera a página; o app mostra `https://` e nenhum aviso (`grep -i "lock\|cadeado\|insegur" kernel/src/desktop` → nada) ⇒ o usuário acredita estar protegido.
2. O segredo efêmero P-256 vem de xorshift64 com semente `rdtsc` (poucas dezenas de bits de entropia efetiva no boot determinístico do QEMU) e o tipo mente ser `CryptoRng` (o compilador deixa de avisar). Um observador **passivo** que adivinhe a semente recupera a chave de sessão: TLS deixa de ser confidencial mesmo sem MITM.
3. A relevância cresce porque o conteúdo recebido vai direto ao parser (A-01/A-02).
Como provar: **PROVADO por leitura** (código acima); a exploração de MITM não foi executada (SUPOSIÇÃO para o cenário de rede, mas o `UnsecureProvider` é, por definição, "sem verificação"). `embedded-tls 0.19` já oferece `CertVerifier` (`src/pki.rs`, features `rustpki` + `ed25519/p384/rsa`; `src/webpki.rs`, feature `webpki`) — conferido em `~/.cargo/registry`.
Correção proposta (não implementada): (1) curto prazo e honesto: mostrar "Conexão não verificada" na barra e **não** exibir cadeado/`https` como seguro; idealmente tornar HTTP/HTTPS não verificado uma escolha explícita; (2) trocar `UnsecureProvider` por `CertVerifier` com `Clock` baseado no RTC (`rtc.rs`) e um pequeno trust store embutido (raízes/pins SPKI dos 2-3 hosts usados); (3) RNG: ChaCha20 (`rand_chacha`, no_std) semeado com `RDRAND`/`RDSEED` (checar CPUID) misturado a jitter de TSC/PIT/eventos de mouse; remover o `impl CryptoRng for Rdtsc`; (4) em redirecionamentos, nunca rebaixar https→http.
Esforço: alto (verificação + trust store + relógio); baixo para aviso de UI e RNG.
Severidade: ALTA.

---

### [MÉDIA] A-04 `fs`: pai não validado ⇒ ciclo/auto-pai ⇒ recursão infinita (stack overflow)

Onde: `osjeff_core/src/fs.rs:149-168 (alloc)`, `:190 (mkdir)`, `:228-270 (trash_slot, purge_slot recursivos)`; uso em `kernel/src/desktop/mod.rs:676-678,738`.
O que acontece: `alloc()` grava `img[o+OFF_PARENT] = parent` sem verificar que `parent` existe, está ativo e é diretório. Se `parent == slot` do próprio item (ou há ciclo A↔B, p.ex. num disco corrompido), `purge_slot`/`trash_slot` recursam para sempre (o estado só é limpo *depois* do laço). `restore_slot` é imune (marca antes de recursar).
Como provar (**PROVADO**):
```
# repro.rs: fs::format; let s = fs::mkdir(&mut img, 0, b"x") /* parent==slot 0 */; fs::purge_slot(&mut img, s);
DO_PURGE=1 cargo test --test repro fs_self_parent -- --nocapture
→ mkdir(parent=0) -> slot 0, parent_at=0 (self-parent)
→ thread has overflowed its stack / fatal runtime error: stack overflow
```
Alcance real no kernel: o `parent` vem de `files_cwd` (`desktop/mod.rs:171,592`); há tentativa de resetar se a pasta não está viva, então pela UI é difícil (SUPOSIÇÃO); mas um **disco ATA corrompido ou forjado** (`ata::read_image` + só `is_formatted` = checar 4 bytes de magia, `desktop/mod.rs:202-203`) cria o ciclo e o kernel estoura a pilha ao apagar/restaurar, ou panica nos acessos (A-05).
Correção proposta: validar em `alloc` (`parent == ROOT || (is_active(parent) && is_dir(parent))`, retornando novo `FsError::BadParent`); fazer `trash_slot`/`purge_slot` marcarem o estado **antes** de recursar (ou iterativos com contador ≤ MAX_FILES); validar a imagem em `is_formatted` (varredura de consistência: estado ∈ {0,1,2}, `parent` válido, sem ciclos, `size ≤ MAX_FILE_SIZE`, `name_len ≤ MAX_NAME`) e reformatar/recusar se inválida. Tipos: `parent: u8` com sentinela `0xFF` → `enum Parent { Root, Dir(SlotId) }`.
Esforço: médio
Severidade: MÉDIA (corrupção/DoS de dado externo; improvável pela UI).

---

### [MÉDIA] A-05 `fs`: campo `size` lido do disco sem teto (`read_slot` panica ou lê além do registro)

Onde: `osjeff_core/src/fs.rs:94-101 (size_at)`, `:133-141 (read_slot)`.
O que acontece: `size_at` devolve `u16` (até 65 535) e `read_slot` faz `&img[o+HEADER .. o+HEADER+s]` sem `min(MAX_FILE_SIZE)`. `name_at` tem o clamp (comentário "prevents out-of-range reads on corrupted length fields"), mas `size` não — inconsistência.
Como provar (**PROVADO**):
```
fs::write(&mut img,b"a",b"hi"); img[4+20]=0xFF; img[4+21]=0xFF;  // size = 0xFFFF
fs::read_slot(&img,0)  → panic (fs.rs:139 slice end index out of range)
size=2048 → Some(2048 bytes)  // vaza bytes do registro seguinte (MAX_FILE_SIZE = 1024)
fuzz de imagem corrompida: "FUZZ fs_corrupt_image: 2/3000 panics"
```
Correção proposta: `let s = size_at(img,i).min(MAX_FILE_SIZE);` em `read_slot` (e em `size_at`), + os testes de imagem corrompida (property test: para qualquer imagem de `IMAGE_SIZE` bytes com magia válida, nenhuma API pública panica). Obs.: `fs::format`, `is_used`, etc. também indexam sem checar `img.len()` (só `is_formatted` checa); documentar/assert a pré-condição ou aceitar `&[u8; IMAGE_SIZE]`.
Esforço: baixo
Severidade: MÉDIA.

---

### [MÉDIA] A-06 Cobertura real ≈ 88% (produção) / 72,5% (branches), kernel 0%, e README inflado

Onde: `README.md:12,42,192-204` ("152 testes, ~98%"; fs·net "~99%"), `ARCHITECTURE.md`.
O que acontece / medido:
```
cargo llvm-cov -p osjeff_core --summary-only
TOTAL  regiões 93,52% | funções 96,95% | linhas 92,33% (4122/316 perdidas) | branches 0 (não instrumentadas por padrão)
cargo llvm-cov -p osjeff_core --branch --summary-only   → branches 72,53% (892, 245 perdidos)
```
Os 92,33% incluem as linhas dos próprios `#[cfg(test)]` (1547 de 1556 executadas ⇒ inflam o número). Só produção (script sobre o lcov, corte na linha do `#[cfg(test)]`): **87,98%**.

| Arquivo | linhas prod. | branches | observação |
|---|---|---|---|
| `browser.rs` | 72,7% | 65,1% | `on_key` (edição de URL: setas/Home/End/Del, 518-578) sem teste; `fold_ascii`/`decode_entity`/`decode_utf8` multibyte (311-412) sem teste |
| `web/mod.rs` | **25,7%** | 25% | `parse_color` inteira sem teste (A-01) |
| `web/style.rs` | 82,2% | 30% | cascata `font-size/weight/align/margin/padding` (145-184) sem teste; **0 testes** no arquivo |
| `web/layout.rs` | 80,9% | 61,9% | marcador de lista (137-144), fundo (152-161), `<br>` (169-175), centralização (181-184) sem teste |
| `web/dom.rs` | 85,2% | 65,6% | atributos sem aspas/escapes (200-209), comentários (288-296), entidades e UTF-8 em texto (328-336) |
| `net.rs` | 94,2% | 62,2% | ramos de erro de `respond` (124,166,186,193,203), `parse_dhcp` com quadro não-UDP/IHL ruim/porta errada/curto (344-356,369), opções DHCP pad/truncada/overrun/DNS (383-402), `msg_type==0` (407) |
| `fs.rs` | 96,6% | 71,8% | `name_at/size_at/parent_at` de slot livre (85,96,112), early-returns de `trash/restore/purge_slot` (230,245,260); nenhum teste de imagem corrompida |
| `heap.rs` | 100% | 100% | mas só a aritmética; o alocador de verdade (`kernel/src/allocator.rs`) é 0% |
| `calc.rs`/`editor.rs`/`clipboard.rs` | 90/90/87% | — | erros de entrada (23-25, 69-85…) |

Por que importa: os números do README descrevem o código mais fácil de testar; o código mais exposto a dado externo (web, DHCP, FS no disco) é onde estão as lacunas, e é onde estão A-01, A-04, A-05. Total estimado do projeto sob teste ≈ 2,2k de ~7–8k linhas executáveis (SUPOSIÇÃO: kernel 6 682 linhas não-comentário, 0 testadas).
Como provar (**PROVADO**): comandos acima; `cargo llvm-cov -p osjeff_core --show-missing-lines` lista as linhas.
Correção proposta: corrigir README/ARCHITECTURE ("189 testes; 88% de linhas de produção, 72% de branches"), adotar `--fail-under-lines 85` no CI, e priorizar testes: `parse_color`, `Browser::on_key` (`browser.rs:505`), cascata CSS, ramos de erro de `parse_dhcp`/`respond`, imagens de FS corrompidas, HTML profundamente aninhado.
Esforço: médio
Severidade: MÉDIA.

---

### [MÉDIA] A-07 `unsafe`: 100 sem justificativa e API segura que esconde invariantes

Onde (contagem por arquivo, `#![warn(clippy::undocumented_unsafe_blocks)]` em worktree; `unsafe_op_in_unsafe_fn` com `deny` → **0** avisos, já em conformidade com a edição 2024):

| Arquivo | sem SAFETY | | Arquivo | sem SAFETY |
|---|---|---|---|---|
| `virtio.rs` | 21 | | `main.rs` | 5 |
| `virtio_gpu.rs` | 16 | | `interrupts.rs` | 4 |
| `allocator.rs` | 10 | | `ne2000.rs` | 4 |
| `sched.rs` | 9 | | `desktop/widgets.rs` | 2 |
| `wasm/mod.rs` | 7 | | `netstack.rs` | 2 |
| `fetch.rs` | 6 | | `ps2.rs` | 2 |
| `io.rs` | 7 | | `desktop/mod.rs`, `fb.rs`, `perf.rs`, `power.rs`, `sync.rs` | 1 cada |

Total **100** (96 blocos + 4 `unsafe impl`: `allocator.rs:22,249`, `sync.rs:25`, `interrupts.rs:72`). Havia 97 blocos `unsafe {`; só `wasm/mod.rs:79` tem `// SAFETY:` (e `:308` é um doc-comment de função).
Como contar (**PROVADO**, reproduzível):
```
grep -rn "unsafe {" kernel/src | wc -l                  # 97
grep -rn "SAFETY" kernel/src                              # 2
cargo clippy -p kernel --target x86_64-unknown-none -- -W clippy::undocumented_unsafe_blocks \
  2>&1 | grep -c "unsafe block missing a safety comment"   # 96 (+4 "unsafe impl")
```
O que realmente preocupa além da falta de comentário (leitura, **PROVADO** nos trechos citados):
- `io.rs:5-58`: `inb/outb/outw/inw/outl/inl` são `pub fn` **seguras** que executam `in/out` em porta arbitrária — qualquer código seguro pode programar hardware. `x86_64::instructions::port::{Port,PortRead,PortWrite}` (já dependência) dá o mesmo com tipos.
- `desktop/mod.rs:50 disk()`, `desktop/widgets.rs:290 scratch_slice()`, `sched.rs:220 scheduler()`, `wasm/mod.rs:309 app_mut()` são `fn` seguras que devolvem `&'static mut` — dois chamadores simultâneos = aliasing de `&mut` (UB), garantido só por convenção.
- `sync.rs:25`: `unsafe impl<T> Sync for RacyCell<T>` sem `T: Send`.
- `sched.rs:107 spawn()`: o contrato "chamar com interrupções desligadas" (`main.rs:241-256`) não é imposto pelo tipo; se esquecido, `Vec::push` concorre com o ISR do timer (`switch_current` lê `threads`).
- `allocator.rs:188-199`: `alloc()` perde o trecho **inicial** entre `region_start` e `alloc_start` quando `align > 8` (só o `excess` final volta à lista; `heap::fit_region` não devolve o gap inicial) ⇒ vazamento permanente de até `align-8` bytes por alocação alinhada (`Box<FxArea>` align 16, `sched.rs:41`).
- `desktop/widgets.rs:270` e `perf.rs:113`: `from_utf8_unchecked` sobre buffers próprios — troque por `from_utf8(..).unwrap_or("")` (custo desprezível, elimina o unsafe).
- `virtio.rs:17-26`: `virt_to_phys` cria `&mut PageTable` aliasando a tabela viva a cada chamada.

Correção proposta (por arquivo crítico):
- `io.rs`: `Port<T>` newtype (`const fn new(u16)`), métodos `unsafe fn read/write` com `// SAFETY: o chamador é dono desta porta (driver X)`; os drivers (`ata`, `ne2000`, `ps2`, `pci`, `interrupts`) constroem suas portas em `static`/`const`. Ou usar `x86_64::instructions::port`.
- `allocator.rs`: `// SAFETY:` em cada `unsafe` (ex.: `SpinGuard::deref`: "o lock está retido, acesso exclusivo"; `add_free_region`: "`addr..addr+size` foi removido da lista/veio de `init`, alinhado a `node_align`, não sobreposto"); corrigir o gap inicial (reinserir `[region_start, alloc_start)` quando `≥ node_size`); extrair o alocador para um crate `osjeff_alloc` host-testável com `miri` (já instalado).
- `sched.rs`: `with_scheduler(|s| ...)` que roda dentro de `interrupts::without_interrupts`; `spawn` faz isso internamente; trocar `scheduler() -> &'static mut` por esse closure. `// SAFETY:` explicando fxsave/fxrstor e o ponteiro de pilha fabricado.
- `interrupts.rs`/`ps2.rs`/`sync.rs`: documentar o argumento "ISR com IF=0 + único consumidor"; `RacyCell<T>: Sync where T: Send`; considerar `spin`-free `AtomicRing` tipado.
- `ne2000.rs`: `NEXT` virar `AtomicU8` (elimina 2 `unsafe`).
Esforço: médio (comentários: baixo; refatoração das APIs: médio)
Severidade: MÉDIA.

---

### [MÉDIA] A-08 `anim_signature` empacota z-order em 3 bits/janela (quebra com a 9ª janela)

Onde: `kernel/src/desktop/render.rs:62-91`, constantes `kernel/src/desktop/mod.rs:60-67`.
O que acontece: `WIN_COUNT = 7`; z-order `(w as u64 & 0x7) << (ZBASE + i*3)`, arrasto `((d.win & 7)+1) << DBASE`. Com ≥ 9 janelas os índices colidem silenciosamente (a assinatura não muda, a camada estática em cache fica velha) e `5*WIN_COUNT+3 ≤ 64` limita a ~11. É dívida que só aparece no crescimento. O comentário do código já admite o limite.
Como provar: **SUPOSIÇÃO** (aritmética do layout de bits; não há como instanciar `Desktop` no host).
Correção proposta: calcular a assinatura com um hash (FNV/ahash simples) sobre `(visible, anim.is_some(), order[i], drag)`, sem layout de bits; `const _: () = assert!(WIN_COUNT <= 8)` enquanto não migrar. Aproveite e troque os `const TERM: usize = 0..` por `#[repr(u8)] enum WinId` (ver A-11).
Esforço: baixo
Severidade: MÉDIA.

---

### [BAIXA] A-09 Baseline: `lint-kernel` e `lint-host` falham (1 erro em `render.rs:74`)

Onde: `kernel/src/desktop/render.rs:74`; aliases em `.cargo/config.toml`.
O que acontece (**PROVADO**):
```
cargo clippy -p kernel --target x86_64-unknown-none -- -D warnings
error: the loop variable `w` is used to index `self.windows`   (needless_range_loop)  --> kernel/src/desktop/render.rs:74:18
cargo clippy -p osjeff_core -p os --all-targets -- -D warnings      # = alias lint-host
error: ... kernel/src/desktop/render.rs:74:18 ... could not compile `kernel`
cargo clippy -p osjeff_core --all-targets -- -D warnings            # passa
```
`lint-host` (README: "clippy host, -D warnings") **também falha**, porque `-p os` compila o kernel como *artifact dependency* sob o mesmo `-D warnings`. `osjeff_core` sozinho passa.
Correção (testada no worktree; ambos os comandos passam depois):
```diff
-        for w in 0..WIN_COUNT {
-            if self.windows[w].visible {
+        for (w, win) in self.windows.iter().enumerate() {
+            if win.visible {
                 s |= 1 << w;
             }
-            if self.windows[w].anim.is_some() {
+            if win.anim.is_some() {
```
(patch completo em `.../scratchpad/render_fix.patch`; `cargo lint-kernel` e `cargo lint-host` → exit 0.) Alterar `lint-host` para `-p osjeff_core --all-targets` evita acoplar os dois.
Esforço: baixo
Severidade: BAIXA (bloqueia CI, não é bug em runtime).

---

### [BAIXA] A-10 `ProcessTable::next_pid` (u16) transborda; pid 0 é sentinela "sem processo"

Onde: `osjeff_core/src/process.rs:92-97` (campo `next_pid`; `pub pid: u16` em `:29`), `kernel/src/desktop/mod.rs:141,349-354,372-376` (`pid == 0` ⇒ "janela fechada").
O que acontece: cada abertura de app faz `spawn` com `next_pid += 1` sem reciclar nem checar. Em debug panica ("attempt to add with overflow"); em **release** (perfil do kernel) dá a volta: depois de 65 535 aberturas o pid volta a **0**, colidindo com o sentinela, e depois a pids já usados.
Como provar (**PROVADO**): `cargo test --test repro pid_wraps` ⇒ debug: panic em `process.rs:97`; `cargo test --release ... pid_wraps` ⇒ `Ok(Some((65534, 0)))` (o spawn nº 65 535 devolveu pid 0).
Correção proposta: `next_pid.checked_add(1).unwrap_or(1)` pulando pids em uso; `Pid(NonZeroU16)` (o `Option<Pid>` substitui o sentinela 0). O mesmo vale para `ticks: u32 += 1` (`process.rs:154`).
Esforço: baixo
Severidade: BAIXA.

---

### [BAIXA] A-11 Tipos fracos: portas, endereços, IDs e retornos `bool`

Exemplos (todos por leitura, **PROVADO**) e tipo proposto:

| Onde | Hoje | Proposta |
|---|---|---|
| `io.rs:6-58`; `ata.rs:12-25` (`const REG_*: u16 = BASE+n`); `ata.rs:132 identify(base: u16, ctrl: u16, slave: bool)` e `:133-147` (`base + 7`, `base + 6`, `base + 2`…) | porta = `u16` solto, offsets mágicos, `bool slave` | `Port<T>`/`AtaChannel{cmd, ctrl}` + `enum Drive{Master,Slave}`; registradores como `enum AtaReg` com `fn port(self, base)` |
| `ne2000.rs:15-39` (`IO + reg`, `PAR0 = 0x01` e `PSTART = 0x01`: mesmo offset em páginas diferentes) | `u16` para porta e para registrador; página implícita | `Reg<Page0>`/`Reg<Page1>` (typestate no `CR`) |
| `pci.rs:25 address(bus,slot,func,offset: u8×4)`, `:34-51` | 4 `u8` posicionais trocáveis; `bar(i: u8)` calcula `0x10 + i*4` (u8, estoura se `i ≥ 60`; `virtio.rs:~210` passa `bar` lido do dispositivo) | `struct PciAddr{bus,dev,func}`; `BarIndex(0..6)` |
| `virtio.rs:17 virt_to_phys(virt: u64, phys_offset: u64) -> Option<u64>`; `virtio_gpu.rs:56-58,69,210 (cmd_phys: u64, phys_offset: u64, attach_backing(phys: u64))`; `virtio.rs:80 Common::new(addr: u64)`; `virtio.rs:187 bar_base -> u64` | endereço físico, virtual e MMIO são todos `u64` | `x86_64::{PhysAddr, VirtAddr}` (já dependência) e um `MmioAddr`; `phys_offset` como `VirtAddr` |
| `ata.rs:74-116,242,266`, `ne2000.rs:76 init() -> bool`, `:181 send(&[u8])` (falha silenciosa), `virtio.rs:171 negotiate -> bool`, `virtio_gpu.rs:199-259 -> bool`, `fetch.rs:48 try_post -> bool`, `process.rs:121,133 set_state/kill -> bool` | `bool` ou `()` como "erro" | `Result<_, AtaError{NoDrive, Timeout, DeviceFault}>` etc.; `ne2000::send -> Result<(), TxError>` (frame > 1514 hoje invade o anel de RX; `send` não checa `len`) |
| `osjeff_core/src/fs.rs:20 ROOT: u8 = 0xFF`, `:108 parent_at -> u8`, `kernel/desktop/mod.rs:171 files_cwd: u8`, `:213 d as u8` | slot como `usize` e `u8` com sentinela | `SlotId(u8)` + `enum Parent{Root, Dir(SlotId)}` |
| `process.rs:29 pid: u16`, `desktop/mod.rs:141,372` | pid `u16` com 0 = nenhum | `Pid(NonZeroU16)` |
| `desktop/mod.rs:60-67` (`const TERM: usize = 0 … WIN_COUNT = 7`), `order: [usize; WIN_COUNT]` | ID de janela = índice cru | `#[repr(u8)] enum WinId` + `WinId::ALL` |

Por que importa: nenhum desses é bug hoje; são a classe de erro (trocar `bus`/`slot`, usar `phys` onde cabia `virt`, ignorar `false`) que o compilador deixaria passar.
Esforço: médio (incremental, driver por driver)
Severidade: BAIXA.

---

### [BAIXA] A-12 `fetch_url` rebaixa/troca o esquema ao seguir redirects relativos; lógica pura presa no kernel

Onde: `kernel/src/fetch.rs:143-161` (`resolve_redirect`), chamada em `:130`.
O que acontece: `Location: /x` ou `x` relativo vira **sempre** `https://<host>/…` (a função só recebe `host`, não o esquema da requisição): um site em `http://` que redireciona para um caminho relativo passa a ser buscado via TLS; e nada impede `https → http` quando o `Location` é absoluto (downgrade silencioso, ver A-03). Também não normaliza `./`, `../`, `?query` nem fragmento.
Como provar: **PROVADO por leitura** (`u.https` é usado para a requisição mas não é passado a `resolve_redirect`); sem teste porque a função está no `kernel` (test = false).
Correção proposta: mover `resolve_redirect` para `osjeff_core::browser` como `resolve(base: &Url, loc: &[u8]) -> Vec<u8>` com testes; recusar downgrade; cap de 5 hops já existe.
Esforço: baixo
Severidade: BAIXA.

---

### [BAIXA] A-13 Pequenos problemas de robustez em parsers/arquivos de rede

- `osjeff_core/src/browser.rs:392`: `decode_utf8(&[])` indexa `bytes[0]` (panica com entrada vazia; hoje o único chamador, `web/dom.rs:332`, garante `i < len` — **PROVADO** por `decode_utf8(&[])` e fuzz `478/20000`). Também não rejeita sequências overlong/surrogates (irrelevante para `fold_ascii`). Devolver `Option`/`(0,0)`.
- `osjeff_core/src/browser.rs:385-388`: doc-comment órfão ("Extract readable, word-wrapped text…") colado em `decode_utf8` (doc rot); `cargo doc` gera 5 avisos (`unresolved link to fail/loaded/take_request`, `<style>` não fechado).
- `osjeff_core/src/net.rs:96-131,262-336`: `arp_announce`, `dhcp_discover/request`, `build_arp_reply` escrevem em `out[..]` sem checar `out.len()` (os parsers `respond`/`parse_dhcp` **não** panicam: 40 000 entradas aleatórias/mutadas, 0 pânicos; os builders panicam se o buffer < 42/≈290 bytes). Chamador atual passa buffers certos.
- `osjeff_core/src/heap.rs:6`: `align_up` faz `addr + align - 1` sem `checked_add` (wrap em release).
- `kernel/src/ne2000.rs:76-95`: `outb(RESET, inb(RESET))` em `0x31F` fixo, sem sondagem; num hardware real escreve em porta de outro dispositivo.
- `kernel/src/virtio.rs:171-183`: `negotiate` lê `device_features(1)` em `_have` mas nunca verifica `VIRTIO_F_VERSION_1` (confia só em `FEATURES_OK`).
- `kernel/src/wasm/wasi.rs:25-27,60-95,196-210`: ponteiros do guest `i32` com `off.max(0)` (negativo vira endereço 0 em vez de erro); laços `for i in 0..n` / `while i < len` com `n/len` controlados pelo guest e sem fuel/limite ⇒ travar a thread `wasmapp` (isolada por preempção, mas o kernel não limita memória/combustível do wasmi). Hoje só roda apps embutidos (build.rs) ⇒ BAIXA.
Esforço: baixo
Severidade: BAIXA.

---

### [BAIXA] A-14 Dependências: `spin 0.9.8` (yanked) está no **kernel**, não só no bootloader

Onde: `Cargo.lock`; `cargo tree -p kernel --target x86_64-unknown-none -i spin` ⇒ `spin 0.9.8 └── wasmi 1.1.0 └── kernel`.
Resultados (**PROVADOS**):

`cargo audit` (1290 advisories, 155 crates): 0 vulnerabilidades, 4 avisos:
| Crate | Aviso | Quem puxa | Impacto |
|---|---|---|---|
| `spin 0.9.8` | **yanked** | `wasmi 1.1.0` (⇒ **linkado no kernel**) | `cargo update -p spin` → 0.9.9 (compatível) |
| `anyhow 1.0.102` | RUSTSEC-2026-0190 unsound (`downcast_mut`, corrigido ≥ 1.0.103) | `bootloader 0.11.17` + wit-tooling | só ferramenta de build (host); `cargo update` → 1.0.104 |
| `bincode 1.3.3` | RUSTSEC-2025-0141 unmaintained | `mbrman` ← `bootloader` | só host; sem correção disponível |
| `proc-macro-error2 2.0.1` | RUSTSEC-2026-0173 unmaintained | `defmt-macros 1.1.0` | `cargo update` remove (`defmt 1.1.1`) |
| (wasm-apps) | `cargo audit -f wasm-apps/{snake,plasma}/Cargo.lock` | 1 crate cada | limpo |

`cargo deny check` (deny.toml temporário via `cargo deny init`, `allow = MIT, Apache-2.0 (+LLVM-exception), BSD-3-Clause, 0BSD, Unicode-3.0, Unlicense`):
- advisories: `error[unmaintained] bincode` e `warning[yanked] spin`.
- licenses: **3 erros `unlicensed`** — `kernel`, `os`, `osjeff_core` não têm `license = "MIT"` no `Cargo.toml` (o repo tem `LICENSE` MIT). `r-efi` aparece como LGPL-2.1-or-later mas é multi-licença (MIT/Apache) ⇒ ok.
- bans: 3 duplicados (`bitflags 1.3.2/2.11.1`, `heapless 0.8.0/0.9.3`, `wit-bindgen`) — `Cargo.lock` tem ainda `thiserror`, `hashbrown`, `wasmparser` ×3, `wasm-encoder` ×2, `defmt` ×2 (todos de ferramentas de build/wasm).
- sources: ok.

`cargo update --dry-run` (43 pacotes) — **seguro** aplicar: `spin 0.9.9`, `anyhow 1.0.104`, `bootloader_api 0.11.15→0.11.17` (hoje o builder `bootloader 0.11.17` e o `bootloader_api` do kernel divergem), `log`, `libc`, `memchr`, etc. (patches/minors, mesma série). **Atrasadas e que exigem decisão** (major/minor com API nova): `smoltcp 0.12→0.14`, `wasmi 1.1→2.0`, `sha2 0.10→0.11`, `rand_core 0.6→0.10` (pinado por `embedded-tls`), `generic-array 0.14.7→.9`.
Quirk de ferramenta: `cargo tree` **entra em pânico** (ICE em `resolver/features.rs:326`, `ArtifactDep(x86_64-unknown-none)`) em qualquer comando sem `-p kernel --target x86_64-unknown-none`, porque o workspace usa `-Z bindeps`; `cargo tree -d` e `-i` só funcionam com esse escopo (usei também `Cargo.lock` + `tomllib`).
Correção proposta: `cargo update -p spin -p anyhow -p bootloader_api`; adicionar `license = "MIT"` + `rust-version`/`publish = false` nos 3 `Cargo.toml`; commitar um `deny.toml` (ignore explícito de RUSTSEC-2025-0141 com justificativa "só host"); `cargo audit`/`deny` no CI.
Esforço: baixo
Severidade: BAIXA.

---

### [BAIXA] A-15 Nightly pinado, `#![feature]` e `-Z bindeps`

- Edição 2024 em `kernel`, `os`, `osjeff_core` (OK); `wasm-apps/{snake,plasma}` em 2021 (workspaces isolados, ok). Sem `rust-version`.
- Pin: `nightly-2026-10-05` (rustc 1.101.0-nightly de 2026-10-04), 1 dia antes da data desta auditoria. O `rust-toolchain.toml` diz "Bump deliberately, together with Cargo.lock" (bom).
- `#![feature(...)]`: **uma** — `abi_x86_interrupt` (`kernel/src/main.rs:3`; usada em `interrupts.rs:131,137,144-156`, 6 handlers). **Ainda necessária** (PROVADO: removendo-a, `E0658: the extern "x86-interrupt" ABI is experimental`). Pode ser eliminada trocando os 6 handlers por trampolins `global_asm!` (como já é feito para o timer em `switch.s`).
- Dependência de nightly **maior** que a feature: `.cargo/config.toml` `[unstable] bindeps = true` (artifact dependency `os → kernel`), que também causa o ICE do `cargo tree` (A-14) e é o que quebra com atualizações de cargo. O que quebra ao atualizar o nightly: (1) mudanças em traits internos (`core::iter::Step` ganhou `forward_overflowing` e quebrou `x86_64 < 0.15.5` — já mitigado); (2) `abi_x86_interrupt` e `-Z bindeps` são instáveis por definição (fonte de quebra recorrente); (3) `build.rs` do `kernel` usa `wat` e `-C`/`--cfg` de crates cripto (`aes_force_soft`, `polyval_force_soft`, `ghash_force_soft` em `.cargo/config.toml`) que dependem dos nomes internos de cada release dessas crates — atualizar `aes-gcm`/`sha2` sem rever esses cfgs quebra o link com SSE desabilitado.
- Higiene: `cargo fmt --all -- --check` acusa **27** diffs; `kernel` compila com 0 warnings de rustc; `cargo doc -p osjeff_core` 5 warnings (A-13).
Correção proposta: fixar `rust-version`; rodar um job semanal "nightly latest" (não-bloqueante) para detectar a quebra cedo; avaliar substituir `bindeps` por um passo de build explícito no `os/build.rs`; `cargo fmt` único e `--check` no CI.
Esforço: médio
Severidade: BAIXA.

---

### [BAIXA] A-16 Duplicação kernel ↔ core e lógica pura que deveria migrar para `osjeff_core`

Não há duplicação de `fs`/`net`/`heap` entre os crates (o kernel *usa* `osjeff_core::{fs,net,heap}`; `heap.rs` só tem a aritmética, o alocador em si fica no kernel). A duplicação real é pequena:
- `desktop/widgets.rs:284-287 two()` ≡ `osjeff_core/src/terminal.rs:338-339`;
- 4 formatadores decimais: `calc.rs:212`, `kernel/perf.rs:125-133`, `kernel/desktop/files_ui.rs:24-32`, `kernel/desktop/widgets.rs:321-325` (+ `put_ms` `perf.rs:147`).

O que migrar para ganhar teste (linhas do HEAD, esforço):

| # | Candidato | Onde | Esforço |
|---|---|---|---|
| 1 | `resolve_redirect` (tem bug, A-12) | `kernel/src/fetch.rs:143-161` | baixo |
| 2 | Layout/hit-testing do desktop: `menu_item_at`, `dock_layout`, `calc_layout`, `calc_button_at`, `browser_home_layout`, `start_*` (+ `calc_button_at` ↔ `Calc`) | `desktop/widgets.rs:7-123,149-183`; cliques `desktop/input.rs:89-138 (browser_click, files_click)` | médio (depende de `Rect` e consts de tema já em core; mover as consts) |
| 3 | Z-order/foco/hit-test de janelas: `focused`, `bring_to_front` (`unwrap_or(0)` mascara janela ausente), `topmost_at`, `window_of_pid`, `anim_signature` | `desktop/mod.rs:317-340,372-390`; `render.rs:62-91` | médio (extrair `WindowStack` puro) |
| 4 | Parse de caps virtio e BAR 64-bit: `discover`, `bar_base` (hoje com `cap_read32` do PCI embutido) | `kernel/src/virtio.rs:187-238` | médio (trait `ConfigSpace` + mock) |
| 5 | Parse de IDENTIFY ATA (modelo byte-swap, LBA48/28, rotação) | `kernel/src/ata.rs:169-209` | baixo (função `parse_identify(&[u16;256])`) |
| 6 | Cliente DHCP (`dhcp_acquire`/`poll_dhcp`) como máquina de estados pura (Discover→Offer→Request→Ack, timeouts em ticks injetados) | `kernel/src/main.rs:712-747` | médio |
| 7 | Matemática de framebuffer: `isqrt`, `Color::lerp`, `blend_pixel`, cobertura de canto de `fill_round_rect_alpha`, `snapshot/blend_from_local` | `kernel/src/fb.rs:6-17,32-45,213-232,235-304,338-381` | médio (separar da escrita no `buf`) |
| 8 | Layout do gerenciador de arquivos (lista/rodapé/painel de disco) e formatação de bytes/ms | `desktop/files_ui.rs:17-38,130-258`, `perf.rs:118-153` | médio |
| 9 | Fluxo/validação WASI: `is_wad_path`, `fd_seek` (aritmética `i64` sem `checked`), mapeamento de erros | `kernel/src/wasm/wasi.rs:48-65,126-141` | baixo |
| 10 | Alocador `LinkedListAllocator` (lista livre + coalescência) → crate novo `osjeff_alloc` (`unsafe` isolado) testável no host e com `miri` | `kernel/src/allocator.rs:76-232` | médio |

Esforço total: médio/alto, mas incremental; cada item ganha testes de borda de graça (hoje 0% no kernel).
Severidade: BAIXA (dívida de testabilidade; sobe para MÉDIA se a lógica de rede/janelas crescer).

---

### [BAIXA] A-17 Qualidade dos testes e lints sugeridos

Distribuição dos 189 testes (soma exata): terminal 26, editor 23, calc 18, web 15 (css 4, dom 5, layout 6, **mod 0, style 0**), browser 15, window 14, net 14, heap 14, keymap 13, process 12, fs 12, anim 8, clipboard 5. Kernel: 0.

- Não são só caminho feliz no `heap`/`keymap`/`fs`/`browser` (muitos `None`/`Err`/`assert!(!…)`), mas `web/*` (15 testes para ~860 linhas) e `net` (14) são majoritariamente felizes: os ramos de erro de parse listados em A-06 estão a descoberto.
- Não existe nenhum teste de propriedade/fuzz/`should_panic` (`grep -rn "proptest\|quickcheck\|fuzz\|should_panic"` → vazio). Os parsers que recebem dado externo são exatamente os candidatos: o fuzz caseiro que escrevi (xorshift + mutação, 20 000 iterações por alvo, `saved_tests/fuzz_smoke.rs`) já achou 3 classes de falha (A-01, A-05, A-13) em minutos. Resultado completo: `render` (3 viewports), `parse_html`, `parse_css`, `parse_url`, `http_body/status_code/header_value/page_body`, `encode_query`, `parse_dhcp`, `respond`: **0 pânicos**; `parse_color` 14, `decode` 478, `fs` (imagem corrompida) 2/3000.
- `#[cfg(test)]` não esconde código de produção (só módulos de teste inline, que inflam a cobertura bruta). Porém `#![cfg_attr(not(test), no_std)]` faz o crate compilar com `std` no prelude durante `cargo test`; isso mascara usos acidentais de `std` — a garantia vem do build `no_std` do kernel (`cargo build -p osjeff_core --target x86_64-unknown-none` passa, **PROVADO**).
- `cargo-fuzz` está instalado (`~/.cargo/bin/cargo-fuzz`): criar `fuzz/` com alvos `render`, `parse_dhcp_respond`, `fs_image`, `http_response`, `parse_url`; `proptest` como `dev-dependency` de `osjeff_core` (somente host).

**Clippy pedantic — relatório (contagem única por local; o `kernel` inclui o `osjeff_core`, que é membro do workspace):**

`osjeff_core` (160 avisos): `must_use_candidate` 99, `cast_lossless` 14, `cast_possible_truncation` 14, `missing_errors_doc` 7, `return_self_not_must_use` 5, `redundant_closure_for_method_calls` 4, `map_unwrap_or` 3, `match_same_arms` 3, `cast_sign_loss` 3, `doc_markdown` 2, `cast_possible_wrap` 2, 5 isolados.
`kernel` só (1147): `unreadable_literal` 557 (todos em `font.rs`: tabela de glifos), `cast_sign_loss` 262 (`desktop/apps.rs` 134, `files_ui.rs` 58, `wasm/mod.rs` 23), `cast_possible_truncation` 92 (+14 do core), `cast_lossless` 63, `cast_possible_wrap` 55, `ptr_as_ptr` 17, `many_single_char_names` 16, `cast_ptr_alignment` 14, `doc_markdown` 13, `single_match_else` 7, `wildcard_imports` 7, `trivially_copy_pass_by_ref` 7, `manual_let_else` 6, `unnested_or_patterns` 4, demais ≤ 3.

Restrição aplicada **só ao código de produção** (excluídos os `#[cfg(test)]`): `indexing_slicing` / `arithmetic_side_effects` / `unwrap_used` / `expect_used` / `panic`.
- `osjeff_core`: **0** `unwrap_used`, **0** `expect_used`, **0** `panic!` em produção (ótimo). `indexing_slicing` 383 / `arithmetic_side_effects` 303, em `net.rs` (153/51), `browser.rs` (44/31), `editor.rs` (44/44), `terminal.rs` (40/32), `fs.rs` (27/21), `web/css.rs` (25/21), `web/dom.rs` (15/29), `web/layout.rs` (0/18), `calc.rs` (18/17), `window.rs` (0/22). A fuzz mostra que **quase todos são índices guardados por checagem prévia**; os que importam de fato estão em A-01, A-04, A-05, A-10. `clippy::string_slice` (restriction) é mais útil: **8** ocorrências (`web/mod.rs` ×6 = A-01; `web/css.rs:183,197` ok na prática).
- `kernel`: `unwrap_used` 6 (`allocator.rs:140,165,179,183` — nós da lista livre; `desktop/mod.rs:195,308`), `panic` 1 (`sched.rs:161`, intencional: canário de pilha), `expect_used` 1 (`sched.rs:221`); `build.rs`: 10. `indexing_slicing` 611 / `arithmetic_side_effects` 997 (framebuffer/ícones: ruído de pixel).

**Top-10 lints reais (ordem de prioridade):**
1. `clippy::undocumented_unsafe_blocks` (100) — A-07 (habilitar como `warn` já).
2. `clippy::string_slice` (8) — A-01, vira `deny` em `web/`.
3. `clippy::indexing_slicing` **somente** em `fs.rs`, `net.rs` builders, `browser.rs`, `web/dom.rs` — habilitar por módulo (`#![warn(clippy::indexing_slicing)]` no topo desses arquivos) e converter para `.get()`.
4. `clippy::arithmetic_side_effects` em `process.rs`, `fs.rs`, `heap.rs` (pid, offsets, `align_up`) — A-10.
5. `clippy::cast_possible_truncation` (14 no core: `fs.rs` ×5 `as u8/u16` de tamanhos/slots, `net.rs` ×4 `as u16` de comprimentos de pacote, `browser.rs`, `web/layout.rs`) — comprimentos de pacote/arquivo devem usar `u16::try_from`.
6. `clippy::needless_range_loop` (1, `render.rs:74`) — A-09 (já é warn padrão).
7. `clippy::cast_ptr_alignment` + `ptr_as_ptr` + `borrow_as_ptr`/`ref_as_ptr` (14+17+4 no kernel: `virtio.rs`, `virtio_gpu.rs`, `allocator.rs`, `sched.rs`) — leituras `read_volatile` de `*const u32` a partir de `*mut u8` (exigem alinhamento; usar `.cast::<u32>()` e asserts).
8. `clippy::missing_errors_doc` (7, `fs.rs`) + `missing_safety_doc` (todos os `unsafe fn` públicos já têm `# Safety`) — documentar `FsError`.
9. `clippy::trivially_copy_pass_by_ref` / `unused_self` (kernel `pci.rs`, `virtio.rs`, `desktop/*`) — baixa, mas indica `&self` sem uso.
10. `clippy::manual_let_else` / `single_match_else` / `match_same_arms` — estilo útil em `main.rs`, `browser.rs`.
Ruído (ignorar ou `allow` no crate): `must_use_candidate` (99+), `unreadable_literal` (557, `font.rs`), `doc_markdown`, `cast_lossless`, `many_single_char_names`, `too_many_lines`, `similar_names`, `wildcard_imports`, `items_after_statements`, `explicit_iter_loop`.

Esforço: médio
Severidade: BAIXA.

---

## 2. CI/automação (item 9)

`ls .github` → **inexistente**. Nada roda `cargo test`, clippy ou build automaticamente; A-09 e a mentira do README passaram sem ser notadas por isso. Proposta mínima (**não criada** no repo; copiar para `.github/workflows/ci.yml`). O `rust-toolchain.toml` já fixa nightly, componentes e targets, então `rustup show` instala tudo. Observações validadas aqui: usar `-p osjeff_core` no `lint-host` (o alias atual arrasta o kernel via `-p os`), e **não** rodar `cargo tree` sem `-p kernel --target x86_64-unknown-none` (ICE).

```yaml
name: ci
on:
  push: { branches: [main] }
  pull_request:
permissions: { contents: read }
env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: ""            # o kernel usa .cargo/config.toml; não sobrescrever
jobs:
  test-core:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show          # instala nightly-2026-10-05 + componentes + targets
      - uses: Swatinem/rust-cache@v2
      - run: cargo test -p osjeff_core
      - run: cargo build -p osjeff_core --target x86_64-unknown-none   # prova que o core é no_std de verdade

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy -p osjeff_core --all-targets -- -D warnings          # lint-host (escopo certo)
      - run: cargo clippy -p kernel --target x86_64-unknown-none -- -D warnings # lint-kernel
      - run: cargo fmt --all -- --check
        continue-on-error: true    # hoje há 27 diffs; remova quando formatar uma vez

  build-image:
    runs-on: ubuntu-latest
    needs: [test-core, lint]
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - uses: Swatinem/rust-cache@v2
      - run: cargo build -p kernel --target x86_64-unknown-none --release
      - run: cargo build -p os --release      # bootloader + imagem BIOS/UEFI
      - uses: actions/upload-artifact@v4
        with: { name: osjeff-images, path: target/release/build/os-*/out/*.img, if-no-files-found: warn }

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - uses: taiki-e/install-action@cargo-llvm-cov
      - run: cargo llvm-cov -p osjeff_core --branch --fail-under-lines 85 --summary-only

  supply-chain:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
        with: { command: check advisories licenses bans sources }
      - run: cargo install cargo-audit --locked && cargo audit
        continue-on-error: true       # yanked/unmaintained de host não devem bloquear merge

  nightly-canary:        # semanal, não bloqueante: detecta cedo o próximo "Step::forward_overflowing"
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install nightly --component rust-src llvm-tools clippy -t x86_64-unknown-none
      - run: cargo +nightly build -p kernel --target x86_64-unknown-none
```
(adicionar `on.schedule: [{cron: "0 6 * * 1"}]` para o canário.) Passos de fuzz (`cargo +nightly fuzz run render -- -max_total_time=60`) entram assim que existir `fuzz/` (A-17). O job `coverage` com `--fail-under-lines 85` passa hoje (88% prod; 92% bruto).

---

## 3. Como reproduzir tudo (comandos)

```
git worktree add <dir> HEAD && cd <dir> && export CARGO_TARGET_DIR=<tgt>
cargo test -p osjeff_core                                       # 189 passed
cargo llvm-cov -p osjeff_core --summary-only                    # 92,33% linhas bruto
cargo llvm-cov -p osjeff_core --branch --summary-only           # 72,53% branches
cargo llvm-cov -p osjeff_core --show-missing-lines              # linhas citadas em A-06
cargo clippy -p kernel --target x86_64-unknown-none -- -D warnings            # 1 erro render.rs:74
cargo clippy -p osjeff_core -p os --all-targets -- -D warnings                # também falha (kernel)
cargo clippy -p osjeff_core --all-targets -- -D warnings                      # passa
cargo clippy -p osjeff_core -- -W clippy::pedantic                            # 160
cargo clippy -p kernel --target x86_64-unknown-none -- -W clippy::pedantic    # 1147 (+160 do core)
# unsafe: adicionar #![deny(unsafe_op_in_unsafe_fn)] #![warn(clippy::undocumented_unsafe_blocks)] em kernel/src/main.rs
cargo audit ; cargo deny init && cargo deny check ; cargo update --dry-run
# repros (saved_tests copiados para osjeff_core/tests/):
cargo test --test repro -- --nocapture --test-threads=1   # parse_color, decode_utf8, pid, fs
cargo test --test color_render -- --nocapture             # A-01 via render()
cargo test --test fuzz_smoke -- --nocapture               # fuzz de 12 alvos (aborta no fim: A-04 estoura a pilha)
cargo test --release --test depth --no-run ; DEPTH=150 <bin> --nocapture   # A-02
```
