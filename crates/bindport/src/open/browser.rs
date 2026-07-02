use super::*;

pub(crate) fn open_url_in_browser(url: &str) -> io::Result<()> {
    let url = validate_browser_url(url)?;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = url;
        return Err(io::Error::other(
            "browser launch is not supported on this platform",
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.args(["--", url]);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.args(["--", url]);
        command
    };

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "browser launcher exited with {status}"
        )))
    }
}

pub(crate) fn validate_browser_url(url: &str) -> io::Result<&str> {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(invalid_browser_url());
    };

    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return Err(invalid_browser_url());
    }

    let Some(authority_and_path) = rest.strip_prefix("//") else {
        return Err(invalid_browser_url());
    };

    let authority = authority_and_path
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.is_empty() {
        return Err(invalid_browser_url());
    }

    Ok(url)
}

pub(crate) fn invalid_browser_url() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "browser launch only supports http:// and https:// URLs",
    )
}
