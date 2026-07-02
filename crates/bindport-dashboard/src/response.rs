pub(crate) fn json_error_body(message: String) -> String {
    format!("{}\n", serde_json::json!({ "error": message }))
}
pub(crate) struct HttpResponse {
    pub(crate) status: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: String,
}

impl HttpResponse {
    pub(crate) fn ok(content_type: &'static str, body: &str) -> Self {
        Self {
            status: "200 OK",
            content_type,
            body: body.to_string(),
        }
    }

    pub(crate) fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            content_type: "text/plain; charset=utf-8",
            body: String::from("not found\n"),
        }
    }

    pub(crate) fn bad_request() -> Self {
        Self {
            status: "400 Bad Request",
            content_type: "text/plain; charset=utf-8",
            body: String::from("bad request\n"),
        }
    }

    pub(crate) fn bad_json_request(body: &str) -> Self {
        Self {
            status: "400 Bad Request",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    pub(crate) fn forbidden() -> Self {
        Self {
            status: "403 Forbidden",
            content_type: "text/plain; charset=utf-8",
            body: String::from("forbidden\n"),
        }
    }

    pub(crate) fn unauthorized() -> Self {
        Self {
            status: "401 Unauthorized",
            content_type: "application/json; charset=utf-8",
            body: json_error_body(String::from("dashboard bearer token is required")),
        }
    }

    pub(crate) fn request_too_large() -> Self {
        Self {
            status: "431 Request Header Fields Too Large",
            content_type: "text/plain; charset=utf-8",
            body: String::from("request too large\n"),
        }
    }

    pub(crate) fn service_unavailable(body: &str) -> Self {
        Self {
            status: "503 Service Unavailable",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    pub(crate) fn internal_error(body: &str) -> Self {
        Self {
            status: "500 Internal Server Error",
            content_type: "application/json; charset=utf-8",
            body: body.to_string(),
        }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        let body = self.body.into_bytes();
        let headers = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.status,
            self.content_type,
            body.len()
        );
        let mut response = headers.into_bytes();
        response.extend(body);
        response
    }
}
