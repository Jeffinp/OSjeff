use super::*;

fn ok(src: &str) -> Vec<Stmt> {
    parse(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"))
}

fn err(src: &str) -> ParseError {
    parse(src).expect_err(src)
}

fn simple(src: &str) -> Simple {
    match &ok(src)[0].and_or.first.cmds[0] {
        Command::Simple(s) => s.clone(),
        other => panic!("not simple: {other:?}"),
    }
}

fn lit(w: &Word) -> String {
    w.iter()
        .map(|p| match p {
            Part::Lit { text, .. } => text.clone(),
            _ => "?".to_string(),
        })
        .collect()
}

#[test]
fn empty_input_is_empty_list() {
    assert!(ok("").is_empty());
    assert!(ok("   \n\n  ").is_empty());
    assert!(ok("# just a comment").is_empty());
}

#[test]
fn simple_words() {
    let s = simple("echo hello  world");
    let w: Vec<String> = s.words.iter().map(lit).collect();
    assert_eq!(w, ["echo", "hello", "world"]);
}

#[test]
fn single_quotes_are_literal() {
    let s = simple("echo 'a $b * c'");
    assert_eq!(
        s.words[1],
        [Part::Lit {
            text: "a $b * c".into(),
            quoted: true
        }]
    );
}

#[test]
fn double_quotes_keep_variables() {
    let s = simple("echo \"x $V y\"");
    let w = &s.words[1];
    assert_eq!(w.len(), 3);
    assert!(matches!(&w[1], Part::Var { name, quoted: true } if name == "V"));
}

#[test]
fn escapes() {
    // echo a\ b "q\"r" \$X
    let s = simple("echo a\\ b \"q\\\"r\" \\$X");
    assert_eq!(s.words.len(), 4);
    assert_eq!(lit(&s.words[1]), "a b");
    assert_eq!(lit(&s.words[2]), "q\"r");
    assert_eq!(lit(&s.words[3]), "$X");
}

#[test]
fn backslash_in_double_quotes_only_escapes_specials() {
    let s = simple("echo \"a\\nb\"");
    assert_eq!(lit(&s.words[1]), "a\\nb");
}

#[test]
fn variables_forms() {
    let s = simple("echo $A ${B} $? $1 $# $@ $*");
    assert!(matches!(&s.words[1][0], Part::Var { name, .. } if name == "A"));
    assert!(matches!(&s.words[2][0], Part::Var { name, .. } if name == "B"));
    assert!(matches!(&s.words[3][0], Part::Status { .. }));
    assert!(matches!(&s.words[4][0], Part::Var { name, .. } if name == "1"));
    assert!(matches!(&s.words[5][0], Part::Count { .. }));
    assert!(matches!(&s.words[6][0], Part::Args { at: true, .. }));
    assert!(matches!(&s.words[7][0], Part::Args { at: false, .. }));
}

#[test]
fn lone_dollar_is_literal() {
    let s = simple("echo $ a$ $-");
    assert_eq!(lit(&s.words[1]), "$");
    assert_eq!(lit(&s.words[2]), "a$");
}

#[test]
fn command_substitution_parses_inner() {
    let s = simple("echo $(echo hi | wc -c)");
    match &s.words[1][0] {
        Part::Cmd { body, .. } => assert_eq!(body[0].and_or.first.cmds.len(), 2),
        p => panic!("{p:?}"),
    }
}

#[test]
fn nested_substitution_with_quotes_and_parens() {
    let s = simple("echo \"$(echo \")\" $(echo 'a)b'))\"");
    assert!(matches!(&s.words[1][0], Part::Cmd { .. }));
}

#[test]
fn arithmetic_expansion() {
    let s = simple("echo $((1 + $X * 2))");
    match &s.words[1][0] {
        Part::Arith { expr, .. } => {
            assert!(expr.iter().any(|p| matches!(p, Part::Var { .. })));
        }
        p => panic!("{p:?}"),
    }
}

#[test]
fn tilde_expands_to_home_only_at_word_start() {
    let s = simple("cd ~ ~/x a~b");
    assert!(matches!(&s.words[1][0], Part::Var { name, .. } if name == "HOME"));
    assert!(matches!(&s.words[2][0], Part::Var { name, .. } if name == "HOME"));
    assert_eq!(lit(&s.words[3]), "a~b");
}

#[test]
fn pipelines_and_lists() {
    let l = ok("a | b | c; d && e || f\ng");
    assert_eq!(l.len(), 3);
    assert_eq!(l[0].and_or.first.cmds.len(), 3);
    assert_eq!(l[1].and_or.rest.len(), 2);
    assert_eq!(l[1].and_or.rest[0].0, Connector::And);
    assert_eq!(l[1].and_or.rest[1].0, Connector::Or);
}

#[test]
fn redirections() {
    let s = simple("cat < in > out >> log");
    assert_eq!(s.redirs.len(), 3);
    assert_eq!(s.redirs[0].kind, RedirKind::In);
    assert_eq!(s.redirs[1].kind, RedirKind::Out);
    assert_eq!(s.redirs[2].kind, RedirKind::Append);
}

#[test]
fn redirect_before_command_word() {
    let s = simple("> out echo hi");
    assert_eq!(s.words.len(), 2);
    assert_eq!(s.redirs.len(), 1);
}

#[test]
fn assignments() {
    let s = simple("A=1 B=\"x y\" cmd arg");
    assert_eq!(s.assigns.len(), 2);
    assert_eq!(s.assigns[0].0, "A");
    assert_eq!(s.words.len(), 2);
    let only = simple("X=5");
    assert!(only.words.is_empty());
    assert_eq!(only.assigns.len(), 1);
}

#[test]
fn equals_after_command_is_an_argument() {
    let s = simple("echo A=1");
    assert!(s.assigns.is_empty());
    assert_eq!(lit(&s.words[1]), "A=1");
}

#[test]
fn comments_end_at_newline() {
    let l = ok("echo a # comment ; echo b\necho c");
    assert_eq!(l.len(), 2);
    let s = simple("echo a#b");
    assert_eq!(lit(&s.words[1]), "a#b");
}

#[test]
fn if_forms() {
    let l = ok("if a; then b; elif c; then d; else e; fi");
    match &l[0].and_or.first.cmds[0] {
        Command::If { arms, else_body } => {
            assert_eq!(arms.len(), 2);
            assert!(else_body.is_some());
        }
        c => panic!("{c:?}"),
    }
    ok("if a\nthen\n b\nfi");
}

#[test]
fn for_and_while() {
    let l = ok("for x in a b c; do echo $x; done");
    match &l[0].and_or.first.cmds[0] {
        Command::For { var, words, body } => {
            assert_eq!(var, "x");
            assert_eq!(words.len(), 3);
            assert_eq!(body.len(), 1);
        }
        c => panic!("{c:?}"),
    }
    ok("while true; do break; done");
    ok("until false; do break; done");
    ok("for x; do echo $x; done");
    ok("for x in; do echo; done");
}

#[test]
fn functions() {
    let l = ok("greet() { echo hi; }\nfunction bye { echo bye; }\nfunction f() { :; }");
    assert_eq!(l.len(), 3);
    assert!(matches!(&l[0].and_or.first.cmds[0], Command::Func { name, .. } if name == "greet"));
}

#[test]
fn groups() {
    let l = ok("{ echo a; echo b; } | wc -l");
    assert_eq!(l[0].and_or.first.cmds.len(), 2);
    assert!(matches!(&l[0].and_or.first.cmds[0], Command::Group(b) if b.len() == 2));
}

#[test]
fn keywords_are_plain_words_in_arguments() {
    let s = simple("echo if then fi done");
    assert_eq!(s.words.len(), 5);
    let s = simple("echo {}");
    assert_eq!(lit(&s.words[1]), "{}");
}

#[test]
fn quoted_keyword_is_a_command_name() {
    let s = simple("'if' x");
    assert_eq!(lit(&s.words[0]), "if");
}

#[test]
fn error_unterminated_quotes() {
    assert_eq!(
        err("echo 'abc").kind,
        ParseErrorKind::UnterminatedSingleQuote
    );
    assert_eq!(err("echo 'abc").pos, 5);
    assert_eq!(
        err("echo \"abc").kind,
        ParseErrorKind::UnterminatedDoubleQuote
    );
    assert_eq!(
        err("echo $(abc").kind,
        ParseErrorKind::UnterminatedSubstitution
    );
    assert_eq!(
        err("echo $((1+2)").kind,
        ParseErrorKind::UnterminatedSubstitution
    );
}

#[test]
fn error_background_job() {
    let e = err("sleep 5 &");
    assert_eq!(e.kind, ParseErrorKind::BackgroundNotSupported);
    assert_eq!(e.pos, 8);
    let _lang = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
    assert!(e.message().contains("background"));
    let _pt = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert!(e.message().contains("segundo plano"));
}

#[test]
fn error_unsupported_redirections() {
    assert_eq!(
        err("cmd 2>err").kind,
        ParseErrorKind::FdRedirectNotSupported
    );
    assert_eq!(err("cmd >&2").kind, ParseErrorKind::FdRedirectNotSupported);
    assert_eq!(err("cat <<EOF").kind, ParseErrorKind::HeredocNotSupported);
    assert_eq!(err("cmd &> f").kind, ParseErrorKind::BackgroundNotSupported);
}

#[test]
fn number_arguments_are_fine() {
    let s = simple("echo 2 3 > out");
    assert_eq!(s.words.len(), 3);
}

#[test]
fn error_missing_redirect_target() {
    assert_eq!(err("echo >").kind, ParseErrorKind::MissingRedirectTarget);
    assert_eq!(err("echo > ;").kind, ParseErrorKind::MissingRedirectTarget);
}

#[test]
fn error_dangling_operators() {
    assert_eq!(err("echo a &&").kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err("echo a |").kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(err("| echo").kind, ParseErrorKind::UnexpectedToken);
    assert_eq!(err("&& echo").kind, ParseErrorKind::UnexpectedToken);
    assert_eq!(err(")").kind, ParseErrorKind::UnexpectedToken);
}

#[test]
fn error_unclosed_constructs() {
    assert_eq!(
        err("if true; then echo").kind,
        ParseErrorKind::Expected("fi")
    );
    assert_eq!(
        err("for x in a; do echo").kind,
        ParseErrorKind::Expected("done")
    );
    assert_eq!(err("while true; do").kind, ParseErrorKind::Expected("done"));
    assert_eq!(err("if true echo").kind, ParseErrorKind::Expected("then"));
    assert_eq!(err("f() { echo").kind, ParseErrorKind::Expected("}"));
    assert_eq!(err("{ echo").kind, ParseErrorKind::Expected("}"));
}

#[test]
fn error_stray_closing_keywords() {
    assert_eq!(err("fi").kind, ParseErrorKind::UnexpectedToken);
    assert_eq!(err("done").kind, ParseErrorKind::UnexpectedToken);
    assert_eq!(err("echo a; }").kind, ParseErrorKind::UnexpectedToken);
    assert_eq!(err("then").kind, ParseErrorKind::UnexpectedToken);
}

#[test]
fn error_bad_function_and_for() {
    assert_eq!(err("1bad() { :; }").kind, ParseErrorKind::BadFunctionName);
    assert_eq!(
        err("function 1x { :; }").kind,
        ParseErrorKind::BadFunctionName
    );
    assert_eq!(
        err("for 1x in a; do :; done").kind,
        ParseErrorKind::UnexpectedToken
    );
}

#[test]
fn error_bad_brace_substitution() {
    assert_eq!(err("echo ${}").kind, ParseErrorKind::BadSubstitution);
    assert_eq!(err("echo ${a-b}").kind, ParseErrorKind::BadSubstitution);
    assert_eq!(err("echo ${abc").kind, ParseErrorKind::BadSubstitution);
}

#[test]
fn error_depth_limit() {
    let mut s = String::new();
    for _ in 0..40 {
        s.push_str("if true; then ");
    }
    s.push_str("echo");
    for _ in 0..40 {
        s.push_str("; fi");
    }
    assert_eq!(err(&s).kind, ParseErrorKind::TooDeep);
    let mut c = String::from("echo ");
    for _ in 0..40 {
        c.push_str("$(echo ");
    }
    for _ in 0..40 {
        c.push(')');
    }
    assert_eq!(err(&c).kind, ParseErrorKind::TooDeep);
}

#[test]
fn error_positions_inside_substitution_are_absolute() {
    let e = err("echo $(echo 'x)");
    assert_eq!(e.kind, ParseErrorKind::UnterminatedSubstitution);
    let e = err("echo $(a &)");
    assert_eq!(e.kind, ParseErrorKind::BackgroundNotSupported);
    assert_eq!(e.pos, 9);
}

#[test]
fn line_and_column() {
    let src = "echo a\necho 'b";
    let e = err(src);
    assert_eq!(e.line_col(src), (2, 6));
}

#[test]
fn multiline_script() {
    let l = ok("A=1\nif [ $A = 1 ]; then\n  echo yes\nfi\n\n# end\n");
    assert_eq!(l.len(), 2);
}

#[test]
fn line_continuation() {
    let s = simple("echo a\\\nb");
    assert_eq!(lit(&s.words[1]), "ab");
}

#[test]
fn unicode_input_is_fine() {
    let s = simple("echo 'ação' é€");
    assert_eq!(s.words.len(), 3);
    let e = err("echo é 'x");
    assert_eq!(e.kind, ParseErrorKind::UnterminatedSingleQuote);
    assert_eq!(e.pos, "echo é ".len());
}

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

#[test]
fn never_panics_on_token_soup() {
    let alphabet: &[u8] = b"abc $(){}[]'\"\\|&;<>#~*?=\n\t12${ifthenfidodonefor";
    let mut x: u64 = 0x1234_5678_9abc_def1;
    for _ in 0..4000 {
        let mut s = String::new();
        let len = (xorshift(&mut x) % 40) as usize;
        for _ in 0..len {
            let k = (xorshift(&mut x) % alphabet.len() as u64) as usize;
            s.push(alphabet[k] as char);
        }
        let _ = parse(&s);
    }
}

#[test]
fn never_panics_on_arbitrary_bytes() {
    let mut x: u64 = 99;
    for _ in 0..3000 {
        let mut bytes = Vec::new();
        for _ in 0..(xorshift(&mut x) % 60) {
            bytes.push((xorshift(&mut x) >> 5) as u8);
        }
        let s = String::from_utf8_lossy(&bytes).into_owned();
        let _ = parse(&s);
    }
}

#[test]
fn never_panics_on_every_prefix_of_a_complex_script() {
    let script = "f() { for x in a*b $(ls | head -n 2) \"$@\"; do if [ $x = 1 ] && echo ${x} > out; then echo $((1+2)); fi; done; }\nwhile true; do break; done # end\n";
    for end in 0..=script.len() {
        if script.is_char_boundary(end) {
            let _ = parse(&script[..end]);
        }
    }
}
