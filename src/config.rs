//! Внешняя конфигурация (TOML) по XDG.
//! Путь: env `RFM_CONFIG` → `$XDG_CONFIG_HOME/rfm/config.toml` → `~/.config/rfm/config.toml`.
//! При отсутствии файла на старте создаётся дефолтный (со встроенными темами).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::{env, fs, io, path::PathBuf};

/// Раскладка панелей.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PanelLayout {
    #[default]
    Vertical,
    Horizontal,
}

/// Формат показа списка файлов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FileListView {
    #[default]
    Flat,
    Tree,
}

/// Пауза после выполнения команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PauseMode {
    Always,
    #[default]
    OnOutput,
    Never,
}

/// Когда сохранять конфиг.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SaveMode {
    /// Писать файл после каждого изменения.
    Always,
    /// Писать при закрытии экрана настроек, если что-то менялось.
    #[default]
    OnChange,
    /// Писать только по явному Save.
    OnSave,
}

/// Описание одной цветовой темы из конфига (секция `[[themes]]`).
/// Цвета — строки: `#RRGGBB` или имя (`black`, `white`, `yellow`, `gray`, …).
/// Пустое/неверное значение — берётся из дефолтной тёмной темы.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub bg: String,
    pub fg: String,
    pub cursor_bg: String,
    pub cursor_fg: String,
    pub mark_fg: String,
    /// Цвета командной строки (по умолчанию наследуют bg/fg).
    pub cmdline_bg: String,
    pub cmdline_fg: String,
}

/// Описание одного типа архива (секция `[[archive]]`).
/// `name` — тип (выбирает встроенный парсер), `extensions` — glob-паттерны расширений,
/// `list_command` — команда листинга (шеллится, к ней дописывается путь к файлу).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ArchiveConfig {
    pub name: String,
    pub extensions: Vec<String>,
    pub list_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub panel_layout: PanelLayout,
    pub show_hidden: bool,
    /// Показывать часы (hh:mm) в правом верхнем углу на рамке.
    pub show_clock: bool,
    pub file_list_view: FileListView,
    pub theme: String,
    pub pause_after_command: PauseMode,
    /// Просмотр файла: `internal` (встроенный) или путь к программе (напр. `/usr/bin/less`).
    pub view: String,
    /// Редактирование файла: `internal` или путь к программе (напр. `/usr/bin/vim`).
    pub edit: String,
    /// Просмотрщик: открывать по умолчанию в hex-дампе.
    pub view_hex: bool,
    /// Просмотрщик: включать по умолчанию перенос длинных строк.
    pub view_wrap: bool,
    /// Просмотрщик: порог размера файла (МБ) для prettify (`p`); больше — не форматируем.
    pub view_prettify_max_mb: u64,
    /// Когда сохранять конфиг: `always` / `on-change` / `on-save`.
    pub config_save: SaveMode,
    /// Типы архивов (секции `[[archive]]`). Массив таблиц — после всех скаляров.
    #[serde(rename = "archive")]
    pub archives: Vec<ArchiveConfig>,
    /// Цветовые темы (секции `[[themes]]`). Активная выбирается ключом `theme`.
    /// Должно оставаться последним полем — TOML требует таблицы/массивы после скаляров.
    pub themes: Vec<ThemeConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            panel_layout: PanelLayout::default(),
            show_hidden: true,
            show_clock: true,
            file_list_view: FileListView::default(),
            theme: "default".to_string(),
            pause_after_command: PauseMode::default(),
            view: "internal".to_string(),
            edit: "internal".to_string(),
            view_hex: false,
            view_wrap: false,
            view_prettify_max_mb: 50,
            config_save: SaveMode::default(),
            archives: Vec::new(),
            themes: Vec::new(),
        }
    }
}

/// Встроенные типы архивов для дефолтного конфига: zip и tar-семейство.
pub fn builtin_archives() -> Vec<ArchiveConfig> {
    vec![
        ArchiveConfig {
            name: "zip".to_string(),
            extensions: vec!["zip".to_string()],
            list_command: "unzip -l".to_string(),
        },
        ArchiveConfig {
            name: "tar".to_string(),
            extensions: vec!["tar".to_string()],
            list_command: "tar -tvf".to_string(),
        },
        ArchiveConfig {
            name: "tar".to_string(),
            extensions: vec!["tar.gz".to_string(), "tgz".to_string()],
            list_command: "tar -tzvf".to_string(),
        },
        ArchiveConfig {
            name: "tar".to_string(),
            extensions: vec!["tar.bz2".to_string(), "tbz2".to_string()],
            list_command: "tar -tjvf".to_string(),
        },
        ArchiveConfig {
            name: "7z".to_string(),
            extensions: vec!["7z".to_string()],
            list_command: "7z l".to_string(),
        },
        ArchiveConfig {
            name: "rar".to_string(),
            // .rar и многотомные старого стиля .r00/.r01…
            extensions: vec!["rar".to_string(), "r[0-9][0-9]".to_string()],
            list_command: "unrar l".to_string(),
        },
    ]
}

/// Встроенные темы для дефолтного конфига: тёмная `default` и синяя `mc` (в духе mc/Norton).
pub fn builtin_themes() -> Vec<ThemeConfig> {
    vec![
        ThemeConfig {
            name: "default".to_string(),
            bg: "#262626".to_string(),
            fg: "#dcdcdc".to_string(),
            cursor_bg: "#b4b4b4".to_string(),
            cursor_fg: "black".to_string(),
            mark_fg: "yellow".to_string(),
            cmdline_bg: "#262626".to_string(),
            cmdline_fg: "#dcdcdc".to_string(),
        },
        ThemeConfig {
            name: "mc".to_string(),
            bg: "#0000aa".to_string(),
            fg: "#c0c0c0".to_string(),
            cursor_bg: "#00aaaa".to_string(),
            cursor_fg: "black".to_string(),
            mark_fg: "yellow".to_string(),
            cmdline_bg: "black".to_string(),
            cmdline_fg: "white".to_string(),
        },
    ]
}

/// Дефолтный конфиг для первого запуска — с встроенными темами.
pub fn builtin() -> Config {
    Config {
        archives: builtin_archives(),
        themes: builtin_themes(),
        ..Config::default()
    }
}

/// Путь к файлу конфигурации по правилам XDG (без проверки наличия).
pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = env::var("RFM_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if let Ok(x) = env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("rfm/config.toml"));
        }
    }
    dirs::home_dir().map(|h| h.join(".config/rfm/config.toml"))
}

/// Сохраняет конфиг в файл (создавая директории). Best effort.
pub fn save(cfg: &Config) -> io::Result<()> {
    let Some(path) = config_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, text)
}

/// Добавляет секции для поддерживаемых архиваторов (`zip`/`tar` — то, что мы поставляем
/// и умеем парсить), если для них **нет ни одной** секции в конфиге. Архиватор, уже
/// заведённый пользователем (пусть иначе), не трогаем — сравнение по имени (`name`).
/// Возвращает `true`, если что-то добавили (тогда конфиг стоит пересохранить).
pub fn merge_archive_defaults(cfg: &mut Config) -> bool {
    // снимок уже настроенных архиваторов — чтобы несколько дефолтных секций одного
    // архиватора (напр. три `tar`) не считали друг друга «уже присутствующими».
    let present: HashSet<String> = cfg.archives.iter().map(|a| a.name.clone()).collect();
    let mut added = false;
    for def in builtin_archives() {
        if !present.contains(&def.name) {
            cfg.archives.push(def);
            added = true;
        }
    }
    added
}

/// Загружает конфиг. Если файла нет — создаёт дефолтный (со встроенными темами) и сохраняет.
/// Если для поддерживаемого архиватора нет секций — добавляет их и сохраняет конфиг.
pub fn load() -> Config {
    match config_path() {
        Some(p) if p.exists() => match fs::read_to_string(&p) {
            Ok(text) => {
                let mut cfg: Config = toml::from_str(&text).unwrap_or_default();
                if merge_archive_defaults(&mut cfg) {
                    let _ = save(&cfg);
                }
                cfg
            }
            // Файл не прочитался — не перезаписываем; дефолты архивов в памяти для работы.
            Err(_) => {
                let mut cfg = Config::default();
                merge_archive_defaults(&mut cfg);
                cfg
            }
        },
        _ => {
            let cfg = builtin();
            let _ = save(&cfg);
            cfg
        }
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;
