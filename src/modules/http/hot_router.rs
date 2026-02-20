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
    extract::Extension,
    http::{Request, Response, StatusCode},
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
            let router = router_arc.read().await;
            // Clone the router and add the engine as Extension
            // This allows using the router without state directly as Service
            let router_with_extension = router.clone().layer(Extension(engine));
            let mut svc = router_with_extension;
            // The Router without state implements Service<Request<Body>> directly
            match svc.call(req).await {
                Ok(response) => Ok(response),
                Err(_) => {
                    tracing::error!("Error handling request in HotRouter");
                    let error_response = Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal Server Error"))
                        .unwrap();
                    Ok(error_response)
                }
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
