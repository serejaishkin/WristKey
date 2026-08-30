use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A file writer that rotates to `name.1`, `name.2`, ... once the current
/// file exceeds `max_bytes`, keeping at most `keep` generations. This bounds
/// disk usage even if a hot code path logs in a tight loop -- the previous
/// daily appender had no size cap and produced multi-GB files.
pub struct RollingWriter {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    file: File,
    size: u64,
    max_bytes: u64,
    keep: usize,
}

impl RollingWriter {
    pub fn new(path: &Path, max_bytes: u64, keep: usize) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                path: path.to_path_buf(),
                file,
                size,
                max_bytes,
                keep: keep.max(1),
            })),
        })
    }

    fn rotate(inner: &mut Inner) -> io::Result<()> {
        let oldest = inner.path.with_extension(format!("{}", inner.keep));
        let _ = fs::remove_file(&oldest);
        for i in (1..inner.keep).rev() {
            let src = inner.path.with_extension(format!("{}", i));
            let dst = inner.path.with_extension(format!("{}", i + 1));
            if src.exists() {
                let _ = fs::rename(&src, &dst);
            }
        }
        let _ = fs::rename(&inner.path, inner.path.with_extension("1"));
        inner.file = OpenOptions::new().create(true).append(true).open(&inner.path)?;
        inner.size = 0;
        Ok(())
    }
}

impl Clone for RollingWriter {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Write for RollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().map_err(|_| io::Error::new(io::ErrorKind::Other, "log lock poisoned"))?;
        if inner.size + buf.len() as u64 > inner.max_bytes {
            RollingWriter::rotate(&mut inner)?;
        }
        let n = inner.file.write(buf)?;
        inner.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().map_err(|_| io::Error::new(io::ErrorKind::Other, "log lock poisoned"))?;
        inner.file.flush()
    }
}
