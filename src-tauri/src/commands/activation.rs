// Активация Windows: проверка статуса лицензии и штатное устранение проблем.
// Используются только легальные механизмы самой Windows: онлайн-активация,
// установка OEM-ключа, вшитого в BIOS (MSDM/OA3), и ключ, который вводит техник.
// Никаких обходов активации (эмуляторы KMS и т. п.) здесь нет и не будет.
// Все действия идут через WMI (SoftwareLicensingService/Product) — не через
// slmgr.vbs, чтобы не зависеть от кодовой страницы вывода.

use crate::powershell::{run_ps, run_ps_json};
use serde::{Deserialize, Serialize};

const APP_ID: &str = "55c92734-d682-4d71-983e-d6ec3f16059f";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ActivationStatus {
    /// Найден ли продукт Windows с установленным ключом
    #[serde(default)]
    pub found: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 0 нет лицензии, 1 лицензирована, 2 льготный период (OOB), 3 льготный (OOT),
    /// 4 льготный (не подлинная), 5 уведомление, 6 расширенный льготный период; -1 нет данных
    #[serde(default)]
    pub license_status: i32,
    #[serde(default)]
    pub partial_key: String,
    #[serde(default)]
    pub grace_minutes: i64,
    /// OEM:DM, Retail, Volume:GVLK и т. д.
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub kms_machine: String,
    /// HRESULT причины статуса, если Windows его сообщает
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub os_caption: String,
    #[serde(default)]
    pub os_build: String,
    /// Есть ли в BIOS вшитый OEM-ключ (сам ключ не передаётся — только последние 5 символов)
    #[serde(default)]
    pub oem_key_present: bool,
    #[serde(default)]
    pub oem_key_tail: String,
}

#[tauri::command(async)]
pub fn get_activation_status() -> Result<ActivationStatus, String> {
    #[cfg(target_os = "windows")]
    {
        // LicenseStatusReason — UInt32 (напр. 0xC004F034 = 3221549108 > Int32.Max): приведение к
        // [int] в скрипте ниже роняло весь запрос на неактивированной Windows ("Не удаётся
        // преобразовать значение … в тип System.Int32"), и автоустранение не запускалось.
        let script = format!(
            r#"
            $p = Get-CimInstance SoftwareLicensingProduct -Filter "ApplicationID='{APP_ID}' AND PartialProductKey IS NOT NULL" -ErrorAction SilentlyContinue | Select-Object -First 1
            $svc = Get-CimInstance SoftwareLicensingService -ErrorAction SilentlyContinue
            $os = Get-CimInstance Win32_OperatingSystem
            $oem = ''
            if ($svc -and $svc.OA3xOriginalProductKey) {{ $oem = [string]$svc.OA3xOriginalProductKey }}
            $reason = ''
            if ($p -and $p.LicenseStatusReason) {{ $reason = ('0x{{0:X8}}' -f [uint32]$p.LicenseStatusReason) }}
            [PSCustomObject]@{{
                found = ($null -ne $p)
                name = [string]$p.Name
                description = [string]$p.Description
                license_status = if ($p) {{ [int]$p.LicenseStatus }} else {{ -1 }}
                partial_key = [string]$p.PartialProductKey
                grace_minutes = if ($p) {{ [int64]$p.GracePeriodRemaining }} else {{ 0 }}
                channel = [string]$p.ProductKeyChannel
                kms_machine = [string]$p.KeyManagementServiceMachine
                reason = $reason
                os_caption = [string]$os.Caption
                os_build = [string]$os.BuildNumber
                oem_key_present = ($oem.Length -gt 0)
                oem_key_tail = if ($oem.Length -ge 5) {{ $oem.Substring($oem.Length - 5) }} else {{ '' }}
            }} | ConvertTo-Json -Compress
        "#
        );
        run_ps_json::<ActivationStatus>(&script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Проверка активации доступна только в Windows-сборке".to_string())
    }
}

/// Проверка формата ключа продукта: 5 групп по 5 символов A–Z/0–9.
fn valid_key(key: &str) -> bool {
    let parts: Vec<&str> = key.split('-').collect();
    parts.len() == 5 && parts.iter().all(|p| p.len() == 5 && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// Шаги устранения. Возвращает текст результата или ошибку от Windows.
/// step: "activate" | "install_oem_key" | "restart_service" | "sync_time" | "install_key" | "settings_troubleshoot"
#[tauri::command(async)]
pub fn run_activation_step(step: String, key: Option<String>) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let activate = format!(
            r#"
            $p = Get-CimInstance SoftwareLicensingProduct -Filter "ApplicationID='{APP_ID}' AND PartialProductKey IS NOT NULL" | Select-Object -First 1
            if ($null -eq $p) {{ throw 'Ключ продукта Windows не установлен' }}
            $null = Invoke-CimMethod -InputObject $p -MethodName Activate
        "#
        );
        let script = match step.as_str() {
            "activate" => format!("try {{ {activate}; 'Онлайн-активация выполнена' }} catch {{ throw $_.Exception.Message }}"),
            "install_oem_key" => format!(
                r#"
                try {{
                    $svc = Get-CimInstance SoftwareLicensingService
                    $k = [string]$svc.OA3xOriginalProductKey
                    if (-not $k) {{ throw 'В BIOS нет вшитого OEM-ключа' }}
                    $null = Invoke-CimMethod -InputObject $svc -MethodName InstallProductKey -Arguments @{{ ProductKey = $k }}
                    $null = Invoke-CimMethod -InputObject $svc -MethodName RefreshLicenseStatus
                    {activate}
                    'OEM-ключ из BIOS установлен, активация выполнена'
                }} catch {{ throw $_.Exception.Message }}
            "#
            ),
            "restart_service" => "try { Stop-Service -Name sppsvc -Force -ErrorAction SilentlyContinue; Start-Service -Name sppsvc -ErrorAction Stop; 'Служба лицензирования перезапущена' } catch { throw $_.Exception.Message }".to_string(),
            // То же, что делает техник руками: Параметры → Активация → кнопка
            // «Устранение неполадок» (штатное средство самой Windows, которое
            // умеет больше, чем WMI-шаги выше — например, привязку цифровой
            // лицензии после замены платы). Программного API у него нет, поэтому
            // открываем страницу и нажимаем кнопку через UI Automation
            // (InvokePattern) — по имени, на русском и английском интерфейсе.
            // Ждём до 20 с появления окна и кнопки; сам результат устранения
            // проверяет вызывающий код (опрос get_activation_status).
            "settings_troubleshoot" => r#"
                try {
                    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
                    Start-Process 'ms-settings:activation'
                    $root = [System.Windows.Automation.AutomationElement]::RootElement
                    $frameCond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'ApplicationFrameWindow')
                    $btn = $null
                    $deadline = (Get-Date).AddSeconds(20)
                    while (-not $btn -and (Get-Date) -lt $deadline) {
                        Start-Sleep -Milliseconds 800
                        foreach ($w in $root.FindAll([System.Windows.Automation.TreeScope]::Children, $frameCond)) {
                            if ($w.Current.Name -notmatch 'Параметры|Settings') { continue }
                            foreach ($e in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
                                if ($e.Current.Name -match 'Устранени[ея] неполадок|Устранить|Troubleshoot' -and $e.Current.ControlType.ProgrammaticName -match 'Button|Hyperlink') { $btn = $e; break }
                            }
                            if ($btn) { break }
                        }
                    }
                    if (-not $btn) { throw 'Кнопка устранения неполадок не найдена в Параметрах (Windows уже активирована, либо другой язык/сборка интерфейса)' }
                    $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
                    'Нажата кнопка «' + $btn.Current.Name + '» в Параметрах'
                } catch { throw $_.Exception.Message }
            "#.to_string(),
            "sync_time" => "try { Set-Service -Name w32time -StartupType Automatic -ErrorAction SilentlyContinue; Start-Service -Name w32time -ErrorAction SilentlyContinue; $null = & w32tm /resync /force 2>&1; 'Синхронизация времени запрошена' } catch { throw $_.Exception.Message }".to_string(),
            "install_key" => {
                let k = key.unwrap_or_default().trim().to_uppercase();
                if !valid_key(&k) {
                    return Err("Ключ должен быть в формате XXXXX-XXXXX-XXXXX-XXXXX-XXXXX".to_string());
                }
                format!(
                    r#"
                    try {{
                        $svc = Get-CimInstance SoftwareLicensingService
                        $null = Invoke-CimMethod -InputObject $svc -MethodName InstallProductKey -Arguments @{{ ProductKey = '{k}' }}
                        $null = Invoke-CimMethod -InputObject $svc -MethodName RefreshLicenseStatus
                        {activate}
                        'Ключ установлен, активация выполнена'
                    }} catch {{ throw $_.Exception.Message }}
                "#
                )
            }
            _ => return Err(format!("Неизвестный шаг: {step}")),
        };
        run_ps(&script)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (step, key, valid_key(""));
        Err("Доступно только в Windows-сборке".to_string())
    }
}

/// Открывает штатную страницу активации в Параметрах Windows.
#[tauri::command(async)]
pub fn open_activation_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .arg("ms-settings:activation")
            .creation_flags(0x08000000)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Доступно только в Windows-сборке".to_string())
    }
}
