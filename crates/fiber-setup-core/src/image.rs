//! Getting the image ready: download (or reuse a copy), check its checksum, unpack it once into
//! the cache as a raw disk image that the writer copies from.

use crate::catalog::{self, OsImage};
use crate::event::{Cancel, Event, Phase, Sink, Throttle};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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

/// The cache folder: `$XDG_CACHE_HOME/fiber-setup` or `~/.cache/fiber-setup`; on Windows
/// `%LOCALAPPDATA%\fiber-setup`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FIBER_SETUP_CACHE") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").filter(|_| cfg!(windows)).map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("fiber-setup")
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub image: OsImage,
    /// Unpacked and ready, nothing to download.
    pub ready: bool,
}

pub fn status(cache: &Path, image: &OsImage) -> Status {
    Status { image: *image, ready: read_marker(cache, image.sha256).is_some() }
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

/// The known image: reuse what is cached, else download it, check it and unpack it.
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
    let path = cache.join(image.file_name);
    if fs::metadata(&path).map(|m| m.len() == image.size).unwrap_or(false) {
        events(Event::Log { line: format!("Checking {}", path.display()) });
        let sha = hash_file(&path, events, cancel)?;
        if sha == image.sha256 {
            compressed = Some(path);
        } else {
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
    let official = sha == catalog::FIBER_IMAGE.sha256;
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

/// How long the download bears with a bad connection. The tests shorten the waits.
struct Patience {
    /// A connection that sends nothing for this long is dropped and the download goes on over a
    /// new one, from where it stopped.
    stall: Duration,
    /// So many attempts in a row that bring no new byte end the download with an error.
    attempts: u32,
    /// The wait before the next attempt; doubled after each attempt that brought nothing.
    pause: Duration,
}

const PATIENCE: Patience = Patience { stall: Duration::from_secs(20), attempts: 4, pause: Duration::from_secs(1) };

/// Why a connection ended before the file was complete.
enum Stop {
    /// The connection's fault (silence, a reset, an early end): worth another connection.
    Again(anyhow::Error),
    /// Nothing another attempt would change: Cancel, a full disk, the server's refusal.
    Fail(anyhow::Error),
}

/// The file as far as it is downloaded.
struct Partial {
    file: File,
    hasher: Sha256,
    done: u64,
}

impl Partial {
    /// Back to an empty file, for a server that sends the file from its beginning again.
    fn restart(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.hasher = Sha256::new();
        self.done = 0;
        Ok(())
    }
}

/// Downloads into `<file>.part`, checking the checksum on the way, and renames it when it matches.
fn download(cache: &Path, image: &OsImage, events: Sink<'_>, cancel: &Cancel) -> Result<PathBuf> {
    download_with(cache, image, events, cancel, &PATIENCE)
}

fn download_with(
    cache: &Path,
    image: &OsImage,
    events: Sink<'_>,
    cancel: &Cancel,
    patience: &Patience,
) -> Result<PathBuf> {
    events(Event::Phase { phase: Phase::Download });
    events(Event::Log { line: format!("Downloading {}", image.url) });
    let target = cache.join(image.file_name);
    let part = Part(cache.join(format!("{}.part", image.file_name)));
    // A connection that goes silent must not block the download forever: each read has its
    // time limit. There is no limit for the whole file, which takes as long as the line needs.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(patience.stall))
        .build()
        .into();
    let mut partial = Partial {
        file: File::create(&part.0).with_context(|| format!("creating {}", part.0.display()))?,
        hasher: Sha256::new(),
        done: 0,
    };
    let mut throttle = Throttle::new(Phase::Download, image.size);
    // Attempts in a row that brought nothing.
    let mut idle = 0u32;
    loop {
        cancel.check()?;
        let before = partial.done;
        let why = match fetch(&agent, image, &mut partial, &mut throttle, events, cancel) {
            Ok(()) => break,
            Err(Stop::Fail(e)) => return Err(e),
            Err(Stop::Again(e)) => e,
        };
        idle = if partial.done > before { 0 } else { idle + 1 };
        if idle >= patience.attempts {
            return Err(why.context(format!("downloading {} ({} attempts without progress)", image.url, idle)));
        }
        events(Event::Log { line: format!("The download stopped at {} MB ({why:#}); continuing", partial.done >> 20) });
        let wait = patience.pause * (1 << idle.saturating_sub(1).min(6));
        let until = Instant::now() + wait;
        while Instant::now() < until {
            cancel.check()?;
            std::thread::sleep(wait.min(Duration::from_millis(100)));
        }
    }
    partial.file.sync_all()?;
    let sha = hex::encode(partial.hasher.finalize());
    drop(partial.file);
    if sha != image.sha256 {
        bail!("the downloaded file has the wrong checksum ({sha}); try again");
    }
    part.finish(&target)?;
    Ok(target)
}

/// One connection: asks for the file from where the download stopped and appends what comes,
/// until the file is complete (`Ok`) or the connection ends.
fn fetch(
    agent: &ureq::Agent,
    image: &OsImage,
    partial: &mut Partial,
    throttle: &mut Throttle,
    events: Sink<'_>,
    cancel: &Cancel,
) -> Result<(), Stop> {
    let mut request = agent
        .get(image.url)
        .header("User-Agent", concat!("fiber-setup/", env!("CARGO_PKG_VERSION")))
        // The file as it is, so that a position in the answer is a position in the file.
        .header("Accept-Encoding", "identity");
    if partial.done > 0 {
        request = request.header("Range", format!("bytes={}-", partial.done));
    }
    let response = match request.call() {
        Ok(r) => r,
        // The server's "no" (not found, forbidden): the same answer would come again. "Too
        // many requests" and "request timeout" are worth waiting out.
        Err(ureq::Error::StatusCode(code)) if (400..500).contains(&code) && code != 408 && code != 429 => {
            return Err(Stop::Fail(anyhow!("downloading {}: the server answered {code}", image.url)));
        }
        Err(e) => return Err(Stop::Again(anyhow::Error::new(e).context("connecting"))),
    };
    if partial.done > 0 {
        if response.status().as_u16() == 206 {
            // "bytes <first>-<last>/<size>": the rest must start exactly where the file ends.
            let range = response.headers().get("content-range").and_then(|v| v.to_str().ok()).unwrap_or_default();
            if !range.starts_with(&format!("bytes {}-", partial.done)) {
                return Err(Stop::Fail(anyhow!("downloading {}: the server sent another part ({range})", image.url)));
            }
        } else {
            // A server that knows no ranges sends the whole file again.
            partial.restart().map_err(|e| Stop::Fail(e.into()))?;
        }
    }

    // The body is read on a thread of its own: a read on a silent connection returns only after
    // its time limit, and Cancel must not wait that long.
    let mut reader = response.into_body().into_reader();
    let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(8);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 256 << 10];
        loop {
            let message = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => Ok(buf[..n].to_vec()),
                Err(e) => Err(e),
            };
            let last = message.is_err();
            // Nobody listens any more after Cancel or an error on the other side.
            if tx.send(message).is_err() || last {
                break;
            }
        }
    });
    loop {
        cancel.check().map_err(Stop::Fail)?;
        let chunk = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(e)) => return Err(Stop::Again(e.into())),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            // The thread is done: the server ended the body.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        partial.done += chunk.len() as u64;
        if partial.done > image.size {
            return Err(Stop::Fail(anyhow!("the download is larger than the image ({} bytes)", image.size)));
        }
        partial
            .file
            .write_all(&chunk)
            .map_err(|e| Stop::Fail(anyhow::Error::new(e).context("writing the downloaded file")))?;
        partial.hasher.update(&chunk);
        throttle.report(events, partial.done);
    }
    if partial.done < image.size {
        return Err(Stop::Again(anyhow!("the connection closed early")));
    }
    Ok(())
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
    // Checked before it gets its name: what is no disk image must not stay in the cache.
    crate::mbr::boot_partition(&part.0)?;
    part.finish(raw)?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// What the test server does with one connection, after reading the request.
    #[derive(Clone, Copy)]
    enum Serve {
        /// Answers with the part of the file that was asked for.
        All,
        /// Sends so many bytes of it, then nothing more; the connection stays open.
        Stall(usize),
        /// Sends so many bytes of it, then closes the connection.
        Cut(usize),
        /// Answers with the whole file whatever part was asked for, as a server without ranges.
        Whole,
        /// Never answers.
        Silent,
        /// Answers with this status and no file.
        Status(u16),
    }

    /// A web server for one file. The connections are served one after another as the script
    /// says; once the script is over, each gets what it asks for.
    struct Server {
        url: String,
        /// The `Range: bytes=N-` of each request, in order; `None` for a request without one.
        asked: Arc<Mutex<Vec<Option<u64>>>>,
    }

    fn serve(file: Vec<u8>, script: Vec<Serve>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image.img.xz", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        std::thread::spawn(move || {
            // The silent connections, kept open for as long as the server lives.
            let mut open: Vec<TcpStream> = Vec::new();
            let mut script = script.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut from = None;
                let mut lines = std::io::BufReader::new(stream.try_clone().unwrap()).lines();
                while let Some(Ok(line)) = lines.next() {
                    if line.is_empty() {
                        break;
                    }
                    if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = range.trim_end_matches('-').parse::<u64>().ok();
                    }
                }
                seen.lock().unwrap().push(from);
                let what = script.next().unwrap_or(Serve::All);
                let start = if matches!(what, Serve::Whole) { 0 } else { from.unwrap_or(0) as usize };
                let body = &file[start.min(file.len())..];
                let head = match what {
                    Serve::Silent => {
                        open.push(stream);
                        continue;
                    }
                    Serve::Status(code) => {
                        format!("HTTP/1.1 {code} No\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    }
                    _ if start > 0 => format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        start,
                        file.len() - 1,
                        file.len()
                    ),
                    _ => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()),
                };
                let _ = stream.write_all(head.as_bytes());
                match what {
                    Serve::All | Serve::Whole => {
                        let _ = stream.write_all(body);
                    }
                    Serve::Stall(n) => {
                        let _ = stream.write_all(&body[..n.min(body.len())]);
                        let _ = stream.flush();
                        open.push(stream);
                    }
                    Serve::Cut(n) => {
                        let _ = stream.write_all(&body[..n.min(body.len())]);
                    }
                    Serve::Silent | Serve::Status(_) => {}
                }
            }
        });
        Server { url, asked }
    }

    /// Bytes that do not repeat, so a part in the wrong place shows in the checksum.
    fn bytes(len: usize) -> Vec<u8> {
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    fn sha256(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    /// An image as the catalog describes one, served by the test server.
    fn image_at(url: &str, file: &[u8]) -> OsImage {
        OsImage {
            id: "test",
            name: "Test",
            version: "1",
            url: Box::leak(url.to_string().into_boxed_str()),
            sha256: Box::leak(sha256(file).into_boxed_str()),
            size: file.len() as u64,
            file_name: "image.img.xz",
        }
    }

    const QUICK: Patience =
        Patience { stall: Duration::from_millis(300), attempts: 3, pause: Duration::from_millis(10) };

    /// Runs the download and collects its log lines.
    fn run(cache: &Path, image: &OsImage, cancel: &Cancel, patience: &Patience) -> (Result<PathBuf>, Vec<String>) {
        let log = Mutex::new(Vec::new());
        let result = download_with(
            cache,
            image,
            &|e| {
                if let Event::Log { line } = e {
                    log.lock().unwrap().push(line);
                }
            },
            cancel,
            patience,
        );
        (result, log.into_inner().unwrap())
    }

    fn stops(log: &[String]) -> usize {
        log.iter().filter(|l| l.starts_with("The download stopped")).count()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn downloads_the_file() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        let path = result.unwrap();
        assert_eq!(path, cache.path().join("image.img.xz"));
        assert_eq!(fs::read(&path).unwrap(), file);
        assert_eq!(files(cache.path()), ["image.img.xz"]);
        assert_eq!(*server.asked.lock().unwrap(), [None]);
        assert_eq!(stops(&log), 0);
    }

    #[test]
    fn goes_on_after_a_silent_and_a_closed_connection() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Stall(100_000), Serve::Cut(50_000), Serve::Silent]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        assert_eq!(fs::read(result.unwrap()).unwrap(), file);
        // Each new connection asked for the rest, from where the one before it stopped.
        assert_eq!(*server.asked.lock().unwrap(), [None, Some(100_000), Some(150_000), Some(150_000)]);
        assert_eq!(stops(&log), 3, "{log:?}");
        assert_eq!(files(cache.path()), ["image.img.xz"]);
    }

    #[test]
    fn starts_again_when_the_server_knows_no_ranges() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Stall(100_000), Serve::Whole]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let (result, _) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        assert_eq!(fs::read(result.unwrap()).unwrap(), file);
        assert_eq!(*server.asked.lock().unwrap(), [None, Some(100_000)]);
    }

    #[test]
    fn gives_up_when_nothing_comes() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Stall(1000), Serve::Silent, Serve::Stall(0), Serve::Silent]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("3 attempts without progress"), "{error}");
        // The first connection brought something, so it does not count; three more brought nothing.
        assert_eq!(*server.asked.lock().unwrap(), [None, Some(1000), Some(1000), Some(1000)]);
        assert_eq!(stops(&log), 3, "{log:?}");
        // Nothing is left of half a file.
        assert!(files(cache.path()).is_empty());
    }

    #[test]
    fn cancel_does_not_wait_for_a_silent_connection() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Stall(1000)]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let cancel = Cancel::new();
        let stop = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stop.cancel();
        });
        let patient = Patience { stall: Duration::from_secs(60), attempts: 3, pause: Duration::from_secs(60) };
        let started = Instant::now();
        let (result, _) = run(cache.path(), &image, &cancel, &patient);
        assert_eq!(result.unwrap_err().to_string(), "cancelled");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(files(cache.path()).is_empty());
    }

    #[test]
    fn cancel_does_not_wait_for_the_next_attempt() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Cut(1000)]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let cancel = Cancel::new();
        let stop = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stop.cancel();
        });
        let patient = Patience { stall: Duration::from_secs(60), attempts: 3, pause: Duration::from_secs(60) };
        let started = Instant::now();
        let (result, log) = run(cache.path(), &image, &cancel, &patient);
        assert_eq!(result.unwrap_err().to_string(), "cancelled");
        assert_eq!(stops(&log), 1);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn the_servers_no_is_final_but_its_trouble_is_not() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![Serve::Status(404)]);
        let cache = tempfile::tempdir().unwrap();
        let image = image_at(&server.url, &file);
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("the server answered 404"), "{error}");
        assert_eq!(server.asked.lock().unwrap().len(), 1);
        assert_eq!(stops(&log), 0);
        assert!(files(cache.path()).is_empty());

        let server = serve(file.clone(), vec![Serve::Status(503), Serve::Status(429)]);
        let image = image_at(&server.url, &file);
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        assert_eq!(fs::read(result.unwrap()).unwrap(), file);
        assert_eq!(stops(&log), 2, "{log:?}");
    }

    #[test]
    fn refuses_a_wrong_checksum_and_a_longer_file() {
        let file = bytes(300_000);
        let server = serve(file.clone(), vec![]);
        let cache = tempfile::tempdir().unwrap();
        let mut image = image_at(&server.url, &file);
        image.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        let (result, _) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        let error = result.unwrap_err().to_string();
        assert!(error.contains("wrong checksum"), "{error}");
        assert!(files(cache.path()).is_empty());

        let mut image = image_at(&server.url, &file);
        image.size = 200_000;
        let (result, log) = run(cache.path(), &image, &Cancel::new(), &QUICK);
        let error = result.unwrap_err().to_string();
        assert!(error.contains("larger than the image"), "{error}");
        assert_eq!(stops(&log), 0);
        assert!(files(cache.path()).is_empty());
    }

    /// The events of a run: its stages in order, and its log.
    #[derive(Default)]
    struct Seen(Mutex<Vec<Event>>);

    impl Seen {
        fn sink(&self) -> impl Fn(Event) + Send + Sync + '_ {
            |e| self.0.lock().unwrap().push(e)
        }

        fn phases(&self) -> Vec<Phase> {
            let events = self.0.lock().unwrap();
            events.iter().filter_map(|e| if let Event::Phase { phase } = e { Some(*phase) } else { None }).collect()
        }
    }

    #[test]
    fn a_raw_image_is_used_where_it_is() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        let disk = crate::testimg::disk();
        let file = t.path().join("my.img");
        fs::write(&file, &disk).unwrap();
        let seen = Seen::default();
        let p = prepare_local(&cache, &file, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(p.raw, file);
        assert_eq!(p.size, disk.len() as u64);
        assert_eq!(p.source_sha256, sha256(&disk));
        assert!(!p.official);
        assert_eq!(p.name, "my.img");
        assert_eq!(seen.phases(), [Phase::Check]);
        assert!(files(&cache).is_empty());
    }

    #[test]
    fn a_packed_image_is_unpacked_once() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        let disk = crate::testimg::disk();
        let packed = crate::testimg::xz(&disk);
        let file = t.path().join("my.img.xz");
        fs::write(&file, &packed).unwrap();
        let seen = Seen::default();
        let p = prepare_local(&cache, &file, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Check, Phase::Unpack]);
        assert_eq!(p.size, disk.len() as u64);
        assert_eq!(p.source_sha256, sha256(&packed));
        assert_eq!(fs::read(&p.raw).unwrap(), disk);
        let short = &p.source_sha256[..16];
        assert_eq!(files(&cache), [format!("{short}.img"), format!("{short}.img.ok")]);

        // The second time it is there already.
        let seen = Seen::default();
        let again = prepare_local(&cache, &file, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Check]);
        assert_eq!(again.raw, p.raw);

        // An unpacked image that lost a part is not trusted: it is unpacked again.
        fs::write(&p.raw, &disk[..disk.len() / 2]).unwrap();
        let seen = Seen::default();
        prepare_local(&cache, &file, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Check, Phase::Unpack]);
        assert_eq!(fs::read(&p.raw).unwrap(), disk);
    }

    #[test]
    fn what_is_no_disk_image_is_refused_and_leaves_nothing() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        let prepare = |name: &str, data: &[u8]| {
            let file = t.path().join(name);
            fs::write(&file, data).unwrap();
            let error = prepare_local(&cache, &file, &|_| {}, &Cancel::new()).unwrap_err();
            assert!(files(&cache).is_empty(), "{:?}", files(&cache));
            format!("{error:#}")
        };
        let disk = crate::testimg::disk();
        let error = prepare("notes.img", &bytes(4096));
        assert!(error.contains("no MBR partition table"), "{error}");
        let error = prepare("odd.img", &disk[..disk.len() - 100]);
        assert!(error.contains("not a whole number of sectors"), "{error}");
        let error = prepare("notes.img.xz", &crate::testimg::xz(&bytes(4096)));
        assert!(error.contains("no MBR partition table"), "{error}");
        let error = prepare("odd.img.xz", &crate::testimg::xz(&disk[..disk.len() - 100]));
        assert!(error.contains("not whole sectors"), "{error}");
        // Packed, but cut off in the middle.
        let packed = crate::testimg::xz(&disk);
        let error = prepare("cut.img.xz", &packed[..packed.len() / 2]);
        assert!(error.contains("unpacking"), "{error}");
        let error = prepare_local(&cache, &t.path().join("none.img"), &|_| {}, &Cancel::new()).unwrap_err();
        assert!(format!("{error:#}").contains("opening"), "{error:#}");
    }

    #[test]
    fn cancel_stops_the_unpacking_and_leaves_nothing() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        let file = t.path().join("my.img.xz");
        fs::write(&file, crate::testimg::xz(&crate::testimg::disk())).unwrap();
        let cancel = Cancel::new();
        let stop = cancel.clone();
        let error = prepare_local(
            &cache,
            &file,
            &|e| {
                if matches!(e, Event::Phase { phase: Phase::Unpack }) {
                    stop.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
        assert!(files(&cache).is_empty());
    }

    #[test]
    fn the_known_image_is_downloaded_once_and_then_reused() {
        let disk = crate::testimg::disk();
        let packed = crate::testimg::xz(&disk);
        let server = serve(packed.clone(), vec![]);
        let image = image_at(&server.url, &packed);
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        assert!(!status(&cache, &image).ready);

        let seen = Seen::default();
        let p = prepare_official(&cache, &image, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Download, Phase::Unpack]);
        assert!(p.official);
        assert_eq!(p.name, "Test (1)");
        assert_eq!(p.source_sha256, image.sha256);
        assert_eq!(fs::read(&p.raw).unwrap(), disk);
        assert!(status(&cache, &image).ready);

        // Ready: neither the server nor the files are touched again.
        let seen = Seen::default();
        let again = prepare_official(&cache, &image, &seen.sink(), &Cancel::new()).unwrap();
        assert!(seen.phases().is_empty());
        assert_eq!(again.raw, p.raw);
        assert_eq!(server.asked.lock().unwrap().len(), 1);

        // Without the unpacked image, the downloaded file is checked and unpacked again.
        fs::remove_file(&p.raw).unwrap();
        assert!(!status(&cache, &image).ready);
        let seen = Seen::default();
        prepare_official(&cache, &image, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Check, Phase::Unpack]);
        assert_eq!(server.asked.lock().unwrap().len(), 1);

        // A downloaded file of the right size and the wrong content is downloaded anew.
        fs::remove_file(&p.raw).unwrap();
        let mut damaged = packed.clone();
        damaged[1000] ^= 1;
        fs::write(cache.join(image.file_name), &damaged).unwrap();
        let seen = Seen::default();
        prepare_official(&cache, &image, &seen.sink(), &Cancel::new()).unwrap();
        assert_eq!(seen.phases(), [Phase::Check, Phase::Download, Phase::Unpack]);
        assert_eq!(server.asked.lock().unwrap().len(), 2);
        assert_eq!(fs::read(cache.join(image.file_name)).unwrap(), packed);
        assert_eq!(fs::read(&p.raw).unwrap(), disk);
    }
}
