//! Finds web servers this project started: local listening ports whose process runs inside a
//! project folder and answers HTTP.

use anyhow::{Context as _, Result};
use collections::{BTreeMap, HashMap};
use std::{
    io::{Read as _, Write as _},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

const PROBE_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, PartialEq)]
pub struct DevServer {
    pub url: String,
    pub command: String,
}

struct Listener {
    pid: u32,
    command: String,
    port: u16,
}

/// The dev servers running from inside `project_dirs`, by port.
pub async fn find_dev_servers(project_dirs: &[PathBuf]) -> Result<Vec<DevServer>> {
    let listening = run_lsof(&["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]).await?;
    let own_pid = std::process::id();
    let listeners: Vec<Listener> = parse_listeners(&listening)
        .into_iter()
        .filter(|listener| listener.pid != own_pid)
        .collect();
    if listeners.is_empty() {
        return Ok(Vec::new());
    }
    let pids = listeners
        .iter()
        .map(|listener| listener.pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let working_dirs =
        parse_working_dirs(&run_lsof(&["-a", "-p", &pids, "-d", "cwd", "-F", "pn"]).await?);

    let mut servers = BTreeMap::new();
    for listener in listeners {
        let in_project = working_dirs
            .get(&listener.pid)
            .is_some_and(|dir| project_dirs.iter().any(|project| dir.starts_with(project)));
        let port = listener.port;
        if in_project
            && !servers.contains_key(&port)
            && smol::unblock(move || answers_http(port)).await
        {
            servers.insert(
                listener.port,
                DevServer {
                    url: format!("http://localhost:{}", listener.port),
                    command: listener.command,
                },
            );
        }
    }
    Ok(servers.into_values().collect())
}

async fn run_lsof(args: &[&str]) -> Result<String> {
    let output = smol::process::Command::new("lsof")
        .args(args)
        .output()
        .await
        .context("running lsof to list listening ports")?;
    // lsof exits with 1 when nothing matches, which is not an error here.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Reads `lsof -F pcn`: `p` starts a process, `c` names its command, `n` is a listening address
/// such as `*:3000`, `127.0.0.1:5173`, or `[::1]:8080`.
fn parse_listeners(output: &str) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let (mut pid, mut command) = (None, String::new());
    for line in output.lines() {
        let (field, value) = line.split_at(line.len().min(1));
        match field {
            "p" => pid = value.parse().ok(),
            "c" => command = value.to_string(),
            "n" => {
                let port = value
                    .rsplit_once(':')
                    .and_then(|(_, port)| port.parse().ok());
                if let (Some(pid), Some(port)) = (pid, port) {
                    listeners.push(Listener {
                        pid,
                        command: command.clone(),
                        port,
                    });
                }
            }
            _ => {}
        }
    }
    listeners
}

/// Reads `lsof -d cwd -F pn`: each process's working directory.
fn parse_working_dirs(output: &str) -> HashMap<u32, PathBuf> {
    let mut dirs = HashMap::default();
    let mut pid = None;
    for line in output.lines() {
        let (field, value) = line.split_at(line.len().min(1));
        match field {
            "p" => pid = value.parse().ok(),
            "n" => {
                if let Some(pid) = pid {
                    dirs.insert(pid, Path::new(value).to_path_buf());
                }
            }
            _ => {}
        }
    }
    dirs
}

fn answers_http(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, PROBE_TIMEOUT) else {
        return false;
    };
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok();
    if stream
        .write_all(b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut start = [0; 5];
    stream.read_exact(&mut start).is_ok() && &start == b"HTTP/"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsof_output_gives_each_process_ports_and_folder() {
        let listening =
            "p501\ncnode\nf21\nn*:3000\nf22\nn[::1]:3000\np777\ncpython3\nf3\nn127.0.0.1:8000\n";
        let listeners = parse_listeners(listening);
        assert_eq!(
            listeners
                .iter()
                .map(|listener| (listener.pid, listener.command.as_str(), listener.port))
                .collect::<Vec<_>>(),
            vec![
                (501, "node", 3000),
                (501, "node", 3000),
                (777, "python3", 8000)
            ]
        );

        let working_dirs = parse_working_dirs("p501\nfcwd\nn/Users/m/app\np777\nfcwd\nn/tmp\n");
        assert_eq!(working_dirs[&501], PathBuf::from("/Users/m/app"));
        assert_eq!(working_dirs[&777], PathBuf::from("/tmp"));
    }

    /// Starts a real web server in a project folder: `cargo test -p browser_panel -- --ignored`.
    #[test]
    #[ignore]
    fn test_finds_a_web_server_started_in_the_project() {
        let project = tempfile::tempdir().unwrap();
        let project_dir = project.path().canonicalize().unwrap();
        let mut server = smol::process::Command::new("python3")
            .args(["-m", "http.server", "0", "--bind", "127.0.0.1"])
            .current_dir(&project_dir)
            .stdout(smol::process::Stdio::null())
            .stderr(smol::process::Stdio::null())
            .spawn()
            .unwrap();
        let (found, elsewhere) = smol::block_on(async {
            let mut found = Vec::new();
            for _ in 0..50 {
                found = find_dev_servers(std::slice::from_ref(&project_dir))
                    .await
                    .unwrap();
                if !found.is_empty() {
                    break;
                }
                smol::unblock(|| std::thread::sleep(Duration::from_millis(100))).await;
            }
            let elsewhere = find_dev_servers(&[PathBuf::from("/nonexistent-project")])
                .await
                .unwrap();
            (found, elsewhere)
        });
        server.kill().ok();

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].url.starts_with("http://localhost:"));
        assert!(found[0].command.starts_with("Python") || found[0].command.starts_with("python"));
        assert!(
            elsewhere.is_empty(),
            "servers outside the project are not offered"
        );
    }
}
