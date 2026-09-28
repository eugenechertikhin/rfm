//! Юнит-тесты модуля `prettify` (в отдельном файле — по уставу).

use super::*;
use std::path::PathBuf;

#[test]
fn ext_detection() {
    assert_eq!(kind_by_ext(&PathBuf::from("a.JSON")), Some(Kind::Json));
    assert_eq!(kind_by_ext(&PathBuf::from("a.xml")), Some(Kind::Xml));
    assert_eq!(kind_by_ext(&PathBuf::from("a.txt")), None);
    assert_eq!(kind_by_ext(&PathBuf::from("noext")), None);
}

#[test]
fn json_basic() {
    let out = prettify_json(r#"{"a":1,"b":[2,3]}"#);
    assert_eq!(out, "{\n   \"a\": 1,\n   \"b\": [\n      2,\n      3\n   ]\n}");
}

#[test]
fn json_empty_containers_inline() {
    assert_eq!(prettify_json(r#"{"a":{},"b":[]}"#),
        "{\n   \"a\": {},\n   \"b\": []\n}");
}

#[test]
fn json_keeps_strings_with_specials() {
    // двоеточие/скобки/запятая внутри строки не трогаем
    let out = prettify_json(r#"{"url":"http://x/y","s":"a,b{c}"}"#);
    assert!(out.contains("\"http://x/y\""));
    assert!(out.contains("\"a,b{c}\""));
}

#[test]
fn xml_basic() {
    let out = prettify_xml("<root><a>1</a><b/></root>");
    assert_eq!(out, "<root>\n   <a>1</a>\n   <b/>\n</root>");
}

#[test]
fn xml_comment_with_gt_inside() {
    let out = prettify_xml("<r><!-- a > b --><x>1</x></r>");
    assert_eq!(out, "<r>\n   <!-- a > b -->\n   <x>1</x>\n</r>");
}
