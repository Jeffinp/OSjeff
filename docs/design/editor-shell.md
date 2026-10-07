# Editor de texto v2 e motor de shell

Dois módulos novos em `osjeff_core`, ambos puros (`no_std` + `alloc`,
`#![forbid(unsafe_code)]`, testados no host): `editor2` (editor de texto) e `shell`
(linha de comando). Nenhum arquivo do kernel, `editor.rs` ou `terminal.rs` foi
alterado: a integração é uma etapa separada. Teclas com modificadores vêm de
`osjeff_core::input` (`KeyEvent`, `KeyCode`, `Mods`), que embrulha o `keymap::Key`
existente em vez de mudá-lo.

## Arquitetura

```text
teclado -> Keymap -> Key ----> KeyEvent::from_key(key, Mods{ctrl,shift,alt})
                                   |                          |
                           editor2::Editor              shell::LineEditor
                                   |                          | Submit(linha)
                     visible_rows() / cursor_screen()   Shell::run_line(linha, Host{fs, sys})
                                   |                          |
                               o kernel desenha        RunResult{status, output, exit, clear}
```

* **`editor2`**: *gap buffer* + índice de início de linhas (`Vec<usize>`). Digitar é
  O(1) amortizado e o texto de um arquivo de 2 MB fica contíguo (exceto a lacuna), então a
  busca roda sobre uma fatia comum. Uma *piece table* tornaria cada leitura (desenho,
  busca, UTF-8) mais cara sem ganho aqui, pois o desfazer já guarda os bytes exatos de
  cada edição. O custo assumido: cada tecla soma um inteiro por linha abaixo do cursor
  (~50 µs com 50 mil linhas).
* **`shell`**: `parse` (lexer + parser com erros tipados e posição) → `exec` (expansão,
  pipelines em buffers de memória, redirecionamentos, controle de fluxo, funções, scripts)
  → `builtins` (48 comandos). `line` é o editor de linha do terminal.
* O motor só conhece **dois traits** (`ShellFs`, `SysInfo`); o kernel os implementa. Para
  testes e fuzzing existem `MemFs` e `MockSys`.

## O que o kernel deve implementar

Definidos em `osjeff_core::shell::fs` e `osjeff_core::shell::sys`.

```rust
pub trait ShellFs {
    fn cwd(&self) -> String;                                   // absoluto, normalizado
    fn set_cwd(&mut self, path: &str) -> Result<(), FsErr>;    // alvo deve ser diretório
    fn resolve(&self, path: &str) -> String { normalize(&self.cwd(), path) }  // padrão
    fn stat(&self, path: &str) -> Result<Stat, FsErr>;         // Stat { kind: File|Dir, size: u64 }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, FsErr>;
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, FsErr>; // padrão: lê tudo
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr>;   // cria/trunca; pai deve existir
    fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr>;  // cria se não existir
    fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsErr>;          // sem "." e ".."
    fn mkdir(&mut self, path: &str) -> Result<(), FsErr>;
    fn remove(&mut self, path: &str) -> Result<(), FsErr>;     // arquivo ou diretório vazio
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsErr>;
    fn usage(&self) -> FsUsage { FsUsage::default() }          // df
}

pub trait SysInfo {
    fn now(&self) -> DateTime;                // obrigatório: date
    fn uptime_ms(&self) -> u64;               // obrigatório: uptime
    fn mem(&self) -> MemInfo;                 // obrigatório: free (total/used em bytes)
    fn disks(&self) -> Vec<DiskInfo> { vec![] }                           // df (vazio = usa ShellFs::usage)
    fn procs(&self) -> Vec<ProcInfo> { vec![] }                           // ps
    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), SysErr>       // kill (padrão: Unsupported)
    fn ping(&mut self, host: &str, count: u32) -> Result<PingStats, SysErr> // ping (padrão: Unsupported)
    fn sleep_ms(&mut self, ms: u64) {}        // sleep (o shell já limita o valor)
    fn clear_screen(&mut self) {}             // clear
    fn hostname(&self) -> String { "osjeff".into() }                      // prompt
}
```

Regras de `ShellFs`: caminhos absolutos ou relativos ao `cwd`, com `.`, `..` e barras
repetidas (`fs::normalize` resolve; `..` acima de `/` fica em `/`). Erros usam `FsErr`
(`NotFound`, `NotADirectory`, `IsADirectory`, `AlreadyExists`, `NotEmpty`, `NoSpace`,
`TooBig`, `NameTooLong`, `InvalidPath`, `ReadOnly`, `Io`). Toda a aritmética é inteira
(o kernel é *soft-float*). O OJFS v3 deve virar um `impl ShellFs`; o motor nunca importa
`osjeff_core::fs`.

O kernel também pode registrar comandos próprios com `Shell::register(nome, ajuda, fn)`
(por exemplo abrir apps) e persistir o histórico com `History::to_bytes`/`load`.

### Laço de integração do terminal

```rust
let mut sh = Shell::new();
let mut ed = LineEditor::new(&sh.prompt(&fs, &sys));
// a cada tecla:
let comp = ShellCompleter { shell: &sh, fs: &fs };
match ed.handle_key(KeyEvent::from_key(key, mods), sh.history(), &comp) {
    LineEvent::Submit(line) => {
        let r = sh.run_line(&line, &mut Host { fs: &mut fs, sys: &mut sys });
        if r.clear { term.clear(); }
        term.print(&r.output);              // stdout e stderr intercalados
        if let Some(code) = r.exit { /* fechar o terminal */ }
        ed.set_prompt(&sh.prompt(&fs, &sys));
    }
    LineEvent::Candidates(names) => term.print_columns(&names),
    LineEvent::Interrupt | LineEvent::ClearScreen | LineEvent::Eof | _ => { /* redesenhar */ }
}
```

### Laço de integração do editor

```rust
let mut ed = Editor::from_bytes(&file);          // ou set_text; to_bytes()/as_slices() para salvar
ed.resize(linhas, colunas);                      // pode mudar a qualquer momento (mesmo 0x0)
match ed.handle_key(KeyEvent::from_key(key, mods), &mut clipboard) {
    Event::SaveRequested => { write(ed.as_slices()); ed.mark_saved(); }
    Event::QuitRequested => if ed.is_modified() { /* perguntar */ },
    _ => {}
}
for (y, row) in ed.visible_rows().enumerate() {  // sem copiar o documento
    // row.line_number (Some na 1ª linha visual), row.continuation, row.cells() -> Cell{ch, selected}
}
if let Some((y, x)) = ed.cursor_screen() { /* desenhar o cursor */ }
if let Some(p) = ed.prompt() { /* barra de busca / ir para linha: p.label, p.text, p.notice */ }
// mouse: ed.mouse_down(y, x, cliques, shift); ed.mouse_drag(y, x); ed.scroll_by(+-3)
```

## Limites

| Limite | Valor padrão | Onde |
|---|---|---|
| Saída de uma execução | 1 MiB (`max_output`) | `shell::Limits` |
| Buffer de pipe / `$( )` | 1 MiB (`max_pipe`) | `Limits` |
| Comandos + iterações por execução | 100 000 (`max_steps`) | `Limits` |
| Iterações de um laço | 10 000 (`max_loop`) | `Limits` |
| Chamadas de função / `source` aninhados | 32 | `Limits::max_call_depth` |
| `$( )` aninhado (execução / parse) | 8 / 24 | `Limits::max_sub_depth`, `parse::MAX_DEPTH` |
| Resultados de um glob | 4096 | `Limits::max_glob` |
| Linha digitada / script | 8 KiB / 256 KiB | `Limits` |
| `sleep` máximo | 10 s | `Limits::max_sleep_ms` |
| Variáveis / valor de variável | 1024 / 64 KiB | `shell::env` |
| Funções / aliases | 256 / 256 | executor |
| Histórico | 500 linhas | `History::DEFAULT_MAX` |
| Linha do `LineEditor` | 4096 caracteres | `line::MAX_LINE` |
| Regex do `grep` | 1024 bytes, 200 000 passos | `shell::regex` |
| Documento do editor | memória (testado com 2 MB; linha única de 2 MB) | `editor2` |
| Histórico de desfazer | ilimitado; `set_undo_limit(bytes)` opcional | `editor2` |
| Área de transferência | 256 bytes (`clipboard::CAP`, truncada em fronteira de caractere) | `clipboard.rs` |

**Pilha.** O executor é recursivo. O pior caso permitido pelos limites (recursão de funções
no limite, `$( )` aninhado, 22 níveis de `if` no parse) cabe numa pilha de 256 KiB em build
debug (estoura com 160 KiB; o build de release usa bem menos). Se a thread do terminal tiver
pilha menor, reduza `Limits::max_call_depth` e `max_sub_depth`. O teste
`worst_case_nesting_fits_a_256_kib_stack` fixa esse número.

Estourar um limite nunca derruba o kernel: o executor aborta a execução, escreve
`sh: step limit exceeded, aborting` (ou similar) e o shell continua utilizável.

## Atalhos do editor

| Tecla | Ação |
|---|---|
| Setas, Home/End, PageUp/PageDown | mover (com Shift: selecionar). Home alterna entre 1º caractere útil e coluna 0 |
| Ctrl+←/→ | por palavra |
| Ctrl+Home/End | início/fim do documento |
| Ctrl+↑/↓ | rolar uma linha sem mover o cursor |
| Ctrl+A, Ctrl+L | selecionar tudo, linha atual |
| Duplo/triplo clique, arrastar | palavra, linha, seleção |
| Ctrl+C / Ctrl+X / Ctrl+V (Shift+Del = recortar) | área de transferência |
| Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z) | desfazer / refazer (digitação agrupada por palavra) |
| Ctrl+F, F3 / Shift+F3 | buscar, próximo / anterior (Alt+C alterna maiúsculas) |
| Ctrl+H | substituir (Tab troca de campo; Enter substitui um; Alt+A todos) |
| Ctrl+G | ir para a linha |
| Ctrl+Backspace / Ctrl+Delete | apagar palavra à esquerda / direita |
| Tab / Shift+Tab | indentar / desindentar (seleção multilinha indenta as linhas) |
| Enter | nova linha com auto-indentação (usa `\r\n` se o arquivo usa) |
| Ctrl+S / Ctrl+Q | `Event::SaveRequested` / `Event::QuitRequested` |

Configurável: `set_tab_width`, `set_use_spaces`, `set_auto_indent`, `set_line_numbers`,
`set_soft_wrap`, `set_case_sensitive`, `set_undo_limit`.

UTF-8: o cursor anda por ponto de código (largura 1; Tab expande até o próximo
múltiplo da largura de tab). Bytes inválidos são preservados e viram um caractere
`U+FFFD` na tela. `\n` e `\r\n` (inclusive misturados) são gravados exatamente como
lidos. O estado "modificado" volta a falso ao desfazer até o ponto salvo.

## Comandos do shell

`help ls cd pwd cat echo mkdir rm rmdir mv cp touch head tail wc grep sort uniq tee
clear env export set unset history alias unalias which date uptime free df ps kill ping
true false test [ sleep`, mais `seq basename dirname stat rev tr cut nl yes` e os que o
executor trata (`exit return break continue shift source . sh :`). `help NOME` mostra
o uso. `grep` aceita regex estendida (`. * + ? | ( ) [] ^ $ \d \w \s`) ou `-F`.

Sintaxe: aspas simples/duplas, `\`, `$VAR`, `${VAR}`, `$?`, `$#`, `$@`, `$*`, `$1..$9`,
`$(cmd)`, `$((aritmética))`, `~`, globs `*` e `?`, `|`, `>`, `>>`, `<` (também
`/dev/null`), `&&`, `||`, `;`, `#`, `NAME=valor cmd`, `if/elif/else/fi`,
`for x in ...; do ...; done`, `while`/`until`, `f() { ...; }`, `{ ...; }`. Scripts `.sh`
rodam por `sh arq`, `source arq`, pelo nome se estiverem no `PATH`, ou por caminho.

Erros de sintaxe: `ParseError { kind, pos }` com `line_col()`; o shell imprime
`sh: syntax error at line L, column C: ...` e devolve status 2. Rejeitados com mensagem
clara: `&` (jobs em segundo plano), `<<` (here-document), `2>` e `>&`.
Códigos de saída: 0 ok, 1 falha, 2 uso/sintaxe, 126 não executável, 127 não encontrado.

Atalhos do editor de linha: ←/→, Home/End, Ctrl+A/E/B/F, Ctrl+←/→ (palavra), Ctrl+K
(apaga até o fim), Ctrl+U (até o início), Ctrl+W (palavra), Ctrl+Y (cola), Ctrl+D (EOF
se vazia), Ctrl+C (descarta), Ctrl+L (limpa a tela), ↑/↓ ou Ctrl+P/N (histórico),
Ctrl+R (busca reversa; Ctrl+R de novo = mais antigo; Enter executa; Esc cancela), Tab
(completa comandos, `$variáveis` e caminhos; com várias opções, completa o prefixo comum
e depois lista).

## Limitações conhecidas

* Sem jobs em segundo plano, here-documents e redirecionamento de descritores.
* Variáveis são globais (sem `local`); `{ ...; }` não consome o stdin, cada comando vê
  a entrada inteira; glob só com `*` e `?` e só em literais (não em resultado de `$VAR`).
* `cat`, `sort` etc. leem o arquivo inteiro: arquivos de vários MB passam pela memória.
* `ShellFs::read_at` existe para o OJFS v3 expor leitura parcial, mas `head`/`tail` ainda
  leem tudo (uma otimização futura sem mudar o trait).
