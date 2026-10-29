//! Remote API access is explicit; project ownership is not authentication.
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::IntoResponse,
    Router,
};
use std::sync::Arc;

pub(super) fn protect(app: Router, secret: String) -> Router {
    let secret = Arc::new(secret);
    app.layer(axum::middleware::from_fn(move|request:Request<Body>,next:Next|{
        let secret=secret.clone();async move {
            if !request.uri().path().starts_with("/api/"){return next.run(request).await;}
            let bearer=request.headers().get(header::AUTHORIZATION).and_then(|h|h.to_str().ok())
                .and_then(|h|h.strip_prefix("Bearer ")).is_some_and(|token|constant_time_eq(token.as_bytes(),secret.as_bytes()));
            // HttpOnly same-origin session permits EventSource/WebSocket clients,
            // whose browser APIs cannot set an Authorization header.
            let cookie=request.headers().get(header::COOKIE).and_then(|h|h.to_str().ok()).is_some_and(|cookies|cookies.split(';').filter_map(|c|c.trim().strip_prefix("macc_access=")).any(|token|constant_time_eq(token.as_bytes(),secret.as_bytes())));
            if !bearer&&!cookie{return (StatusCode::UNAUTHORIZED,axum::Json(serde_json::json!({"error":{"code":"AUTH_REQUIRED","category":"authentication","message":"A remote MACC access token is required"}}))).into_response();}
            let origin=request.headers().get(header::ORIGIN).and_then(|h|h.to_str().ok());
            if let Some(origin)=origin{
                let host=request.headers().get(header::HOST).and_then(|h|h.to_str().ok()).unwrap_or_default();
                if origin!=format!("http://{host}")&&origin!=format!("https://{host}"){return (StatusCode::FORBIDDEN,"Cross-origin MACC access denied").into_response();}
            }else if cookie&&!bearer&&!matches!(*request.method(),axum::http::Method::GET|axum::http::Method::HEAD){return (StatusCode::FORBIDDEN,"Cookie-authenticated mutations require a same-origin Origin header").into_response();}
            let secure=origin.is_some_and(|o|o.starts_with("https://"));
            let mut response=next.run(request).await;
            if bearer{
                let value=format!("macc_access={}; HttpOnly; SameSite=Strict; Path=/api; Max-Age=3600{}",secret,if secure{"; Secure"}else{""});
                if let Ok(value)=value.parse(){response.headers_mut().insert(header::SET_COOKIE,value);}
            }
            response
        }
    }))
}
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn remote_auth_rejects_missing_tokens_and_cross_origin_and_supports_stream_cookie() {
        let app = protect(
            Router::new().route(
                "/api/test",
                axum::routing::get(|| async { "ok" }).post(|| async { "ok" }),
            ),
            "a".repeat(32),
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .header("authorization", format!("Bearer {}", "a".repeat(32)))
                    .header("host", "macc.test")
                    .header("origin", "https://evil.test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .header("authorization", format!("Bearer {}", "a".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("HttpOnly; SameSite=Strict"));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .header("cookie", format!("macc_access={}", "a".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/test")
                    .header("cookie", format!("macc_access={}", "a".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
