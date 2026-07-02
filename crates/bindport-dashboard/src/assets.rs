use super::*;

pub(crate) fn dashboard_index_response(options: &DashboardOptions) -> HttpResponse {
    let body = match static_file(options.static_dir.as_deref(), "index.html", INDEX_HTML) {
        Ok(page) => {
            maybe_inject_dev_reload(inject_app_metadata(page), options.static_dir.as_deref())
        }
        Err(message) => Err(message),
    };
    static_response(body, "text/html; charset=utf-8")
}

pub(crate) fn inject_app_metadata(page: Cow<'static, str>) -> Cow<'static, str> {
    Cow::Owned(
        page.replace("{{APP_NAME}}", DASHBOARD_APP_NAME)
            .replace("{{APP_VERSION}}", env!("CARGO_PKG_VERSION")),
    )
}

pub(crate) fn static_asset_response(
    filename: &'static str,
    embedded: &'static str,
    content_type: &'static str,
    options: &DashboardOptions,
) -> HttpResponse {
    static_response(
        static_file(options.static_dir.as_deref(), filename, embedded),
        content_type,
    )
}

#[cfg(debug_assertions)]
pub(crate) fn dev_version_response(options: &DashboardOptions) -> HttpResponse {
    static_response(
        dev_static_version(options.static_dir.as_deref()),
        "text/plain; charset=utf-8",
    )
}

pub(crate) fn static_response(
    body: Result<Cow<'static, str>, &'static str>,
    content_type: &'static str,
) -> HttpResponse {
    match body {
        Ok(body) => HttpResponse::ok(content_type, &body),
        Err(message) => HttpResponse::internal_error(&json_error_body(message.to_string())),
    }
}

pub(crate) fn static_file(
    static_dir: Option<&Path>,
    filename: &'static str,
    embedded: &'static str,
) -> Result<Cow<'static, str>, &'static str> {
    if let Some(static_dir) = static_dir {
        return fs::read_to_string(static_dir.join(filename))
            .map(Cow::Owned)
            .map_err(|_| "failed to read dashboard asset");
    }

    Ok(Cow::Borrowed(embedded))
}

pub(crate) fn maybe_inject_dev_reload(
    page: Cow<'static, str>,
    static_dir: Option<&Path>,
) -> Result<Cow<'static, str>, &'static str> {
    #[cfg(debug_assertions)]
    {
        if static_dir.is_none() {
            return Ok(page);
        }

        let page = page.into_owned();
        let Some(index) = page.rfind("</body>") else {
            return Err("dashboard HTML is missing </body>");
        };
        let tag = r#"  <script src="/assets/dev-reload.js"></script>
"#;
        let mut output = String::with_capacity(page.len() + tag.len());
        output.push_str(&page[..index]);
        output.push_str(tag);
        output.push_str(&page[index..]);
        Ok(Cow::Owned(output))
    }

    #[cfg(not(debug_assertions))]
    {
        let _ = static_dir;
        Ok(page)
    }
}

#[cfg(debug_assertions)]
pub(crate) fn dev_static_version(
    static_dir: Option<&Path>,
) -> Result<Cow<'static, str>, &'static str> {
    let Some(static_dir) = static_dir else {
        return Err("dashboard static directory is not configured");
    };
    let version = ["index.html", "app.css", "app.js", "dev-reload.js"]
        .into_iter()
        .map(|filename| {
            fs::metadata(static_dir.join(filename))
                .and_then(|metadata| metadata.modified())
                .and_then(|modified| {
                    modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(io::Error::other)
                })
                .map(|duration| duration.as_millis().to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "failed to read dashboard asset metadata")?
        .join(".");

    Ok(Cow::Owned(version))
}
