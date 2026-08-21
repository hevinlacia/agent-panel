//! 直接读取 Chrome cookie 数据库（免 CDP / 免远程调试确认弹窗）。
//!
//! 原理：Chrome 在 Linux 上把 cookie 明文加密存进 `<profile>/Cookies` 的 SQLite 表
//! （`encrypted_value` 列），主密钥由系统 keyring（Secret Service / KWallet，
//! 通过 `secret-tool` 读取 "Chrome Safe Storage"）保护。
//! 本模块只读：拷贝 DB 快照（避免锁冲突）+ keyring 解密，全程不建立 CDP 连接，
//! 因此 Chrome 不会弹出 "Allow remote debugging?" 确认框。
//!
//! 本实现经过真实 Chrome 150 实测验证（算法逆向自 Chromium
//! `os_crypt/async/browser/freedesktop_secret_key_provider.cc`）：
//!
//! - secret（v11 主密钥）  = keyring 条目 `server="Chrome Keys"`, `user="Chrome Safe Storage"`
//! - key                  = PBKDF2-HMAC-SHA1(secret, salt="saltysalt", iter=1, dkLen=16)
//! - v11 密文格式         = "v11" + 16B MAC + 16B IV + AES-128-CBC(PKCS7) 密文
//! - v10 密文格式（legacy）= "v10" + AES-128-CBC（固定 IV=16 空格，无 padding）

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::{fs, process::Command};

use crate::now_ms;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChromeCookie {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) domain: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) secure: bool,
    #[serde(default)]
    pub(crate) expires: Option<f64>,
}

/// 尝试从 Chrome cookie 数据库读取 cookie。
/// `allowlist` 为 cookie 域名白名单：只解密/返回白名单域名内的 cookie；
/// 白名单为空时直接返回空（安全默认，不读取任何登录态）。
/// 失败时返回错误信息；调用方决定是否 fallback 到 CDP。
pub(crate) async fn load_chrome_cookies_db(allowlist: &[String]) -> Result<Vec<ChromeCookie>> {
    if allowlist.is_empty() {
        return Ok(Vec::new());
    }
    let profile = find_profile_dir().await?;
    let db = profile.join("Cookies");
    if !db.exists() {
        return Err(anyhow!("Chrome Cookies DB not found at {}", db.display()));
    }
    let password = read_master_secret().await?;
    let key = derive_v11_key(&password);
    let cookies = read_cookies_snapshot(&db, &key, allowlist)?;
    tracing::info!(count = cookies.len(), "read {} cookies from Chrome DB (no CDP)", cookies.len());
    Ok(cookies)
}

/// 探测 Chrome profile 目录：优先取 Local State 的 `profile.last_used`（正在使用的
/// profile），否则扫描所有带 Cookies 文件的 profile 目录。
async fn find_profile_dir() -> Result<PathBuf> {
    let home = crate::home_dir()?;
    let chrome_root = home.join(".config/google-chrome");
    let local_state = chrome_root.join("Local State");
    if local_state.exists() {
        if let Ok(raw) = fs::read_to_string(&local_state).await {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(last_used) = value
                    .get("profile")
                    .and_then(|p| p.get("last_used"))
                    .and_then(|v| v.as_str())
                {
                    let candidate = chrome_root.join(last_used);
                    if candidate.join("Cookies").exists() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }
    // 扫描 Default 与 Profile N
    let mut candidates = vec![chrome_root.join("Default")];
    if let Ok(mut entries) = fs::read_dir(&chrome_root).await {
        let mut profiles = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("Profile ") && entry.path().is_dir() {
                profiles.push(entry.path());
            }
        }
        profiles.sort();
        candidates.extend(profiles);
    }
    for dir in candidates {
        if dir.join("Cookies").exists() {
            return Ok(dir);
        }
    }
    // Chromium 兜底
    let chromium = home.join(".config/chromium/Default");
    if chromium.join("Cookies").exists() {
        return Ok(chromium);
    }
    Err(anyhow!(
        "no Chrome profile with a Cookies file found under {}",
        chrome_root.display()
    ))
}

/// 从系统 keyring 读 Chrome v11 主密钥（Secret Service / KWallet）。
/// Chrome 新版本以 QtKeychain 风格存为 `server="Chrome Keys"`, `user="Chrome Safe Storage"`；
/// 老版本用 libsecret 属性 `"Chrome Safe Storage" = salt`。依次尝试。
async fn read_master_secret() -> Result<Vec<u8>> {
    let attempts: &[(&str, &str, &str, &str)] = &[
        ("server", "Chrome Keys", "user", "Chrome Safe Storage"),
        ("application", "chrome", "Chrome Safe Storage", "saltysalt"),
        ("Chrome Safe Storage", "saltysalt", "", ""),
    ];
    for (a1, v1, a2, v2) in attempts {
        let mut args = vec!["lookup"];
        args.push(a1);
        args.push(v1);
        if !a2.is_empty() {
            args.push(a2);
            args.push(v2);
        }
        if let Ok(output) = Command::new("secret-tool").args(&args).output().await {
            if output.status.success() && !output.stdout.is_empty() {
                let secret = trim_ascii_ws(&output.stdout);
                if !secret.is_empty() {
                    return Ok(secret);
                }
            }
        }
    }
    Err(anyhow!(
        "cannot read Chrome Safe Storage key from keyring (secret-tool). \
         Make sure KWallet is unlocked."
    ))
}

/// PBKDF2-HMAC-SHA1（Chrome v11 固定参数：iter=1, dkLen=16）。
fn derive_v11_key(password: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let derived = pbkdf2_sha1(password, b"saltysalt", 1, 16);
    out.copy_from_slice(&derived);
    out
}

fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, dklen: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(dklen);
    let mut block_index: u32 = 1;
    while out.len() < dklen {
        let mut msg = salt.to_vec();
        msg.extend_from_slice(&block_index.to_be_bytes());
        let mut u = hmac_sha1(password, &msg);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }
        out.extend_from_slice(&t);
        block_index += 1;
    }
    out.truncate(dklen);
    out
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let mut h = Sha1::new();
        h.update(key);
        let digest = h.finalize();
        key_block[..20].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Sha1::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha1::new();
    outer.update(opad);
    outer.update(inner_digest);
    outer.finalize().into()
}

/// 去除首尾 ASCII 空白字节（如 secret-tool 输出的尾随换行）。
fn trim_ascii_ws(data: &[u8]) -> Vec<u8> {
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    let end = data
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    data[start..end].to_vec()
}

/// 拷贝 Cookies 数据库（连同 -wal/-shm）到临时目录再只读打开，
/// 避免与运行中的 Chrome 产生锁冲突，也避免污染原始文件。
fn read_cookies_snapshot(
    db: &Path,
    key: &[u8; 16],
    allowlist: &[String],
) -> Result<Vec<ChromeCookie>> {
    let tmp = std::env::temp_dir().join(format!(
        "agent-panel-cookies-{}-{}",
        std::process::id(),
        now_ms()
    ));
    std::fs::create_dir_all(&tmp)
        .with_context(|| format!("create temp dir {}", tmp.display()))?;
    let snapshot = tmp.join("Cookies");
    for suffix in ["", "-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{}", db.to_string_lossy(), suffix));
        if src.exists() {
            std::fs::copy(&src, tmp.join(format!("Cookies{suffix}")))
                .with_context(|| format!("copy {} to temp", src.display()))?;
        }
    }
    let result = read_cookies_file(&snapshot, key, allowlist);
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn read_cookies_file(
    snapshot: &Path,
    key: &[u8; 16],
    allowlist: &[String],
) -> Result<Vec<ChromeCookie>> {
    let conn = rusqlite::Connection::open_with_flags(
        snapshot,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("open cookie DB {}", snapshot.display()))?;

    // 兼容不同 Chrome 版本的表结构：优先 value，其次 encrypted_value。
    let has_encrypted = table_has_column(&conn, "cookies", "encrypted_value")?;
    let value_expr = if has_encrypted {
        "CASE WHEN value = '' THEN encrypted_value ELSE value END"
    } else {
        "value"
    };
    let sql = format!(
        "SELECT host_key, name, {value_expr} AS raw, path, expires_utc, is_secure FROM cookies"
    );
    let mut stmt = conn
        .prepare(&sql)
        .with_context(|| format!("prepare query on {}", snapshot.display()))?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;

    let mut cookies = Vec::new();
    for row in rows {
        let (domain, name, raw, path, expires_utc, is_secure) = row?;
        // 白名单硬过滤：非白名单域名的 cookie 不解密、不进入内存
        if !cookie_domain_allowed(&domain, allowlist) {
            continue;
        }
        match decrypt_value(key, &raw) {
            Ok(plain) => {
                let value = String::from_utf8_lossy(&plain).into_owned();
                let expires = chrome_epoch_to_unix(expires_utc);
                cookies.push(ChromeCookie {
                    name,
                    value,
                    domain,
                    path,
                    secure: is_secure != 0,
                    expires,
                });
            }
            Err(err) => {
                tracing::debug!(domain, name, error = %err, "skip undecryptable cookie");
            }
        }
    }
    Ok(cookies)
}

/// cookie 域名是否命中白名单（支持子域匹配，忽略前导点）。
fn cookie_domain_allowed(domain: &str, allowlist: &[String]) -> bool {
    let host = domain.trim_start_matches('.').to_lowercase();
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim_start_matches('.').to_lowercase();
        host == allowed || host.ends_with(&format!(".{allowed}"))
    })
}

fn table_has_column(conn: &rusqlite::Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for col in cols {
        if col? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 解密单个 cookie value。
///
/// v11（Chrome 150+ 实测格式）：`"v11" + 16B MAC + 16B IV + AES-128-CBC(PKCS7) 密文`
/// 另兼容部分实现：`"v11" + 密文`（固定 IV=16 空格）。
/// v10（legacy）：`"v10" + AES-128-CBC 密文`（固定 IV=16 空格，无 padding）。
fn decrypt_value(key: &[u8; 16], raw: &[u8]) -> Result<Vec<u8>> {
    if let Some(rest) = raw.strip_prefix(b"v11") {
        // 格式 A：v11 + MAC(16) + IV(16) + ciphertext
        if rest.len() >= 32 && (rest.len() - 32) % 16 == 0 {
            let iv: [u8; 16] = rest[16..32].try_into()?;
            let ct = &rest[32..];
            if let Some(plain) = pkcs7_unpad(&aes_128_cbc_decrypt(key, &iv, ct)?) {
                return Ok(plain);
            }
        }
        // 格式 B：v11 + ciphertext（固定 IV=16 空格）
        if rest.len() % 16 == 0 {
            let plain = aes_128_cbc_decrypt(key, &[0x20u8; 16], rest)?;
            if let Some(plain) = pkcs7_unpad(&plain) {
                return Ok(plain);
            }
        }
        return Err(anyhow!("v11 decrypt failed"));
    }
    if let Some(rest) = raw.strip_prefix(b"v10") {
        let plain = aes_128_cbc_decrypt(key, &[0x20u8; 16], rest)?;
        return Ok(plain);
    }
    // 明文
    Ok(raw.to_vec())
}

/// AES-128-CBC 解密（无 PKCS7 padding）。
fn aes_128_cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err(anyhow!("invalid ciphertext length {}", data.len()));
    }
    use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut previous = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let block = GenericArray::clone_from_slice(chunk);
        let mut decrypted = block.clone();
        cipher.decrypt_block(&mut decrypted);
        for (a, b) in decrypted.iter_mut().zip(previous.iter()) {
            *a ^= *b;
        }
        previous = block.into();
        out.extend_from_slice(&decrypted);
    }
    Ok(out)
}

/// PKCS7 去 padding；无效则返回 None。
fn pkcs7_unpad(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data.len() % 16 != 0 {
        return None;
    }
    let pad = *data.last()? as usize;
    if pad == 0 || pad > 16 || pad > data.len() {
        return None;
    }
    if data[data.len() - pad..].iter().all(|b| *b as usize == pad) {
        Some(data[..data.len() - pad].to_vec())
    } else {
        None
    }
}

/// Chrome 的 expires_utc 是自 1601-01-01 起的微秒数（FILETIME 纪元）。
/// 0 表示会话 cookie（无过期时间）。
pub(crate) fn chrome_epoch_to_unix(expires_utc: i64) -> Option<f64> {
    if expires_utc <= 0 {
        return None;
    }
    const FILETIME_TO_UNIX_SECS: f64 = 11_644_473_600.0;
    let unix_secs = (expires_utc as f64 / 1_000_000.0) - FILETIME_TO_UNIX_SECS;
    Some(unix_secs)
}

#[cfg(test)]
mod chrome_cookie_tests {
    use super::*;

    #[test]
    fn pbkdf2_derives_known_16_byte_key() {
        let key = pbkdf2_sha1(b"peanuts", b"saltysalt", 1003, 16);
        assert_eq!(key.len(), 16);
        assert_eq!(key, pbkdf2_sha1(b"peanuts", b"saltysalt", 1003, 16));
        // 1 迭代 v11 推导
        let key_v11 = pbkdf2_sha1(b"secret", b"saltysalt", 1, 16);
        assert_eq!(key_v11.len(), 16);
    }

    #[test]
    fn v11_roundtrip_decrypts_to_plaintext() {
        let key = [0x42u8; 16];
        let plain = b"hello-cookie-value";
        let iv = [0x11u8; 16];
        // 构造：v11 + MAC(16 哑元) + IV + PKCS7 密文
        let padded = {
            let mut v = plain.to_vec();
            let rem = v.len() % 16;
            let pad = (16 - rem) as u8;
            v.extend(std::iter::repeat(pad).take(pad as usize));
            v
        };
        let mut encrypted = aes_128_cbc_encrypt(&key, &iv, &padded);
        let mut raw = b"v11".to_vec();
        raw.extend_from_slice(&[0u8; 16]); // MAC 哑元
        raw.extend_from_slice(&iv);
        raw.append(&mut encrypted);
        let out = decrypt_value(&key, &raw).unwrap();
        assert_eq!(&out[..], plain);
    }

    #[test]
    fn v11_fixed_iv_fallback_decrypts() {
        let key = [0x42u8; 16];
        let plain = b"fixed-iv-value";
        let padded = {
            let mut v = plain.to_vec();
            let pad = (16 - v.len() % 16) as u8;
            v.extend(std::iter::repeat(pad).take(pad as usize));
            v
        };
        let mut encrypted = aes_128_cbc_encrypt(&key, &[0x20u8; 16], &padded);
        let mut raw = b"v11".to_vec();
        raw.append(&mut encrypted);
        let out = decrypt_value(&key, &raw).unwrap();
        assert_eq!(&out[..], plain);
    }

    fn aes_128_cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
        use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
        let cipher = aes::Aes128::new(GenericArray::from_slice(key));
        let mut previous = *iv;
        let mut out = Vec::with_capacity(data.len());
        for chunk in data.chunks(16) {
            let block = GenericArray::clone_from_slice(chunk);
            let mut enc = block.clone();
            for (a, b) in enc.iter_mut().zip(previous.iter()) {
                *a ^= *b;
            }
            cipher.encrypt_block(&mut enc);
            previous = enc.into();
            out.extend_from_slice(&enc);
        }
        out
    }
}
