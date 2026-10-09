use super::*;

#[test]
fn ranks_prefix_then_word_then_substring() {
    assert_eq!(rank("ter", "Terminal"), Some(0));
    assert_eq!(rank("tar", "Gerenciador de Tarefas"), Some(1));
    assert_eq!(rank("nal", "Terminal"), Some(2));
    assert_eq!(rank("xyz", "Terminal"), None);
    assert_eq!(rank("", "anything"), Some(2));
    // Case and accents do not matter.
    assert_eq!(rank("CONFIG", "Configurações"), Some(0));
    assert_eq!(rank("configuracoes", "Configurações"), Some(0));
    assert_eq!(rank("imagens", "Álbum de Imagens"), Some(1));
    assert_eq!(rank("calculadora", "Calculadora"), Some(0));
}

#[test]
fn evaluates_arithmetic_exactly() {
    assert_eq!(eval("1+2").as_deref(), Some("3"));
    assert_eq!(eval("2 * (3 + 4)").as_deref(), Some("14"));
    assert_eq!(eval("10 / 4").as_deref(), Some("2,5"));
    assert_eq!(eval("1/3").as_deref(), Some("0,333333"));
    assert_eq!(eval("0.1 + 0.2").as_deref(), Some("0,3"));
    assert_eq!(eval("0,5*4").as_deref(), Some("2"));
    assert_eq!(eval("-5 + 2").as_deref(), Some("-3"));
    assert_eq!(eval("7 % 3").as_deref(), Some("1"));
    assert_eq!(eval("2x3").as_deref(), Some("6"));
    assert_eq!(eval("--4+1").as_deref(), Some("5"));
    assert_eq!(eval("100-100").as_deref(), Some("0"));
    assert_eq!(eval("1.5 - 3").as_deref(), Some("-1,5"));
    assert_eq!(eval("123456789 * 1000").as_deref(), Some("123456789000"));
    // Precedence and associativity.
    assert_eq!(eval("2+3*4").as_deref(), Some("14"));
    assert_eq!(eval("20-5-3").as_deref(), Some("12"));
    assert_eq!(eval("100/10/5").as_deref(), Some("2"));
}

#[test]
fn ordinary_searches_are_not_calculations() {
    for s in [
        "terminal", "42", "", "   ", "1 2", "a+b", "(", ")", "1+", "*3", "2..3+1",
    ] {
        assert_eq!(eval(s), None, "{s:?}");
    }
    assert_eq!(eval("1/0"), None);
    assert_eq!(eval("5 % 0"), None);
    assert_eq!(eval("((((((((((((((((((1))))))))))))))))))+1"), None); // too deep
}

#[test]
fn overflow_is_refused_not_wrapped() {
    assert_eq!(eval("999999999999999 * 999999999999999"), None);
    assert_eq!(eval("99999999999999999999999999999999999999999 + 1"), None);
    assert!(eval("999999 * 999999").is_some());
}

#[test]
fn every_short_string_parses_without_panic() {
    let alphabet = b"0123456789+-*/%(). ,xX";
    let mut buf = [0u8; 5];
    fn walk(d: usize, buf: &mut [u8; 5], a: &[u8]) {
        if d == buf.len() {
            if let Ok(s) = core::str::from_utf8(buf) {
                let _ = eval(s);
            }
            return;
        }
        for &c in a {
            buf[d] = c;
            walk(d + 1, buf, a);
        }
    }
    walk(0, &mut buf, alphabet);
}
