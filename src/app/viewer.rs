//! Встроенный просмотрщик: клавиши, hex/wrap, открытие view/edit.
//! Файл читается **лениво**: контент в память не грузится, индекс начал строк
//! достраивается по мере прокрутки, а рендер тянет из файла только видимое окно.

use super::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Максимум байт на одну строку (защита от «строки» размером в весь файл).
const MAX_LINE: u64 = 1 << 16; // 64 KiB
/// Размер чанка при индексации/сканировании.
const CHUNK: usize = 1 << 16;

impl App {
    pub(super) fn handle_help_key(&mut self, key: KeyEvent) -> Action {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.help = None;
            return Action::Redraw;
        }
        if let Some(v) = self.help.as_mut() {
            let page = v.page.max(1);
            match key.code {
                KeyCode::Down => v.scroll_down(1),
                KeyCode::Up => v.scroll_up(1),
                KeyCode::Char(' ') | KeyCode::PageDown => v.scroll_down(page),
                KeyCode::PageUp => v.scroll_up(page),
                KeyCode::Right => v.hoff += 4,
                KeyCode::Left => v.hoff = v.hoff.saturating_sub(4),
                KeyCode::Home => v.go_start(),
                KeyCode::End => v.go_end(),
                _ => {}
            }
        }
        Action::Redraw
    }

    pub(super) fn op_edit(&mut self) -> Action {
        let prog = self.config.edit.clone();
        self.view_or_edit(&prog, "edit")
    }

    pub(super) fn op_view(&mut self) -> Action {
        let prog = self.config.view.clone();
        self.view_or_edit(&prog, "view")
    }

    pub(super) fn view_or_edit(&mut self, prog: &str, verb: &str) -> Action {
        let path = match self.cursor_local_path() {
            Some(p) => p,
            None => {
                self.status = format!("no file under cursor to {verb}");
                return Action::Redraw;
            }
        };
        if prog == "internal" {
            if verb == "edit" {
                self.open_editor(&path);
            } else {
                self.open_viewer(&path);
            }
            Action::Redraw
        } else {
            Action::RunShell(format!("{prog} {}", shell::shell_quote(&path.to_string_lossy())))
        }
    }

    pub(super) fn open_viewer(&mut self, path: &std::path::Path) {
        match load_viewer(path, 0, self.config.view_hex, self.config.view_wrap) {
            Some(v) => {
                self.active_panel_mut().viewer = Some(v);
                self.panels_dirty = true;
                self.status = VIEWER_HINT.to_string();
            }
            None => self.status = format!("view: cannot read {}", path.display()),
        }
    }

    pub(super) fn handle_viewer_key(&mut self, key: KeyEvent) -> Action {
        // Ввод строки поиска (`/`) перехватывает все клавиши до Enter/Esc.
        if self
            .active_panel()
            .viewer
            .as_ref()
            .map(|v| v.find_input.is_some())
            .unwrap_or(false)
        {
            return self.handle_viewer_find_input(key);
        }
        // Аккорд Ctrl+x в просмотрщике: доступны только настройки и помощь.
        if self.prefix == Prefix::Root {
            self.prefix = Prefix::None;
            self.status.clear();
            match key.code {
                KeyCode::Char('x') => self.open_settings(),
                KeyCode::Char('h') => self.open_help(),
                _ => {} // прочее (в т.ч. Esc) — отмена аккорда
            }
            return Action::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('x') {
            self.prefix = Prefix::Root;
            self.status = HINT_VIEWER.to_string();
            return Action::Redraw;
        }
        // Переключение панелей работает и при открытом просмотрщике (он остаётся в своей панели).
        if key.code == KeyCode::Tab {
            self.next_panel();
            return Action::Redraw;
        }
        // `/` — начать поиск; `n`/`p` — навигация по совпадениям (когда поиск активен).
        // Без активного поиска `p` сохраняет старую роль (prettify) в блоке ниже.
        if key.code == KeyCode::Char('/') {
            self.viewer_start_find();
            return Action::Redraw;
        }
        let find_active = self
            .active_panel()
            .viewer
            .as_ref()
            .map(|v| v.find.is_some())
            .unwrap_or(false);
        if find_active {
            match key.code {
                KeyCode::Char('n') => {
                    self.viewer_find_step(1);
                    return Action::Redraw;
                }
                KeyCode::Char('p') => {
                    self.viewer_find_step(-1);
                    return Action::Redraw;
                }
                _ => {}
            }
        }
        let close = matches!(key.code, KeyCode::Esc | KeyCode::Char('q'));
        if close {
            self.active_panel_mut().viewer = None;
            self.panels_dirty = true;
            self.status.clear();
            return Action::Redraw;
        }
        // Изменившийся режим (hex/wrap) — чтобы запомнить его в конфиге после снятия borrow.
        let mut new_hex: Option<bool> = None;
        let mut new_wrap: Option<bool> = None;
        let mut do_pretty = false;
        if let Some(v) = self.active_panel_mut().viewer.as_mut() {
            let page = v.page.max(1);
            match key.code {
                KeyCode::Down => v.scroll_down(1),
                KeyCode::Up => v.scroll_up(1),
                // Пробел / PageDown — на экран вниз.
                KeyCode::Char(' ') | KeyCode::PageDown => v.scroll_down(page),
                KeyCode::PageUp => v.scroll_up(page),
                // Горизонтальный скролл — только когда перенос строк выключен.
                KeyCode::Right if !v.wrap => v.hoff += 8,
                KeyCode::Left if !v.wrap => v.hoff = v.hoff.saturating_sub(8),
                KeyCode::Home => v.go_start(),
                KeyCode::End => v.go_end(),
                // d — переключить hex/ascii; смещения сбрасываем (наборы строк разные).
                KeyCode::Char('d') => {
                    v.hex = !v.hex;
                    v.voff = 0;
                    v.hoff = 0;
                    v.find = None; // набор строк сменился — совпадения недействительны
                    new_hex = Some(v.hex);
                }
                // w — перенос строк (в hex-режиме не имеет смысла).
                KeyCode::Char('w') => {
                    v.wrap = !v.wrap;
                    v.hoff = 0;
                    new_wrap = Some(v.wrap);
                }
                // p — prettify по расширению (json/xml); повтор — назад к исходнику.
                KeyCode::Char('p') => do_pretty = true,
                _ => {}
            }
        }
        // Запоминаем последний режим в конфиге.
        if let Some(h) = new_hex {
            self.config.view_hex = h;
            self.persist_view_pref();
        }
        if let Some(w) = new_wrap {
            self.config.view_wrap = w;
            self.persist_view_pref();
        }
        if do_pretty {
            self.toggle_prettify();
        }
        Action::Redraw
    }

    /// `p` — prettify текущего файла по расширению (json/xml); повторное нажатие
    /// возвращает исходный текст. Неподдерживаемый тип / ошибка чтения — сообщение в статус.
    pub(super) fn toggle_prettify(&mut self) {
        let Some(v) = self.active_panel().viewer.as_ref() else {
            return;
        };
        if v.path.as_os_str().is_empty() {
            return; // напр. окно помощи — файла нет
        }
        let path = v.path.clone();
        let wrap = v.wrap;
        let name = v.name.clone();
        let turn_off = v.pretty;

        // Выключение prettify — возврат к ленивому чтению файла.
        if turn_off {
            if let Some(mut nv) = load_viewer(&path, 0, false, wrap) {
                nv.name = name;
                if let Some(slot) = self.active_panel_mut().viewer.as_mut() {
                    *slot = nv;
                }
            }
            return;
        }

        let kind = crate::prettify::kind_by_ext(&path);
        if kind.is_none() {
            self.status = "prettify: unsupported file type".to_string();
            return;
        }
        // Порог размера: prettify держит весь файл в памяти.
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let max = self.config.view_prettify_max_mb.saturating_mul(1024 * 1024);
        if size > max {
            self.status = format!(
                "prettify: file too large (> {} MB)",
                self.config.view_prettify_max_mb
            );
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("prettify: {e}");
                return;
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<String> = crate::prettify::prettify(kind.unwrap(), &text)
            .lines()
            .map(sanitize_line)
            .collect();
        if let Some(slot) = self.active_panel_mut().viewer.as_mut() {
            let mut nv = Viewer::from_memory(name, lines);
            nv.path = path;
            nv.pretty = true;
            nv.wrap = wrap;
            *slot = nv;
        }
    }

    /// `/` — начать ввод строки поиска: печать идёт в строку статуса, Enter ищет, Esc отменяет.
    pub(super) fn viewer_start_find(&mut self) {
        if let Some(v) = self.active_panel_mut().viewer.as_mut() {
            v.find_input = Some(String::new());
        }
    }

    /// Обрабатывает клавиши во время ввода строки поиска.
    pub(super) fn handle_viewer_find_input(&mut self, key: KeyEvent) -> Action {
        let mut input = match self
            .active_panel_mut()
            .viewer
            .as_mut()
            .and_then(|v| v.find_input.take())
        {
            Some(s) => s,
            None => return Action::None,
        };
        match key.code {
            KeyCode::Enter => self.viewer_execute_find(input),
            KeyCode::Esc => self.status = VIEWER_HINT.to_string(), // find_input уже снят — отмена
            KeyCode::Backspace => {
                input.pop();
                if let Some(v) = self.active_panel_mut().viewer.as_mut() {
                    v.find_input = Some(input);
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                input.push(c);
                if let Some(v) = self.active_panel_mut().viewer.as_mut() {
                    v.find_input = Some(input);
                }
            }
            _ => {
                if let Some(v) = self.active_panel_mut().viewer.as_mut() {
                    v.find_input = Some(input);
                }
            }
        }
        Action::Redraw
    }

    /// Выполняет поиск `query` по текущим строкам просмотрщика (регистронезависимо,
    /// непересекающиеся вхождения). Прыгает к первому совпадению.
    pub(super) fn viewer_execute_find(&mut self, query: String) {
        if query.is_empty() {
            self.status = VIEWER_HINT.to_string(); // пустой запрос — отмена
            return;
        }
        let q: Vec<char> = query.to_ascii_lowercase().chars().collect();
        let qlen = q.len();
        let matches: Vec<(usize, usize)> = match self.active_panel_mut().viewer.as_mut() {
            Some(v) => v.search(&q),
            None => return,
        };
        if matches.is_empty() {
            self.status = format!("not found: {query}");
            if let Some(v) = self.active_panel_mut().viewer.as_mut() {
                v.find = None;
            }
            return;
        }
        let n = matches.len();
        let (line, col) = matches[0];
        if let Some(v) = self.active_panel_mut().viewer.as_mut() {
            viewer_ensure_visible(v, line, col, qlen);
            v.find = Some(ViewerFind {
                query,
                matches,
                qlen,
                current: 0,
            });
        }
        self.status = format!("viewer: match 1/{n} · n next · p prev · Esc/q close");
    }

    /// Переходит к следующему (`dir=1`) / предыдущему (`dir=-1`) совпадению по кругу.
    pub(super) fn viewer_find_step(&mut self, dir: isize) {
        let mut info: Option<(usize, usize, String)> = None;
        if let Some(v) = self.active_panel_mut().viewer.as_mut() {
            if let Some(f) = v.find.as_ref() {
                let n = f.matches.len();
                if n > 0 {
                    let cur = (f.current as isize + dir).rem_euclid(n as isize) as usize;
                    let (line, col) = f.matches[cur];
                    let qlen = f.qlen;
                    let query = f.query.clone();
                    v.find.as_mut().unwrap().current = cur;
                    viewer_ensure_visible(v, line, col, qlen);
                    info = Some((cur + 1, n, query));
                }
            }
        }
        self.status = match info {
            Some((i, n, q)) => format!("viewer: /{q} — match {i}/{n} · n next · p prev · Esc/q close"),
            None => "viewer: no active search (/ to search)".to_string(),
        };
    }
}

/// Прокручивает просмотрщик так, чтобы совпадение `(line, col)` было видно.
fn viewer_ensure_visible(v: &mut Viewer, line: usize, col: usize, qlen: usize) {
    let page = v.page.max(1);
    if line < v.voff || line >= v.voff + page {
        v.voff = line.saturating_sub(page / 2); // примерно по центру
    }
    if v.wrap {
        v.hoff = 0;
    } else {
        let cols = v.cols.max(1);
        if col < v.hoff {
            v.hoff = col;
        } else if col + qlen > v.hoff + cols {
            v.hoff = (col + qlen).saturating_sub(cols);
        }
    }
}

/// Подсказка в строке статуса при открытом просмотрщике.
pub(super) const VIEWER_HINT: &str =
    "viewer: q/Esc close · arrows scroll · / search · n next · p prev · d hex · w wrap · Tab panel";

/// Текст окна помощи (список биндингов).
const HELP_TEXT: &str = "\
Panels:
  Tab                    switch panel
  c-x p c / p x          create / close panel
  c-t                    mark file
  Shift+Up/Down          mark and move
  + / -                  select / unselect by mask (empty command line)
  c-s                    incremental search in list
  Up / Down              move cursor
  Enter                  open dir / run exec / run command (in tree: expand/collapse)

Command line:
  printable              type into command line
  c-a/e c-b/f c-u/k c-w  line editing
  Esc                    clear line
  c-v                    insert name under cursor (quoted)
  c-p/c-n  c-g           history prev/next · search
  c-r                    reload directory content
  c-o                    show shell screen
  c-l                    redraw screen

Operations (c-x):
  n c m d e v            create copy move delete edit view
  s s/d/n/f              sort size/date/name/dirs-first
  x                      settings
  h                      this help
  q                      quit

Viewer (c-x v):
  q/Esc                  close viewer screen
  d                      toggle hex dump/ascii text
  w                      toggle wrap/unwrap
  /  n / p               search · next / prev match
  p                      prettify json/xml (no active search)
  tab                    switch to another panel

Editor (c-x e):
  c-x h                  editor keys (inside the editor)
  c-x s / c-x x          save / save and close
  Esc / c-x q            close WITHOUT saving

Help: arrows scroll · q/Esc close";

/// Строит просмотрщик для окна помощи (in-memory).
pub(super) fn help_viewer() -> Viewer {
    Viewer::from_memory("Help".to_string(), HELP_TEXT.lines().map(|l| l.to_string()).collect())
}

/// Открывает файл в просмотрщик **лениво** (контент в память не грузится).
pub(super) fn load_viewer(path: &std::path::Path, voff: usize, hex: bool, wrap: bool) -> Option<Viewer> {
    let fs = FileSource::open(path)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(Viewer {
        name,
        path: path.to_path_buf(),
        src: Source::File(fs),
        voff,
        hoff: 0,
        page: 0,
        cols: 0,
        hex,
        wrap,
        pretty: false,
        find_input: None,
        find: None,
    })
}

/// Источник данных просмотрщика: память (help/prettify) или ленивое чтение файла.
enum Source {
    Memory(Vec<String>),
    File(FileSource),
}

/// Ленивое чтение файла: индекс начал строк достраивается по мере прокрутки,
/// контент читается окном через `seek`.
struct FileSource {
    file: File,
    size: u64,
    /// Смещения начал строк (`index[0] == 0`), достраивается лениво.
    index: Vec<u64>,
    /// До какого смещения уже просканированы переводы строк (для инкремента индекса).
    scan_pos: u64,
    /// Достигнут ли конец файла при индексации.
    done: bool,
}

impl FileSource {
    fn open(path: &std::path::Path) -> Option<Self> {
        let file = File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        Some(Self { file, size, index: vec![0], scan_pos: 0, done: size == 0 })
    }

    /// Читает до `len` байт с позиции `off`.
    fn read_at(&mut self, off: u64, len: usize) -> Vec<u8> {
        if self.file.seek(SeekFrom::Start(off)).is_err() {
            return Vec::new();
        }
        let mut buf = vec![0u8; len];
        let mut got = 0;
        while got < len {
            match self.file.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(_) => break,
            }
        }
        buf.truncate(got);
        buf
    }

    /// Достраивает индекс, пока не станет известно ≥ `want+1` строк (или до EOF).
    /// `want = usize::MAX` — полная индексация файла.
    fn ensure_indexed(&mut self, want: usize) {
        while self.index.len() <= want && !self.done {
            let base = self.scan_pos;
            let buf = self.read_at(base, CHUNK);
            if buf.is_empty() {
                self.done = true;
                break;
            }
            for (i, b) in buf.iter().enumerate() {
                if *b == b'\n' {
                    let ns = base + i as u64 + 1;
                    if ns < self.size {
                        self.index.push(ns);
                    }
                }
            }
            self.scan_pos += buf.len() as u64;
            if self.scan_pos >= self.size {
                self.done = true;
            }
        }
    }

    fn hex_total(&self) -> usize {
        self.size.div_ceil(16) as usize
    }

    /// Видимые текстовые строки `[start, start+count)` (санитайзенные).
    fn text_window(&mut self, start: usize, count: usize) -> Vec<String> {
        self.ensure_indexed(start.saturating_add(count));
        let mut out = Vec::new();
        for i in start..start.saturating_add(count) {
            if i >= self.index.len() {
                break;
            }
            let begin = self.index[i];
            let end = if i + 1 < self.index.len() { self.index[i + 1] } else { self.size };
            let len = (end - begin).min(MAX_LINE) as usize;
            let bytes = self.read_at(begin, len);
            out.push(sanitize_line(&String::from_utf8_lossy(trim_eol(&bytes))));
        }
        out
    }

    /// Видимые hex-строки `[start, start+count)`.
    fn hex_window(&mut self, start: usize, count: usize) -> Vec<String> {
        let total = self.hex_total();
        let mut out = Vec::new();
        for i in start..start.saturating_add(count) {
            if i >= total {
                break;
            }
            let off = i as u64 * 16;
            let len = (self.size - off).min(16) as usize;
            let bytes = self.read_at(off, len);
            out.push(hex_line(off, &bytes));
        }
        out
    }

    /// Полный потоковый проход: строит индекс целиком и (если задан `query`,
    /// уже в нижнем регистре) собирает совпадения `(строка, символьная колонка)`.
    fn scan(&mut self, query: Option<&[char]>) -> Vec<(usize, usize)> {
        let _ = self.file.seek(SeekFrom::Start(0));
        self.index.clear();
        self.index.push(0);
        let mut matches = Vec::new();
        let mut line_no = 0usize;
        let mut line_buf: Vec<u8> = Vec::new();
        let mut off = 0u64;
        let mut chunk = vec![0u8; CHUNK];
        loop {
            let n = match self.file.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            for &b in &chunk[..n] {
                if b == b'\n' {
                    finalize_line(&line_buf, line_no, query, &mut matches);
                    line_no += 1;
                    line_buf.clear();
                    if off + 1 < self.size {
                        self.index.push(off + 1);
                    }
                } else if (line_buf.len() as u64) < MAX_LINE {
                    line_buf.push(b);
                }
                off += 1;
            }
        }
        if !line_buf.is_empty() {
            finalize_line(&line_buf, line_no, query, &mut matches);
        }
        self.scan_pos = self.size;
        self.done = true;
        matches
    }
}

/// Ищет `query` в санитайзенной строке `bytes` и добавляет совпадения (непересекающиеся).
fn finalize_line(bytes: &[u8], line_no: usize, query: Option<&[char]>, out: &mut Vec<(usize, usize)>) {
    let Some(q) = query else { return };
    if q.is_empty() {
        return;
    }
    let s = sanitize_line(&String::from_utf8_lossy(bytes));
    let lower: Vec<char> = s.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut i = 0;
    while i + q.len() <= lower.len() {
        if lower[i..i + q.len()] == *q {
            out.push((line_no, i));
            i += q.len();
        } else {
            i += 1;
        }
    }
}

/// Обрезает завершающие `\n`/`\r`.
fn trim_eol(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    if end > 0 && b[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && b[end - 1] == b'\r' {
        end -= 1;
    }
    &b[..end]
}

/// Встроенный просмотрщик текста файла (view/edit=internal).
pub struct Viewer {
    pub name: String,
    /// Полный путь к файлу (для восстановления после перезапуска; пусто для help/памяти).
    pub path: PathBuf,
    /// Источник данных (память или ленивый файл).
    src: Source,
    /// Смещение по вертикали (номер верхней видимой строки).
    pub voff: usize,
    /// Смещение по горизонтали (в символах).
    pub hoff: usize,
    /// Высота видимой области (строк) — обновляется при рендере; для `Space`/`PageDown`.
    pub page: usize,
    /// Ширина видимой области (символов) — обновляется при рендере; для гориз. прыжка к совпадению.
    pub cols: usize,
    /// Hex-режим (toggle `d`).
    pub hex: bool,
    /// Перенос строк (toggle `w`): при включённом `←`/`→` не работают.
    pub wrap: bool,
    /// Показан ли prettify-вариант (toggle `p`).
    pub pretty: bool,
    /// Активный ввод строки поиска (`/`): `Some` — пользователь печатает запрос.
    pub find_input: Option<String>,
    /// Результат поиска по тексту: запрос + позиции совпадений + текущее.
    pub find: Option<ViewerFind>,
}

/// Результат текстового поиска в просмотрщике.
pub struct ViewerFind {
    /// Исходный запрос (для показа/сообщений).
    pub query: String,
    /// Позиции совпадений: `(индекс строки, символьная колонка начала)`.
    pub matches: Vec<(usize, usize)>,
    /// Длина запроса в символах.
    pub qlen: usize,
    /// Индекс текущего совпадения в `matches`.
    pub current: usize,
}

impl Viewer {
    /// In-memory просмотрщик (help, prettify-результат).
    pub(super) fn from_memory(name: String, lines: Vec<String>) -> Viewer {
        Viewer {
            name,
            path: PathBuf::new(),
            src: Source::Memory(lines),
            voff: 0,
            hoff: 0,
            page: 0,
            cols: 0,
            hex: false,
            wrap: false,
            pretty: false,
            find_input: None,
            find: None,
        }
    }

    /// Видимое окно строк `[start, start+count)` (текст или hex), читается по месту.
    pub fn window(&mut self, start: usize, count: usize) -> Vec<String> {
        let hex = self.hex;
        match &mut self.src {
            Source::Memory(l) => l.iter().skip(start).take(count).cloned().collect(),
            Source::File(fs) => {
                if hex {
                    fs.hex_window(start, count)
                } else {
                    fs.text_window(start, count)
                }
            }
        }
    }

    /// Строки in-memory источника (help/prettify) — для расчёта размеров и тестов.
    pub fn content_lines(&self) -> Option<&Vec<String>> {
        match &self.src {
            Source::Memory(l) => Some(l),
            _ => None,
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.voff = self.voff.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let hex = self.hex;
        let target = self.voff.saturating_add(n);
        self.voff = match &mut self.src {
            Source::Memory(l) => target.min(l.len().saturating_sub(1)),
            Source::File(fs) => {
                if hex {
                    target.min(fs.hex_total().saturating_sub(1))
                } else {
                    fs.ensure_indexed(target);
                    target.min(fs.index.len().saturating_sub(1))
                }
            }
        };
    }

    /// Прокрутка на `delta` строк (колесо мыши).
    pub fn scroll(&mut self, delta: isize) {
        if delta < 0 {
            self.scroll_up((-delta) as usize);
        } else {
            self.scroll_down(delta as usize);
        }
    }

    pub fn go_start(&mut self) {
        self.voff = 0;
        self.hoff = 0;
    }

    pub fn go_end(&mut self) {
        let hex = self.hex;
        let page = self.page.max(1);
        let last = match &mut self.src {
            Source::Memory(l) => l.len().saturating_sub(1),
            Source::File(fs) => {
                if hex {
                    fs.hex_total().saturating_sub(1)
                } else {
                    fs.ensure_indexed(usize::MAX); // полная индексация
                    fs.index.len().saturating_sub(1)
                }
            }
        };
        self.voff = last.saturating_sub(page - 1);
        self.hoff = 0;
    }

    /// Все совпадения запроса `q` (уже в нижнем регистре) во всём содержимом.
    pub fn search(&mut self, q: &[char]) -> Vec<(usize, usize)> {
        if q.is_empty() {
            return Vec::new();
        }
        match &mut self.src {
            Source::Memory(lines) => {
                let mut out = Vec::new();
                for (li, s) in lines.iter().enumerate() {
                    let lower: Vec<char> = s.chars().map(|c| c.to_ascii_lowercase()).collect();
                    let mut i = 0;
                    while i + q.len() <= lower.len() {
                        if lower[i..i + q.len()] == *q {
                            out.push((li, i));
                            i += q.len();
                        } else {
                            i += 1;
                        }
                    }
                }
                out
            }
            Source::File(fs) => fs.scan(Some(q)),
        }
    }
}

/// Заменяет непечатные (control) символы на `.`, табы → 4 пробела.
/// В ASCII-режиме просмотрщика непечатные байты не выводим в терминал.
pub(super) fn sanitize_line(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\t' {
                "    ".to_string()
            } else if c.is_control() {
                ".".to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Одна строка hex-дампа: `OFFSET  hh hh .. hh  hh .. hh  |text|` (до 16 байт).
pub(super) fn hex_line(offset: u64, chunk: &[u8]) -> String {
    let mut hex = String::new();
    let mut text = String::new();
    for (j, b) in chunk.iter().enumerate() {
        if j == 8 {
            hex.push(' ');
        }
        hex.push_str(&format!("{b:02x} "));
        let c = *b as char;
        text.push(if (0x20..0x7f).contains(b) { c } else { '.' });
    }
    let pad = 49_usize.saturating_sub(hex.len());
    format!("{:08x}  {}{}  |{}|", offset, hex, " ".repeat(pad), text)
}
