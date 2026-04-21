use sqlx::{SqlitePool, Sqlite, Pool};
use std::sync::OnceLock;
use libsqlite3_sys as sqlite3;
use std::ffi::CString;
use std::ptr;
// 全局单例数据库池（一次初始化，全局使用）
static DB_POOL: OnceLock<Pool<Sqlite>> = OnceLock::new();

fn execute_sql_script(db_path: &str, sql_script: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 打开数据库连接
    let db_path_c = CString::new(db_path)?;
    let mut db: *mut sqlite3::sqlite3 = ptr::null_mut();
    
    let result = unsafe {
        sqlite3::sqlite3_open(db_path_c.as_ptr(), &mut db)
    };
    
    if result != sqlite3::SQLITE_OK {
        return Err("Failed to open database".into());
    }
    
    // 准备SQL语句
    let sql_c = CString::new(sql_script)?;
    let mut stmt: *mut sqlite3::sqlite3_stmt = ptr::null_mut();
    let mut tail: *const i8 = ptr::null();
    
    let result = unsafe {
        sqlite3::sqlite3_prepare_v2(
            db,
            sql_c.as_ptr(),
            -1,
            &mut stmt,
            &mut tail,
        )
    };
    
    if result != sqlite3::SQLITE_OK {
        unsafe { sqlite3::sqlite3_close(db) };
        return Err("Failed to prepare SQL statement".into());
    }
    
    // 执行SQL语句
    loop {
        let result = unsafe { sqlite3::sqlite3_step(stmt) };
        
        if result == sqlite3::SQLITE_DONE {
            break;
        } else if result != sqlite3::SQLITE_ROW {
            unsafe { 
                sqlite3::sqlite3_finalize(stmt);
                sqlite3::sqlite3_close(db);
            };
            return Err("Failed to execute SQL statement".into());
        }
    }
    
    // 清理资源
    unsafe {
        sqlite3::sqlite3_finalize(stmt);
        sqlite3::sqlite3_close(db);
    }
    
    Ok(())
}
