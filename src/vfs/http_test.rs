//! Тесты HTTP-локации: разбор URL и парсинг HTML-автоиндекса.

use super::*;

#[test]
fn parse_url_variants() {
    let u = parse_url("http://host/a/b/").unwrap();
    assert_eq!(u.origin, "http://host");
    assert_eq!(u.path, "/a/b/");

    let u = parse_url("https://host:8080/x").unwrap();
    assert_eq!(u.origin, "https://host:8080");
    assert_eq!(u.path, "/x");

    let u = parse_url("http://host").unwrap();
    assert_eq!(u.path, "/"); // без пути → корень

    assert!(parse_url("ftp://x").is_none());
    assert!(parse_url("https://").is_none());
}

#[test]
fn autoindex_parses_links_and_skips_service() {
    let html = r#"
        <html><body>
        <a href="../">Parent Directory</a>
        <a href="sub/">sub/</a>
        <a href="file%20name.txt">file name.txt</a>
        <a href="readme">readme</a>
        <a href="?C=N;O=D">sort</a>
        <a href="http://external/">external</a>
        <a href="/abs">absolute</a>
        </body></html>
    "#;
    let items = parse_autoindex(html);
    let names: Vec<&str> = items.iter().map(|e| e.name.as_str()).collect();
    // служебные/внешние/абсолютные/родитель — отброшены
    assert_eq!(names, vec!["sub", "file name.txt", "readme"]);
    assert!(items[0].is_dir()); // sub/ — директория
    assert!(!items[1].is_dir()); // file — файл
    assert_eq!(items[1].name, "file name.txt"); // percent-decode %20
}

#[test]
fn autoindex_dedups() {
    let html = r#"<a href="x/">x</a><a href="x/">x again</a>"#;
    assert_eq!(parse_autoindex(html).len(), 1);
}
