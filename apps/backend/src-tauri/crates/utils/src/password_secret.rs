use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
/// 加密密码（存数据库用）
pub fn hash_password(password: &str) -> Result<String, String> {
    // argon2 0.6：盐由 Argon2 内部经 getrandom 生成 16 字节随机值，
    // 不再需要手工构造 SaltString（password-hash 0.6 已移除该类型）
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes())
        .map_err(|e| format!("哈希失败: {}", e))?
        .to_string();

    Ok(password_hash)
}
/// 验证密码（登录时用）
pub fn verify_password(password: &str, hash: &str) -> Result<bool, String> {
    // 解析数据库里的哈希（phc::PasswordHash，0.6 起为完全拥有的类型）
    let parsed_hash = PasswordHash::new(hash).map_err(|e| format!("解析哈希失败: {}", e))?;

    // 校验
    let ok = Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok();

    Ok(ok)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_password_hashing() {
        let password = "admin123";
        let hash = hash_password(password).expect("哈希失败");
        println!("哈希结果: {}", hash);

        // 验证正确密码
        assert!(verify_password(password, &hash).expect("验证失败"));
        println!("正确密码验证成功");

        // 验证错误密码
        assert!(!verify_password("wrong_password", &hash).expect("验证失败"));
        println!("错误密码验证成功");
    }
}
