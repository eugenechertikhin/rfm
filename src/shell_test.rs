//! Юнит-тесты модуля `shell` (в отдельном файле — по уставу).

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
