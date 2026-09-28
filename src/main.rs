//! rfm — lightweight terminal file manager. Точка входа и главный event-loop.

mod app;
mod clock;
mod config;
mod history;
mod ops;
mod persist;
mod prettify;
mod shell;
mod theme;
mod ui;
mod users;
mod vfs;

use std::io;

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{Action, App};

fn main() -> io::Result<()> {
    let config = config::load();
    let mut app = App::new(config);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, Show)?;
    result
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;

        // Когда часы включены — просыпаемся раз в секунду, чтобы hh:mm обновлялось
        // (перерисовка диффом, только изменившиеся ячейки). Иначе блокируемся на чтении.
        if app.config.show_clock {
            match event::poll(std::time::Duration::from_secs(1)) {
                Ok(true) => {}
                Ok(false) => continue, // таймаут: перерисуем кадр (часы) сверху цикла
                Err(_) => break,
            }
        }
        // Ошибка чтения (напр. EOF на не-tty stdin) — завершаемся штатно.
        let event = match event::read() {
            Ok(ev) => ev,
            Err(_) => break,
        };
        // Клавиши и мышь дают одинаковый набор действий — обрабатываем единообразно.
        let action = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
            Event::Mouse(me) => app.handle_mouse(me),
            // Resize и прочее — перерисуемся диффом на следующей итерации.
            _ => Action::None,
        };
        match action {
            Action::Quit => {
                app.close_all_sftp();
                break;
            }
            Action::SftpConnect(target, sock) => {
                // Интерактивно поднимаем мастер (пароль/known_hosts спрашивает ssh),
                // затем передаём результат в App для листинга удалённой директории.
                let args = vfs::sftp::master_args(&sock, &target);
                let ok = shell::run_interactive(terminal, "ssh", &args)?;
                app.finish_sftp(target, sock, ok);
            }
            Action::RunShell(cmd) => {
                if let Some(cwd) = app.cwd() {
                    shell::run_command(terminal, &cwd, &cmd, app.config.pause_after_command)?;
                }
                app.reload();
                // Возврат из alt-screen делает known-буфер невалидным — полный сброс.
                terminal.clear()?;
            }
            Action::ShowShell => {
                shell::show_shell_screen(terminal)?;
                terminal.clear()?;
            }
            // Полная перерисовка только по явному запросу (Ctrl+l).
            Action::ClearRedraw => terminal.clear()?,
            // Обычное обновление UI — рисуем диффом (вверху цикла), без clear() и мигания.
            Action::Redraw | Action::None => {}
        }
    }
    Ok(())
}
