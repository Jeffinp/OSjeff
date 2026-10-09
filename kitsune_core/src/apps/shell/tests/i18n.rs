//! The shell's messages follow the language in effect at execution time, and nothing a script
//! may parse changes with it.
//!
//! **Stable in every language** (scripts may rely on them): command names and flags, exit
//! statuses, the data a command prints (`echo`, `cat`, `seq`, `wc`, `sort`, `head`, `tail`,
//! `cut`, `tr`, `date`, `env`, `export`, `alias`, `history`, `basename`, `dirname`, `ls`
//! names and the `-l` columns, the `Mem:` row of `free`, the numbers and `%` of `df`, the
//! process states of `ps`), and the `PING host` line. **Translated**: error and usage
//! messages (`sh: ...`, `ls: cannot access ...`), `help`, and the headings and labels of
//! `df`, `free`, `ps`, `ping`, `ifconfig`, `uptime`, `nslookup`, `which`, `stat`.

use super::*;

/// Run `lines` once in each language, returning the output in `(pt, en)` order.
fn both(lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let mut res = (Vec::new(), Vec::new());
    for (l, dst) in [(Lang::Pt, &mut res.0), (Lang::En, &mut res.1)] {
        let mut t = T::with_files();
        t.lang(l);
        for line in lines {
            dst.push(t.out(line));
        }
    }
    res
}

#[test]
fn errors_follow_the_language() {
    let (pt, en) = both(&["ls /nonexistent", "foo", "cat /nope", "cd /nope", "rm"]);
    assert_eq!(
        pt[0],
        "ls: não foi possível acessar '/nonexistent': Arquivo ou diretório inexistente\n"
    );
    assert_eq!(
        en[0],
        "ls: cannot access '/nonexistent': No such file or directory\n"
    );
    assert_eq!(pt[1], "sh: foo: comando não encontrado\n");
    assert_eq!(en[1], "sh: foo: command not found\n");
    assert_eq!(pt[2], "cat: /nope: Arquivo ou diretório inexistente\n");
    assert_eq!(en[2], "cat: /nope: No such file or directory\n");
    assert_eq!(pt[3], "cd: /nope: Arquivo ou diretório inexistente\n");
    assert_eq!(pt[4], "rm: operando ausente\n");
    assert_eq!(en[4], "rm: missing operand\n");
}

#[test]
fn the_language_can_change_between_commands() {
    let mut t = T::with_files();
    t.lang(Lang::Pt);
    assert_eq!(t.out("foo"), "sh: foo: comando não encontrado\n");
    t.lang(Lang::En);
    assert_eq!(t.out("foo"), "sh: foo: command not found\n");
    t.lang(Lang::Pt);
    // One parse error, one runtime message: both follow the language of the moment.
    assert!(
        t.out("echo 'x")
            .contains("erro de sintaxe na linha 1, coluna")
    );
    t.lang(Lang::En);
    assert!(t.out("echo 'x").contains("syntax error at line 1, column"));
}

#[test]
fn usage_lines_and_help_are_translated() {
    let (pt, en) = both(&["mv a", "grep", "help ls", "help nosuch", "help"]);
    assert_eq!(pt[0], "mv: uso: mv ORIGEM... DESTINO\n");
    assert_eq!(en[0], "mv: usage: mv SRC... DEST\n");
    assert_eq!(pt[1], "grep: uso: grep [OPÇÕES] PADRÃO [ARQUIVO...]\n");
    assert_eq!(
        pt[2],
        "ls [-aF1l] [CAMINHO...]: lista o conteúdo de diretórios\n"
    );
    assert_eq!(en[2], "ls [-aF1l] [PATH...]: list directory contents\n");
    assert_eq!(pt[3], "help: sem ajuda para 'nosuch'\n");
    assert_eq!(en[3], "help: no help for 'nosuch'\n");
    assert!(pt[4].starts_with("Comandos (help NOME mostra um):\n"));
    assert!(pt[4].contains("\n  help [COMANDO]: lista os comandos ou descreve um\n"));
    assert!(en[4].starts_with("Commands (help NAME for one):\n"));
    assert!(en[4].contains("\n  help [CMD]: list commands or describe one\n"));
}

#[test]
fn every_command_has_help_in_both_languages() {
    let sh = Shell::new();
    for l in Lang::ALL {
        let _g = LangGuard::new(l);
        let list = sh.registry().list();
        assert!(list.len() > 55, "{} commands", list.len());
        for (name, help) in list {
            // A missing key would show up as the key itself.
            assert!(!help.starts_with("sh."), "{l:?} {name}: {help}");
            assert!(
                help.starts_with(name.as_str()) || name == "[",
                "{name}: {help}"
            );
            assert!(help.contains(": "), "{name}: {help}");
        }
    }
}

#[test]
fn headings_of_system_commands_are_translated_and_the_data_is_not() {
    let (pt, en) = both(&["df", "free", "ps", "uptime", "ping -c 2 10.0.2.2"]);
    // df: same numbers, translated column titles (the columns stay aligned).
    let words = |s: &str| s.lines().next().unwrap().split_whitespace().count();
    assert!(pt[0].starts_with("Sist. arq. "), "{}", pt[0]);
    assert!(
        pt[0]
            .lines()
            .next()
            .unwrap()
            .ends_with("Usado     Livre  Uso%  Montado em")
    );
    assert!(en[0].starts_with("Filesystem "), "{}", en[0]);
    assert!(
        en[0]
            .lines()
            .next()
            .unwrap()
            .ends_with("Used     Avail  Use%  Mounted on")
    );
    assert_eq!(
        words(&pt[0]),
        words(&en[0]) + 1,
        "\"Sist. arq.\" is two words"
    );
    assert_eq!(
        pt[0].lines().nth(1),
        en[0].lines().nth(1),
        "the row is data"
    );
    // free: the `Mem:` row is identical.
    let head = |s: &str| {
        s.lines()
            .next()
            .unwrap()
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(head(&pt[1]), ["KiB", "total", "usado", "livre"]);
    assert_eq!(head(&en[1]), ["KiB", "total", "used", "free"]);
    let row = |s: &str| s.lines().nth(1).unwrap().to_string();
    assert_eq!(row(&pt[1]), row(&en[1]));
    // ps: process states are data.
    assert_eq!(head(&pt[2]), ["PID", "ESTADO", "MEM(K)", "NOME"]);
    assert_eq!(head(&en[2]), ["PID", "STATE", "MEM(K)", "NAME"]);
    assert_eq!(
        pt[2].lines().nth(1),
        en[2].lines().nth(1),
        "process rows do not change"
    );
    // uptime: the days follow the plural rule of the language.
    assert_eq!(pt[3], "ativo há 1 dia, 02:03:04\n");
    assert_eq!(en[3], "up 1 day, 02:03:04\n");
    // ping: the first line is stable, the summary is translated, the figures keep the
    // language's decimal separator.
    assert!(pt[4].starts_with("PING 10.0.2.2\npacotes transmitidos: 2, recebidos: 2, perda: 0%\n"));
    assert!(
        en[4].starts_with("PING 10.0.2.2\n2 packets transmitted, 2 received, 0% packet loss\n")
    );
}

#[test]
fn plurals_in_uptime_and_ping() {
    let mut t = T::new();
    t.sys.uptime = 2 * 86_400 * 1000 + 5_000;
    assert_eq!(t.out("uptime"), "up 2 days, 00:00:05\n");
    t.lang(Lang::Pt);
    assert_eq!(t.out("uptime"), "ativo há 2 dias, 00:00:05\n");
    t.sys.uptime = 3_661_000;
    assert_eq!(t.out("uptime"), "ativo há 01:01:01\n");
    assert!(
        t.out("ping -c 1 h")
            .contains("pacotes transmitidos: 1, recebidos: 1")
    );
    t.lang(Lang::En);
    assert!(
        t.out("ping -c 1 h")
            .contains("1 packet transmitted, 1 received")
    );
}

#[test]
fn script_visible_output_is_the_same_in_both_languages() {
    let script = "\
for i in 1 2 3; do echo \"n=$i\"; done
seq 1 3 | wc -l
echo a b c | cut -d ' ' -f 2
date +%Y-%m-%d
printf_free=$(free -m | tail -n 1 | cut -d ' ' -f 1)
echo \"$printf_free\"
x=$(ls /d)
echo \"$x\"
ls -l /d
cat /a.txt | sort -r | head -n 1
env | sort | head -n 3
history 2
test -d /d && echo dir
false || echo \"status=$?\"
echo '$((1+2))' $((1+2))
";
    let run = |l| {
        let mut t = T::with_files();
        t.lang(l);
        let r = t.script(script, &[]);
        (r.status, r.text())
    };
    assert_eq!(run(Lang::Pt), run(Lang::En));
}

#[test]
fn exit_statuses_do_not_depend_on_the_language() {
    let lines = [
        "ls /nonexistent",
        "foo",
        "cat /nope",
        "mkdir",
        "kill abc",
        "test 1 -eq x",
        "sleep nope",
        "seq",
        "grep '('x /a.txt",
    ];
    let mut by_lang = Vec::new();
    for l in Lang::ALL {
        let mut t = T::with_files();
        t.lang(l);
        by_lang.push(lines.iter().map(|ln| t.run(ln).status).collect::<Vec<_>>());
    }
    assert_eq!(by_lang[0], by_lang[1]);
    assert!(by_lang[0].iter().all(|&s| s != 0), "{:?}", by_lang[0]);
}

#[test]
fn loops_limits_and_syntax_errors_speak_the_current_language() {
    let (pt, en) = both(&[
        "while true; do :; done",
        "echo $(",
        "x &",
        "cp /a.txt",
        "touch",
    ]);
    assert!(
        pt[0].contains("limite de iterações do laço atingido"),
        "{}",
        pt[0]
    );
    assert!(en[0].contains("loop iteration limit reached"), "{}", en[0]);
    assert!(
        pt[1].contains("substituição $( ) sem fechamento"),
        "{}",
        pt[1]
    );
    assert!(pt[2].contains("tarefas em segundo plano"), "{}", pt[2]);
    assert!(en[2].contains("background jobs"), "{}", en[2]);
    assert_eq!(pt[3], "cp: uso: cp [-r] ORIGEM... DESTINO\n");
    assert_eq!(pt[4], "touch: operando de arquivo ausente\n");
}

#[test]
fn network_commands_are_translated() {
    let mut t = T::new();
    t.sys.net = Some(crate::apps::shell::sys::NetInfo {
        nic: "ne2000".into(),
        link_up: true,
        ip: Some("10.0.2.15".into()),
        prefix: 24,
        gateway: Some("10.0.2.2".into()),
        dns: alloc::vec!["10.0.2.3".into()],
        dhcp: "bound".into(),
        rx_packets: 3,
        rx_bytes: 2048,
        tx_packets: 1,
        tx_bytes: 100,
    });
    t.sys.dns.push(("a.org".into(), [1, 2, 3, 4]));
    t.lang(Lang::Pt);
    let s = t.out("ifconfig");
    assert!(s.contains("eth0: ne2000  link ativo"), "{s}");
    assert!(s.contains("  RX 3 pacotes, 2048 bytes (2,0 KiB)"), "{s}");
    assert!(s.contains("  TX 1 pacotes, 100 bytes (100 B)"), "{s}");
    assert_eq!(
        t.out("nslookup a.org"),
        "Nome:     a.org\nEndereço: 1.2.3.4\n"
    );
    assert!(
        t.out("nslookup nope.invalid")
            .contains("não encontrei nope.invalid: NXDOMAIN")
    );
    assert!(t.out("curl ftp://x/").contains("protocolo não suportado"));
    t.lang(Lang::En);
    assert_eq!(
        t.out("nslookup a.org"),
        "Name:    a.org\nAddress: 1.2.3.4\n"
    );
}

#[test]
fn scrollback_text_is_not_retranslated() {
    // The output of a finished command is bytes: switching the language later cannot change it.
    let mut t = T::with_files();
    t.lang(Lang::Pt);
    let r = t.run("foo");
    t.lang(Lang::En);
    assert_eq!(r.text(), "sh: foo: comando não encontrado\n");
}
