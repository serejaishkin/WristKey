use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Size-bounded log writer.
///
/// The previous implementation had two Windows-specific problems:
/// 1. it tried to rename the active log while its File handle was still open;
/// 2. Path::with_extension("1") changed `wristkey.log` into `wristkey.1`,
///    while the reader expected `wristkey.log.1`.
///
/// This implementation closes the active handle before rotation and uses
/// explicit `.log.N` generation names. An oversized existing log is rotated
/// immediately when the writer starts, so a 30 GB legacy log cannot keep the
/// new writer in a broken state.
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
        let max_bytes = max_bytes.max(1);
        let keep = keep.max(1);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let mut size = file.metadata()?.len();

        if size > max_bytes {
            drop(file);
            Self::rotate_path(path, keep)?;
            file = OpenOptions::new().create(true).append(true).open(path)?;
            size = file.metadata()?.len();
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                path: path.to_path_buf(),
                file,
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
        // Windows does not allow renaming an open file in the normal sharing
        // mode. Close the active handle before moving the file.
        inner.file.flush()?;
        let path = inner.path.clone();
        let keep = inner.keep;
        let max_bytes = inner.max_bytes;
        drop(std::mem::replace(
            &mut inner.file,
            OpenOptions::new().create(true).append(true).open(&path)?,
        ));

        Self::rotate_path(&path, keep)?;
        inner.file = OpenOptions::new().create(true).append(true).open(&path)?;
        inner.size = inner.file.metadata()?.len();
        inner.max_bytes = max_bytes;
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
            RollingWriter::rotate(&mut inner)?;
        }

        let n = inner.file.write(buf)?;
        inner.size = inner.size.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "log lock poisoned"))?;
        inner.file.flush()
    }
}
