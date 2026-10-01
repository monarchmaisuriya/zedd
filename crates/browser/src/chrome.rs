//! Finds and runs the headless Chrome that every browser panel in this zedd process shares.

use crate::cdp::CdpConnection;
use anyhow::{Context as _, Result, anyhow};
use futures::{FutureExt as _, future::Shared};
use gpui::{App, AppContext as _, BackgroundExecutor, Global, Task};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Chromium-based browsers zedd can drive, in the order it looks for them.
#[cfg(target_os = "macos")]
const KNOWN_BROWSERS: &[&str] = &[
    "Google Chrome.app/Contents/MacOS/Google Chrome",
    "Chromium.app/Contents/MacOS/Chromium",
    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "Brave Browser.app/Contents/MacOS/Brave Browser",
];
#[cfg(not(target_os = "macos"))]
const KNOWN_BROWSERS: &[&str] = &[];

/// The browser to run: `configured` when set, else the first known browser installed.
pub fn find_chrome(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(configured) = configured {
        anyhow::ensure!(
            configured.is_file(),
            "`browser.chrome_path` is set to {}, which is not a file",
            configured.display()
        );
        return Ok(configured.to_path_buf());
    }
    let application_dirs = [
        Some(PathBuf::from("/Applications")),
        paths::home_dir().join("Applications").into(),
    ];
    application_dirs
        .iter()
        .flatten()
        .flat_map(|dir| KNOWN_BROWSERS.iter().map(move |browser| dir.join(browser)))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "No Chrome, Chromium, Edge or Brave found. Install one, or set \
                 `browser.chrome_path` to a Chromium-based browser."
            )
        })
}

/// A running Chrome and the connection to it. Chrome exits when zedd's ends of its pipes close,
/// which happens when the connection is dropped or zedd exits, even by crashing.
pub struct Chrome {
    pub connection: Arc<CdpConnection>,
    _process: smol::process::Child,
}

/// Starts Chrome with a persistent profile in `profile_dir`.
pub fn launch(
    executable: &Path,
    profile_dir: &Path,
    executor: &BackgroundExecutor,
) -> Result<Chrome> {
    std::fs::create_dir_all(profile_dir)
        .with_context(|| format!("creating the browser profile at {}", profile_dir.display()))?;
    let (chrome_reads, zedd_writes) = std::io::pipe()?;
    let (zedd_reads, chrome_writes) = std::io::pipe()?;

    let mut command = util::command::new_std_command(executable);
    command
        .arg("--headless")
        .arg("--remote-debugging-pipe")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check");
    pass_pipes_as_fds_3_and_4(&mut command, &chrome_reads, &chrome_writes)?;
    let process = smol::process::Command::from(command)
        .stdin(smol::process::Stdio::null())
        .stdout(smol::process::Stdio::null())
        .stderr(smol::process::Stdio::null())
        .spawn()
        .with_context(|| format!("starting {}", executable.display()))?;
    // Chrome holds its own copies now; zedd keeps only its ends.
    drop((chrome_reads, chrome_writes));

    let (outgoing_tx, outgoing_rx) = async_channel::unbounded::<String>();
    let (incoming_tx, incoming_rx) = async_channel::unbounded::<String>();
    executor
        .spawn(async move {
            use futures::AsyncWriteExt as _;
            let mut writer = smol::Unblock::new(zedd_writes);
            while let Ok(message) = outgoing_rx.recv().await {
                if writer.write_all(message.as_bytes()).await.is_err()
                    || writer.write_all(b"\0").await.is_err()
                    || writer.flush().await.is_err()
                {
                    break;
                }
            }
        })
        .detach();
    executor
        .spawn(async move {
            use futures::AsyncBufReadExt as _;
            let mut reader = futures::io::BufReader::new(smol::Unblock::new(zedd_reads));
            let mut message = Vec::new();
            loop {
                message.clear();
                match reader.read_until(0, &mut message).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        message.pop_if(|byte| *byte == 0);
                        let text = String::from_utf8_lossy(&message).into_owned();
                        if incoming_tx.send(text).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .detach();

    Ok(Chrome {
        connection: CdpConnection::new(outgoing_tx, incoming_rx, executor),
        _process: process,
    })
}

/// Chrome's `--remote-debugging-pipe` reads commands from fd 3 and writes replies to fd 4.
#[cfg(unix)]
fn pass_pipes_as_fds_3_and_4(
    command: &mut std::process::Command,
    chrome_reads: &std::io::PipeReader,
    chrome_writes: &std::io::PipeWriter,
) -> Result<()> {
    use std::os::{fd::AsRawFd as _, unix::process::CommandExt as _};
    let read_fd = chrome_reads.as_raw_fd();
    let write_fd = chrome_writes.as_raw_fd();
    // SAFETY: the closure only makes async-signal-safe libc calls.
    unsafe {
        command.pre_exec(move || {
            // Move both ends above 3 and 4 first, so placing one cannot overwrite the other.
            let high_read = libc::fcntl(read_fd, libc::F_DUPFD, 10);
            let high_write = libc::fcntl(write_fd, libc::F_DUPFD, 10);
            if high_read < 0
                || high_write < 0
                || libc::dup2(high_read, 3) < 0
                || libc::dup2(high_write, 4) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            libc::close(high_read);
            libc::close(high_write);
            Ok(())
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn pass_pipes_as_fds_3_and_4(
    _command: &mut std::process::Command,
    _chrome_reads: &std::io::PipeReader,
    _chrome_writes: &std::io::PipeWriter,
) -> Result<()> {
    anyhow::bail!("zedd's browser is not supported on this platform yet")
}

/// The shared Chrome's connection, for a [`crate::Browser`]'s connector.
pub fn connect_to_shared_chrome(
    executable: Option<PathBuf>,
    cx: &mut App,
) -> Task<Result<Arc<CdpConnection>>> {
    let launch = shared_chrome(executable, cx);
    cx.background_spawn(async move {
        let chrome = launch.await.map_err(|error| anyhow!("{error:#}"))?;
        Ok(chrome.connection.clone())
    })
}

type SharedLaunch = Shared<Task<Result<Arc<Chrome>, Arc<anyhow::Error>>>>;

#[derive(Default)]
struct GlobalChrome(Option<SharedLaunch>);

impl Global for GlobalChrome {}

/// The shared Chrome, started on first use and started again if it has exited.
pub fn shared_chrome(executable: Option<PathBuf>, cx: &mut App) -> SharedLaunch {
    let global = cx.default_global::<GlobalChrome>();
    if let Some(launch) = &global.0 {
        let exited = launch.peek().is_some_and(|chrome| {
            chrome
                .as_ref()
                .map_or(true, |chrome| chrome.connection.is_closed())
        });
        if !exited {
            return launch.clone();
        }
    }
    let profile_dir = paths::data_dir().join("browser").join("profile");
    let executor = cx.background_executor().clone();
    let launch: SharedLaunch = cx
        .background_spawn(async move {
            let executable = find_chrome(executable.as_deref()).map_err(Arc::new)?;
            let chrome = launch(&executable, &profile_dir, &executor).map_err(Arc::new)?;
            chrome
                .connection
                .send(None, "Browser.getVersion", serde_json::json!({}))
                .await
                .with_context(|| {
                    format!(
                        "{} exited at startup. Another zedd may be using the browser profile \
                         at {}.",
                        executable.display(),
                        profile_dir.display()
                    )
                })
                .map_err(Arc::new)?;
            Ok(Arc::new(chrome))
        })
        .shared();
    cx.default_global::<GlobalChrome>().0 = Some(launch.clone());
    launch
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn test_configured_path_must_exist() {
        let error = find_chrome(Some(Path::new("/no/such/browser"))).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`browser.chrome_path` is set to /no/such/browser, which is not a file"
        );
    }

    /// Runs the installed Chrome: `cargo test -p browser -- --ignored`. Dropping the connection
    /// closes zedd's ends of the pipes, as a crash of zedd would.
    #[gpui::test]
    #[ignore]
    async fn test_real_chrome_answers_and_exits_with_its_pipes(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let executable = find_chrome(None).unwrap();
        let profile = tempfile::tempdir().unwrap();
        let chrome = launch(&executable, profile.path(), &cx.executor()).unwrap();

        let version = chrome
            .connection
            .send(None, "Browser.getVersion", serde_json::json!({}))
            .await
            .unwrap();
        assert!(version["product"].as_str().unwrap().contains('/'));

        let pid = chrome._process.id();
        drop(chrome);
        for _ in 0..50 {
            // `kill -0` succeeds while the process exists.
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                return;
            }
            cx.executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
        }
        panic!("Chrome {pid} was still running 5 s after zedd dropped it");
    }
}
