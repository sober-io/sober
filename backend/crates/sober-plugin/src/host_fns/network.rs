//! Host function: outbound HTTP requests.

use std::collections::HashMap;

use extism::{CurrentPlugin, UserData, Val};

use super::{
    CapabilityKind, HostContext, HostError, HttpRequest, HttpResponse, capability_denied_error,
    read_input, write_output,
};
use crate::capability::Capability;

/// Makes an outbound HTTP request.
///
/// Requires the `Network` capability.  When the capability restricts domains,
/// the request URL's host must match one of the allowed entries.  Uses `ureq`
/// for synchronous HTTP (Extism host functions cannot be async).
pub(crate) fn host_http_request_impl(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), extism::Error> {
    let req: HttpRequest = read_input(plugin, inputs)?;
    let data = user_data.get()?;

    // Extract what we need from the context, then drop the lock.
    let allowed_domains = {
        let ctx = data
            .lock()
            .map_err(|e| extism::Error::msg(format!("lock poisoned: {e}")))?;

        if !ctx.has_capability(&CapabilityKind::Network) {
            return capability_denied_error(plugin, outputs, "network");
        }

        // Collect the domain restriction from the Network capability.
        ctx.capabilities
            .iter()
            .find_map(|c| match c {
                Capability::Network { domains } => Some(domains.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };

    // Enforce domain restrictions when the list is non-empty.
    if !allowed_domains.is_empty() {
        let host = extract_host(&req.url);
        match host {
            Some(h) => {
                if !allowed_domains.iter().any(|d| d == &h) {
                    let err = HostError {
                        error: format!("network: domain {h:?} not in allowed list"),
                    };
                    return write_output(plugin, outputs, &err);
                }
            }
            None => {
                let err = HostError {
                    error: format!("network: could not extract host from URL {:?}", req.url),
                };
                return write_output(plugin, outputs, &err);
            }
        }
    }

    // Build the ureq agent — disable treating 4xx/5xx as errors so the
    // plugin sees the actual status code.
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);

    // Dispatch by HTTP method.
    let result = match req.method.to_uppercase().as_str() {
        "GET" => {
            let mut builder = agent.get(&req.url);
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            builder.call()
        }
        "POST" => send_with_body(agent.post(&req.url), &req.headers, &req.body),
        "PUT" => send_with_body(agent.put(&req.url), &req.headers, &req.body),
        "PATCH" => send_with_body(agent.patch(&req.url), &req.headers, &req.body),
        "DELETE" => {
            let mut builder = agent.delete(&req.url);
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            builder.call()
        }
        "HEAD" => {
            let mut builder = agent.head(&req.url);
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            builder.call()
        }
        "OPTIONS" => {
            let mut builder = agent.options(&req.url);
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            builder.call()
        }
        other => {
            let err = HostError {
                error: format!("unsupported HTTP method: {other}"),
            };
            return write_output(plugin, outputs, &err);
        }
    };

    match result {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let mut resp_headers = HashMap::new();
            for (name, value) in response.headers() {
                if let Ok(v) = value.to_str() {
                    resp_headers.insert(name.to_string(), v.to_string());
                }
            }
            let body = response.body_mut().read_to_string().unwrap_or_default();
            let resp = HttpResponse {
                status,
                headers: resp_headers,
                body,
            };
            write_output(plugin, outputs, &resp)
        }
        Err(e) => {
            let err = HostError {
                error: format!("HTTP request failed: {e}"),
            };
            write_output(plugin, outputs, &err)
        }
    }
}

/// Sends an HTTP request that carries a body (POST, PUT, PATCH).
fn send_with_body(
    builder: ureq::RequestBuilder<ureq::typestate::WithBody>,
    headers: &HashMap<String, String>,
    body: &Option<String>,
) -> Result<http::Response<ureq::Body>, ureq::Error> {
    let mut b = builder;
    for (k, v) in headers {
        b = b.header(k.as_str(), v.as_str());
    }
    match body {
        Some(data) => b.send(data.as_bytes()),
        None => b.send_empty(),
    }
}

/// Extracts the host (domain) from a URL string without pulling in the `url` crate.
///
/// Handles `scheme://host:port/path` and `scheme://host/path` forms.
/// Returns `None` if the URL does not contain a recognisable host.
pub(crate) fn extract_host(url: &str) -> Option<String> {
    // Skip past the scheme (e.g. "https://").
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);

    // Strip userinfo if present (user:pass@host).
    let after_userinfo = match after_scheme.find('@') {
        Some(i) => &after_scheme[i + 1..],
        None => after_scheme,
    };

    // Take everything before the first `/` or `?` (path/query start).
    let host_port = after_userinfo
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_userinfo);

    // Strip the port if present.
    let host = if host_port.starts_with('[') {
        // IPv6 bracket notation: [::1]:8080
        host_port.find(']').map(|i| &host_port[1..i])
    } else {
        Some(host_port.rsplit_once(':').map_or(host_port, |(h, _)| h))
    };

    host.filter(|h| !h.is_empty()).map(|h| h.to_lowercase())
}
