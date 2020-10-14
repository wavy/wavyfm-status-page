#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::core_reexport::time::Duration;
use pretty_env_logger::env_logger::Builder;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io;
use tokio::io::AsyncWriteExt;
use warp::Filter;

lazy_static! {
    static ref PORT: u16 = {
        let var = std::env::var("PORT").unwrap_or_else(|_| String::from("80"));
        var.parse::<u16>()
            .unwrap_or_else(|_| panic!("Invalid port number: {}", var))
    };
    static ref HTTPS: bool = {
        let var = std::env::var("HTTPS").unwrap_or_else(|_| String::from("0"));
        var.parse::<u16>()
            .unwrap_or_else(|_| panic!("Invalid HTTPS flag: {}", var))
            > 0
    };
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
enum SystemStatus {
    Green = 1,
    Yellow = 2,
    Red = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemInfo {
    pub id: String,
    pub status: SystemStatus,
    pub updated: u64,
    pub created: u64,
}

impl SystemInfo {
    fn new(id: &str) -> SystemInfo {
        SystemInfo {
            id: String::from(id),
            status: SystemStatus::Green,
            created: 0,
            updated: 0,
        }
    }
}

fn current_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn update_status(system: &mut SystemInfo, new_status: SystemStatus) -> io::Result<()> {
    let current_time = current_time_seconds();
    if system.status != new_status || system.created == 0 {
        system.status = new_status;
        system.created = current_time;
    }
    system.updated = current_time;
    let json = serde_json::to_vec(&system).unwrap();

    let mut file = File::create(format!("./status/{}.json", system.id)).await?;
    file.write_all(json.as_slice()).await?;

    Ok(())
}

fn logger_builder() -> Builder {
    let mut builder = pretty_env_logger::formatted_timed_builder();

    if let Ok(level) = std::env::var("RUST_LOG") {
        builder.parse_filters(&level);
    }

    builder
}

async fn website_check_task(interval: &mut tokio::time::Interval, system: &mut SystemInfo) {
    loop {
        interval.tick().await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let status = match client.get("https://wavy.fm").send().await {
            Ok(resp) => resp.status().as_u16(),
            Err(e) => {
                error!("Error performing request: {:?}", e);
                999
            }
        };

        let up = status >= 200 && status < 400;
        debug!("Status: {}", status);
        let new_status = if up {
            SystemStatus::Green
        } else {
            SystemStatus::Red
        };
        update_status(system, new_status).await.unwrap();

        info!("{:?}", system);
    }
}

async fn api_check_task(interval: &mut tokio::time::Interval, system: &mut SystemInfo) {
    loop {
        interval.tick().await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let status = match client.get("https://api.wavy.fm/healthz").send().await {
            Ok(resp) => resp.status().as_u16(),
            Err(e) => {
                error!("Error performing request: {:?}", e);
                999
            }
        };

        let up = status >= 200 && status < 400;
        debug!("Status: {}", status);
        let new_status = if up {
            SystemStatus::Green
        } else {
            SystemStatus::Red
        };
        update_status(system, new_status).await.unwrap();

        info!("{:?}", system);
    }
}

async fn get_metrics() -> std::result::Result<String, warp::reject::Rejection> {
    match reqwest::get("https://api.wavy.fm/metrics/listens/48h").await {
        Ok(resp) => match resp.text().await {
            Ok(text) => Ok(text),
            Err(e) => {
                error!("Failed to get metrics: {:?}", e);
                Ok(String::from(""))
            }
        },
        Err(e) => {
            error!("Failed to get metrics: {:?}", e);
            Ok(String::from(""))
        }
    }
}

async fn warp_main() {
    let status_route = warp::get()
        .and(warp::path("status"))
        .and(warp::fs::dir("./status/"));

    let metrics_route = warp::get().and(warp::path("metrics")).and_then(get_metrics);

    let index_route = warp::get()
        .and(warp::path::end())
        .and(warp::fs::file("index.html"));

    let routes = status_route.or(index_route).or(metrics_route);
    if *HTTPS {
        warp::serve(routes)
            .tls()
            .cert_path("/etc/letsencrypt/live/status.wavy.fm/fullchain.pem")
            .key_path("/etc/letsencrypt/live/status.wavy.fm/privkey.pem")
            .run(([0, 0, 0, 0], *PORT))
            .await
    } else {
        warp::serve(routes).run(([0, 0, 0, 0], *PORT)).await
    }
}

async fn warp_http_redirect() {
    if *HTTPS {
        warp::serve(
            warp::any()
                .map(|| warp::redirect(warp::http::Uri::from_static("https://status.wavy.fm"))),
        )
        .run(([0, 0, 0, 0], 80))
        .await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger_builder().init();

    tokio::spawn(async {
        let mut api_interval = tokio::time::interval(Duration::from_secs(30));
        let mut api_system = SystemInfo::new("api");
        api_check_task(&mut api_interval, &mut api_system).await
    });

    tokio::spawn(async {
        let mut website_interval = tokio::time::interval(Duration::from_secs(30));
        let mut website_system = SystemInfo::new("website");
        website_check_task(&mut website_interval, &mut website_system).await
    });

    tokio::spawn(async { warp_http_redirect().await });

    Ok(warp_main().await)
}
