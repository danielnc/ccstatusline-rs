//! Upstream `src/utils/open-url.ts`: open an http(s) URL with the platform's opener.

fn run(command: &str, args: &[&str]) -> bool {
    std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `openExternalUrl(url).success`.
pub fn open_external_url(url: &str) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    if cfg!(target_os = "macos") {
        run("open", &[url])
    } else if cfg!(target_os = "linux") {
        run("xdg-open", &[url]) || run("gio", &["open", url])
    } else {
        false
    }
}
