//! 自动管理worker的文件锁；文件句柄继续拥有排他权。
use fs2::FileExt;
use std::{fs::File, io};

pub(super) fn try_acquire(file: File) -> io::Result<Option<File>> {
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if is_contention(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn is_contention(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || matches!(
            (error.raw_os_error(), fs2::lock_contended_error().raw_os_error()),
            (Some(actual), Some(contended)) if actual == contended
        )
}

#[cfg(test)]
#[path = "identity_join_management_file_lock_tests.rs"]
mod tests;
