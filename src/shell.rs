//! Слой работы с командами shell: разбор командной строки, перехват `cd`,
//! выполнение команды с сохранением экрана shell через alternate screen.

use std::env;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command as ProcessCommand;

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::Backend, Terminal};

use crate::config::PauseMode;

/// Результат разбора строки командной строки.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedCommand {
    Empty,
    /// `cd` с аргументом (пустой аргумент = домашняя директория).
    Cd(String),
    /// Внешняя команда для `$SHELL -c`.
    Shell(String),
}

/// Разбирает строку: `cd ...` перехватывается, всё остальное — внешняя команда.
pub fn parse(line: &str) -> ParsedCommand {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedCommand::Empty;
    }
    let mut it = trimmed.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("");
    if first == "cd" {
        let arg = it.next().unwrap_or("").trim().to_string();
        ParsedCommand::Cd(arg)
    } else {
        ParsedCommand::Shell(trimmed.to_string())
    }
}

/// Оборачивает строку в одинарные кавычки для безопасной передачи в shell.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn shell_program() -> String {
    env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

/// Выходит из TUI в обычный экран shell (cooked mode, основной буфер, курсор).
fn leave_tui() -> io::Result<()> {
    disable_raw_mode()?;
    // Мышь отдаём терминалу на время команды (нативное выделение/копирование).
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, Show)?;
    Ok(())
}

/// Возвращается в TUI (alt screen, raw mode, мышь захвачена, курсор скрыт).
fn enter_tui() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
    Ok(())
}

/// Печатает приглашение и ждёт одно нажатие клавиши.
fn wait_key(prompt: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\r\n{prompt}")?;
    stdout.flush()?;
    enable_raw_mode()?;
    loop {
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                break;
            }
        }
    }
    disable_raw_mode()?;
    write!(io::stdout(), "\r\n")?;
    io::stdout().flush()?;
    Ok(())
}

/// Пока выполняется дочерняя команда, rfm игнорирует SIGINT/SIGQUIT: в cooked mode
/// `Ctrl+C`/`Ctrl+\` терминал шлёт всей foreground-группе — и команде, и rfm,
/// который без этого молча завершался бы. Прежние обработчики возвращаются в Drop.
struct SignalGuard {
    old_int: libc::sighandler_t,
    old_quit: libc::sighandler_t,
}

impl SignalGuard {
    fn ignore() -> Self {
        unsafe {
            Self {
                old_int: libc::signal(libc::SIGINT, libc::SIG_IGN),
                old_quit: libc::signal(libc::SIGQUIT, libc::SIG_IGN),
            }
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.old_int);
            libc::signal(libc::SIGQUIT, self.old_quit);
        }
    }
}

/// Ребёнок наследует SIG_IGN родителя — перед exec возвращаем дефолтные
/// обработчики, чтобы `Ctrl+C` прерывал саму команду.
fn restore_child_signals(cmd: &mut ProcessCommand) {
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGQUIT, libc::SIG_DFL);
            Ok(())
        });
    }
}

/// Выполняет команду в `cwd` через `$SHELL -c`, вернув экран shell как был.
/// Пауза после команды — по `pause`. NB: `OnOutput` пока трактуется как `Always`
/// (при наследуемом tty вывод не перехватываем).
pub fn run_command<B: Backend>(
    terminal: &mut Terminal<B>,
    cwd: &Path,
    cmd: &str,
    pause: PauseMode,
) -> io::Result<()> {
    leave_tui()?;

    // Игнорируем Ctrl+C/Ctrl+\ у себя на время команды (и паузы после неё).
    let _signals = SignalGuard::ignore();
    let mut command = ProcessCommand::new(shell_program());
    command.arg("-c").arg(cmd).current_dir(cwd);
    restore_child_signals(&mut command);
    let status = command.status();
    if let Err(e) = status {
        let _ = writeln!(io::stderr(), "rfm: failed to run command: {e}");
    }

    if matches!(pause, PauseMode::Always | PauseMode::OnOutput) {
        wait_key("Press any key to continue...")?;
    }

    enter_tui()?;
    terminal.clear()?;
    Ok(())
}

/// Запускает интерактивную команду (напр. `ssh -M`) вне TUI: cooked mode + основной
/// экран, наследование реального tty (для промптов пароля/known_hosts). Возвращает успех.
/// При неуспехе ждёт клавишу, чтобы дать прочитать сообщение об ошибке.
pub fn run_interactive<B: Backend>(
    terminal: &mut Terminal<B>,
    program: &str,
    args: &[String],
) -> io::Result<bool> {
    leave_tui()?;
    let _signals = SignalGuard::ignore();
    let mut command = ProcessCommand::new(program);
    command.args(args);
    restore_child_signals(&mut command);
    let status = command.status();
    let ok = matches!(&status, Ok(s) if s.success());
    if !ok {
        let _ = wait_key("connection failed -- press any key to return...");
    }
    enter_tui()?;
    terminal.clear()?;
    Ok(ok)
}

/// Показывает экран выполнения команд (shell) и ждёт нажатия клавиши (`Ctrl+o`).
pub fn show_shell_screen<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    leave_tui()?;
    wait_key("-- shell screen -- press any key to return --")?;
    enter_tui()?;
    terminal.clear()?;
    Ok(())
}

#[cfg(test)]
#[path = "shell_test.rs"]
mod tests;
