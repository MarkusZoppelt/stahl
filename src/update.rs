//! Finding, verifying and installing new versions from GitHub Releases.
//!
//! What an update can do depends on how this copy was installed: see [`Installation`] and [`Plan`].
//! Nothing here runs by itself. [`crate::state::State`] checks in the background, and installs
//! only after someone presses the button.
//!
//! Nothing is installed without its SHA-256 matching the `<asset>.sha256` file from the same
//! release. That catches a corrupt or truncated download. It does not protect against a release
//! that was itself tampered with, because the attacker could publish matching checksums too.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    future::Future,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    pin::Pin,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use reqwest::{Client, Url, redirect};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::Builder;
use tokio::task;
use zip::ZipArchive;

/// The releases page, and the only place a release may send anyone.
pub const RELEASES_PAGE: &str = "https://github.com/MarkusZoppelt/stahl/releases";
const RELEASES_API: &str = "https://api.github.com/repos/MarkusZoppelt/stahl/releases?per_page=30";
/// Where a download must start. It may only be redirected to [`github_host`].
const DOWNLOADS: &str = "https://github.com/MarkusZoppelt/stahl/releases/download/";
/// Far larger than any installer, so a bad server can't fill the disk.
const MAX_DOWNLOAD: u64 = 500 * 1024 * 1024;
const MAX_NOTES: usize = 6000;
const BUNDLE_ID: &str = "net.zoppelt.stahl";

// Absolute paths, so a changed PATH can't substitute them.
const HDIUTIL: &str = "/usr/bin/hdiutil";
const DITTO: &str = "/usr/bin/ditto";
const CODESIGN: &str = "/usr/bin/codesign";
const PLUTIL: &str = "/usr/bin/plutil";
const OPEN: &str = "/usr/bin/open";

/// A pending answer from a [`ReleaseSource`].
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Where releases come from. Tests and the demo use a fake, so they never reach the network.
pub trait ReleaseSource: Send + Sync {
    fn releases(&self) -> Pending<'_, Vec<Release>>;
    /// Save an asset to `to`. The caller decides whether to trust it.
    fn download<'a>(&'a self, asset: &'a Asset, to: &'a Path) -> Pending<'a, ()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    /// Its page on GitHub. Always under [`RELEASES_PAGE`].
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    body: Option<String>,
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
}

/// Only a plain file name: this ends up in a path.
fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether `url` is under `prefix`, after parsing has resolved `..` however it's spelled.
fn is_under(url: &str, prefix: &str) -> bool {
    Url::parse(url).is_ok_and(|url| url.as_str().starts_with(prefix))
}

fn trusted_download(url: &str) -> bool {
    is_under(url, DOWNLOADS)
}

/// GitHub's own hosts: downloads redirect from `github.com` to `*.githubusercontent.com`.
fn github_host(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| {
            matches!(host, "github.com" | "api.github.com")
                || host.ends_with(".githubusercontent.com")
        })
}

/// A release's page, or the list of releases if it claims to be anywhere else.
fn trusted_page(url: Option<String>) -> String {
    url.filter(|url| is_under(url, RELEASES_PAGE))
        .unwrap_or_else(|| RELEASES_PAGE.to_string())
}

fn clip(notes: String) -> String {
    if notes.len() <= MAX_NOTES {
        return notes;
    }
    let mut end = MAX_NOTES;
    while !notes.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n…", &notes[..end])
}

/// A version from a tag. Prereleases don't count, whatever the release says about itself.
fn stable_version(tag: &str) -> Option<Version> {
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    version.pre.is_empty().then_some(version)
}

/// GitHub's list of releases, keeping published, stable ones whose tags are versions.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let raw: Vec<RawRelease> =
        serde_json::from_str(json).map_err(|error| format!("unexpected answer: {error}"))?;
    Ok(raw
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            Some(Release {
                version: stable_version(&release.tag_name)?,
                notes: clip(release.body.unwrap_or_default()),
                page: trusted_page(release.html_url),
                assets: release
                    .assets
                    .into_iter()
                    .filter(|asset| {
                        safe_asset_name(&asset.name)
                            && trusted_download(&asset.browser_download_url)
                    })
                    .map(|asset| Asset {
                        name: asset.name,
                        url: asset.browser_download_url,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// The newest release after `current`. Never an older one, so nothing can downgrade this.
pub fn newest_after(releases: Vec<Release>, current: &Version) -> Option<Release> {
    releases
        .into_iter()
        .filter(|release| release.version.pre.is_empty() && release.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// How this copy got here, which decides what updating it can honestly mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Installation {
    /// In the read-only Nix store, and managed by whoever installed it. Never replaced.
    Nix,
    /// An app bundle from the disk image.
    MacApp { bundle: PathBuf },
    /// `stahl.exe` from the zip, wherever it was unpacked.
    WindowsExe { exe: PathBuf },
    /// Probably the Debian package: replacing it needs root and belongs to dpkg.
    Deb,
    /// Anything else, so nothing we could safely replace.
    Other { why: &'static str },
}

const NOT_AN_INSTALL: &str =
    "This copy wasn't installed from a release, so Stahl won't replace it.";
const MAC_TEMPORARY: &str = "This copy is running from a disk image or a temporary location. \
                             Move it to Applications and open it from there to update in place.";
const NO_BUILD: &str = "There is no release build for this platform yet.";

/// Work out the installation from where the executable really is. Pure, so every platform's rules
/// are tested on every platform.
pub fn detect(exe: &Path, os: &str, arch: &str, has_dpkg: bool) -> Installation {
    if exe.starts_with("/nix/store") {
        return Installation::Nix;
    }
    if is_cargo_build(exe) {
        return Installation::Other {
            why: NOT_AN_INSTALL,
        };
    }
    match (os, arch) {
        ("macos", "aarch64") => match mac_bundle(exe) {
            Some(_) if mac_location_is_temporary(exe) => Installation::Other { why: MAC_TEMPORARY },
            Some(bundle) => Installation::MacApp { bundle },
            None => Installation::Other {
                why: NOT_AN_INSTALL,
            },
        },
        ("windows", "x86_64") => Installation::WindowsExe {
            exe: exe.to_path_buf(),
        },
        ("linux", "x86_64") if has_dpkg && exe.starts_with("/usr") => Installation::Deb,
        ("macos" | "windows", _) => Installation::Other { why: NO_BUILD },
        ("linux", _) if exe.starts_with("/usr") => Installation::Other { why: NO_BUILD },
        _ => Installation::Other {
            why: NOT_AN_INSTALL,
        },
    }
}

fn is_cargo_build(exe: &Path) -> bool {
    let parts: Vec<Component> = exe.components().collect();
    parts.windows(2).any(|pair| {
        pair[0].as_os_str() == "target"
            && (pair[1].as_os_str() == "debug" || pair[1].as_os_str() == "release")
    })
}

/// The `.app` holding this executable, if it's where a bundle's executable belongs.
fn mac_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// macOS runs a quarantined app from a read-only random path until it's moved, and the disk image
/// is read-only too. A swap there would either fail or replace something about to vanish.
fn mac_location_is_temporary(exe: &Path) -> bool {
    exe.starts_with("/Volumes")
        || exe
            .components()
            .any(|part| part.as_os_str() == "AppTranslocation")
}

/// What an update will do, for one installation and one release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Replace the app, then offer to restart.
    Replace { installer: Asset, checksum: Asset },
    /// Verify the package and save it, for someone with the rights to install it.
    SavePackage { package: Asset, checksum: Asset },
    /// Only point at the release, and say why.
    Link { why: &'static str },
}

impl Installation {
    fn wanted_asset<'a>(&self, release: &'a Release) -> Option<&'a Asset> {
        let suffix = match self {
            Installation::MacApp { .. } => "-macos-arm64.dmg",
            Installation::WindowsExe { .. } => "-windows-x64.zip",
            Installation::Deb => "_amd64.deb",
            Installation::Nix | Installation::Other { .. } => return None,
        };
        release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with(suffix))
    }

    pub fn plan(&self, release: &Release) -> Plan {
        let why = match self {
            Installation::Nix => {
                return Plan::Link {
                    why: "Nix manages this copy, so Stahl won't replace it. \
                          Update it the way you installed it, for example with nix profile upgrade.",
                };
            }
            Installation::Other { why } => return Plan::Link { why },
            _ => {
                "This release has no installer with a checksum for this platform, so it can't \
                  be verified. Download it yourself from the release page."
            }
        };
        let Some(asset) = self.wanted_asset(release) else {
            return Plan::Link { why };
        };
        let Some(checksum) = release.asset(&format!("{}.sha256", asset.name)) else {
            return Plan::Link { why };
        };
        let (asset, checksum) = (asset.clone(), checksum.clone());
        if *self == Installation::Deb {
            Plan::SavePackage {
                package: asset,
                checksum,
            }
        } else {
            Plan::Replace {
                installer: asset,
                checksum,
            }
        }
    }

    /// In a sentence, for Settings.
    pub fn describe(&self) -> &'static str {
        match self {
            Installation::Nix => {
                "Managed by Nix. Stahl tells you about updates but won't replace it."
            }
            Installation::MacApp { .. } => "Updates install in place, and you choose when.",
            Installation::WindowsExe { .. } => "Updates replace stahl.exe, and you choose when.",
            Installation::Deb => "Updates download as a verified .deb for you to install.",
            Installation::Other { why } => why,
        }
    }
}

/// What installing left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Installed {
    /// The new version is in place and starts next launch.
    Replaced,
    /// A verified package is saved here, and needs installing by hand.
    Saved(PathBuf),
}

/// Starts the new version of the app.
pub type Relaunch = Arc<dyn Fn(&Installation) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
pub struct Updater {
    source: Option<Arc<dyn ReleaseSource>>,
    installation: Installation,
    current: Version,
    relaunch: Relaunch,
    downloads: PathBuf,
    check_on_startup: bool,
}

/// This build's version.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo versions are semver")
}

impl Updater {
    /// One that never goes anywhere: the default, for tests and the demo.
    pub fn disabled() -> Updater {
        Updater {
            source: None,
            installation: Installation::Other {
                why: NOT_AN_INSTALL,
            },
            current: current_version(),
            relaunch: Arc::new(|_| Ok(())),
            downloads: env::temp_dir(),
            check_on_startup: false,
        }
    }

    /// For tests and the demo. Relaunching does nothing until [`Updater::relaunch_with`].
    pub fn new(
        source: Arc<dyn ReleaseSource>,
        installation: Installation,
        current: Version,
        downloads: PathBuf,
    ) -> Updater {
        Updater {
            source: Some(source),
            installation,
            current,
            relaunch: Arc::new(|_| Ok(())),
            downloads,
            check_on_startup: true,
        }
    }

    /// The real thing, for the app itself.
    pub fn github() -> Updater {
        // Through symlinks, because a Nix profile is one. Not on Windows, where that only turns
        // the path into a verbatim one.
        let exe = env::current_exe()
            .and_then(|exe| {
                if cfg!(windows) {
                    Ok(exe)
                } else {
                    fs::canonicalize(exe)
                }
            })
            .unwrap_or_default();
        let has_dpkg = Path::new("/usr/bin/dpkg").exists();
        let installation = detect(&exe, env::consts::OS, env::consts::ARCH, has_dpkg);
        remove_leftovers(&installation);
        let source: Arc<dyn ReleaseSource> = match GithubReleases::new() {
            Ok(source) => Arc::new(source),
            Err(_) => return Updater::disabled(),
        };
        Updater {
            source: Some(source),
            installation,
            current: current_version(),
            relaunch: Arc::new(relaunch),
            downloads: downloads_dir(),
            check_on_startup: env::var_os("STAHL_NO_UPDATE_CHECK").is_none(),
        }
    }

    pub fn relaunch_with(mut self, relaunch: Relaunch) -> Updater {
        self.relaunch = relaunch;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.source.is_some()
    }

    pub fn checks_on_startup(&self) -> bool {
        self.is_enabled() && self.check_on_startup
    }

    pub fn installation(&self) -> &Installation {
        &self.installation
    }

    pub fn current(&self) -> &Version {
        &self.current
    }

    pub fn plan(&self, release: &Release) -> Plan {
        self.installation.plan(release)
    }

    /// Start the version just installed. The caller quits afterwards.
    pub fn relaunch(&self) -> Result<(), String> {
        (self.relaunch)(&self.installation)
    }

    /// The newest release this copy could move to, if there is one.
    pub async fn check(self) -> Result<Option<Release>, String> {
        let Some(source) = &self.source else {
            return Ok(None);
        };
        let releases = source.releases().await?;
        Ok(newest_after(releases, &self.current))
    }

    /// Download, verify and apply. Any error leaves the installed copy as it was.
    pub async fn install(self, release: Release) -> Result<Installed, String> {
        let Some(source) = &self.source else {
            return Err("Updates are turned off in this build.".to_string());
        };
        if release.version <= self.current {
            return Err(format!(
                "{} is not newer than this version.",
                release.version
            ));
        }
        let (asset, checksum) = match self.plan(&release) {
            Plan::Replace {
                installer,
                checksum,
            } => (installer, checksum),
            Plan::SavePackage { package, checksum } => (package, checksum),
            Plan::Link { why } => return Err(why.to_string()),
        };
        // Private to this user, so nothing can swap the file between verifying and using it.
        let scratch = Builder::new()
            .prefix("stahl-update-")
            .tempdir()
            .map_err(|error| format!("can't create a temporary folder: {error}"))?;
        let file = scratch.path().join(&asset.name);
        let sums = scratch.path().join(&checksum.name);
        source.download(&checksum, &sums).await?;
        source.download(&asset, &file).await?;

        let installation = self.installation.clone();
        let downloads = self.downloads.clone();
        let version = release.version.clone();
        let name = asset.name.clone();
        task::spawn_blocking(move || {
            verify(&file, &name, &sums)?;
            let result = match &installation {
                Installation::MacApp { bundle } => {
                    install_mac_app(&file, bundle, &version).map(|()| Installed::Replaced)
                }
                Installation::WindowsExe { exe } => {
                    install_windows_exe(&file, exe).map(|()| Installed::Replaced)
                }
                Installation::Deb => save_package(&file, &name, &downloads).map(Installed::Saved),
                Installation::Nix | Installation::Other { .. } => {
                    Err("This copy can't be replaced.".to_string())
                }
            };
            // The scratch folder goes with this closure, on success or failure.
            drop(scratch);
            result
        })
        .await
        .map_err(|error| format!("the update stopped unexpectedly: {error}"))?
    }
}

/// The real source: GitHub, over HTTPS only, including every redirect.
pub struct GithubReleases {
    client: Client,
}

impl GithubReleases {
    pub fn new() -> Result<GithubReleases, String> {
        let client = Client::builder()
            .https_only(true)
            .redirect(redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many redirects")
                } else if github_host(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirected away from GitHub")
                }
            }))
            .user_agent(concat!("stahl-updater/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(GithubReleases { client })
    }

    async fn fetch_to_file(&self, asset: &Asset, to: &Path) -> Result<(), String> {
        if !trusted_download(&asset.url) {
            return Err(format!("refusing to download from {}", asset.url));
        }
        let mut response = self
            .client
            .get(&asset.url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|error| format!("download failed: {error}"))?;
        let mut file =
            File::create(to).map_err(|error| format!("can't write the download: {error}"))?;
        let mut written = 0u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("download failed: {error}"))?
        {
            written += chunk.len() as u64;
            if written > MAX_DOWNLOAD {
                return Err("download is larger than any installer should be".to_string());
            }
            file.write_all(&chunk)
                .map_err(|error| format!("can't write the download: {error}"))?;
        }
        Ok(())
    }
}

impl ReleaseSource for GithubReleases {
    fn releases(&self) -> Pending<'_, Vec<Release>> {
        Box::pin(async move {
            let response = self
                .client
                .get(RELEASES_API)
                .header("Accept", "application/vnd.github+json")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .map_err(|error| format!("can't reach GitHub: {error}"))?;
            let body = response
                .text()
                .await
                .map_err(|error| format!("can't read GitHub's answer: {error}"))?;
            parse_releases(&body)
        })
    }

    fn download<'a>(&'a self, asset: &'a Asset, to: &'a Path) -> Pending<'a, ()> {
        Box::pin(self.fetch_to_file(asset, to))
    }
}

/// The digest for `name` in a checksum file, as `sha256sum` writes it.
pub fn parse_checksum(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    })
}

/// The SHA-256 of a file, in lowercase hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Refuse anything but the exact file the release's checksum describes.
fn verify(file: &Path, name: &str, sums: &Path) -> Result<(), String> {
    let text = fs::read_to_string(sums).map_err(|_| "the checksum can't be read".to_string())?;
    let expected = parse_checksum(&text, name)
        .ok_or_else(|| format!("the release has no valid checksum for {name}"))?;
    let actual = sha256_file(file).map_err(|error| format!("can't read the download: {error}"))?;
    if actual != expected {
        return Err(format!(
            "the download doesn't match the release's checksum (expected {expected}, got {actual})"
        ));
    }
    Ok(())
}

/// Put `new` where `target` is, keeping `target` at `parked`. If the second step fails, `target`
/// goes back, and so nothing has changed.
pub fn swap_into_place(new: &Path, target: &Path, parked: &Path) -> io::Result<()> {
    fs::rename(target, parked)?;
    let Err(error) = fs::rename(new, target) else {
        return Ok(());
    };
    match fs::rename(parked, target) {
        Ok(()) => Err(error),
        Err(restoring) => Err(io::Error::other(format!(
            "{error}, and the previous copy couldn't be put back ({restoring}). It is at {}",
            parked.display()
        ))),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path);
    name.push(suffix);
    PathBuf::from(name)
}

/// Windows can't delete a running exe but can rename it, so the old one lasts until the next launch.
fn windows_old(exe: &Path) -> PathBuf {
    with_suffix(exe, ".old")
}

fn windows_new(exe: &Path) -> PathBuf {
    with_suffix(exe, ".new")
}

fn remove_leftovers(installation: &Installation) {
    if let Installation::WindowsExe { exe } = installation {
        let _ = fs::remove_file(windows_old(exe));
        let _ = fs::remove_file(windows_new(exe));
    }
}

fn install_windows_exe(zip: &Path, exe: &Path) -> Result<(), String> {
    let new = windows_new(exe);
    let result = extract_exe(zip, &new).and_then(|()| {
        let parked = windows_old(exe);
        // From an earlier update, if this copy never restarted.
        let _ = fs::remove_file(&parked);
        swap_into_place(&new, exe, &parked)
            .map_err(|error| format!("can't replace {}: {error}", exe.display()))
    });
    if result.is_err() {
        let _ = fs::remove_file(&new);
    }
    result
}

fn extract_exe(zip: &Path, to: &Path) -> Result<(), String> {
    let bad = |error: String| format!("the archive can't be unpacked: {error}");
    let file = File::open(zip).map_err(|error| bad(error.to_string()))?;
    let mut archive = ZipArchive::new(file).map_err(|error| bad(error.to_string()))?;
    // By its exact name, never a path from the archive.
    let mut entry = archive
        .by_name("stahl.exe")
        .map_err(|error| bad(error.to_string()))?;
    let mut out = File::create(to).map_err(|error| {
        format!(
            "can't write next to the app ({}): {error}",
            to.parent().unwrap_or(to).display()
        )
    })?;
    let mut magic = [0; 2];
    entry
        .read_exact(&mut magic)
        .map_err(|error| bad(error.to_string()))?;
    if magic != *b"MZ" {
        return Err(bad("it doesn't hold a Windows program".to_string()));
    }
    out.write_all(&magic)
        .and_then(|()| io::copy(&mut entry.take(MAX_DOWNLOAD), &mut out).map(|_| ()))
        .map_err(|error| bad(error.to_string()))
}

fn run_tool(tool: &str, args: &[&OsStr]) -> Result<String, String> {
    let output = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("can't run {tool}: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Err(format!(
        "{tool} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// Unmounts when dropped, so a failure can't leave the disk image attached.
struct Mounted(PathBuf);

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = run_tool(
            HDIUTIL,
            &["detach".as_ref(), self.0.as_os_str(), "-force".as_ref()],
        );
    }
}

fn the_app_in(dir: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
    let mut apps = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"));
    match (apps.next(), apps.next()) {
        (Some(app), None) => Ok(app),
        _ => Err("the disk image should hold exactly one app".to_string()),
    }
}

fn plist_value(plist: &Path, key: &str) -> Result<String, String> {
    run_tool(
        PLUTIL,
        &[
            "-extract".as_ref(),
            key.as_ref(),
            "raw".as_ref(),
            "-o".as_ref(),
            "-".as_ref(),
            plist.as_os_str(),
        ],
    )
}

/// Before anything moves: is this really intact, and really the version the release promised?
/// The app is only ad-hoc signed, so `codesign` proves it's undamaged, not who built it.
fn check_mac_app(app: &Path, version: &Version) -> Result<(), String> {
    run_tool(
        CODESIGN,
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            app.as_os_str(),
        ],
    )?;
    let plist = app.join("Contents/Info.plist");
    let identifier = plist_value(&plist, "CFBundleIdentifier")?;
    if identifier != BUNDLE_ID {
        return Err(format!(
            "the disk image holds a different app: {identifier}"
        ));
    }
    let found = plist_value(&plist, "CFBundleShortVersionString")?;
    if found != version.to_string() {
        return Err(format!("the disk image holds {found}, not {version}"));
    }
    let executable = plist_value(&plist, "CFBundleExecutable")?;
    if !app.join("Contents/MacOS").join(&executable).is_file() {
        return Err("the app in the disk image has no executable".to_string());
    }
    Ok(())
}

fn install_mac_app(dmg: &Path, bundle: &Path, version: &Version) -> Result<(), String> {
    let parent = bundle
        .parent()
        .ok_or_else(|| "the app has no folder".to_string())?;
    // Beside the app, so the swap is a rename on one volume. The old bundle lands in here and
    // goes with it: macOS keeps a deleted bundle's running code mapped.
    let stage = Builder::new()
        .prefix(".stahl-update-")
        .tempdir_in(parent)
        .map_err(|error| {
            format!(
                "can't write next to the app ({}): {error}",
                parent.display()
            )
        })?;
    let mount = dmg.with_extension("mount");
    fs::create_dir(&mount).map_err(|error| error.to_string())?;
    run_tool(
        HDIUTIL,
        &[
            "attach".as_ref(),
            dmg.as_os_str(),
            "-nobrowse".as_ref(),
            "-readonly".as_ref(),
            "-noautoopen".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
        ],
    )?;
    let mounted = Mounted(mount);
    let app = the_app_in(&mounted.0)?;
    let new = stage.path().join("new.app");
    // --noqtn: a quarantined copy of an ad-hoc signed app might refuse to open, leaving someone
    // with a broken install. It is only reached after the checksum and these checks passed, and
    // after the user pressed Install. This does mean Gatekeeper doesn't review the new version.
    run_tool(
        DITTO,
        &["--noqtn".as_ref(), app.as_os_str(), new.as_os_str()],
    )?;
    drop(mounted);
    check_mac_app(&new, version)?;
    let swapped = swap_into_place(&new, bundle, &stage.path().join("old.app"))
        .map_err(|error| format!("can't replace {}: {error}", bundle.display()));
    // The old bundle is the only copy left, so it must outlive this folder.
    if swapped.is_err() && !bundle.exists() {
        let _ = stage.keep();
    }
    swapped
}

/// Where a browser would save it. Never overwritten until the copy is complete.
fn save_package(file: &Path, name: &str, dir: &Path) -> Result<PathBuf, String> {
    let to = dir.join(name);
    let partial = with_suffix(&to, ".part");
    let saved = fs::copy(file, &partial).and_then(|_| fs::rename(&partial, &to));
    if let Err(error) = saved {
        let _ = fs::remove_file(&partial);
        return Err(format!("can't save to {}: {error}", dir.display()));
    }
    Ok(to)
}

fn downloads_dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from);
    env::var_os("XDG_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .or_else(|| home.clone().map(|home| home.join("Downloads")))
        .filter(|dir| dir.is_dir())
        .or(home)
        .unwrap_or_else(env::temp_dir)
}

/// The command that installs a saved package, quoted for a shell.
pub fn install_command(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\'', r"'\''");
    format!("sudo apt install '{path}'")
}

fn relaunch(installation: &Installation) -> Result<(), String> {
    let started = match installation {
        // A new instance: this one is still quitting.
        Installation::MacApp { bundle } => Command::new(OPEN).arg("-n").arg(bundle).spawn(),
        Installation::WindowsExe { exe } => Command::new(exe).spawn(),
        Installation::Nix | Installation::Deb | Installation::Other { .. } => {
            return Err("this copy can't restart itself".to_string());
        }
    };
    started
        .map(|_| ())
        .map_err(|error| format!("can't start the new version: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn versions(releases: &[Release]) -> Vec<String> {
        releases
            .iter()
            .map(|release| release.version.to_string())
            .collect()
    }

    const GITHUB: &str = r#"[
      {"tag_name": "v9.0.0", "draft": true, "prerelease": false, "assets": []},
      {"tag_name": "v8.0.0-rc.1", "draft": false, "prerelease": false, "assets": []},
      {"tag_name": "v7.0.0", "draft": false, "prerelease": true, "assets": []},
      {"tag_name": "nightly", "draft": false, "prerelease": false, "assets": []},
      {"tag_name": "v0.10.0", "draft": false, "prerelease": false, "body": "Notes",
       "html_url": "https://github.com/MarkusZoppelt/stahl/releases/tag/v0.10.0",
       "assets": [
         {"name": "stahl-capital-macos-arm64.dmg", "browser_download_url":
          "https://github.com/MarkusZoppelt/stahl/releases/download/v0.10.0/stahl-capital-macos-arm64.dmg"},
         {"name": "evil.dmg", "browser_download_url": "https://example.com/evil.dmg"},
         {"name": "../../evil.dmg", "browser_download_url":
          "https://github.com/MarkusZoppelt/stahl/releases/download/v0.10.0/evil.dmg"},
         {"name": "http.dmg", "browser_download_url":
          "http://github.com/MarkusZoppelt/stahl/releases/download/v0.10.0/http.dmg"}
       ]},
      {"tag_name": "v0.9.0", "draft": false, "prerelease": false, "body": null,
       "html_url": "https://example.com/phish", "assets": []}
    ]"#;

    #[test]
    fn a_download_cant_climb_out_of_the_release_folder() {
        assert!(trusted_download(&format!("{DOWNLOADS}v1.0.0/a.dmg")));
        for path in [
            "../../../evil/a.dmg",
            "%2e%2e/%2E%2E/%2e%2e/evil/a.dmg",
            ".%2e/.%2e/a.dmg",
        ] {
            let url = format!("{DOWNLOADS}{path}");
            assert!(!trusted_download(&url), "{url}");
        }
        assert_eq!(
            trusted_page(Some(format!("{RELEASES_PAGE}/%2e%2e/%2e%2e/evil"))),
            RELEASES_PAGE
        );
    }

    #[test]
    fn redirects_stay_on_github() {
        let host = |url: &str| github_host(&Url::parse(url).unwrap());
        assert!(host("https://github.com/a"));
        assert!(host("https://release-assets.githubusercontent.com/a"));
        assert!(!host("http://release-assets.githubusercontent.com/a"));
        assert!(!host("https://evilgithubusercontent.com/a"));
        assert!(!host("https://github.com.evil.example/a"));
        assert!(!host("https://githubusercontent.com.evil.example/a"));
    }

    #[test]
    fn only_published_stable_versions_count() {
        let releases = parse_releases(GITHUB).unwrap();
        assert_eq!(versions(&releases), ["0.10.0", "0.9.0"]);
    }

    #[test]
    fn versions_compare_as_numbers_not_text() {
        let releases = parse_releases(GITHUB).unwrap();
        let newest = newest_after(releases.clone(), &version("0.2.0")).unwrap();
        assert_eq!(newest.version, version("0.10.0"), "0.10 is after 0.9");
        assert!(newest_after(releases.clone(), &version("0.10.0")).is_none());
        assert!(newest_after(releases, &version("1.0.0")).is_none());
    }

    #[test]
    fn a_prerelease_is_never_offered_even_from_a_fake_source() {
        let release = Release {
            version: version("9.0.0-beta.1"),
            notes: String::new(),
            page: RELEASES_PAGE.to_string(),
            assets: Vec::new(),
        };
        assert!(newest_after(vec![release], &version("0.2.0")).is_none());
    }

    #[test]
    fn untrusted_urls_and_names_are_dropped() {
        let releases = parse_releases(GITHUB).unwrap();
        let names: Vec<&str> = releases[0].assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["stahl-capital-macos-arm64.dmg"]);
        assert_eq!(releases[1].page, RELEASES_PAGE, "not the phishing page");
    }

    #[test]
    fn long_notes_are_clipped_on_a_character_boundary() {
        let clipped = clip("€".repeat(MAX_NOTES));
        assert!(clipped.len() < MAX_NOTES + 10);
    }

    #[test]
    fn nix_is_never_replaced_on_any_platform() {
        for (os, arch) in [
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "aarch64"),
        ] {
            let exe = Path::new("/nix/store/abc-stahl-0.2.0/bin/.stahl-wrapped");
            assert_eq!(detect(exe, os, arch, true), Installation::Nix);
        }
        // Even if someone built a bundle in there.
        let exe = Path::new("/nix/store/abc/Stahl Capital.app/Contents/MacOS/stahl");
        assert_eq!(detect(exe, "macos", "aarch64", false), Installation::Nix);
    }

    #[test]
    fn each_platform_is_recognised() {
        let bundle = "/Applications/Stahl Capital.app";
        assert_eq!(
            detect(
                Path::new(&format!("{bundle}/Contents/MacOS/stahl")),
                "macos",
                "aarch64",
                false
            ),
            Installation::MacApp {
                bundle: PathBuf::from(bundle)
            }
        );
        assert_eq!(
            detect(Path::new("/usr/bin/stahl"), "linux", "x86_64", true),
            Installation::Deb
        );
        assert_eq!(
            detect(Path::new("C:/Tools/stahl.exe"), "windows", "x86_64", false),
            Installation::WindowsExe {
                exe: PathBuf::from("C:/Tools/stahl.exe")
            }
        );
    }

    #[test]
    fn nothing_uninstalled_or_unsupported_gets_replaced() {
        let other = |exe: &str, os: &str, arch: &str, dpkg: bool| {
            matches!(
                detect(Path::new(exe), os, arch, dpkg),
                Installation::Other { .. }
            )
        };
        assert!(other("/x/target/debug/stahl", "macos", "aarch64", false));
        assert!(other(
            "C:/x/target/release/stahl.exe",
            "windows",
            "x86_64",
            false
        ));
        assert!(
            other("/usr/local/bin/stahl", "macos", "aarch64", false),
            "bare binary"
        );
        assert!(other("/home/me/.cargo/bin/stahl", "linux", "x86_64", true));
        assert!(
            other("/usr/bin/stahl", "linux", "x86_64", false),
            "not dpkg"
        );
        assert!(
            other("/usr/bin/stahl", "linux", "aarch64", true),
            "no arm deb"
        );
        assert!(
            other(
                "/Applications/S.app/Contents/MacOS/stahl",
                "macos",
                "x86_64",
                false
            ),
            "the disk image is Apple Silicon only"
        );
    }

    #[test]
    fn a_quarantined_or_mounted_app_is_not_swapped() {
        for exe in [
            "/private/var/folders/x/AppTranslocation/ABC/d/Stahl Capital.app/Contents/MacOS/stahl",
            "/Volumes/Stahl Capital/Stahl Capital.app/Contents/MacOS/stahl",
        ] {
            assert!(matches!(
                detect(Path::new(exe), "macos", "aarch64", false),
                Installation::Other { .. }
            ));
        }
    }

    fn release_with(names: &[&str]) -> Release {
        Release {
            version: version("0.3.0"),
            notes: String::new(),
            page: RELEASES_PAGE.to_string(),
            assets: names
                .iter()
                .map(|name| Asset {
                    name: name.to_string(),
                    url: format!("{DOWNLOADS}v0.3.0/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn a_plan_needs_the_right_installer_and_its_checksum() {
        let mac = Installation::MacApp {
            bundle: PathBuf::from("/Applications/S.app"),
        };
        let dmg = "stahl-capital-macos-arm64.dmg";
        let dmg_sum = "stahl-capital-macos-arm64.dmg.sha256";
        assert!(matches!(
            mac.plan(&release_with(&[dmg, dmg_sum])),
            Plan::Replace { .. }
        ));
        assert!(
            matches!(mac.plan(&release_with(&[dmg])), Plan::Link { .. }),
            "no checksum, no install"
        );
        assert!(matches!(mac.plan(&release_with(&[])), Plan::Link { .. }));

        // Asset names as the release workflow really makes them.
        let deb = "stahl_0.3.0_amd64.deb";
        assert!(matches!(
            Installation::Deb.plan(&release_with(&[deb, "stahl_0.3.0_amd64.deb.sha256"])),
            Plan::SavePackage { .. }
        ));
        let windows = Installation::WindowsExe {
            exe: PathBuf::from("stahl.exe"),
        };
        assert!(matches!(
            windows.plan(&release_with(&[
                "stahl-capital-windows-x64.zip",
                "stahl-capital-windows-x64.zip.sha256"
            ])),
            Plan::Replace { .. }
        ));
        assert!(matches!(
            Installation::Nix.plan(&release_with(&[dmg, dmg_sum])),
            Plan::Link { .. }
        ));
    }

    #[test]
    fn checksums_parse_as_every_tool_writes_them() {
        let hex = "ab".repeat(32);
        assert_eq!(
            parse_checksum(&format!("{hex}  a.dmg\n"), "a.dmg").as_deref(),
            Some(hex.as_str())
        );
        assert_eq!(
            parse_checksum(&format!("{hex} *a.zip\r\n"), "a.zip").as_deref(),
            Some(hex.as_str()),
            "binary mode and Windows line endings"
        );
        assert_eq!(
            parse_checksum(&format!("{}  a.dmg", hex.to_uppercase()), "a.dmg").as_deref(),
            Some(hex.as_str())
        );
        assert_eq!(parse_checksum(&format!("{hex}  b.dmg"), "a.dmg"), None);
        assert_eq!(parse_checksum("abc  a.dmg", "a.dmg"), None, "too short");
        assert_eq!(parse_checksum("<html>not found</html>", "a.dmg"), None);
        assert_eq!(parse_checksum("", "a.dmg"), None);
    }

    #[test]
    fn verifying_refuses_a_changed_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.dmg");
        fs::write(&file, b"installer").unwrap();
        let sums = dir.path().join("a.dmg.sha256");
        let digest = sha256_file(&file).unwrap();
        fs::write(&sums, format!("{digest}  a.dmg\n")).unwrap();
        verify(&file, "a.dmg", &sums).unwrap();

        fs::write(&file, b"tampered!").unwrap();
        let error = verify(&file, "a.dmg", &sums).unwrap_err();
        assert!(error.contains("doesn't match"), "{error}");
    }

    #[test]
    fn sha256_matches_a_known_value() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("abc");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_failed_swap_puts_the_old_copy_back() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("app");
        let parked = dir.path().join("app.old");
        fs::write(&target, b"old").unwrap();

        let missing = dir.path().join("missing");
        assert!(swap_into_place(&missing, &target, &parked).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!parked.exists());

        let new = dir.path().join("new");
        fs::write(&new, b"new").unwrap();
        swap_into_place(&new, &target, &parked).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(&parked).unwrap(), b"old");
    }

    fn zip_of(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn the_windows_flow_swaps_the_exe_and_keeps_the_old_one_until_next_launch() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("stahl.exe");
        fs::write(&exe, b"MZ old").unwrap();
        let archive = dir.path().join("update.zip");
        fs::write(&archive, zip_of("stahl.exe", b"MZ new")).unwrap();

        install_windows_exe(&archive, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"MZ new");
        assert_eq!(fs::read(windows_old(&exe)).unwrap(), b"MZ old");

        remove_leftovers(&Installation::WindowsExe { exe: exe.clone() });
        assert!(!windows_old(&exe).exists());
        assert_eq!(fs::read(&exe).unwrap(), b"MZ new");
    }

    #[test]
    fn a_bad_windows_archive_changes_nothing() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("stahl.exe");
        fs::write(&exe, b"MZ old").unwrap();
        for (name, contents) in [
            ("other.exe", b"MZ new".as_slice()),
            ("stahl.exe", b"#!/bin/sh".as_slice()),
        ] {
            let archive = dir.path().join("update.zip");
            fs::write(&archive, zip_of(name, contents)).unwrap();
            assert!(install_windows_exe(&archive, &exe).is_err(), "{name}");
            assert_eq!(fs::read(&exe).unwrap(), b"MZ old");
            assert!(!windows_new(&exe).exists(), "no debris");
            assert!(!windows_old(&exe).exists());
        }
        let archive = dir.path().join("update.zip");
        fs::write(&archive, b"not a zip").unwrap();
        assert!(install_windows_exe(&archive, &exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"MZ old");
    }

    #[test]
    fn a_package_is_saved_whole_or_not_at_all() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("verified.deb");
        fs::write(&file, b"package").unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).unwrap();

        let saved = save_package(&file, "stahl.deb", &downloads).unwrap();
        assert_eq!(saved, downloads.join("stahl.deb"));
        assert_eq!(fs::read(&saved).unwrap(), b"package");
        assert!(!with_suffix(&saved, ".part").exists());

        let nowhere = dir.path().join("does-not-exist");
        assert!(save_package(&file, "stahl.deb", &nowhere).is_err());
    }

    #[test]
    fn the_install_command_is_quoted_for_a_shell() {
        assert_eq!(
            install_command(Path::new("/home/a b/it's.deb")),
            r"sudo apt install '/home/a b/it'\''s.deb'"
        );
    }

    /// Builds a real disk image and swaps a real, throwaway app bundle, in a temp directory. It
    /// needs `hdiutil`, which a sandboxed build may not be allowed to use, so it runs by hand:
    /// `cargo test --lib -- --ignored mac_flow`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses hdiutil and codesign; run by hand"]
    fn mac_flow_on_a_real_disk_image() {
        fn make_app(app: &Path, version: &str, marker: &str) {
            let macos = app.join("Contents/MacOS");
            fs::create_dir_all(&macos).unwrap();
            fs::write(
                app.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
                     <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>\
                     <key>CFBundleShortVersionString</key><string>{version}</string>\
                     <key>CFBundleExecutable</key><string>stahl</string>\
                     </dict></plist>"
                ),
            )
            .unwrap();
            fs::write(macos.join("stahl"), format!("#!/bin/sh\necho {marker}\n")).unwrap();
            let status = Command::new("/bin/chmod")
                .arg("+x")
                .arg(macos.join("stahl"))
                .status()
                .unwrap();
            assert!(status.success());
            run_tool(
                CODESIGN,
                &[
                    "--force".as_ref(),
                    "--deep".as_ref(),
                    "--sign".as_ref(),
                    "-".as_ref(),
                    app.as_os_str(),
                ],
            )
            .unwrap();
        }

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir(&source).unwrap();
        make_app(&source.join("Stahl Capital.app"), "0.3.0", "new");
        let dmg = dir.path().join("update.dmg");
        run_tool(
            HDIUTIL,
            &[
                "create".as_ref(),
                "-volname".as_ref(),
                "Stahl Capital".as_ref(),
                "-srcfolder".as_ref(),
                source.as_os_str(),
                "-ov".as_ref(),
                "-format".as_ref(),
                "UDZO".as_ref(),
                dmg.as_os_str(),
            ],
        )
        .unwrap();

        let installed = dir.path().join("Applications");
        fs::create_dir(&installed).unwrap();
        let bundle = installed.join("Stahl Capital.app");
        make_app(&bundle, "0.2.0", "old");
        let marker = || fs::read_to_string(bundle.join("Contents/MacOS/stahl")).unwrap();

        // The wrong version must not get in, and nothing changes.
        assert!(install_mac_app(&dmg, &bundle, &version("0.4.0")).is_err());
        assert!(marker().contains("old"));
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 1, "no debris");
        let _ = fs::remove_dir_all(dmg.with_extension("mount"));

        install_mac_app(&dmg, &bundle, &version("0.3.0")).unwrap();
        assert!(marker().contains("new"));
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 1, "no debris");
        check_mac_app(&bundle, &version("0.3.0")).unwrap();
        let quarantine = Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(&bundle)
            .output()
            .unwrap();
        assert!(!quarantine.status.success(), "no quarantine on the new app");
    }
}
