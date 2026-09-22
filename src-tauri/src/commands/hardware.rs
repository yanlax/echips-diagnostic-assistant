// USB / Wi-Fi / Bluetooth / отпечаток пальца — перечисление через WMI /
// Get-PnpDevice. Это то, что реально доступно без сторонних библиотек:
// список устройств и их статус, а не полноценный bandwidth-тест порта
// (тот, что описан в дизайн-прототипе как "запись/чтение с контрольной
// суммой") — для него нужно физически знать, какое устройство воткнуто
// в тестовый порт, это отдельная задача на будущее.

use crate::powershell::run_ps;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PnpDevice {
    pub name: String,
    pub status: String,
}

fn parse_pnp_lines(raw: &str) -> Vec<PnpDevice> {
    // Формат строки: "Имя устройства || Статус" — задаём его сами в
    // PowerShell-скрипте через -join, чтобы не тащить JSON-парсинг для
    // списка произвольной длины.
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, "||");
            let name = parts.next()?.trim().to_string();
            let status = parts.next().unwrap_or("").trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(PnpDevice { name, status })
            }
        })
        .collect()
}

#[tauri::command(async)]
pub fn list_usb_devices() -> Result<Vec<PnpDevice>, String> {
    #[cfg(target_os = "windows")]
    {
        let raw = run_ps(
            "Get-PnpDevice -Class USB -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName } | \
             ForEach-Object { \"$($_.FriendlyName)||$($_.Status)\" }; exit 0",
        )?;
        Ok(parse_pnp_lines(&raw))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[tauri::command(async)]
pub fn list_bluetooth_devices() -> Result<Vec<PnpDevice>, String> {
    #[cfg(target_os = "windows")]
    {
        let raw = run_ps(
            "Get-PnpDevice -Class Bluetooth -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName } | \
             ForEach-Object { \"$($_.FriendlyName)||$($_.Status)\" }; exit 0",
        )?;
        Ok(parse_pnp_lines(&raw))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkAdapter {
    pub name: String,
    pub status: String,
    pub mac: String,
}

#[tauri::command(async)]
pub fn list_wifi_adapters() -> Result<Vec<NetworkAdapter>, String> {
    #[cfg(target_os = "windows")]
    {
        // -Physical — как и в list_lan_adapters: без него в списке попадаются
        // виртуальные адаптеры (Microsoft Wi-Fi Direct Virtual Adapter),
        // у которых статус почти всегда "Not Present" — реальный физический
        // адаптер при этом мог быть подключён и рабочим, но тест всё равно
        // считался проваленным из-за виртуального "соседа" (реальный отчёт).
        let raw = run_ps(
            "Get-NetAdapter -Physical | Where-Object { $_.InterfaceDescription -match 'Wireless|Wi-?Fi|802.11' } | \
             ForEach-Object { \"$($_.Name)||$($_.Status)||$($_.MacAddress)\" }",
        )?;
        Ok(parse_adapters(&raw))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

fn parse_adapters(raw: &str) -> Vec<NetworkAdapter> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, "||");
            let name = parts.next()?.trim().to_string();
            let status = parts.next().unwrap_or("").trim().to_string();
            let mac = parts.next().unwrap_or("").trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(NetworkAdapter { name, status, mac })
            }
        })
        .collect()
}

/// Список видимых Wi-Fi сетей через `netsh wlan show networks` — покрывает
/// то же, что в дизайн-прототипе ("скан сетей"), без реального замера
/// скорости (нужна тестовая точка доступа и известный SSID, это конфигурация
/// конкретного сервисного центра, а не то, что можно захардкодить).
#[tauri::command(async)]
pub fn scan_wifi_networks() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let raw = run_ps(
            "(netsh wlan show networks) | Select-String 'SSID [0-9]+ :' | \
             ForEach-Object { ($_ -split ':',2)[1].Trim() }",
        )?;
        Ok(raw.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}

/// Наличие сенсора отпечатка пальца, видимого системе через WinBio (класс
/// Biometric в PnP). Пробную регистрацию/сравнение штатными средствами без
/// диалогов Windows Hello не вызвать — это техник делает вручную и отмечает
/// результат сам.
#[tauri::command(async)]
pub fn get_fingerprint_sensor() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let raw = run_ps(
            "Get-PnpDevice -Class Biometric -PresentOnly -ErrorAction SilentlyContinue | \
             Where-Object { $_.FriendlyName } | Select-Object -First 1 -ExpandProperty FriendlyName; exit 0",
        )?;
        let trimmed = raw.trim().to_string();
        Ok(if trimmed.is_empty() { None } else { Some(trimmed) })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}
