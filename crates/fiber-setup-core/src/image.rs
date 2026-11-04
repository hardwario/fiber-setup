//! Getting the image ready: download (or reuse a copy), check its checksum, unpack it once into
//! the cache as a raw disk image that the writer copies from.

use crate::catalog::{self, OsImage};
use crate::event::{Cancel, Event, Phase, Sink, Throttle};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 4 << 20;

/// A raw disk image ready to be written.
#[derive(Debug, Clone, Serialize)]
pub struct Prepared {
    /// The raw (unpacked) image.
    pub raw: PathBuf,
    pub size: u64,
    /// sha256 of the file it came from (compressed or not).
    pub source_sha256: String,
    /// Whether that checksum is one of the images FIBER Setup knows.
    pub official: bool,
    pub name: String,
}

/// The cache folder: `$XDG_CACHE_HOME/fiber-setup` or `~/.cache/fiber-setup`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FIBER_SETUP_CACHE") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("fiber-setup")
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub image: OsImage,
    /// Unpacked and ready, nothing to download.
    pub ready: bool,
    /// A copy of the compressed file another program already downloaded (not yet checked).
    pub other_copy: Option<PathBuf>,
}

pub fn status(cache: &Path, image: &OsImage) -> Status {
    Status { image: *image, ready: read_marker(cache, image.sha256).is_some(), other_copy: other_copy(image) }
}

fn other_copy(image: &OsImage) -> Option<PathBuf> {
    catalog::known_copies().into_iter().find(|p| fs::metadata(p).map(|m| m.len() == image.size).unwrap_or(false))
}

fn raw_path(cache: &Path, sha256: &str) -> PathBuf {
    cache.join(format!("{}.img", &sha256[..16.min(sha256.len())]))
}

fn marker_path(cache: &Path, sha256: &str) -> PathBuf {
    cache.join(format!("{}.img.ok", &sha256[..16.min(sha256.len())]))
}

/// The unpacked image, if a previous run finished unpacking it (the marker is written last).
fn read_marker(cache: &Path, sha256: &str) -> Option<u64> {
    let text = fs::read_to_string(marker_path(cache, sha256)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != sha256 {
        return None;
    }
    let size: u64 = lines.next()?.parse().ok()?;
    let actual = fs::metadata(raw_path(cache, sha256)).ok()?.len();
    (actual == size).then_some(size)
}

/// The known image: reuse what is cached, else take another program's copy or download it,
/// check it and unpack it.
pub fn prepare_official(cache: &Path, image: &OsImage, events: Sink<'_>, cancel: &Cancel) -> Result<Prepared> {
    fs::create_dir_all(cache).with_context(|| format!("creating {}", cache.display()))?;
    let done = |size| Prepared {
        raw: raw_path(cache, image.sha256),
        size,
        source_sha256: image.sha256.to_string(),
        official: true,
        name: format!("{} ({})", image.name, image.version),
    };
    if let Some(size) = read_marker(cache, image.sha256) {
        events(Event::Log { line: format!("Using the unpacked image in {}", cache.display()) });
        return Ok(done(size));
    }

    let mut compressed: Option<PathBuf> = None;
    for path in [Some(cache.join(image.file_name)), other_copy(image)].into_iter().flatten() {
        if fs::metadata(&path).map(|m| m.len() == image.size).unwrap_or(false) {
            events(Event::Log { line: format!("Checking {}", path.display()) });
            let sha = hash_file(&path, events, cancel)?;
            if sha == image.sha256 {
                compressed = Some(path);
                break;
            }
            events(Event::Log { line: format!("{} has another checksum, not used", path.display()) });
        }
    }
    let compressed = match compressed {
        Some(p) => p,
        None => download(cache, image, events, cancel)?,
    };
    let size = unpack(&compressed, &raw_path(cache, image.sha256), events, cancel)?;
    fs::write(marker_path(cache, image.sha256), format!("{}\n{}\n", image.sha256, size))?;
    Ok(done(size))
}

/// An image file the user picked: `.img.xz` is checked and unpacked into the cache, a raw `.img`
/// is used where it is.
pub fn prepare_local(cache: &Path, path: &Path, events: Sink<'_>, cancel: &Cancel) -> Result<Prepared> {
    fs::create_dir_all(cache).with_context(|| format!("creating {}", cache.display()))?;
    let sha = hash_file(path, events, cancel)?;
    let official = sha == catalog::RASPIOS_LITE.sha256;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if is_xz(path)? {
        let size = match read_marker(cache, &sha) {
            Some(size) => size,
            None => {
                let size = unpack(path, &raw_path(cache, &sha), events, cancel)?;
                fs::write(marker_path(cache, &sha), format!("{sha}\n{size}\n"))?;
                size
            }
        };
        Ok(Prepared { raw: raw_path(cache, &sha), size, source_sha256: sha, official, name })
    } else {
        let size = fs::metadata(path)?.len();
        if !size.is_multiple_of(crate::mbr::SECTOR) {
            bail!("{} is not a disk image (its size is not a whole number of sectors)", path.display());
        }
        crate::mbr::boot_partition(path)?;
        Ok(Prepared { raw: path.to_path_buf(), size, source_sha256: sha, official, name })
    }
}

fn is_xz(path: &Path) -> Result<bool> {
    let mut magic = [0u8; 6];
    let n =
        File::open(path).and_then(|mut f| f.read(&mut magic)).with_context(|| format!("opening {}", path.display()))?;
    Ok(n == 6 && magic == [0xFD, b'7', b'z', b'X', b'Z', 0x00])
}

pub fn hash_file(path: &Path, events: Sink<'_>, cancel: &Cancel) -> Result<String> {
    events(Event::Phase { phase: Phase::Check });
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let total = f.metadata()?.len();
    let mut throttle = Throttle::new(Phase::Check, total);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        cancel.check()?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        throttle.report(events, done);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// A file being written under a temporary name; removed unless [`Part::finish`] renames it, so an
/// error or Cancel never leaves gigabytes of half a file in the cache.
struct Part(PathBuf);

impl Part {
    fn finish(self, to: &Path) -> Result<()> {
        fs::rename(&self.0, to)?;
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Downloads into `<file>.part`, checking the checksum on the way, and renames it when it matches.
fn download(cache: &Path, image: &OsImage, events: Sink<'_>, cancel: &Cancel) -> Result<PathBuf> {
    events(Event::Phase { phase: Phase::Download });
    events(Event::Log { line: format!("Downloading {}", image.url) });
    let target = cache.join(image.file_name);
    let part = Part(cache.join(format!("{}.part", image.file_name)));
    // Without timeouts a stalled connection would block the read forever and Cancel with it.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(3600)))
        .build()
        .into();
    let response = agent
        .get(image.url)
        .header("User-Agent", concat!("fiber-setup/", env!("CARGO_PKG_VERSION")))
        .call()
        .with_context(|| format!("downloading {}", image.url))?;
    let total = response.body().content_length().unwrap_or(image.size);
    let mut reader = response.into_body().into_reader();
    let mut out = BufWriter::new(File::create(&part.0).with_context(|| format!("creating {}", part.0.display()))?);
    let mut hasher = Sha256::new();
    let mut throttle = Throttle::new(Phase::Download, total);
    let mut buf = vec![0u8; 256 << 10];
    let mut done = 0u64;
    loop {
        cancel.check()?;
        let n = reader.read(&mut buf).context("downloading the image")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        throttle.report(events, done);
    }
    out.flush()?;
    drop(out);
    let sha = hex::encode(hasher.finalize());
    if sha != image.sha256 {
        bail!("the downloaded file has the wrong checksum ({sha}); try again");
    }
    part.finish(&target)?;
    Ok(target)
}

/// Counts the compressed bytes read, for the unpack progress.
struct Counting<R> {
    inner: R,
    count: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

/// Unpacks an `.xz` file into `<raw>.part` and renames it when complete.
fn unpack(xz: &Path, raw: &Path, events: Sink<'_>, cancel: &Cancel) -> Result<u64> {
    events(Event::Phase { phase: Phase::Unpack });
    let input = File::open(xz).with_context(|| format!("opening {}", xz.display()))?;
    let total = input.metadata()?.len();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut decoder = liblzma::read::XzDecoder::new_multi_decoder(Counting {
        inner: std::io::BufReader::new(input),
        count: count.clone(),
    });
    let part = Part(raw.with_extension("img.part"));
    let mut out = BufWriter::with_capacity(
        CHUNK,
        File::create(&part.0).with_context(|| format!("creating {}", part.0.display()))?,
    );
    let mut throttle = Throttle::new(Phase::Unpack, total);
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        cancel.check()?;
        let n = decoder.read(&mut buf).with_context(|| format!("unpacking {}", xz.display()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        size += n as u64;
        throttle.report(events, count.load(std::sync::atomic::Ordering::Relaxed));
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);
    if !size.is_multiple_of(crate::mbr::SECTOR) {
        bail!("{} does not unpack to a disk image (not whole sectors)", xz.display());
    }
    part.finish(raw)?;
    crate::mbr::boot_partition(raw)?;
    Ok(size)
}
