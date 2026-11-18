//! 单实例锁：同一台机器上只允许程序的一个实例在运行。
//!
//! Linux/macOS 用 flock 锁住配置目录下的 `instance.lock` 文件，进程退出或崩溃时
//! 内核自动释放；Windows 用同名互斥体，句柄随进程退出自动释放。
//! 其它平台不支持单实例，直接放行。

use std::io;

/// 获取实例锁失败的原因。
#[derive(Debug)]
pub enum AcquireError {
    /// 已经有一个实例持有锁。
    AlreadyRunning,
    /// 打开锁文件或系统 API 报错。
    Io(io::Error),
}

impl From<io::Error> for AcquireError {
    fn from(err: io::Error) -> Self {
        AcquireError::Io(err)
    }
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcquireError::AlreadyRunning => write!(f, "程序已经在运行"),
            AcquireError::Io(err) => write!(f, "获取单实例锁失败：{err}"),
        }
    }
}

impl std::error::Error for AcquireError {}

/// 持有锁的守卫。活到它 drop 为止锁都在；进程崩溃时由操作系统回收。
pub struct InstanceGuard {
    #[cfg(unix)]
    // 只为把 fd 持有到 guard drop；flock 随 fd 关闭自动释放。
    #[allow(dead_code)]
    file: std::fs::File,
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(any(unix, windows)))]
    _marker: std::marker::PhantomData<()>,
}

/// 在默认配置目录下获取实例锁。
///
/// 路径与 `config::state_path()` 同属 `config_dir()/gistpic-creator`，目录不存在时先建。
pub fn acquire() -> Result<InstanceGuard, AcquireError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| AcquireError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "拿不到配置目录，无法确定单实例锁路径",
        )))?
        .join("gistpic-creator");
    std::fs::create_dir_all(&dir)?;
    acquire_at(&dir.join("instance.lock"))
}

#[cfg(unix)]
fn acquire_at(path: &std::path::Path) -> Result<InstanceGuard, AcquireError> {
    use std::os::fd::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    // LOCK_EX | LOCK_NB = 2 | 4；拿不到锁就说明已有实例，别阻塞等待。
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == -1 {
        let err = io::Error::last_os_error();
        let kind = err.raw_os_error();
        return if kind == Some(libc::EWOULDBLOCK) || kind == Some(libc::EAGAIN) {
            Err(AcquireError::AlreadyRunning)
        } else {
            Err(AcquireError::Io(err))
        };
    }
    Ok(InstanceGuard { file })
}

#[cfg(windows)]
fn acquire_at(_path: &std::path::Path) -> Result<InstanceGuard, AcquireError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\gistpic-creator-single-instance"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
        .map_err(|err| AcquireError::Io(io::Error::other(err)))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // 锁已被别的实例持有，这个句柄属于「打开已有互斥体」，要关掉。
        unsafe { windows::Win32::Foundation::CloseHandle(handle) }
            .map_err(|err| AcquireError::Io(io::Error::other(err)))?;
        return Err(AcquireError::AlreadyRunning);
    }
    Ok(InstanceGuard { handle })
}

#[cfg(not(any(unix, windows)))]
fn acquire_at(_path: &std::path::Path) -> Result<InstanceGuard, AcquireError> {
    Ok(InstanceGuard {
        _marker: std::marker::PhantomData,
    })
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // File drop 会关闭 fd，flock 随最后一个 fd 关闭自动释放。
        }
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::ReleaseMutex;
            unsafe {
                let _ = ReleaseMutex(self.handle);
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// 用进程号 + 原子自增生成互不干扰的临时锁文件路径。
    fn fresh_path() -> std::path::PathBuf {
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "gistpic-single-instance-test-{}-{}.lock",
            std::process::id(),
            seq,
        ))
    }

    #[test]
    fn first_acquire_succeeds_then_same_path_fails() {
        let path = fresh_path();
        let guard = acquire_at(&path).expect("首次获取应成功");
        assert!(matches!(
            acquire_at(&path),
            Err(AcquireError::AlreadyRunning)
        ), "同一个路径重复获取应报已有实例");
        drop(guard);
        // guard drop 后锁释放，可以再次获取；锁文件还在，不影响语义。
        let again = acquire_at(&path).expect("guard drop 后应能重新获取");
        drop(again);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stale_or_garbled_lock_file_does_not_matter() {
        // flock 锁的是文件 inode，不看内容；预先写入乱七八糟的内容也不影响获取。
        let path = fresh_path();
        std::fs::write(&path, b"not a valid lock file").expect("应能写测试文件");
        let guard = acquire_at(&path).expect("陈旧、内容损坏的锁文件不该挡住获取");
        drop(guard);
        let _ = std::fs::remove_file(&path);
    }
}
