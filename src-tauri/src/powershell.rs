// Общий хелпер для вызова PowerShell и декодирования вывода.
// Вывод PowerShell на русских Windows часто приходит в cp1251, а не UTF-8 —
// та же проблема встречалась в echips-driver-assistant.

use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Выполняет PowerShell-команду и возвращает stdout, декодированный из cp1251
/// с заменой некорректных байт (errors="replace").
pub fn run_ps(script: &str) -> Result<String, String> {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| format!("Не удалось запустить PowerShell: {e}"))?;

    if !output.status.success() {
        let stderr = decode_cp1251(&output.stderr);
        return Err(format!("PowerShell завершился с ошибкой: {stderr}"));
    }

    Ok(decode_cp1251(&output.stdout))
}

/// Выполняет PowerShell-команду и парсит stdout как JSON (через `ConvertTo-Json`
/// на стороне скрипта).
pub fn run_ps_json<T: serde::de::DeserializeOwned>(script: &str) -> Result<T, String> {
    let raw = run_ps(script)?;
    serde_json::from_str(raw.trim()).map_err(|e| format!("Не удалось разобрать JSON: {e}\nВывод: {raw}"))
}

fn decode_cp1251(bytes: &[u8]) -> String {
    let (decoded, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
    decoded.into_owned()
}
