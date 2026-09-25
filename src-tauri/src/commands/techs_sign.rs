// Подпись списка инженеров (ECDSA P-256 / SHA-256). Публичный ключ вшит в exe
// (data/techs_pub.txt), приватный есть только у администратора (хранится DPAPI-шифрованным).
// Формат файла в репозитории: {"payload":"<JSON-строка>","sig":"<hex, 64 байта r||s>"}.
// Подписывается ровно строка payload (без канонизации JSON), поэтому проверка не зависит от
// порядка полей и пробелов. Модуль без зависимостей от Tauri — проверяется юнит-тестами
// в пустом cargo-проекте (см. CLAUDE.md).

use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Signed {
    pub payload: String,
    pub sig: String,
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

pub fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Проверяет подпись; true — payload подписан владельцем приватного ключа.
pub fn verify(pub_hex: &str, payload: &str, sig_hex: &str) -> bool {
    let (Some(pk), Some(sg)) = (hex_decode(pub_hex), hex_decode(sig_hex)) else { return false };
    let Ok(vk) = VerifyingKey::from_sec1_bytes(&pk) else { return false };
    let Ok(sig) = Signature::from_slice(&sg) else { return false };
    vk.verify(payload.as_bytes(), &sig).is_ok()
}

pub fn sign(priv_hex: &str, payload: &str) -> Result<String, String> {
    let bytes = hex_decode(priv_hex).ok_or("Ключ подписи: ожидается hex-строка")?;
    let sk = SigningKey::from_slice(&bytes).map_err(|_| "Ключ подписи повреждён (нужно 64 hex-символа)".to_string())?;
    let sig: Signature = sk.sign(payload.as_bytes());
    Ok(hex_encode(&sig.to_bytes()))
}

/// Публичный ключ (SEC1, несжатый, hex) по приватному — чтобы при импорте проверить, что ключ «тот самый».
pub fn public_of(priv_hex: &str) -> Result<String, String> {
    let bytes = hex_decode(priv_hex).ok_or("Ключ подписи: ожидается hex-строка")?;
    let sk = SigningKey::from_slice(&bytes).map_err(|_| "Ключ подписи повреждён (нужно 64 hex-символа)".to_string())?;
    Ok(hex_encode(sk.verifying_key().to_encoded_point(false).as_bytes()))
}


/// Срок годности списка: сколько секунд после последней проверки в сети (или сборки exe) обычные
/// инженеры ещё могут входить без сети; дальше — только администратор.
pub const TTL_SECS: i64 = 7 * 86400;

#[derive(Debug, PartialEq)]
pub struct Freshness {
    /// Когда список последний раз подтверждён (сеть или сборка), unix-секунды; 0 — неизвестно.
    pub fresh_at: i64,
    pub age_days: i64,
    pub expired: bool,
    /// Часы ПК «откатывали» назад — считаем список просроченным.
    pub clock_rollback: bool,
}

/// last_oks — метки последней успешной проверки в сети (из файлов на флешке и в %LOCALAPPDATA%),
/// max_seens — максимальное время, которое приложение когда-либо видело на часах,
/// baked_at — время сборки exe (когда в него вшит список). Метки из будущего (>сутки) игнорируются.
pub fn freshness(now: i64, last_oks: &[i64], max_seens: &[i64], baked_at: i64) -> Freshness {
    let sane = |t: &i64| *t > 0 && *t <= now + 86400;
    let fresh_at = last_oks.iter().copied().chain(std::iter::once(baked_at)).filter(sane).max().unwrap_or(0);
    let clock_rollback = max_seens.iter().any(|m| *m > now + 3600);
    let age = if fresh_at > 0 { (now - fresh_at).max(0) } else { i64::MAX / 4 };
    Freshness {
        fresh_at,
        age_days: if fresh_at > 0 { age / 86400 } else { -1 },
        expired: fresh_at == 0 || clock_rollback || age > TTL_SECS,
        clock_rollback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // фиксированный тестовый ключ (не используется нигде, кроме тестов)
    const PRIV: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn sign_and_verify() {
        let pubk = public_of(PRIV).unwrap();
        let p = r#"{"version":1,"issued_at":"2026-09-25T00:00:00Z","techs":[]}"#;
        let sig = sign(PRIV, p).unwrap();
        assert!(verify(&pubk, p, &sig));
        assert!(!verify(&pubk, &format!("{p} "), &sig), "любая правка payload ломает подпись");
        let mut bad = sig.clone();
        bad.replace_range(0..2, if &sig[0..2] == "00" { "01" } else { "00" });
        assert!(!verify(&pubk, p, &bad));
    }

    #[test]
    fn foreign_key_rejected() {
        let other = "0202020202020202020202020202020202020202020202020202020202020202";
        let p = "x";
        let sig = sign(other, p).unwrap();
        assert!(!verify(&public_of(PRIV).unwrap(), p, &sig));
    }

    #[test]
    fn garbage_is_false_not_panic() {
        assert!(!verify("zz", "x", "yy"));
        assert!(!verify("", "x", ""));
        assert!(sign("12", "x").is_err());
    }

    #[test]
    fn freshness_rules() {
        let now = 1_000_000_000;
        let d = 86400;
        // свежая проверка в сети
        assert!(!freshness(now, &[now - 2 * d], &[now - d], 0).expired);
        // сеть давно не видели, но exe собран недавно
        assert!(!freshness(now, &[now - 30 * d], &[], now - 3 * d).expired);
        // всё старше 7 суток
        let f = freshness(now, &[now - 8 * d], &[], now - 20 * d);
        assert!(f.expired && f.age_days == 8);
        // нет ни одной метки
        assert!(freshness(now, &[], &[], 0).expired);
        // метка из будущего игнорируется (нельзя «продлить» правкой файла)
        assert!(freshness(now, &[now + 400 * d], &[], now - 30 * d).expired);
        // откат часов назад
        let f = freshness(now, &[now - d], &[now + 10 * d], 0);
        assert!(f.expired && f.clock_rollback);
    }
}
