# Editor de texto v2 e motor de shell

Dois módulos novos em `kitsune_core`, ambos puros (`no_std` + `alloc`,
`#![forbid(unsafe_code)]`, testados no host): `editor2` (editor de texto) e `shell`
(linha de comando). Desde a onda W15b eles são o Editor e o Terminal do desktop (veja
[Integração no desktop](#integração-no-desktop-w15b)); `editor.rs` e `terminal.rs` (a grade fixa
antiga) foram removidos. Teclas com modificadores vêm de
`kitsune_core::input` (`KeyEvent`, `KeyCode`, `Mods`), que embrulha o `keymap::Key`
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
  → `builtins` (48 comandos) e `netcmds` (`nslookup`, `curl`, `wget`, `ifconfig`). `line` é o editor
  de linha do terminal, `screen` o histórico rolável (linhas lógicas, quebra na largura da
  janela) e `term` a sessão que liga os dois às teclas.
* O motor só conhece **dois traits** (`ShellFs`, `SysInfo`); o kernel os implementa. Para
  testes e fuzzing existem `MemFs` e `MockSys`.

## O que o kernel deve implementar

Definidos em `kitsune_core::shell::fs` e `kitsune_core::shell::sys`.

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
    fn resolve(&mut self, host: &str) -> Result<Vec<[u8; 4]>, SysErr>      // nslookup (padrão: Unsupported)
    fn http_get(&mut self, url: &str, max_body: usize) -> Result<HttpResponse, SysErr> // curl, wget
    fn net_info(&self) -> Option<NetInfo> { None }                        // ifconfig
    fn interrupted(&self) -> bool { false }   // Ctrl+C: o executor consulta a cada comando e iteração
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
`kitsune_core::fs`.

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

## Integração no desktop (W15b)

O Editor e o Terminal do desktop são esses dois módulos; o que o kernel acrescenta é só cola
(`kernel/src/desktop/`): `edit.rs`, `term.rs` e `shellhost.rs`. Tudo que toca arquivo passa por
`desktop/vfs.rs`. Os módulos antigos `kitsune_core::editor` e `terminal` (grade fixa 44x18 / 40x14)
foram removidos.

```text
tecla ─► Desktop::dispatch_key ─┬─► Terminal: Term::key ─► TermAction::Run(linha)
                                │        ▲                         │ post
                                │        │ finish(RunResult)       ▼
                                │   step_shell_jobs ◄── Done ── thread shelld (x2) ── Shell::run_line
                                │                                      │ Host { VfsFs, KSys }
                                └─► Editor: Editor::handle_key / Picker / CloseAsk
```

### Terminal

* **Estado:** `TermState { uid, term: Term, ctx: Option<Box<Ctx>> }`. `Term` (puro, em
  `kitsune_core::shell::term`) liga o `LineEditor` ao `Screen`; `Ctx` = `Shell` + `VfsFs` e **muda de
  mãos** a cada linha: o terminal o entrega à thread de comandos e o recebe de volta com o resultado.
* **Grade:** texto na escala 2 (célula 12x18 px); colunas e linhas são o que cabe na janela
  (`term_grid`), então maximizar mostra mais texto em vez de letra maior. O histórico (`Screen`) guarda
  linhas *lógicas* (no máximo 5000 linhas, 1 Mi caracteres, 2048 caracteres por linha: mais que isso
  continua na linha seguinte) e quebra na largura atual ao desenhar, de modo que redimensionar refaz a
  quebra do histórico inteiro. O prompt é desenhado na cor de destaque e há um indicador de rolagem
  fino à direita.
* **Teclas:** setas/Home/End/Ctrl+setas, Ctrl+A/E/K/U/W/Y, Ctrl+R (busca reversa), ↑/↓ (histórico),
  Tab (comandos, variáveis, caminhos; várias opções: prefixo comum e depois a lista em colunas),
  Ctrl+C (descarta a linha, ou **cancela o comando em execução**), Ctrl+L e `clear`, Ctrl+D (linha vazia
  fecha), PageUp/PageDown e a roda do mouse rolam o histórico (Ctrl+Home/End: início/fim); digitar volta
  ao fim. Ctrl+V cola (quebras de linha viram espaços: colar nunca executa) e Ctrl+Shift+C copia a linha
  digitada (Ctrl+C sozinho interrompe, como num terminal de verdade).
* **Comandos que esperam:** a linha roda numa de duas threads do kernel (`shelld`, `shelld2`), uma fila
  só. `sleep`, `ping`, `nslookup`, `curl` e `wget` esperam ali, nunca no compositor: a janela mostra
  "executando... Ctrl+C cancela" (sem prompt) e é redesenhada a cada quadro; outro terminal continua
  usável (a segunda thread). Ctrl+C acende uma flag que o executor consulta a cada comando e iteração
  (`SysInfo::interrupted`) e que `sleep`, `ping` e as buscas de rede consultam enquanto esperam; o
  resultado volta com status 130. Os limites de recursão são reduzidos (`max_call_depth` 16,
  `max_sub_depth` 4): a pilha da thread é de 128 KiB.
* **`ShellFs` (`VfsFs`):** diretório corrente por terminal, caminhos absolutos normalizados. `rm`
  manda para a **lixeira** (restaurável no gerenciador; pasta não vazia é `NotEmpty`, então `rm -r`
  desce até as folhas), `mv` sobre um arquivo existente manda o antigo para a lixeira, `df` mostra o
  volume (`ojfs3` ou `ramfs`) e os discos IDE (`ata::identify`).
* **`SysInfo` (`KSys`):** `date` vem de `clock::trusted_unix_secs` (SNTP, senão o RTC lido no boot) no
  fuso das Configurações; `uptime` de `netd::now_ms`; `free` do heap (amostra do monitor); `ps` lista
  as janelas (a `ProcessTable`) e as threads do kernel (`[fetcher]`, `[shelld]`...), copiadas quando a
  linha é enviada; `kill PID` fecha a janela do processo (threads e sistema: "operation not permitted");
  `ping` usa `netd::ping_us` (um pedido ICMP por vez, 1 s entre eles, cancelável); `nslookup`,
  `curl` e `wget` usam a caixa de correio `fetch::run_job` (veja abaixo); `ifconfig` lê `netd::stats()`.
  A RTC não é lida pela thread de comandos (as portas CMOS são do compositor).
* **Comandos do desktop** (registrados com `Shell::register`; deixam um `UiReq` que o compositor
  aplica): `edit [ARQ]`, `files`, `tasks`, `calc`, `reboot`, `shutdown` (os dois últimos perguntam antes
  se algum editor tem alterações não salvas).
* **Rede:** `fetch::run_job(NetJob::Resolve | Get, cancel)` é uma segunda caixa de correio, bloqueante,
  servida pela thread `fetcher` (a única dona da NIC) entre as páginas do navegador e o serviço de
  DHCP/ping: `IDLE → CLAIMED → REQUESTED → RUNNING → DONE`, e `ABANDONED` para quem desistiu (Ctrl+C ou
  90 s) no meio de uma busca: a resposta é descartada e a caixa só volta a `IDLE` quando o worker
  termina. `curl`/`wget` aceitam `http://` e `https://` (redirecionamentos, TLS e limites do
  navegador); o corpo vai até 4 MiB para arquivo e `max_output` para a tela.

### Editor

* **Estado:** `EditorState { ed: editor2::Editor, path: Option<Vec<u8>>, modal, msg, ... }`. Números
  de linha ligados, tabulação de 4 espaços, auto-indentação, UTF-8 (bytes inválidos preservados), `\n`
  e `\r\n` exatos. Arquivos de até **16 MiB** abrem inteiros (testado com 1 MB e 165 mil linhas, e com
  60 mil linhas de 16 caracteres); acima disso "Arquivo grande demais".
* **Teclas:** as do `editor2` (tabela abaixo) mais Ctrl+O (abrir), Ctrl+S (salvar; sem arquivo ainda abre
  "Salvar como"), Ctrl+Shift+S (salvar como), Ctrl+Q (fechar), Ctrl+N (outro editor), Alt+A/Alt+R/Alt+C
  na barra de busca. PageUp/PageDown/F3 chegam pelos scancodes (`Special`). Esc não fecha mais a janela
  (limpa a seleção ou fecha a barra).
* **Mouse:** clique posiciona, duplo clique seleciona a palavra, triplo a linha, arrastar seleciona (e
  rola ao sair do texto), a roda rola 3 linhas por notch.
* **Abrir / Salvar como:** `editor2::dialog::Picker` (pastas primeiro em ordem natural, `..`, campo de
  nome/caminho com Tab, Backspace sobe uma pasta, mouse, substituir pergunta antes). Ctrl+O sobre um
  documento já em uso abre o arquivo **em outra janela** (ou na que já o mostra): nada é substituído.
  A janela de um arquivo aberto pelo Arquivos (`open_path`) e por `edit` é a mesma coisa.
* **Alterações não salvas:** título `OSJEFF EDIT - nome *`; fechar a janela (botão da barra, Ctrl+Q,
  Tarefas, `kill`, Reiniciar/Desligar) mostra "Salvar alterações?" com **Salvar / Descartar /
  Cancelar** (S, D, C, setas+Enter, clique; Esc cancela). Salvar sem nome abre "Salvar como" e só fecha
  se gravar; erro de gravação mantém a janela aberta com a mensagem.
* **Limite conhecido:** a área de transferência tem 256 bytes (`clipboard::CAP`): copiar mais que isso
  copia o começo.

### Capturas (QEMU, `tools/perf/scen/w15b-*.sh`)

| Arquivo (`docs/img/`) | Mostra |
|---|---|
| `w15b-terminal-rede.png` | `ping`, `nslookup` (DNS real do QEMU), `ifconfig` |
| `w15b-terminal-dois.png` | um segundo terminal responde enquanto o primeiro roda `sleep 20` |
| `w15b-terminal-rolagem.png` | `seq 10000` depois de PageUp, com o indicador de rolagem |
| `w15b-editor-salvar-como.png` | Ctrl+S num documento sem nome: o seletor de arquivos |
| `w15b-editor-substituir.png` | Ctrl+H, Alt+A: "1 troca(s)" |
| `w15b-editor-fechar.png` | a pergunta Salvar / Descartar / Cancelar |
| `w15b-editor-grande.png` | um arquivo de 1 MB com 165 mil linhas, no fim (Ctrl+End) |
| `w15b-guarda-desligar.png` | `shutdown` com um editor sujo: o desktop pergunta em vez de desligar |

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
| Ctrl+O, Ctrl+Shift+S (só no desktop) | abrir, salvar como (ver "Integração no desktop") |

Configurável: `set_tab_width`, `set_use_spaces`, `set_auto_indent`, `set_line_numbers`,
`set_soft_wrap`, `set_case_sensitive`, `set_undo_limit`.

UTF-8: o cursor anda por ponto de código (largura 1; Tab expande até o próximo
múltiplo da largura de tab). Bytes inválidos são preservados e viram um caractere
`U+FFFD` na tela. `\n` e `\r\n` (inclusive misturados) são gravados exatamente como
lidos. O estado "modificado" volta a falso ao desfazer até o ponto salvo.

## Comandos do shell

`help ls cd pwd cat echo mkdir rm rmdir mv cp touch head tail wc grep sort uniq tee
clear env export set unset history alias unalias which date uptime free df ps kill ping
true false test [ sleep`, mais `seq basename dirname stat rev tr cut nl yes`, os de rede
`nslookup curl wget ifconfig` (52 ao todo) e os que o executor trata (`exit return break continue
shift source . sh :`). No desktop há ainda `edit files tasks calc reboot shutdown`. `tr` aceita
`\n`, `\t`, `\r` e `\\` nos conjuntos. `help NOME` mostra
o uso. `grep` aceita regex estendida (`. * + ? | ( ) [] ^ $ \d \w \s`) ou `-F`.

Sintaxe: aspas simples/duplas, `\`, `$VAR`, `${VAR}`, `$?`, `$#`, `$@`, `$*`, `$1..$9`,
`$(cmd)`, `$((aritmética))`, `~`, globs `*` e `?`, `|`, `>`, `>>`, `<` (também
`/dev/null`), `&&`, `||`, `;`, `#`, `NAME=valor cmd`, `if/elif/else/fi`,
`for x in ...; do ...; done`, `while`/`until`, `f() { ...; }`, `{ ...; }`. Scripts `.sh`
rodam por `sh arq`, `source arq`, pelo nome se estiverem no `PATH`, ou por caminho.

Erros de sintaxe: `ParseError { kind, pos }` com `line_col()`; o shell imprime
`sh: erro de sintaxe na linha L, coluna C: ...` (em inglês: `sh: syntax error at line L, column C: ...`)
e devolve status 2. Rejeitados com mensagem clara: `&` (jobs em segundo plano), `<<`
(here-document), `2>` e `>&`.
Códigos de saída: 0 ok, 1 falha, 2 uso/sintaxe, 126 não executável, 127 não encontrado.

**Idioma das mensagens.** O motor fala o idioma da interface **no momento da execução** (`i18n::lang()`;
a thread `shelld` lê o mesmo átomo que o desktop). Mudam: mensagens de erro e de uso (`ls: não foi
possível acessar ...`, `sh: foo: comando não encontrado`, `uso: mv ORIGEM... DESTINO`), a linha de
cada comando em `help` (a sinopse fica, a descrição muda), os títulos e rótulos de `df`, `free`, `ps`,
`uptime`, `ping` (resumo), `ifconfig`, `nslookup`, `which`, `stat`, `wget`, `curl`, e o texto
`[saída truncada]`. As mensagens saem do catálogo (`sh.*`; `FsErr`/`SysErr`/`RegexErr`/`ArithErr`/
`ParseError` guardam a chave e consultam na hora). Texto já impresso fica como foi impresso.
**Estável em qualquer idioma** (scripts podem depender): nomes de comandos e opções, status de saída,
os dados que os comandos imprimem (`echo`, `cat`, `seq`, `wc`, `sort`, `head`, `tail`, `cut`, `tr`,
`date`, `env`, `export`, `alias`, `history`, `basename`, `dirname`, nomes e colunas de `ls -l`, a
linha `Mem:` de `free`, os números de `df`, os estados de `ps`, a linha `PING host`). O teste
`shell::tests::i18n` roda o mesmo script nos dois idiomas e exige saída e status idênticos.

Atalhos do editor de linha: ←/→, Home/End, Ctrl+A/E/B/F, Ctrl+←/→ (palavra), Ctrl+K
(apaga até o fim), Ctrl+U (até o início), Ctrl+W (palavra), Ctrl+Y (cola), Ctrl+D (EOF
se vazia), Ctrl+C (descarta), Ctrl+L (limpa a tela), ↑/↓ ou Ctrl+P/N (histórico),
Ctrl+R (busca reversa; Ctrl+R de novo = mais antigo; Enter executa; Esc cancela), Tab
(completa comandos, `$variáveis` e caminhos; com várias opções, completa o prefixo comum
e depois lista).

## Limitações conhecidas

* Sem jobs em segundo plano, here-documents e redirecionamento de descritores.
* No terminal do desktop, enquanto um comando roda a linha de entrada não aceita digitação (só
  Ctrl+C e rolagem); a saída aparece quando o comando termina (não há saída em fluxo), então
  `ping -c 4` mostra tudo no fim.
* `ps` e `kill` enxergam a tabela de processos do compositor (janelas) e as threads do kernel; não
  há processos de verdade.
* Variáveis são globais (sem `local`); `{ ...; }` não consome o stdin, cada comando vê
  a entrada inteira; glob só com `*` e `?` e só em literais (não em resultado de `$VAR`).
* `cat`, `sort` etc. leem o arquivo inteiro: arquivos de vários MB passam pela memória.
* `ShellFs::read_at` existe para o OJFS v3 expor leitura parcial, mas `head`/`tail` ainda
  leem tudo (uma otimização futura sem mudar o trait).
