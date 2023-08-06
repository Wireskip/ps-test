use crate::cfg::Cfg;
use axum::{
    extract::{rejection::JsonRejection, Query},
    Json,
};
use axum::{
    routing::{get, post},
    Router,
};
use config::Config;
use http::{Method, Request};
use hyper::{body, Body};
use log::*;
use once_cell::sync::Lazy;
use semver::Version;
use serde::Deserialize;
use std::{
    env, fs,
    time::{Duration, SystemTime},
};
use ws_common::{
    api::{
        Accesskey, AccesskeyRequest, HttpClient, Status, Withdrawal, WithdrawalRequest,
        WithdrawalStateData,
    },
    nonce,
    time::utime,
};

mod cfg;

// version of this binary
static VERSION: Lazy<Version> = Lazy::new(|| Version::parse(env!("CARGO_PKG_VERSION")).unwrap());

#[derive(Deserialize)]
pub struct BuyParams {
    pub quantity: u64,
}

pub async fn buy_get_handler(
    q: Query<BuyParams>,
) -> axum::response::Result<Json<Accesskey>, Json<Status>> {
    let pr = AccesskeyRequest {
        quantity: q.quantity,
        pof_type: "test".to_string(),
        duration: 600,
    };
    let json_s = serde_json::to_string(&pr).map_err(|e| {
        Json(Status {
            code: 500,
            desc: e.to_string(),
        })
    })?;
    let req = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:8081/issue-accesskeys")
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Body::from(json_s))
        .expect("request builder");
    let res = HttpClient::new().request(req).await.map_err({
        |e| {
            Json(Status {
                code: 500,
                desc: format!(
                    "could not perform auth request to issue accesskeys: {}",
                    e.to_string()
                ),
            })
        }
    })?;
    let bytes = &body::to_bytes(res.into_body()).await.map_err(|e| {
        Json(Status {
            code: 500,
            desc: format!("could not get accesskey from auth: {}", e.to_string()),
        })
    })?;
    let ak: Accesskey = serde_json::from_slice(&bytes).map_err(|e| {
        Json(Status {
            code: 500,
            desc: format!(
                "could not deserialize auth-provided accesskey body ({}): {}",
                std::str::from_utf8(bytes.as_ref()).unwrap(),
                e.to_string()
            ),
        })
    })?;
    Ok(Json(ak))
}

pub async fn withdrawals_post_handler(
    body: Result<Json<WithdrawalRequest>, JsonRejection>,
) -> axum::response::Result<Json<Withdrawal>, Json<Status>> {
    match body {
        Ok(Json(wr)) => {
            let json_s = serde_json::to_string(&wr).map_err(|e| {
                Json(Status {
                    code: 500,
                    desc: e.to_string(),
                })
            })?;
            let req = Request::builder()
                .method(Method::POST)
                .uri("http://localhost:8081/verify-withdrawal-request")
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json_s))
                .expect("request builder");
            // just need to check if it's 200 OK
            let _res = HttpClient::new().request(req).await.map_err({
                |e| {
                    Json(Status {
                        code: 500,
                        desc: format!(
                            "could not perform auth request to verify withdrawal: {}",
                            e.to_string()
                        ),
                    })
                }
            })?;
            let state = {
                match wr.destination.as_ref() {
                    "want_error" => {
                        return Err(Json(Status {
                            code: 400,
                            desc: "as requested, withdrawal failed with error!".to_string(),
                        }))
                    }
                    "want_pending" => ws_common::api::WithdrawalState::Pending,
                    _ => {
                        tokio::time::sleep(Duration::from_secs(1)).await; // simulate work
                        ws_common::api::WithdrawalState::Complete
                    }
                }
            };
            let w = Withdrawal {
                id: nonce::mk_nonce(32),
                state_data: WithdrawalStateData {
                    state,
                    state_changed: utime(SystemTime::now()),
                },
                withdrawal_request: wr,
                receipt: "RECEIPT".to_string(),
            };
            Ok(Json(w))
        }
        Err(e) => Err(Json(Status {
            code: 400,
            desc: e.to_string(),
        })),
    }
}

pub async fn withdrawals_get_handler() -> Json<WithdrawalStateData> {
    Json(WithdrawalStateData {
        state: ws_common::api::WithdrawalState::Complete,
        state_changed: utime(SystemTime::now()),
    })
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut p = env::current_exe().unwrap();
    p.pop();

    let main = p.join("config.json5");
    if !main.exists() {
        fs::write(main, serde_json::to_string(&Cfg::default()).unwrap())
            .expect("Unable to write config file");
    }

    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "init" {
        // our job here is done
        return;
    }

    let cfg: Cfg = Config::builder()
        .add_source(config::File::from(p.join("config")))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap();

    info!("Listening on {}", cfg.address);

    // NOTE: double routes for now
    // (they are considered equal / canonicalized by go stdlib in client but not axum)
    let app = Router::new()
        .route("/buy", get(buy_get_handler))
        .route("//buy", get(buy_get_handler))
        .route("/withdrawals", post(withdrawals_post_handler))
        .route("//withdrawals", post(withdrawals_post_handler))
        .route("/withdrawals", post(withdrawals_post_handler))
        .route("//withdrawals", post(withdrawals_post_handler))
        .route("/withdrawals/:id", get(withdrawals_get_handler))
        .route("//withdrawals/:id", get(withdrawals_get_handler));

    axum::Server::bind(&cfg.address.parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
