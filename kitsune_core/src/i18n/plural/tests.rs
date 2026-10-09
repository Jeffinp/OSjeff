use super::*;

#[test]
fn english_is_one_only_for_one() {
    let r = Rule::OneIsSingular;
    assert_eq!(r.category(0), Category::Other);
    assert_eq!(r.category(1), Category::One);
    assert_eq!(r.category(2), Category::Other);
    assert_eq!(r.category(21), Category::Other);
    assert_eq!(r.category(u64::MAX), Category::Other);
}

#[test]
fn portuguese_treats_zero_and_one_as_singular() {
    let r = Rule::ZeroAndOneSingular;
    assert_eq!(r.category(0), Category::One);
    assert_eq!(r.category(1), Category::One);
    assert_eq!(r.category(2), Category::Other);
    assert_eq!(r.category(100), Category::Other);
    assert_eq!(r.category(u64::MAX), Category::Other);
}

#[test]
fn invariant_is_always_other() {
    for n in [0, 1, 2, 11, 1000] {
        assert_eq!(Rule::Invariant.category(n), Category::Other);
    }
}

#[test]
fn rule_names() {
    assert_eq!(Rule::from_name("zero-one"), Rule::ZeroAndOneSingular);
    assert_eq!(Rule::from_name(" invariant "), Rule::Invariant);
    assert_eq!(Rule::from_name("one"), Rule::OneIsSingular);
    assert_eq!(Rule::from_name(""), Rule::OneIsSingular);
    assert_eq!(Category::One.suffix(), "one");
    assert_eq!(Category::Other.suffix(), "other");
}
