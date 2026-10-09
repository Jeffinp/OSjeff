#!/usr/bin/env python3
"""Audit of the user-visible strings of Kitsune, for the i18n migration.

    python3 -I tools/i18n-audit.py            # rewrite docs/design/i18n-audit.md
    python3 -I tools/i18n-audit.py --check    # fail if the file on disk is out of date
    python3 -I tools/i18n-audit.py --summary  # print the per-area table only

It reads the Rust sources of `kernel/src` and `kitsune_core/src` (comments and `#[cfg(test)]`
modules skipped), finds the string literals that reach the screen (arguments of the drawing and
widget calls, menu entries, notifications, error messages, `String::from("...")` in UI code,
match arms that name things) and classifies them:

  * language   pt, en, mixed (both) or neutral (a name, a symbol, a unit);
  * no accent  a Portuguese word that needs one (the list in tools/i18n/accents.txt, shared
               with the host test `cargo test -p kitsune_core i18n`);
  * english    English text in a place where the UI speaks Portuguese;
  * migrated   already goes through the catalog (`t!`, `tk!`, ...), listed only as a count.

Each literal gets a proposed catalog key (`<area>.<slug>`), and the files are grouped by the app
that owns them so the migration can be split with no overlap.

Heuristics, not a parser: a literal is "visible" when the call it sits in is a drawing / widget /
message call, or when it reads like a sentence. Logs (`klog!`, `println!`), `panic!`, `assert!`,
`expect(...)`, serial output and byte strings are skipped on purpose (logs stay English). The
output is deterministic, so `--check` can run in CI.
"""

import os
import re
import sys
import unicodedata
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "docs", "design", "i18n-audit.md")
SRC_DIRS = ["kernel/src", "kitsune_core/src"]

# ---------------------------------------------------------------------------- tokenizer


def tokenize(src):
    """Yield (kind, text, line) with kind in ident, punct, str, bstr. Comments are dropped."""
    n = len(src)
    i = 0
    line = 1
    out = []
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
        elif c.isspace():
            i += 1
        elif src.startswith("//", i):
            while i < n and src[i] != "\n":
                i += 1
        elif src.startswith("/*", i):
            depth = 1
            i += 2
            while i < n and depth:
                if src[i] == "\n":
                    line += 1
                if src.startswith("/*", i):
                    depth += 1
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        elif c == '"' or (c == "b" and src.startswith('b"', i)):
            kind = "bstr" if c == "b" else "str"
            start = line
            i += 2 if c == "b" else 1
            buf = []
            while i < n and src[i] != '"':
                ch = src[i]
                if ch == "\n":
                    line += 1
                if ch == "\\" and i + 1 < n:
                    i += 1
                    e = src[i]
                    if e == "n":
                        buf.append("\n")
                    elif e == "t":
                        buf.append("\t")
                    elif e in "\\\"'":
                        buf.append(e)
                    elif e == "u":
                        j = src.find("}", i)
                        try:
                            buf.append(chr(int(src[i + 2 : j], 16)))
                        except ValueError:
                            pass
                        i = j
                    elif e == "x":
                        i += 2
                    elif e == "\n":
                        line += 1
                        while i + 1 < n and src[i + 1].isspace():
                            if src[i + 1] == "\n":
                                line += 1
                            i += 1
                    else:
                        buf.append(e)
                    i += 1
                else:
                    buf.append(ch)
                    i += 1
            i += 1
            out.append((kind, "".join(buf), start))
        elif c == "r" and re.match(r'r#*"', src[i : i + 40]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'r(#*)"', src[i:])
            hashes = m.group(1)
            start = line
            j = i + m.end()
            end = src.find('"' + hashes, j)
            if end < 0:
                end = n
            body = src[j:end]
            line += body.count("\n")
            out.append(("str", body, start))
            i = end + 1 + len(hashes)
        elif c == "'":
            if src.startswith("'\\", i):
                j = src.find("'", i + 2)
                i = j + 1 if j >= 0 else n
            elif i + 2 < n and src[i + 2] == "'":
                i += 3
            else:
                i += 1
        elif c.isalpha() or c == "_":
            j = i
            while j < n and (src[j].isalnum() or src[j] == "_"):
                j += 1
            out.append(("ident", src[i:j], line))
            i = j
        elif c.isdigit():
            j = i
            while j < n and (src[j].isalnum() or src[j] in "_."):
                if src[j] == "." and src.startswith("..", j):
                    break
                j += 1
            i = j
        else:
            out.append(("punct", c, line))
            i += 1
    return out


def drop_test_modules(toks):
    """Remove `#[cfg(test)] mod x { ... }` and `#[test] fn` bodies."""
    out = []
    i = 0
    n = len(toks)

    def is_(k, kind, text):
        return k < n and toks[k][0] == kind and toks[k][1] == text

    def skip_block(j):
        # j at the `{`
        depth = 0
        while j < n:
            if toks[j][0] == "punct" and toks[j][1] == "{":
                depth += 1
            elif toks[j][0] == "punct" and toks[j][1] == "}":
                depth -= 1
                if depth == 0:
                    return j + 1
            j += 1
        return n

    while i < n:
        if is_(i, "punct", "#") and is_(i + 1, "punct", "["):
            # attribute: find its end
            j = i + 2
            depth = 1
            while j < n and depth:
                if toks[j][1] == "[" and toks[j][0] == "punct":
                    depth += 1
                elif toks[j][1] == "]" and toks[j][0] == "punct":
                    depth -= 1
                j += 1
            attr = " ".join(t[1] for t in toks[i + 2 : j - 1])
            if attr in ("cfg ( test )", "test"):
                # skip the item that follows: up to its closing brace or `;`
                k = j
                while k < n and not (toks[k][0] == "punct" and toks[k][1] in "{;"):
                    k += 1
                if k < n and toks[k][1] == "{":
                    i = skip_block(k)
                else:
                    i = k + 1
                continue
            out.extend(toks[i:j])
            i = j
            continue
        out.append(toks[i])
        i += 1
    return out


# ---------------------------------------------------------------------------- word lists


def load_accents():
    exact, suffix, allow = {}, [], set()
    with open(os.path.join(ROOT, "tools", "i18n", "accents.txt"), encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("~"):
                suffix.append(line[1:])
            elif line.startswith("!"):
                allow.add(line[1:])
            elif "=" in line:
                a, b = line.split("=", 1)
                exact[a] = b
    return exact, suffix, allow


ACC_EXACT, ACC_SUFFIX, ACC_ALLOW = load_accents()


def words(s):
    return re.findall(r"[^\W\d_]+", s)


def fold(s):
    return "".join(c for c in unicodedata.normalize("NFD", s) if unicodedata.category(c) != "Mn")


def unaccented(s):
    out = []
    for w in words(s):
        if not w.isascii() or len(w) < 3:
            continue
        lw = w.lower()
        if lw in ACC_ALLOW:
            continue
        if lw in ACC_EXACT:
            out.append((w, ACC_EXACT[lw]))
        elif any(lw.endswith(sfx) and len(lw) > len(sfx) for sfx in ACC_SUFFIX):
            out.append((w, "(acento)"))
    return out


PT_WORDS = set(
    """de da do das dos para com sem um uma uns umas os as no na nos nas em por que não nao ou é
    arquivo arquivos pasta pastas erro erros aviso abrir fechar salvar nova novo novos novas copiar
    colar recortar desfazer refazer janela janelas rede disco memória memoria tempo ligado usuário
    endereço enviado recebido padrão sobre ajustes configurações cancelar reiniciar desligar
    confirmar renomear excluir mover lixeira vazia tudo todos todas nenhum nenhuma sua seu suas seus
    digite escolha clique mostrar buscar busca limpar aplicar ler lido nome tamanho data modificado
    modificação ultima última pelo pela ainda já mais menos aqui agora ontem hoje sistema tela
    fonte cor claro escuro automático automática tema papel parede teclado idioma região hora
    horas minuto segundo segundos dia mês ano você voce comando comandos encontrado encontrada
    inválido inválida permitido negado existe existem falhou falha sucesso concluído pronto aguarde
    carregando conectando conectado desconectado sem pasta diretório diretorio caminho imagem imagens
    página pagina voltar avançar recarregar favoritos histórico início inicio ajuda sair
    propriedades tipo local origem destino copiando movendo apagar apagado restaurar restaurado
    esvaziar abrir_ como para_ nesta neste esta este desta deste isso nada algum alguma quando onde
    porque então entao também tambem mas se ao aos à às""".split()
)
EN_WORDS = set(
    """the and or not file files folder folders error errors warning open close save new copy paste
    cut undo redo window windows network disk memory time used free total address sent received
    default about settings cancel restart shut down confirm rename delete move trash empty is are
    to for with from of in on at by no yes failed cannot could unable invalid unknown found command
    usage permission denied exists exist missing expected unexpected too long path name size type
    date modified created select all none loading connecting connected disconnected please wait done
    ready press enter type here click choose show hide clear apply read write this that these those
    it its you your an any some there was were been being have has had can will would should may
    might must out up over under again back forward reload bookmarks history home help exit quit
    properties source destination local remote page title text image images wallpaper keyboard
    language region hour minute second day month year clock theme light dark automatic color
    colour accent system screen font bad op ok""".split()
)
# English words that are also names or units and must not count as "English text" alone.
NEUTRAL_HINTS = set("ok no op os ip dns dhcp mac tcp udp http https html css url wasm png bmp ppm".split())


def classify(s):
    """Return (lang, pt_hits, en_hits) for a literal."""
    ws = [w.lower() for w in words(s)]
    pt = 0
    en = 0
    for w in ws:
        fw = fold(w)
        if w in PT_WORDS or fw in PT_WORDS or w in ACC_EXACT or any(ord(ch) > 127 and ch.isalpha() for ch in w):
            pt += 1
        elif w in EN_WORDS and w not in NEUTRAL_HINTS:
            en += 1
    if pt and en:
        return "misto", pt, en
    if pt:
        return "pt", pt, en
    if en:
        return "en", pt, en
    return "neutro", pt, en


# ---------------------------------------------------------------------------- scanning

# Calls whose string arguments are shown to the user.
VISIBLE_CALL = re.compile(
    r"^(draw\w*|\w*label|caption|tooltip|push_button|text_field|search_field|header|title|segmented|"
    r"say|item|section|row\w*|kv_row|message|notify|toast\w*|chip|badge|button|field|from|push_str|"
    r"message_box|status|hint|placeholder|ellipsize\w*|measure|wrap|centered|\w*_text|text|format|"
    r"write|writeln|checkbox|radio|menu\w*|entry|new|sub|tab\w*|set_title|ask\w*|confirm\w*|"
    r"say_\w+|banner|note|empty|heading|name)$"
)
# Calls whose arguments are logs, assertions or other non-UI text.
SKIP_CALL = re.compile(
    r"^(klog|println|print|eprintln|serial\w*|log\w*|panic|assert\w*|debug_assert\w*|unreachable|"
    r"todo|unimplemented|expect|trace\w*|perf\w*|dbg|include_str|include_bytes|concat|stringify|env|"
    r"option_env|cfg|derive|allow|deny|warn|forbid|doc|link_section|target_feature|"
    r"core_error|error_at|boot\w*|bench\w*|sink|probe|selftest|self_test|dump\w*)$"
)
MIGRATED_CALL = {"t", "tp", "tk", "tr", "tr_in", "tr_fmt", "tr_fmt_in", "plural", "plural_in", "plural_fmt", "plural_fmt_in"}


def call_context(toks):
    """For each token index of kind str, return the chain of enclosing call names (inner first)."""
    stack = []  # (open char, call name)
    ctx = {}
    for i, (k, t, ln) in enumerate(toks):
        if k == "punct" and t in "([{":
            name = ""
            if t == "(" and i > 0:
                p = toks[i - 1]
                if p[0] == "ident":
                    name = p[1]
                elif p[0] == "punct" and p[1] == "!" and i > 1 and toks[i - 2][0] == "ident":
                    name = toks[i - 2][1]
            elif t == "[" and i > 1 and toks[i - 1][0] == "punct" and toks[i - 1][1] == "!" and toks[i - 2][0] == "ident":
                name = toks[i - 2][1]
            elif t == "{" and i > 1 and toks[i - 1][0] == "punct" and toks[i - 1][1] == "!" and toks[i - 2][0] == "ident":
                name = toks[i - 2][1]
            stack.append((t, name))
        elif k == "punct" and t in ")]}":
            if stack:
                stack.pop()
        elif k == "str":
            ctx[i] = [n for (_, n) in reversed(stack) if n]
    return ctx


def looks_like_text(s):
    if len(s) < 2:
        return False
    if not re.search(r"[^\W\d_]{2,}", s):
        return False
    if s.startswith(("/", "http", "kitsune://", ".", "#", "<", "%", "$", "--")):
        return False
    if "{" in s and "}" in s and ":" in s and ";" in s:
        return False  # a style sheet: selectors such as `area` are tag names, not words
    if re.fullmatch(r"[A-Za-z0-9_.:/\-+*=<>!&|^~%@#\\]+", s) and " " not in s:
        # identifiers, paths, commands, keys: text only if capitalised words
        return bool(re.fullmatch(r"[A-ZÀ-Ý][a-zà-ÿ]{2,}(-[A-Za-z]+)?", s)) and not s.isupper()
    return True


def slug(s):
    f = fold(s).lower()
    ws = re.findall(r"[a-z0-9]+", f)
    ws = [w for w in ws if w not in ("de", "da", "do", "a", "o", "e", "um", "uma", "the", "of", "to")]
    return "_".join(ws[:4]) or "text"


def rs_files(base):
    out = []
    for d, _, fs in os.walk(os.path.join(ROOT, base)):
        for f in fs:
            if f.endswith(".rs"):
                out.append(os.path.relpath(os.path.join(d, f), ROOT).replace(os.sep, "/"))
    return sorted(out)


def scan_file(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
        src = f.read()
    toks = drop_test_modules(tokenize(src))
    ctx = call_context(toks)
    found = []
    migrated = 0
    for i, (k, t, ln) in enumerate(toks):
        if k != "str":
            continue
        chain = ctx.get(i, [])
        if any(c in MIGRATED_CALL for c in chain[:2]):
            migrated += 1
            continue
        if any(SKIP_CALL.match(c) for c in chain):
            continue
        s = t
        # attribute-like or format-only strings
        if not re.search(r"[^\W\d_]{2,}", re.sub(r"\{[^}]*\}", "", s)):
            continue
        near = chain[0] if chain else ""
        visible_call = bool(near and VISIBLE_CALL.match(near)) or any(VISIBLE_CALL.match(c) for c in chain[:3])
        if not looks_like_text(s):
            continue
        lang, pt, en = classify(s)
        sentence = " " in s.strip() or s[:1].isupper()
        if not (visible_call or (sentence and lang != "neutro")):
            continue
        # drop strings that are only format punctuation after removing placeholders
        found.append({"line": ln, "text": s, "lang": lang, "call": near, "acc": unaccented(s) if lang != "en" else []})
    return found, migrated


# ---------------------------------------------------------------------------- areas

AREAS = [
    # (area id, display name, owner note, regex over the path)
    ("shell", "Shell (migrado nesta onda)", "painel, barra de apps, Apps/Busca, diálogo de energia, banners",
     r"^kernel/src/desktop/(panel|taskbar|overlays|shell|chrome|toasts_ui|lang|glass)\.rs$|^kitsune_core/src/(launcher|chrome|taskbar|notify|sysif|snap|search)\.rs$"),
    ("settings", "Ajustes", "janela de Ajustes e o modelo de configurações",
     r"^kernel/src/desktop/settings_ui\.rs$|^kitsune_core/src/(settings|wallpaper|keymap)\.rs$|^kernel/src/settings\.rs$"),
    ("files", "Arquivos", "gerenciador de arquivos, lixeira, VFS",
     r"^kernel/src/desktop/(files|files_ui|vfs|sysstore)\.rs$|^kitsune_core/src/(fileman|vfs|fs)(\.rs|/)|^kitsune_core/src/fs3/|^kernel/src/storage\.rs$"),
    ("editor", "Editor", "editor de texto e diálogos",
     r"^kernel/src/desktop/(edit|edit_ui)\.rs$|^kitsune_core/src/editor2/"),
    ("terminal", "Terminal", "terminal, interpretador e comandos",
     r"^kernel/src/desktop/(term|shellhost)\.rs$|^kitsune_core/src/(shell|termui)(\.rs|/)"),
    ("tasks", "Tarefas", "monitor de atividade",
     r"^kernel/src/desktop/tarefas\.rs$|^kitsune_core/src/(activity|sysmon|netstats|process)\.rs$"),
    ("log", "Registro", "visualizador do registro",
     r"^kernel/src/desktop/logview\.rs$|^kitsune_core/src/klog\.rs$"),
    ("calc", "Calculadora", "calculadora",
     r"^kernel/src/desktop/calc_ui\.rs$|^kitsune_core/src/calc\.rs$"),
    ("viewer", "Imagens", "visualizador de imagens e decodificadores",
     r"^kernel/src/desktop/viewer\.rs$|^kitsune_core/src/(viewer|image|png|bmp|ppm|inflate|deflate|gzip)(\.rs|/)"),
    ("browser", "Navegador", "navegador, páginas internas, erros de rede e TLS (outro agente está editando)",
     r"^kernel/src/desktop/apps\.rs$|^kitsune_core/src/(browser|web|redirect|dns|net|icmp|lease|sntp|tlsverify|x509|appnet)(\.rs|/)|^kernel/src/(fetch|netd|netstack|tlsv)\.rs$"),
    ("apps", "Apps de terceiros (WASM)", "janela de app, manifesto, instalação, SDK",
     r"^kernel/src/desktop/(wasmwin|appart|appui)\.rs$|^kernel/src/wasm/|^kitsune_core/src/(appabi|appart|appfs|appinstall|appmanifest|wasmsec)(\.rs|/)"),
    ("kit", "Kit de componentes", "widgets, galeria e primitivas",
     r"^kernel/src/desktop/(kit|ui|widgets|gallery|cursor|input|render|live|instance|mod)\.rs$|^kitsune_core/src/(widgets|style|iconart|window|winman|wm)\.rs$"),
    ("system", "Sistema (logs e tela de falha: ficam em inglês)", "boot, falha grave, drivers",
     r"^kernel/src/(crash|main|boot|serial|klog|logd|power|rtc|sysinfo|trace|perf|sched|vm|allocator)\.rs$|^kernel/src/"),
]


def area_of(rel):
    for aid, name, note, rx in AREAS:
        if re.search(rx, rel):
            return aid
    return "other"


AREA_INFO = {a[0]: (a[1], a[2]) for a in AREAS}
AREA_INFO["other"] = ("Outros", "núcleo sem dono claro")


def render():
    files = []
    for base in SRC_DIRS:
        files += rs_files(base)
    per_file = {}
    migrated = {}
    for rel in files:
        if rel.endswith(("/tests.rs", "/test.rs")) or "/tests/" in rel:
            continue
        found, mig = scan_file(rel)
        if found:
            per_file[rel] = found
        if mig:
            migrated[rel] = mig
    by_area = defaultdict(list)
    for rel in per_file:
        by_area[area_of(rel)].append(rel)

    def counts(items):
        tot = len(items)
        return (
            tot,
            sum(1 for s in items if s["acc"]),
            sum(1 for s in items if s["lang"] == "en"),
            sum(1 for s in items if s["lang"] == "misto"),
        )

    L = []
    w = L.append
    w("# Auditoria de textos do Kitsune (i18n)")
    w("")
    w("Gerado por `python3 -I tools/i18n-audit.py` (não edite à mão; `--check` confere se está em dia).")
    w("Lista os literais de texto visíveis ao usuário em `kernel/src` e `kitsune_core/src`, por app, para que a")
    w("migração para o catálogo (`docs/design/i18n.md`) seja dividida **sem sobreposição de arquivos**.")
    w("")
    w("Como ler: *sem acento* = palavra em português que precisa de acento (lista em `tools/i18n/accents.txt`);")
    w("*inglês* = texto em inglês numa interface em português; *misto* = os dois idiomas no mesmo texto.")
    w("A contagem é heurística (veja o cabeçalho do script): ela erra para mais ou para menos em casos")
    w("de borda, mas cada linha abaixo tem arquivo e linha para conferir. Logs (`klog!`, serial) e a tela")
    w("de falha ficam em inglês de propósito e não entram na conta.")
    w("")
    tot = [0, 0, 0, 0]
    rows = []
    for aid in [a[0] for a in AREAS] + ["other"]:
        fs = by_area.get(aid, [])
        items = [s for f in fs for s in per_file[f]]
        t = counts(items)
        for k in range(4):
            tot[k] += t[k]
        mig = sum(migrated.get(f, 0) for f in files if area_of(f) == aid)
        rows.append((aid, fs, t, mig))
    w("## Resumo por app (ordem sugerida de migração)")
    w("")
    w("| App / área | Arquivos | Textos | Sem acento | Inglês | Misto | Já no catálogo |")
    w("|---|---:|---:|---:|---:|---:|---:|")
    for aid, fs, t, mig in rows:
        name = AREA_INFO[aid][0]
        w(f"| {name} | {len(fs)} | {t[0]} | {t[1]} | {t[2]} | {t[3]} | {mig} |")
    w(f"| **Total** | {sum(len(r[1]) for r in rows)} | {tot[0]} | {tot[1]} | {tot[2]} | {tot[3]} | {sum(r[3] for r in rows)} |")
    w("")
    w("Cada app só mexe nos arquivos da sua linha; os arquivos de `Kit de componentes` e do `Shell` já")
    w("foram tratados (Shell) ou só mudam se um app precisar de uma chave nova (use o prefixo do próprio app).")
    w("")
    w("## Arquivos por app")
    w("")
    for aid, fs, t, mig in rows:
        if not fs:
            continue
        name, note = AREA_INFO[aid]
        w(f"### {name}")
        w("")
        w(f"{note}.")
        w("")
        w("| Arquivo | Textos | Sem acento | Inglês | Misto |")
        w("|---|---:|---:|---:|---:|")
        for f in fs:
            c = counts(per_file[f])
            w(f"| `{f}` | {c[0]} | {c[1]} | {c[2]} | {c[3]} |")
        w("")
    w("## Textos sem acento (a corrigir ao migrar)")
    w("")
    w("| Arquivo:linha | Texto | Correção |")
    w("|---|---|---|")
    n = 0
    for rel in sorted(per_file):
        for s in per_file[rel]:
            if s["acc"]:
                fix = ", ".join(f"{a} → {b}" for a, b in s["acc"])
                w(f"| `{rel}:{s['line']}` | {md(s['text'])} | {fix} |")
                n += 1
    if n == 0:
        w("| (nenhum) | | |")
    w("")
    w("## Inglês e textos mistos numa interface em português")
    w("")
    w("| Arquivo:linha | Texto | Tipo |")
    w("|---|---|---|")
    n = 0
    for rel in sorted(per_file):
        for s in per_file[rel]:
            if s["lang"] in ("en", "misto"):
                w(f"| `{rel}:{s['line']}` | {md(s['text'])} | {s['lang']} |")
                n += 1
    if n == 0:
        w("| (nenhum) | | |")
    w("")
    w("## Todos os textos, por arquivo, com a chave proposta")
    w("")
    w("A chave é `<app>.<resumo>`; ao migrar, ajuste o resumo, mantenha o prefixo do app e")
    w("use `.one`/`.other` para plurais. Textos neutros (nomes próprios, unidades) também aparecem:")
    w("decida caso a caso se vão para o catálogo.")
    w("")
    prefix = {
        "shell": "shell", "settings": "settings", "files": "files", "editor": "editor",
        "terminal": "term", "tasks": "tasks", "log": "log", "calc": "calc", "viewer": "viewer",
        "browser": "web", "apps": "apps", "kit": "kit", "system": "sys", "other": "misc",
    }
    for aid, fs, t, mig in rows:
        if not fs:
            continue
        w(f"### {AREA_INFO[aid][0]}")
        w("")
        for f in fs:
            w(f"**`{f}`** ({len(per_file[f])})")
            w("")
            w("| Linha | Texto | Idioma | Chave proposta |")
            w("|---:|---|---|---|")
            seen = set()
            for s in per_file[f]:
                key = f"{prefix[aid]}.{slug(s['text'])}"
                base, k = key, 2
                while key in seen:
                    key = f"{base}_{k}"
                    k += 1
                seen.add(key)
                flag = s["lang"] + (" sem-acento" if s["acc"] else "")
                w(f"| {s['line']} | {md(s['text'])} | {flag} | `{key}` |")
            w("")
    return "\n".join(L) + "\n", rows, tot


def md(s):
    s = s.replace("\n", "\\n").replace("|", "\\|")
    if len(s) > 110:
        s = s[:107] + "..."
    return "`" + s.replace("`", "'") + "`"


def main():
    text, rows, tot = render()
    if "--summary" in sys.argv:
        for aid, fs, t, mig in rows:
            print(f"{AREA_INFO[aid][0]:50} files={len(fs):3} strings={t[0]:4} no_accent={t[1]:3} en={t[2]:3} mixed={t[3]:3}")
        print(f"{'TOTAL':50} strings={tot[0]} no_accent={tot[1]} en={tot[2]} mixed={tot[3]}")
        return 0
    if "--check" in sys.argv:
        try:
            with open(OUT, encoding="utf-8") as f:
                cur = f.read()
        except OSError:
            cur = ""
        if cur != text:
            print("docs/design/i18n-audit.md is out of date: run python3 -I tools/i18n-audit.py", file=sys.stderr)
            return 1
        return 0
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {os.path.relpath(OUT, ROOT)}: {tot[0]} strings, {tot[1]} without accents, {tot[2]} English, {tot[3]} mixed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
