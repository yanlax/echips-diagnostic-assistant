// Общий хелпер для вызова PowerShell и декодирования вывода.
//
// Ранее здесь декодировался вывод как cp1251 — это и давало кракозябры
// ("Ь ©Ea©6©дв Windows 11 Pro" вместо "Microsoft Windows 11 Pro"). Правильный
// подход (проверенный в echips-driver-assistant): принудительно переключить
// кодировку консоли PowerShell на UTF-8 в начале самого скрипта, а вывод
// читать как UTF-8. Кодировка вывода определяется тем, что просит сам
// процесс, а не системной локалью — поэтому декодировать нужно то, что мы
// сами же и попросили.

use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const UTF8_PREAMBLE: &str = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8;";

fn build_command(script: &str) -> Command {
    let full_script = format!("{UTF8_PREAMBLE} {script}");
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full_script]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd
}

/// Выполняет PowerShell-команду и возвращает stdout как UTF-8 (с заменой
/// некорректных байт), triml-нутый.
pub fn run_ps(script: &str) -> Result<String, String> {
    let output = build_command(script)
        .output()
        .map_err(|e| format!("Не удалось запустить PowerShell: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("PowerShell завершился с ошибкой: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Как run_ps, но также возвращает, завершилась ли команда успешно (код 0) —
/// нужно там, где важен сам факт успеха, а не только текст (точка
/// восстановления, установка драйверов).
pub fn run_ps_with_status(script: &str) -> (String, bool) {
    let full_script = format!("{script}; exit $LASTEXITCODE");
    match build_command(&full_script).output() {
        Ok(out) => (String::from_utf8_lossy(&out.stdout).trim().to_string(), out.status.success()),
        Err(_) => (String::new(), false),
    }
}

/// Выполняет PowerShell-команду и парсит stdout как JSON (через `ConvertTo-Json`
/// на стороне скрипта).
pub fn run_ps_json<T: serde::de::DeserializeOwned>(script: &str) -> Result<T, String> {
    let raw = run_ps(script)?;
    if raw.trim().is_empty() {
        return Err("PowerShell не вернул данных".to_string());
    }
    serde_json::from_str(raw.trim()).map_err(|e| format!("Не удалось разобрать JSON: {e}\nВывод: {raw}"))
}
