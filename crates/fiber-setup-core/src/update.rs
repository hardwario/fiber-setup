//! Update notices: whether GitHub has a newer published release of FIBER Setup than this
//! version. Nothing is downloaded or installed: the app says so and opens the release's page,
//! where the user takes the new version from.

use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REPOSITORY: &str = "https://github.com/hardwario/fiber-setup";
/// The newest release that is neither a draft nor a pre-release.
pub const LATEST: &str = "https://api.github.com/repos/hardwario/fiber-setup/releases/latest";
const TIMEOUT: Duration = Duration::from_secs(30);

/// A release that is newer than this version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    /// Without the tag's "v": "1.2.0".
    pub version: String,
    /// Its page on GitHub, with the downloads and the changes.
    pub url: String,
}

/// Why a check failed.
#[derive(Debug, PartialEq, Eq)]
pub enum CheckError {
    /// No answer: offline, or GitHub's limit of 60 requests an hour for one address reached
    /// (many computers behind one address). Worth asking again within the hour.
    Network(String),
    /// An answer that cannot be used (a tag that is no version). It will not change soon.
    Answer(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::Network(e) | CheckError::Answer(e) => f.write_str(e),
        }
    }
}

/// Asks `url` (GitHub's [`LATEST`]) for the newest release; gives it when it is newer than this
/// version.
pub fn check(url: &str) -> Result<Option<Release>, CheckError> {
    let current = Version::parse(crate::VERSION)
        .map_err(|e| CheckError::Answer(format!("this version, {}, is not one: {e}", crate::VERSION)))?;
    fetch(url, &current)
}

fn fetch(url: &str, current: &Version) -> Result<Option<Release>, CheckError> {
    let network = |e: ureq::Error| CheckError::Network(e.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // GitHub's API wants one; it says nothing about the computer or the version.
        .user_agent("fiber-setup")
        .build()
        .into();
    let body = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(network)?
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(network)?;
    let latest: Latest = serde_json::from_str(&body).map_err(|e| CheckError::Answer(e.to_string()))?;
    newer(&latest, current).map_err(CheckError::Answer)
}

/// The part of GitHub's release that matters here.
#[derive(Debug, Deserialize)]
struct Latest {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The release, when it is newer than `current`. Its page is built from the checked tag rather
/// than taken from the answer, so the app never opens an address it got from the network.
fn newer(latest: &Latest, current: &Version) -> Result<Option<Release>, String> {
    if latest.draft || latest.prerelease {
        return Ok(None);
    }
    let tag = &latest.tag_name;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|e| format!("unexpected release tag {tag:?}: {e}"))?;
    // A release candidate published without the pre-release mark is no update for everyone.
    if !version.pre.is_empty() {
        return Ok(None);
    }
    Ok((version > *current)
        .then(|| Release { version: version.to_string(), url: format!("{REPOSITORY}/releases/tag/{tag}") }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn latest(tag: &str) -> Latest {
        Latest { tag_name: tag.into(), draft: false, prerelease: false }
    }

    fn current() -> Version {
        Version::new(1, 1, 0)
    }

    #[test]
    fn a_newer_release_is_found_with_its_page() {
        assert_eq!(
            newer(&latest("v1.1.1"), &current()),
            Ok(Some(Release {
                version: "1.1.1".into(),
                url: "https://github.com/hardwario/fiber-setup/releases/tag/v1.1.1".into(),
            }))
        );
        assert_eq!(newer(&latest("1.2.0"), &current()).map(|r| r.map(|r| r.version)), Ok(Some("1.2.0".into())));
        assert_eq!(newer(&latest("v2.0.0"), &current()).map(|r| r.map(|r| r.version)), Ok(Some("2.0.0".into())));
        // Numbers, not letters: 1.10 comes after 1.9.
        assert!(newer(&latest("v1.10.0"), &Version::new(1, 9, 3)).is_ok_and(|r| r.is_some()));
    }

    #[test]
    fn the_same_or_an_older_release_is_not_an_update() {
        assert_eq!(newer(&latest("v1.1.0"), &current()), Ok(None));
        assert_eq!(newer(&latest("v1.0.0"), &current()), Ok(None));
        // A development build of the next version is newer than the last release.
        let dev = Version::parse("1.2.0-rc.1").ok();
        assert!(dev.is_some_and(|v| newer(&latest("v1.1.9"), &v) == Ok(None)));
    }

    #[test]
    fn drafts_and_pre_releases_are_ignored() {
        let mut pre = latest("v2.0.0-beta.1");
        pre.prerelease = true;
        assert_eq!(newer(&pre, &current()), Ok(None));
        let mut draft = latest("v2.0.0");
        draft.draft = true;
        assert_eq!(newer(&draft, &current()), Ok(None));
        // Not marked as one, but its version says so.
        assert_eq!(newer(&latest("v2.0.0-rc.1"), &current()), Ok(None));
    }

    #[test]
    fn a_tag_that_is_no_version_is_an_error() {
        assert!(newer(&latest("nightly"), &current()).is_err());
        // Nothing but a version can reach the page's address.
        assert!(newer(&latest("v1.2.0/../../evil"), &current()).is_err());
        assert!(newer(&latest("v1.2.0?x=https://evil.example"), &current()).is_err());
    }

    #[test]
    fn this_version_is_one() {
        assert!(Version::parse(crate::VERSION).is_ok(), "{}", crate::VERSION);
    }

    /// A server that answers one request with `status` and `body`, and tells what was asked.
    fn serve(status: &'static str, body: String) -> (String, mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/repos/hardwario/fiber-setup/releases/latest", listener.local_addr().unwrap());
        let (asked, request) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let lines: Vec<String> = BufReader::new(stream.try_clone().unwrap())
                .lines()
                .map_while(Result::ok)
                .take_while(|line| !line.is_empty())
                .collect();
            let _ = asked.send(lines);
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        (url, request)
    }

    /// GitHub's answer, cut down to a few of its fields.
    fn github(tag: &str) -> String {
        format!(
            r#"{{"url":"x","html_url":"https://elsewhere.example/{tag}","tag_name":"{tag}","name":"{tag}",
            "draft":false,"prerelease":false,"assets":[{{"name":"fiber-setup.dmg"}}],"body":"Changes"}}"#
        )
    }

    #[test]
    fn asks_and_reads_the_answer() {
        let (url, request) = serve("200 OK", github("v1.3.0"));
        let found = fetch(&url, &current());
        assert_eq!(
            found,
            Ok(Some(Release {
                version: "1.3.0".into(),
                // Built from the tag: not the address the answer carries.
                url: "https://github.com/hardwario/fiber-setup/releases/tag/v1.3.0".into(),
            }))
        );
        let request = request.recv().unwrap();
        assert_eq!(request[0], "GET /repos/hardwario/fiber-setup/releases/latest HTTP/1.1");
        let header = |name: &str| {
            request.iter().find_map(|line| {
                let (key, value) = line.split_once(": ")?;
                key.eq_ignore_ascii_case(name).then(|| value.to_string())
            })
        };
        // Who asks, and nothing about the computer or the version it runs.
        assert_eq!(header("user-agent").as_deref(), Some("fiber-setup"));
        assert_eq!(header("accept").as_deref(), Some("application/vnd.github+json"));
        assert!(!request.iter().any(|line| line.contains(crate::VERSION)), "{request:?}");

        let (url, _request) = serve("200 OK", github("v1.1.0"));
        assert_eq!(fetch(&url, &current()), Ok(None));
    }

    #[test]
    fn tells_no_answer_from_an_answer_of_no_use() {
        // GitHub's limit reached, or no release published yet: ask again soon.
        for status in ["403 Forbidden", "404 Not Found", "500 Internal Server Error"] {
            let (url, _request) = serve(status, r#"{"message":"no"}"#.into());
            assert!(matches!(fetch(&url, &current()), Err(CheckError::Network(_))), "{status}");
        }
        // Nobody there.
        let closed = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/", listener.local_addr().unwrap())
        };
        assert!(matches!(fetch(&closed, &current()), Err(CheckError::Network(_))));
        // An answer, but not GitHub's, or with a tag that is no version: tomorrow is soon enough.
        for body in ["<html>a login page</html>".to_string(), r#"{"name":"x"}"#.to_string(), github("nightly")] {
            let (url, _request) = serve("200 OK", body.clone());
            assert!(matches!(fetch(&url, &current()), Err(CheckError::Answer(_))), "{body}");
        }
    }

    /// The real request, TLS included: `cargo test -p fiber-setup-core -- --ignored asks_github`.
    #[test]
    #[ignore = "needs the network"]
    fn asks_github() {
        let found = fetch(LATEST, &Version::new(0, 0, 1));
        assert!(matches!(&found, Ok(Some(r)) if r.url.starts_with(REPOSITORY)), "{found:?}");
    }
}
