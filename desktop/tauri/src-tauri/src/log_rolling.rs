use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Size-bounded log writer safe for Windows and existing oversized logs.
pub struct RollingWriter {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    keep: usize,
}

impl RollingWriter {
    pub fn new(path: &Path, max_bytes: u64, keep: usize) -> io::Result<Self> {
        let max_bytes = max_bytes.max(1);
        let keep = keep.max(1);
        let mut size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        // Rotate an old 30 GB (or otherwise oversized) log before opening it.
        // This is important on Windows where an open file cannot normally be
        // renamed by the same process.
        if size > max_bytes {
            Self::rotate_path(path, keep)?;
            size = 0;
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;
        size = file.metadata()?.len();

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                path: path.to_path_buf(),
                file: Some(file),
                size,
                max_bytes,
                keep,
            })),
        })
    }

    fn generation_path(path: &Path, generation: usize) -> PathBuf {
        PathBuf::from(format!("{}.{}", path.display(), generation))
    }

    fn rotate_path(path: &Path, keep: usize) -> io::Result<()> {
        let oldest = Self::generation_path(path, keep);
        let _ = fs::remove_file(&oldest);

        for i in (1..keep).rev() {
            let src = Self::generation_path(path, i);
            let dst = Self::generation_path(path, i + 1);
            if src.exists() {
                let _ = fs::remove_file(&dst);
                fs::rename(&src, &dst)?;
            }
        }

        if path.exists() {
            let first = Self::generation_path(path, 1);
            let _ = fs::remove_file(&first);
            fs::rename(path, &first)?;
        }

        Ok(())
    }

    fn rotate(inner: &mut Inner) -> io::Result<()> {
        if let Some(mut file) = inner.file.take() {
            file.flush()?;
            drop(file);
        }

        let path = inner.path.clone();
        Self::rotate_path(&path, inner.keep)?;

        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        inner.size = file.metadata()?.len();
        inner.file = Some(file);
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
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "log lock poisoned"))?;

        if inner.size > inner.max_bytes
            || inner.size.saturating_add(buf.len() as u64) > inner.max_bytes
        {
            Self::rotate(&mut inner)?;
        }

        let file = inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "log file is closed"))?;
        let n = file.write(buf)?;
        inner.size = inner.size.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "log lock poisoned"))?;
        inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "log file is closed"))?
            .flush()
    }
}
