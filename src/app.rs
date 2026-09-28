//! Состояние приложения, роутинг клавиш и бизнес-логика навигации.

mod cmdline;
mod complete;
mod dialog;
mod editor;
mod mouse;
mod panel;
mod search;
mod settings;
mod viewer;
pub use cmdline::CmdLine;
pub use complete::Completion;
pub use editor::Editor;
pub use panel::{Panel, SortKey};
pub use settings::{SettingId, SettingRow, SettingsState, SETTINGS_FIRST, SETTINGS_ROWS};
pub use viewer::Viewer;
use complete::{complete_key, CompleteKey};
use panel::toggle_sort_key;
use viewer::{help_viewer, load_viewer};

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::config::{self, Config, FileListView, PanelLayout, PauseMode, SaveMode};
use crate::history::History;
use crate::ops;
use crate::persist::{self, PanelState};
use crate::shell::{self, ParsedCommand};
use crate::theme::Theme;
use crate::vfs::{VfsEntry, VfsPath};

/// Подсказки which-key для аккордов `Ctrl+x` (полноценный вид — этап 4).
const HINT_ROOT: &str   = "c-x: n create · c copy · m move · e edit · v view · d delete · h help · x settings · q quit · s sort · p panel";
const HINT_SORT: &str   = "c-x s: s size · d date · n name · f dirs-first";
const HINT_PANEL: &str  = "c-x p: c create · x close";
/// Подсказка `Ctrl+x` в просмотрщике — доступны только настройки и помощь.
const HINT_VIEWER: &str = "c-x: x settings · h help; q/Esc close · / search · n next · p prev · d hex · w wrap · Tab panel";

/// Состояние префикс-аккорда `Ctrl+x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefix {
    None,
    /// Нажат `Ctrl+x`, ждём операцию.
    Root,
    /// Нажато `Ctrl+x s`, ждём вид сортировки.
    Sort,
    /// Нажато `Ctrl+x p`, ждём операцию с панелью.
    Panel,
}

/// Что главному циклу сделать после обработки клавиши.
///
/// Политика перерисовки: цикл рисует кадр на каждой итерации, а ratatui сама шлёт
/// в терминал только изменившиеся ячейки (double-buffer diff). Поэтому `None`/`Redraw`
/// НЕ чистят экран — обновление идёт диффом (без мигания). `ClearRedraw` — принудительный
/// полный сброс known-буфера (`terminal.clear()`): нужен, когда содержимое терминала
/// стало неизвестным — после выхода из shell (alt-screen round-trip) и по `Ctrl+l`.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    /// Принудительная полная перерисовка (сброс known-буфера).
    ClearRedraw,
    Quit,
    RunShell(String),
    ShowShell,
    /// Интерактивно поднять мастер-соединение SFTP (пароль/known_hosts спрашивает ssh на
    /// экране shell), затем `App::finish_sftp`. Несёт цель и путь control-сокета.
    SftpConnect(crate::vfs::sftp::SftpTarget, PathBuf),
}

/// Отложенная файловая операция (выполняется после подтверждения диалога).
pub enum PendingOp {
    Delete(Vec<PathBuf>),
    Copy(Vec<PathBuf>),
    Move(Vec<PathBuf>),
    /// Создать директорию в указанной базовой директории (имя — из ввода).
    MkDir(PathBuf),
    /// Подключиться к FTP с введённым паролем (пароль приходит как «аргумент» диалога).
    FtpConnect(crate::vfs::ftp::FtpTarget),
}

/// Модальный диалог.
pub enum Dialog {
    Confirm {
        message: String,
        op: PendingOp,
    },
    Input {
        prompt: String,
        input: CmdLine,
        op: PendingOp,
    },
    /// Поиск по истории команд (`Ctrl+g`).
    HistorySearch {
        input: CmdLine,
        results: Vec<String>,
        sel: usize,
    },
}

/// Выбирает активную тему из конфига по имени `theme`; иначе — дефолтная тёмная.
fn resolve_theme(config: &Config) -> Theme {
    config
        .themes
        .iter()
        .find(|t| t.name == config.theme)
        .map(Theme::from_config)
        .unwrap_or_else(Theme::default_dark)
}

/// Промпт командной строки по текущему пользователю и хосту:
/// `login@host $`, а для root (uid 0) — `root@host #`.
fn shell_prompt() -> String {
    let uid = crate::users::current_uid();
    let login = crate::users::user_name(uid);
    let host = crate::users::hostname();
    let sep = if uid == 0 { '#' } else { '$' };
    format!("{login}@{host} {sep} ")
}

/// Состояние инкрементального поиска по списку файлов (`Ctrl+s`).
pub struct SearchState {
    pub query: String,
    /// Позиция курсора до начала поиска (для отмены по `Esc`).
    origin: usize,
}


pub struct App {
    pub config: Config,
    pub theme: Theme,
    pub panels: Vec<Panel>,
    pub active: usize,
    pub cmdline: CmdLine,
    pub status: String,
    /// Текущее состояние префикс-аккорда `Ctrl+x`.
    pub prefix: Prefix,
    /// Активный модальный диалог (подтверждение/ввод).
    pub dialog: Option<Dialog>,
    /// Окно помощи (если открыто) — как просмотрщик со скроллом.
    pub help: Option<Viewer>,
    /// Активный инкрементальный поиск по списку файлов.
    pub search: Option<SearchState>,
    /// Открыт ли экран настроек.
    pub settings: Option<SettingsState>,
    /// История введённых команд.
    pub history: History,
    /// Индекс навигации по истории (`Ctrl+p`/`Ctrl+n`); None — не в режиме истории.
    hist_nav: Option<usize>,
    /// Изменился ли набор/директории/активная панель (нужно ли сохранить).
    panels_dirty: bool,
    /// Последний клик мышью (время, панель, индекс записи) — для распознавания двойного.
    last_click: Option<(std::time::Instant, usize, usize)>,
    /// Промпт командной строки: `login@host $` (или `root@host #` для uid 0).
    pub prompt: String,
    /// Активный выбор варианта автодополнения (`Shift+Tab`), если их несколько.
    pub completion: Option<Completion>,
}

impl App {
    pub fn new(config: Config) -> Self {
        // Восстанавливаем панели из data-dir; иначе — одна панель в cwd.
        // В тестах не читаем реальный файл состояния (детерминированность).
        let restored = if cfg!(test) { None } else { persist::load() };
        let (active, states) = match restored {
            Some((a, ss)) => (a, ss),
            None => (
                0,
                vec![PanelState {
                    dir: env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
                    columns: 1,
                    view_override: None,
                    viewer: None,
                    editor: None,
                }],
            ),
        };
        let mut panels: Vec<Panel> = Vec::new();
        for ps in states {
            let mut panel = Panel::new(VfsPath::local(ps.dir));
            panel.columns = ps.columns.max(1);
            panel.view_override = ps.view_override;
            let view = ps.view_override.unwrap_or(config.file_list_view);
            let _ = panel.reload(config.show_hidden, view);
            // Восстанавливаем открытый просмотрщик.
            if let Some((vpath, voff)) = ps.viewer {
                panel.viewer = load_viewer(&vpath, voff, config.view_hex, config.view_wrap);
            }
            // Восстанавливаем открытый редактор (курсор клампится к содержимому:
            // файл мог измениться между запусками).
            if let Some((epath, line, col)) = ps.editor {
                panel.editor = Editor::load(&epath)
                    .map(|mut e| {
                        e.restore_cursor(line, col);
                        e
                    })
                    .ok();
            }
            panels.push(panel);
        }
        if panels.is_empty() {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let mut panel = Panel::new(VfsPath::local(cwd));
            let _ = panel.reload(config.show_hidden, config.file_list_view);
            panels.push(panel);
        }
        let active = active.min(panels.len() - 1);
        let theme = resolve_theme(&config);
        let prompt = shell_prompt();
        Self {
            config,
            theme,
            panels,
            active,
            cmdline: CmdLine::default(),
            status: String::new(),
            prefix: Prefix::None,
            dialog: None,
            help: None,
            search: None,
            settings: None,
            history: if cfg!(test) {
                History::default()
            } else {
                History::load()
            },
            hist_nav: None,
            panels_dirty: false,
            last_click: None,
            prompt,
            completion: None,
        }
    }

    /// Сохраняет состояние панелей в data-dir (best effort).
    fn save_panels(&self) {
        if cfg!(test) {
            return;
        }
        let states: Vec<PanelState> = self
            .panels
            .iter()
            .filter_map(|p| {
                p.path.base_local_path().map(|d| PanelState {
                    dir: d.to_path_buf(),
                    columns: p.columns,
                    view_override: p.view_override,
                    viewer: p.viewer.as_ref().map(|v| (v.path.clone(), v.voff)),
                    editor: p.editor.as_ref().map(|e| (e.path.clone(), e.cur_line, e.cur_col)),
                })
            })
            .collect();
        persist::save(self.active, &states);
    }

    pub fn active_panel(&self) -> &Panel {
        &self.panels[self.active]
    }

    pub fn active_panel_mut(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    /// Локальный cwd активной панели (для запуска команд).
    pub fn cwd(&self) -> Option<PathBuf> {
        self.active_panel().path.base_local_path().map(|p| p.to_path_buf())
    }

    /// Эффективный вид списка для панели: override панели или глобальный.
    fn effective_view(&self, idx: usize) -> FileListView {
        self.panels[idx]
            .view_override
            .unwrap_or(self.config.file_list_view)
    }

    /// Перечитать активную панель (после команды/по Ctrl+r).
    pub fn reload(&mut self) {
        let show_hidden = self.config.show_hidden;
        let view = self.effective_view(self.active);
        if let Err(e) = self.active_panel_mut().reload(show_hidden, view) {
            self.status = format!("cannot read directory: {e}");
        }
    }

    /// Горизонтальный шаг между колонками (column-major) = число строк в столбце.
    fn col_step(&self) -> isize {
        self.active_panel().grid_rows.max(1) as isize
    }

    fn move_cursor(&mut self, delta: isize) {
        let panel = self.active_panel_mut();
        if panel.entries.is_empty() {
            return;
        }
        let max = panel.entries.len() as isize - 1;
        panel.cursor = (panel.cursor as isize + delta).clamp(0, max) as usize;
    }

    // ---- Управление панелями (Этап 2) ----

    /// Создать новую панель справа от активной, открытую в её же директории.
    fn create_panel(&mut self) {
        let path = self.active_panel().path.clone();
        let mut panel = Panel::new(path);
        let _ = panel.reload(self.config.show_hidden, self.config.file_list_view);
        let idx = self.active + 1;
        self.panels.insert(idx, panel);
        self.active = idx;
        self.panels_dirty = true;
    }

    /// Закрыть активную панель (последнюю закрыть нельзя).
    fn close_panel(&mut self) {
        if self.panels.len() <= 1 {
            self.status = "cannot close the last panel".to_string();
            return;
        }
        self.panels.remove(self.active);
        if self.active >= self.panels.len() {
            self.active = self.panels.len() - 1;
        }
        self.panels_dirty = true;
    }

    fn next_panel(&mut self) {
        self.active = (self.active + 1) % self.panels.len();
        self.panels_dirty = true;
    }

    /// Обёртка: помощь (скролл/закрытие) и сохранение состояния панелей.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if self.help.is_some() {
            return self.handle_help_key(key);
        }
        let action = self.route_key(key);
        if self.panels_dirty {
            self.save_panels();
            self.panels_dirty = false;
        }
        action
    }

    /// Открывает экран настроек (`Ctrl+x x`).
    fn open_settings(&mut self) {
        self.settings = Some(SettingsState {
            sel: SETTINGS_FIRST,
            dirty: false,
            editing: None,
        });
    }

    /// Открывает окно помощи (`Ctrl+x h`).
    fn open_help(&mut self) {
        self.help = Some(help_viewer());
    }

    /// Главный роутинг клавиш.
    fn route_key(&mut self, key: KeyEvent) -> Action {
        if self.dialog.is_some() {
            return self.handle_dialog_key(key);
        }
        // Настройки — модальное окно поверх всего (в т.ч. поверх открытого просмотрщика).
        if self.settings.is_some() {
            return self.handle_settings_key(key);
        }
        if self.active_panel().editor.is_some() {
            return self.handle_editor_key(key);
        }
        if self.active_panel().viewer.is_some() {
            return self.handle_viewer_key(key);
        }
        if self.prefix != Prefix::None {
            return self.handle_prefix_key(key);
        }
        if self.search.is_some() {
            return self.handle_search_key(key);
        }
        // Автодополнение имён (Shift+Tab): пока активно — перехватывает клавиши.
        if self.completion.is_some() || key.code == KeyCode::BackTab {
            let base = self.active_panel().path.local_path().map(|p| p.to_path_buf());
            match complete_key(&mut self.cmdline, &mut self.completion, base.as_deref(), false, true, key) {
                CompleteKey::Consumed(msg) => {
                    if let Some(m) = msg {
                        self.status = m;
                    }
                    return Action::Redraw;
                }
                CompleteKey::Passthrough => {}
            }
        }
        self.status.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Char('q') if ctrl => Action::Quit,
            KeyCode::Char('s') if ctrl => {
                self.search = Some(SearchState {
                    query: String::new(),
                    origin: self.active_panel().cursor,
                });
                Action::Redraw
            }
            KeyCode::Char('o') if ctrl => Action::ShowShell,
            KeyCode::Char('l') if ctrl => Action::ClearRedraw,
            KeyCode::Char('r') if ctrl => {
                self.reload();
                Action::None
            }
            KeyCode::Char('t') if ctrl => {
                self.toggle_mark();
                Action::None
            }
            KeyCode::Char('v') if ctrl => {
                self.insert_name_under_cursor();
                Action::Redraw
            }
            KeyCode::Char('x') if ctrl => {
                self.prefix = Prefix::Root;
                self.status = HINT_ROOT.to_string();
                Action::Redraw
            }

            KeyCode::Tab => {
                self.next_panel();
                Action::Redraw
            }

            // Пометка «протягиванием» стрелками с Shift (по одному, вниз по столбцу).
            KeyCode::Up if shift => {
                self.mark_and_move(-1);
                Action::None
            }
            KeyCode::Down if shift => {
                self.mark_and_move(1);
                Action::None
            }
            KeyCode::Up => {
                self.move_cursor(-1);
                Action::None
            }
            KeyCode::Down => {
                self.move_cursor(1);
                Action::None
            }
            // Переход между колонками (column-major): шаг = число строк в столбце.
            KeyCode::Left if self.active_panel().columns > 1 => {
                let s = self.col_step();
                self.move_cursor(-s);
                Action::None
            }
            KeyCode::Right if self.active_panel().columns > 1 => {
                let s = self.col_step();
                self.move_cursor(s);
                Action::None
            }
            KeyCode::PageUp => {
                self.move_cursor(-10);
                Action::None
            }
            KeyCode::PageDown => {
                self.move_cursor(10);
                Action::None
            }
            // В начало / в конец списка файлов.
            KeyCode::Home => {
                let n = self.active_panel().entries.len() as isize;
                self.move_cursor(-n);
                Action::None
            }
            KeyCode::End => {
                let n = self.active_panel().entries.len() as isize;
                self.move_cursor(n);
                Action::None
            }

            KeyCode::Enter => self.on_enter(),
            KeyCode::Esc => {
                self.cmdline.clear();
                Action::None
            }
            KeyCode::Backspace => {
                self.cmdline.backspace();
                Action::None
            }

            // Редактирование командной строки (emacs-style).
            KeyCode::Char('a') if ctrl => {
                self.cmdline.home();
                Action::None
            }
            KeyCode::Char('e') if ctrl => {
                self.cmdline.end();
                Action::None
            }
            KeyCode::Char('b') if ctrl => {
                self.cmdline.left();
                Action::None
            }
            KeyCode::Char('f') if ctrl => {
                self.cmdline.right();
                Action::None
            }
            KeyCode::Char('u') if ctrl => {
                self.cmdline.kill_to_start();
                Action::None
            }
            KeyCode::Char('k') if ctrl => {
                self.cmdline.kill_to_end();
                Action::None
            }
            KeyCode::Char('w') if ctrl => {
                self.cmdline.kill_word();
                Action::None
            }

            // История команд.
            KeyCode::Char('p') if ctrl => {
                self.history_prev();
                Action::None
            }
            KeyCode::Char('n') if ctrl => {
                self.history_next();
                Action::None
            }
            KeyCode::Char('g') if ctrl => {
                self.open_history_search();
                Action::Redraw
            }

            KeyCode::Char(c) if !ctrl && !alt => {
                self.cmdline.insert(c);
                self.hist_nav = None;
                Action::None
            }

            _ => Action::None,
        }
    }

    /// Обработка клавиши в состоянии префикс-аккорда `Ctrl+x`.
    fn handle_prefix_key(&mut self, key: KeyEvent) -> Action {
        let state = self.prefix;
        // Esc всегда отменяет аккорд.
        if key.code == KeyCode::Esc {
            self.prefix = Prefix::None;
            self.status.clear();
            return Action::Redraw;
        }
        match state {
            Prefix::Root => match key.code {
                // Под-префиксы.
                KeyCode::Char('s') => {
                    self.prefix = Prefix::Sort;
                    self.status = HINT_SORT.to_string();
                    Action::Redraw
                }
                KeyCode::Char('p') => {
                    self.prefix = Prefix::Panel;
                    self.status = HINT_PANEL.to_string();
                    Action::Redraw
                }
                KeyCode::Char('h') => {
                    self.prefix = Prefix::None;
                    self.open_help();
                    Action::Redraw
                }
                // Файловые операции.
                KeyCode::Char('n') => {
                    self.prefix = Prefix::None;
                    self.op_mkdir();
                    Action::Redraw
                }
                KeyCode::Char('d') => {
                    self.prefix = Prefix::None;
                    self.op_delete();
                    Action::Redraw
                }
                KeyCode::Char('c') => {
                    self.prefix = Prefix::None;
                    self.op_copy();
                    Action::Redraw
                }
                KeyCode::Char('m') => {
                    self.prefix = Prefix::None;
                    self.op_move();
                    Action::Redraw
                }
                KeyCode::Char('e') => {
                    self.prefix = Prefix::None;
                    self.op_edit()
                }
                KeyCode::Char('v') => {
                    self.prefix = Prefix::None;
                    self.op_view()
                }
                KeyCode::Char('x') => {
                    self.prefix = Prefix::None;
                    self.open_settings();
                    Action::Redraw
                }
                KeyCode::Char('q') => {
                    self.prefix = Prefix::None;
                    Action::Quit
                }
                _ => {
                    self.prefix = Prefix::None;
                    self.status.clear();
                    Action::Redraw
                }
            },
            Prefix::Panel => {
                self.prefix = Prefix::None;
                self.status.clear();
                match key.code {
                    KeyCode::Char('c') => self.create_panel(),
                    KeyCode::Char('x') => self.close_panel(),
                    _ => {}
                }
                Action::Redraw
            }
            Prefix::Sort => {
                self.prefix = Prefix::None;
                if let KeyCode::Char(c @ ('s' | 'd' | 'n' | 'f')) = key.code {
                    self.apply_sort(c);
                } else {
                    self.status.clear();
                }
                Action::Redraw
            }
            Prefix::None => Action::None,
        }
    }

    // ---- Пометки, сортировка, операции (Этап 3) ----

    /// Переключить пометку файла под курсором (без перемещения). `..` не помечается.
    fn mark_current(&mut self) {
        let panel = self.active_panel_mut();
        if panel.entries.is_empty() {
            return;
        }
        let name = panel.entries[panel.cursor].name.clone();
        if name != ".." && !panel.marked.remove(&name) {
            panel.marked.insert(name);
        }
    }

    /// `Ctrl+t` — пометить текущий и сдвинуться вниз.
    fn toggle_mark(&mut self) {
        self.mark_current();
        self.move_cursor(1);
    }

    /// `Shift+↑`/`Shift+↓` — пометить текущий и сдвинуться в направлении `delta`.
    fn mark_and_move(&mut self, delta: isize) {
        self.mark_current();
        self.move_cursor(delta);
    }

    /// `Ctrl+v` — вставить имя записи под курсором в командную строку (в позицию курсора),
    /// всегда в одинарных кавычках (пробелы/спецсимволы не нужно разбирать). `..` → `'..'`.
    fn insert_name_under_cursor(&mut self) {
        let panel = self.active_panel();
        let Some(e) = panel.entries.get(panel.cursor) else {
            return;
        };
        let quoted = shell::shell_quote(&e.name);
        for c in quoted.chars() {
            self.cmdline.insert(c);
        }
    }

    /// Применить выбор сортировки: повторный тот же ключ инвертирует порядок.
    fn apply_sort(&mut self, c: char) {
        {
            let sort = &mut self.active_panel_mut().sort;
            match c {
                'f' => sort.dirs_first = !sort.dirs_first,
                's' => toggle_sort_key(sort, SortKey::Size),
                'd' => toggle_sort_key(sort, SortKey::Date),
                'n' => toggle_sort_key(sort, SortKey::Name),
                _ => {}
            }
        }
        self.reload();
    }

    /// Путь под курсором (для edit/view).
    fn cursor_local_path(&self) -> Option<PathBuf> {
        let panel = self.active_panel();
        let base = panel.path.local_path()?;
        let e = panel.entries.get(panel.cursor)?;
        if e.name == ".." || e.is_dir() {
            None
        } else {
            Some(base.join(&e.name))
        }
    }

    /// Перечитать все панели (после файловой операции — обновить и назначение).
    fn reload_all(&mut self) {
        let show_hidden = self.config.show_hidden;
        let global = self.config.file_list_view;
        for panel in &mut self.panels {
            let view = panel.view_override.unwrap_or(global);
            let _ = panel.reload(show_hidden, view);
        }
    }

    /// Обработка `Enter`: непустая строка — команда/`cd`; пустая — действие по курсору.
    fn on_enter(&mut self) -> Action {
        if !self.cmdline.is_blank() {
            let line = self.cmdline.text();
            match shell::parse(&line) {
                ParsedCommand::Cd(arg) => {
                    // Пишем в историю нетривиальные cd (пропускаем `cd` и `cd ..`).
                    if !arg.is_empty() && arg != ".." {
                        self.history.record(&format!("cd {arg}"));
                        self.hist_nav = None;
                    }
                    let action = self.do_cd(&arg);
                    self.cmdline.clear();
                    action
                }
                ParsedCommand::Shell(cmd) => {
                    self.history.record(&cmd);
                    self.hist_nav = None;
                    self.cmdline.clear();
                    Action::RunShell(cmd)
                }
                ParsedCommand::Empty => Action::None,
            }
        } else {
            self.activate_cursor()
        }
    }

    /// Действие по файлу под курсором (устав, «Действия по <enter>»).
    fn activate_cursor(&mut self) -> Action {
        let panel = self.active_panel();
        if panel.entries.is_empty() {
            return Action::None;
        }
        let entry = panel.entries[panel.cursor].clone();
        if entry.name == ".." {
            self.go_up();
            return Action::Redraw;
        }
        if entry.is_dir() {
            // В дереве Enter сворачивает/разворачивает узел (name = относительный путь).
            if self.active_panel().is_tree {
                self.toggle_expand(&entry.name);
                return Action::Redraw;
            }
            let panel = self.active_panel_mut();
            panel.path.enter_dir(&entry.name);
            panel.cursor = 0;
            panel.marked.clear();
            panel.expanded.clear();
            self.reload();
            self.panels_dirty = true;
            return Action::Redraw;
        }
        // Действия над файлом — только на локальном слое (внутри архива/сессии файлы
        // не извлекаем и локально не запускаем).
        if let Some(dir) = self.active_panel().path.local_path() {
            if entry.executable {
                return Action::RunShell(shell::shell_quote(&format!("./{}", entry.name)));
            }
            // Архив — открыть как директорию.
            let file = dir.join(&entry.name);
            if let Some(cfg) =
                crate::vfs::archive::match_archive(&entry.name, &self.config.archives).cloned()
            {
                self.open_archive(&file, &cfg);
                return Action::Redraw;
            }
        }
        Action::None
    }

    /// Открывает архив в активной панели: шеллит листинг, парсит, push’ит слой `Archive`.
    fn open_archive(&mut self, file: &std::path::Path, cfg: &crate::config::ArchiveConfig) {
        match crate::vfs::archive::open(file, cfg) {
            Ok(items) => {
                let label = file
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                {
                    let panel = self.active_panel_mut();
                    panel.path.push_archive(label, items);
                    panel.cursor = 0;
                    panel.marked.clear();
                    panel.expanded.clear();
                }
                self.reload();
                self.panels_dirty = true;
            }
            Err(e) => self.status = format!("archive: {e}"),
        }
    }

    /// Свернуть/развернуть узел дерева и перестроить список.
    fn toggle_expand(&mut self, rel: &str) {
        {
            let p = self.active_panel_mut();
            if !p.expanded.remove(rel) {
                p.expanded.insert(rel.to_string());
            }
        }
        self.reload();
    }

    /// Поднимается на уровень вверх, ставя курсор на элемент, откуда пришли
    /// (директория для локального выхода или файл-архив при снятии слоя архива).
    fn go_up(&mut self) {
        let focus = {
            let panel = self.active_panel_mut();
            let name = panel.path.go_up();
            panel.cursor = 0;
            panel.marked.clear();
            panel.expanded.clear();
            name
        };
        self.reload();
        if let Some(name) = focus {
            let panel = self.active_panel_mut();
            if let Some(idx) = panel.entries.iter().position(|e| e.name == name) {
                panel.cursor = idx;
            }
        }
        self.panels_dirty = true;
    }

    /// Перехваченный `cd`.
    fn do_cd(&mut self, arg: &str) -> Action {
        // Удалённый хост: cd sftp://login@server/path → поднять мастер-соединение.
        if arg.starts_with("sftp://") {
            return match crate::vfs::sftp::parse_url(arg) {
                Some(target) => Action::SftpConnect(target, crate::vfs::sftp::new_sock()),
                None => {
                    self.status = format!("cd: bad sftp url: {arg}");
                    Action::Redraw
                }
            };
        }
        // FTP: анонимно — открываем сразу; с логином — спрашиваем пароль в диалоге.
        if arg.starts_with("ftp://") {
            return match crate::vfs::ftp::parse_url(arg) {
                Some(target) if target.user.is_some() => {
                    self.dialog = Some(Dialog::Input {
                        prompt: format!("FTP password for {}", crate::vfs::ftp::user_host(&target)),
                        input: CmdLine::default(),
                        op: PendingOp::FtpConnect(target),
                    });
                    Action::Redraw
                }
                Some(target) => {
                    self.open_ftp(target, None);
                    Action::Redraw
                }
                None => {
                    self.status = format!("cd: bad ftp url: {arg}");
                    Action::Redraw
                }
            };
        }
        // HTTP(S): директория как HTML-автоиндекс.
        if arg.starts_with("http://") || arg.starts_with("https://") {
            return match crate::vfs::http::parse_url(arg) {
                Some(u) => {
                    self.open_http(u);
                    Action::Redraw
                }
                None => {
                    self.status = format!("cd: bad http url: {arg}");
                    Action::Redraw
                }
            };
        }
        if arg.contains("://") {
            self.status = format!("cd: unsupported scheme: {arg}");
            return Action::Redraw;
        }
        if arg.is_empty() || arg == "~" {
            match dirs::home_dir() {
                Some(h) => self.set_dir(h),
                None => self.status = "no home directory".to_string(),
            }
            return Action::Redraw;
        }
        if arg == ".." {
            self.go_up();
            return Action::Redraw;
        }
        // Внутри удалённой сессии cd идёт по удалённым путям (без локальной canonicalize).
        if self.active_panel().path.local_path().is_none()
            && self.active_panel().path.base_local_path().is_none()
        {
            self.status = "cd: use arrows/Enter to navigate remote".to_string();
            return Action::Redraw;
        }

        let base = self
            .active_panel()
            .path
            .base_local_path()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        let target = if let Some(rest) = arg.strip_prefix("~/") {
            dirs::home_dir().unwrap_or_else(|| base.clone()).join(rest)
        } else {
            let p = PathBuf::from(arg);
            if p.is_absolute() {
                p
            } else {
                base.join(p)
            }
        };

        match std::fs::canonicalize(&target) {
            Ok(c) if c.is_dir() => self.set_dir(c),
            // Файл-архив — открываем как директорию (push слоя Archive).
            Ok(c) if c.is_file() => {
                let fname = c.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                match crate::vfs::archive::match_archive(&fname, &self.config.archives).cloned() {
                    Some(cfg) => self.open_archive(&c, &cfg),
                    None => self.status = format!("not a directory: {}", target.display()),
                }
            }
            Ok(_) => self.status = format!("not a directory: {}", target.display()),
            Err(e) => self.status = format!("cd: {}: {e}", target.display()),
        }
        Action::Redraw
    }

    /// Завершение подключения SFTP (после интерактивного мастера в главном цикле).
    /// `ok` — успешно ли поднялся мастер. Пробуем листинг корня; при ошибке — статус.
    pub fn finish_sftp(&mut self, target: crate::vfs::sftp::SftpTarget, sock: PathBuf, ok: bool) {
        if !ok {
            self.status = "sftp: connection failed".to_string();
            return;
        }
        match crate::vfs::sftp::list(&sock, &target, &target.path) {
            Ok(_) => {
                let root = target.path.clone();
                {
                    let panel = self.active_panel_mut();
                    panel.path.push_sftp(target, sock, root);
                    panel.cursor = 0;
                    panel.marked.clear();
                    panel.expanded.clear();
                }
                self.reload();
                self.panels_dirty = true;
            }
            Err(e) => {
                crate::vfs::sftp::exit_master(&sock, &target);
                self.status = format!("sftp: {e}");
            }
        }
    }

    /// Открывает FTP-локацию: пробный листинг, затем push слоя `Ftp`.
    fn open_ftp(&mut self, target: crate::vfs::ftp::FtpTarget, password: Option<String>) {
        match crate::vfs::ftp::list(&target, password.as_deref(), &target.path) {
            Ok(_) => {
                let root = target.path.clone();
                {
                    let panel = self.active_panel_mut();
                    panel.path.push_ftp(target, password, root);
                    panel.cursor = 0;
                    panel.marked.clear();
                    panel.expanded.clear();
                }
                self.reload();
                self.panels_dirty = true;
            }
            Err(e) => self.status = format!("ftp: {e}"),
        }
    }

    /// Открывает HTTP-локацию (автоиндекс): пробный листинг, затем push слоя `Http`.
    fn open_http(&mut self, u: crate::vfs::http::HttpUrl) {
        match crate::vfs::http::list(&u.origin, &u.path) {
            Ok(_) => {
                {
                    let panel = self.active_panel_mut();
                    panel.path.push_http(u.origin, u.path);
                    panel.cursor = 0;
                    panel.marked.clear();
                    panel.expanded.clear();
                }
                self.reload();
                self.panels_dirty = true;
            }
            Err(e) => self.status = format!("http: {e}"),
        }
    }

    /// Закрывает все мастер-соединения SFTP (при выходе из приложения).
    pub fn close_all_sftp(&self) {
        for p in &self.panels {
            p.path.close_remote();
        }
    }

    fn set_dir(&mut self, path: PathBuf) {
        let panel = self.active_panel_mut();
        panel.path.set_local(path);
        panel.cursor = 0;
        panel.marked.clear();
        panel.expanded.clear();
        self.reload();
        self.panels_dirty = true;
    }
}
#[cfg(test)]
#[path = "app_test.rs"]
mod tests;
