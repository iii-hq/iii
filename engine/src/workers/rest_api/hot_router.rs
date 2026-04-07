// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    Router,
    body::Body,
    http::{Request, Response},
    serve::IncomingStream,
};
use futures::Future;
use tokio::sync::RwLock;
use tower::Service;

use crate::engine::Engine;

#[derive(Clone)]
pub struct HotRouter {
    pub inner: Arc<RwLock<Router>>,
    pub engine: Arc<Engine>,
}

impl Service<Request<Body>> for HotRouter {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let router_arc = self.inner.clone();
        let engine = self.engine.clone();
        Box::pin(async move {
            let router_clone = {
                let router_guard = router_arc.read().await;
                router_guard.clone()
            };

            let router_with_extension = router_clone.layer(axum::extract::Extension(engine));
            let mut router_service = router_with_extension.into_service();

            use tower::Service;
            match Service::call(&mut router_service, req).await {
                Ok(response) => Ok(response),
                Err(_infallible) => match _infallible {},
            }
        })
    }
}

pub struct MakeHotRouterService {
    pub router: HotRouter,
}

impl<'a, L: axum::serve::Listener> tower::Service<IncomingStream<'a, L>> for MakeHotRouterService {
    type Response = HotRouter;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: IncomingStream<'a, L>) -> Self::Future {
        let router = self.router.clone();
        Box::pin(async move { Ok(router) })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{
        Extension,
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use futures::task::noop_waker_ref;
    use http_body_util::BodyExt;
    use tokio::sync::{RwLock, oneshot};

    use super::*;

    #[tokio::test]
    async fn hot_router_routes_requests_and_injects_engine() {
        async fn handler(Extension(engine): Extension<Arc<Engine>>) -> impl IntoResponse {
            assert!(engine.functions.get("missing").is_none());
            "hot-router-ok"
        }

        let engine = Arc::new(Engine::new());
        let mut service = HotRouter {
            inner: Arc::new(RwLock::new(Router::new().route("/", get(handler)))),
            engine,
        };

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);
        assert!(matches!(service.poll_ready(&mut cx), Poll::Ready(Ok(()))));

        let response = service
            .call(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .expect("call hot router");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        assert_eq!(&body[..], b"hot-router-ok");
    }

    #[tokio::test]
    async fn make_hot_router_service_serves_requests() {
        let engine = Arc::new(Engine::new());
        let router = HotRouter {
            inner: Arc::new(RwLock::new(
                Router::new().route("/", get(|| async { "make-service-ok" })),
            )),
            engine,
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let server = tokio::spawn(async move {
            axum::serve(listener, MakeHotRouterService { router })
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve router");
        });

        let response = reqwest::get(format!("http://{addr}/"))
            .await
            .expect("request response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.text().await.expect("response text"),
            "make-service-ok"
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server shutdown timeout")
            .expect("server join");
    }
}
