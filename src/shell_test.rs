//! Юнит-тесты модуля `shell`

use super::*;

#[test]
fn parse_empty() {
    assert_eq!(parse("   "), ParsedCommand::Empty);
    assert_eq!(parse(""), ParsedCommand::Empty);
}

#[test]
fn parse_cd_variants() {
    assert_eq!(parse("cd"), ParsedCommand::Cd(String::new()));
    assert_eq!(parse("cd   "), ParsedCommand::Cd(String::new()));
    assert_eq!(parse("cd /tmp"), ParsedCommand::Cd("/tmp".to_string()));
    assert_eq!(parse("cd .."), ParsedCommand::Cd("..".to_string()));
    assert_eq!(
        parse("cd ~/some dir"),
        ParsedCommand::Cd("~/some dir".to_string())
    );
}

#[test]
fn parse_shell() {
    assert_eq!(parse("ls -la"), ParsedCommand::Shell("ls -la".to_string()));
    // `cdx` — не `cd`.
    assert_eq!(parse("cdx"), ParsedCommand::Shell("cdx".to_string()));
}

#[test]
fn parse_cd_unescapes_argument() {
    assert_eq!(parse("cd My\\ Dir"), ParsedCommand::Cd("My Dir".to_string()));
    assert_eq!(parse("cd 'My Dir'"), ParsedCommand::Cd("My Dir".to_string()));
    assert_eq!(parse("cd \"a \\$b\""), ParsedCommand::Cd("a $b".to_string()));
    assert_eq!(parse("cd a\\(1\\)"), ParsedCommand::Cd("a(1)".to_string()));
}

#[test]
fn escape_plain_name_unchanged() {
    assert_eq!(shell_escape("report.txt"), "report.txt");
    assert_eq!(shell_escape(".."), "..");
    assert_eq!(shell_escape("файл-1_2.txt"), "файл-1_2.txt");
    // `~`/`=` экранируются только в начале.
    assert_eq!(shell_escape("a~b=c"), "a~b=c");
}

#[test]
fn escape_special_chars_with_backslash() {
    assert_eq!(shell_escape("My Doc (1).txt"), "My\\ Doc\\ \\(1\\).txt");
    assert_eq!(shell_escape("it's"), "it\\'s");
    assert_eq!(shell_escape("a$b&c;d|e"), "a\\$b\\&c\\;d\\|e");
    assert_eq!(shell_escape("*?[x]"), "\\*\\?\\[x\\]");
    assert_eq!(shell_escape("~home"), "\\~home");
    assert_eq!(shell_escape("a\\b"), "a\\\\b");
}

#[test]
fn escape_control_chars_fall_back_to_quotes() {
    assert_eq!(shell_escape("a\nb"), "'a\nb'");
    assert_eq!(shell_escape(""), "''");
}

#[test]
fn escape_unescape_roundtrip() {
    for s in ["My Doc (1).txt", "it's", "a\\b\"c", "~x", "#tag", "a\nb", "plain"] {
        assert_eq!(shell_unescape(&shell_escape(s)), s, "roundtrip {s:?}");
    }
}

#[test]
fn quote_escapes_single_quotes() {
    assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    assert_eq!(shell_quote("./x y"), "'./x y'");
}

/// Один тест на всё поведение сигналов: обработчики процесс-глобальны, отдельные
/// тесты гонялись бы между собой при параллельном прогоне.
#[test]
fn sigint_ignored_by_parent_default_in_child_restored_on_drop() {
    // Родитель игнорирует SIGINT (как на время выполнения команды), ребёнок через
    // pre_exec получает дефолтный обработчик: `kill -INT $$` должен убить ребёнка,
    // не дав дойти до `exit 0` (без restore_child_signals он унаследовал бы игнор).
    let before = unsafe {
        let h = libc::signal(libc::SIGINT, libc::SIG_DFL); // прочитали текущее
        libc::signal(libc::SIGINT, h); // и вернули на место
        h
    };
    {
        let _guard = SignalGuard::ignore();
        let mut cmd = ProcessCommand::new("/bin/sh");
        cmd.arg("-c").arg("kill -INT $$; exit 0");
        restore_child_signals(&mut cmd);
        let status = cmd.status().unwrap();
        assert!(!status.success()); // завершился по сигналу, а не exit 0
    }
    // После Drop обработчик родителя вернулся к прежнему (не остался SIG_IGN).
    let after = unsafe {
        let h = libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGINT, h);
        h
    };
    assert_eq!(before, after);
}

#[test]
fn command_echo_has_separator_prompt_and_command() {
    assert_eq!(command_echo("user@box $ ", "ls -la"), "\r\nuser@box $ ls -la\r\n");
    assert_eq!(command_echo("root@box # ", "id"), "\r\nroot@box # id\r\n");
}

#[test]
fn command_echo_keeps_command_verbatim() {
    // Кавычки/спецсимволы — как будут выполнены (напр. запуск `+x` и sudo).
    assert_eq!(
        command_echo("u@h $ ", "'./my script.sh'"),
        "\r\nu@h $ './my script.sh'\r\n"
    );
}

#[test]
fn shell_name_is_basename() {
    assert_eq!(shell_name("/bin/zsh"), "zsh");
    assert_eq!(shell_name("/usr/local/bin/bash"), "bash");
    assert_eq!(shell_name("sh"), "sh");
    assert_eq!(shell_name("/opt/fish/"), "fish");
}
