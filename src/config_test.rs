//! Юнит-тесты модуля `config` 

use super::*;

#[test]
fn builtin_serializes_and_reparses_with_themes() {
    let cfg = builtin();
    let text = toml::to_string_pretty(&cfg).unwrap();
    let back: Config = toml::from_str(&text).unwrap();
    assert_eq!(back.themes.len(), 2);
    assert_eq!(back.themes[1].name, "mc");
    assert_eq!(back.theme, "default");
}

#[test]
fn show_clock_defaults_true_and_roundtrips() {
    assert!(Config::default().show_clock); // дефолт — показывать
    // Отсутствие ключа → дефолт true; явное false — сохраняется.
    let cfg: Config = toml::from_str("").unwrap();
    assert!(cfg.show_clock);
    let cfg: Config = toml::from_str("show_clock = false\n").unwrap();
    assert!(!cfg.show_clock);
}

#[test]
fn save_mode_parses_kebab() {
    let cfg: Config = toml::from_str("config_save = \"on-save\"\n").unwrap();
    assert_eq!(cfg.config_save, SaveMode::OnSave);
}

#[test]
fn save_and_reload_from_disk() {
    let tmp = env::temp_dir().join(format!("rfm_cfgtest_{}.toml", std::process::id()));
    env::set_var("RFM_CONFIG", &tmp);
    let mut cfg = builtin();
    cfg.config_save = SaveMode::Always;
    cfg.view = "/usr/bin/less".to_string();
    save(&cfg).unwrap();
    let back = load();
    assert_eq!(back.config_save, SaveMode::Always);
    assert_eq!(back.view, "/usr/bin/less");
    env::remove_var("RFM_CONFIG");
    fs::remove_file(&tmp).ok();
}
