//! 来源版本摘要：固定区域排序和文件集合，供 Native 与 Worker 共用。

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use icu_collator::{Collator, CollatorBorrowed};
use sha2::{Digest, Sha256};

static COLLATOR: LazyLock<CollatorBorrowed<'static>> = LazyLock::new(|| {
    Collator::try_new(icu_locale_core::locale!("en-US").into(), Default::default())
        .expect("compiled English collation data")
});

/// 版本比较保留原始文件名；只跳过这两种目录中的内容。
pub fn includes_file(relative_path: &str) -> bool {
    let mut parts = relative_path.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_some() && matches!(part, ".git" | "node_modules") {
            return false;
        }
    }
    true
}

/// 文件读取可来自磁盘或内容存储，摘要始终使用同一排序、路径和字节规则。
pub fn hash_files<T, R: Read>(
    mut files: Vec<(String, T)>,
    mut open: impl FnMut(T) -> io::Result<R>,
    is_cancelled: impl Fn() -> bool,
) -> io::Result<String> {
    files.retain(|(path, _)| includes_file(path));
    files.sort_by(|left, right| COLLATOR.compare(&left.0, &right.0));
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    for (path, file) in files {
        check_cancelled(&is_cancelled)?;
        hash.update(path.as_bytes());
        let mut reader = open(file)?;
        loop {
            check_cancelled(&is_cancelled)?;
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
    }
    check_cancelled(&is_cancelled)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// 从普通来源文件计算版本；安装时省略的 metadata 和缓存文件仍属于来源版本。
pub fn hash_directory(root: &Path, is_cancelled: impl Fn() -> bool) -> io::Result<String> {
    let mut files = Vec::new();
    collect_files(root, "", &mut files, &is_cancelled)?;
    hash_files(files, fs::File::open, is_cancelled)
}

fn collect_files(
    directory: &Path,
    prefix: &str,
    files: &mut Vec<(String, PathBuf)>,
    is_cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        check_cancelled(is_cancelled)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 source path"))?;
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry.file_type()?;
        if kind.is_dir() && !matches!(name.as_str(), ".git" | "node_modules") {
            collect_files(&entry.path(), &path, files, is_cancelled)?;
        } else if kind.is_file() {
            files.push((path, entry.path()));
        }
    }
    Ok(())
}

fn check_cancelled(is_cancelled: &impl Fn() -> bool) -> io::Result<()> {
    if is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "source hash cancelled",
        ))
    } else {
        Ok(())
    }
}
