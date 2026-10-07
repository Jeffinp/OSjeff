# Testes e verificação

O OSjeff tem cinco camadas de verificação. Cada uma responde a uma pergunta
diferente, e a lista abaixo diz **o que cada uma não cobre**.

| Camada | Pergunta que responde | Comando | Cobre | Não cobre |
|---|---|---|---|---|
| Testes unitários | A lógica pura está certa? | `cargo test-core` | `osjeff_core` (terminal, shell, editor, editor2, calc, janelas, heap, FS v2/v3, blockdev/blockcache, rede, HTML/CSS, browser) | `kernel/` (hardware) |
| Fuzzing | Dado hostil derruba o parser? | `cd fuzz && cargo fuzz run <alvo>` | `net`, `fs` (v2 e v3), `web`, `shell`, `editor2` | TCP/TLS/DNS (`smoltcp`, `embedded-tls`), drivers |
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
| `net_parse` | bytes como frame Ethernet | `osjeff_core::net` (ARP, IPv4, ICMP, UDP, DHCP, `respond`) com buffers de saída de vários tamanhos |
| `ojfs_parse` | bytes como imagem de disco | todas as operações do OJFS v2 (`list/read/write/remove/mkdir/trash/purge`) |
| `ojfs3_parse` | remendos sobre um OJFS v3 válido (com todos os CRC refeitos, para passar do checksum), bytes crus, ou dispositivo de tamanho qualquer | `detect`, `mount`, caminhada (`readdir/stat/read_at/path_of/trash_list`), `fsck`, 16 operações, `fsck` de novo (um FS são continua são) |
| `ojfs3_ops` | sequência de operações, com queda de energia opcional (em ordem ou cache volátil) | escrita lida de volta, `fsck` limpo, remount idêntico, estado exatamente antes/depois da operação cortada |
| `web_parse` | bytes como HTML/CSS/URL/resposta HTTP | parser, CSS, layout, `dechunk`, URL |
| `shell_parse` | bytes como linha/script de shell | lexer/parser, executor (FS em memória com limites, `SysInfo` mock), editor de linha com Tab |
| `editor_ops` | documento + sequência de operações (`arbitrary`) | `editor2`: teclas, mouse, busca/substituição, undo/redo, wrap; invariantes depois de cada operação |

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

## 3. Boot em QEMU

Sem tela e sem KVM (TCG), BIOS e UEFI:

```bash
tools/qemu-headless.sh bios /tmp/osj 25      # serial.log + screen.png
QEMU_MEM=256M tools/qemu-headless.sh uefi /tmp/osj-uefi 40
KEEP_FS=1 tools/qemu-headless.sh bios /tmp/osj 25   # reaproveita fs.img (persistência, disco corrompido)
```

`tools/verify-boot.sh <outdir> [baseline]` constrói, sobe os dois modos e falha se
o boot não completar, se aparecer `KERNEL PANIC`/`FATAL` na serial, ou se o desktop
diferir da baseline (o HUD e o relógio são mascarados porque mudam a cada execução).
Para tirar uma baseline, rode o script uma vez num commit bom e passe o `outdir`
como segundo argumento nos seguintes.

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

## O que ainda não é testado

- O `kernel/` não tem testes automatizados. A migração de lógica pura para o core
  é contínua (ver [`ROADMAP.md`](ROADMAP.md)).
- Nenhum teste em hardware real.
- TLS real e DNS: o sandbox de QEMU não tem internet útil; os caminhos de rede
  são testados por unidade e por boot.
- O CI (`ci.yml`) foi escrito mas ainda não rodou no GitHub; cada comando dele
  foi executado localmente.
