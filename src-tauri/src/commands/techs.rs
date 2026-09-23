// Общий PIN-экран при запуске программы (см. CLAUDE.md, задача №2): список
// инженеров и хэшей их PIN лежит не в коде, а в публичном репозитории
// (data/techs.json на GitHub) — тот же принцип, что уже работает для
// авто-обновления (см. update.rs): правка файла в репозитории вместо
// пересборки и релиза приложения. Список подтягивается заново при каждом
// запуске, так что новый инженер (или изменённый PIN) появляется сразу на
// всех станциях без обновления .exe.
//
// Здесь — только скачивание и кэширование списка. Сам PIN нигде не хранится
// и не передаётся в открытом виде: только SHA-256(salt+":"+pin), сравнение
// введённого PIN с хэшем происходит на стороне JS (src/app.js, sha256Hex +
// lockSubmit) через Web Crypto API — тем же способом, каким этот хэш и
// генерируется на экране "Добавить инженера" (app.js, techadminGenerate),
// поэтому дублировать алгоритм хэширования в Rust не нужно.
//
// Файл в репозитории — публичный, как и сами релизы. Это осознанный выбор
// (см. обсуждение с пользователем): PIN нужен только как экран входа для
// сервисной станции, а не как криптографическая защита данных, поэтому
// достаточно, чтобы сам PIN нельзя было восстановить из хэша (соль + SHA-256
// делают перебор по радужным таблицам бессмысленным; для реальной защиты от
// прямого перебора PIN должен быть длиннее 4 цифр — см. пример в data/techs.json).

use serde::{Deserialize, Serialize};

fn default_role() -> String {
    "tech".to_string()
}

/// role: "tech" (по умолчанию, если поля нет в старой записи/кэше — обратная
/// совместимость) или "admin" — админские фичи (например, панель команд по
/// Shift+F10) показываются только при role=="admin", см. app.js.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tech {
    pub id: String,
    pub name: String,
    pub pin_hash: String,
    pub salt: String,
    #[serde(default = "default_role")]
    pub role: String,
}

const TECHS_URL: &str =
    "https://raw.githubusercontent.com/yanlax/echips-diagnostic-assistant/main/data/techs.json";

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base)
        .join("Echips")
        .join("HardwareCheck")
        .join("techs_cache.json")
}

/// Сеть недоступна на сервисной станции — не редкость (мастерская без
/// интернета), поэтому при неудаче используем последний успешно
/// загруженный список из локального кэша, а не блокируем вход целиком.
#[tauri::command(async)]
pub async fn fetch_techs() -> Result<Vec<Tech>, String> {
    let client = reqwest::Client::new();
    let fetched = client
        .get(TECHS_URL)
        .header("User-Agent", "echips-diagnostic-app")
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success());

    if let Some(resp) = fetched {
        if let Ok(text) = resp.text().await {
            if let Ok(list) = serde_json::from_str::<Vec<Tech>>(&text) {
                if let Some(parent) = cache_path().parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(cache_path(), &text);
                return Ok(list);
            }
        }
    }

    match std::fs::read_to_string(cache_path()) {
        Ok(text) => serde_json::from_str::<Vec<Tech>>(&text)
            .map_err(|e| format!("Сохранённый список инженеров повреждён: {e}")),
        Err(_) => Err(
            "Нет сети и нет ранее сохранённого списка инженеров. Подключите станцию к интернету хотя бы один раз."
                .to_string(),
        ),
    }
}
