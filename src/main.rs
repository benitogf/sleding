extern crate base64;
extern crate pretty_env_logger;
use base64::encode;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use warp::ws::{Message, WebSocket};
use warp::Filter;

#[macro_use]
extern crate log;

#[derive(Serialize, Deserialize, Debug)]
struct Object {
    created: u128,
    updated: u128,
    data: String,
}

// Vector of connections
type Connections = Vec<mpsc::UnboundedSender<Result<Message, warp::Error>>>;
// Hash map key -> active connections
type Pool = HashMap<String, Connections>;
// ??
type Pools = Arc<Mutex<Pool>>;

fn now() -> u128 {
    let start = SystemTime::now();
    let result = start.duration_since(UNIX_EPOCH);
    match result {
        Ok(content) => content.as_nanos(),
        Err(error) => {
            error!("the writter failed to insert: {0}", error.to_string());
            0
        }
    }
}

async fn ws_reader(ws: WebSocket, data: Object) {
    let (user_ws_tx, mut user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (_tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            error!("websocket send error: {}", e);
        }
    }));

    let j = serde_json::to_string(&data);
    _tx.send(Ok(Message::text(j.unwrap()))).ok();

    // TODO: add connection to the corresponding pool
    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                // TODO: remove connection on disconnect
                eprintln!("websocket error: {}", e);
                break;
            }
        };
        // user_message(my_id, msg, &users).await;
        info!("received: {:?}", msg);
    }
}

fn storage() -> Arc<Mutex<sled::Db>> {
    // Database
    let db = sled::open("db").unwrap();
    let test = Object {
        created: now(),
        updated: 0,
        data: encode("{\"data\": \"🎁\"}"),
    };
    let serialized = serde_json::to_string(&test).unwrap();
    let result = db.insert("test", serialized.as_bytes());
    let _status = match result {
        Ok(_content) => {
            info!("sled success");
        }
        Err(error) => {
            error!("sled failed to insert: {0}", error.to_string());
        }
    };

    return Arc::new(Mutex::new(db));
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let db = storage();
    // pool
    // Keep track of connections, key is usize, value
    // is a websocket sender.
    let pools: Pools = Arc::new(Mutex::new(Pool::new()));

    // https://users.rust-lang.org/t/why-closure-outlives-variables-owned-by-main/16725
    // even if the variable is Arc<Mutex> you need to clone?
    let subscription_pools = pools.clone();
    let rest_db = db.clone();

    // routes
    let subscription = warp::path::param()
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(move |key: String, ws: warp::ws::Ws| {
            let mut data = Object {
                created: 0,
                updated: 0,
                data: "".to_string(),
            };
            if key == "" {
                // Clock connection
                info!("subscription to the clock");
                data = Object {
                    created: 0,
                    updated: 0,
                    data: now().to_string(),
                };
            } else {
                info!("subscription on {}!", key);
                let result = db.lock().unwrap().get(key).unwrap();
                if result.is_some() {
                    let raw = &result.unwrap();
                    let serialized = String::from_utf8_lossy(raw);
                    data = serde_json::from_str(&serialized).unwrap();
                }
            }
            info!("pool {}", subscription_pools.lock().unwrap().len());
            ws.on_upgrade(|websocket| ws_reader(websocket, data))
        });

    let rest = warp::path::param().map(move |key: String| {
        let result = rest_db.lock().unwrap().get(key).unwrap();
        if result.is_some() {
            let raw = &result.unwrap();
            let serialized = String::from_utf8_lossy(raw);
            let deserialized: Object = serde_json::from_str(&serialized).unwrap();
            warp::reply::json(&deserialized)
        } else {
            warp::reply::json(&Object {
                created: 0,
                updated: 0,
                data: "".to_string(),
            })
        }
    });

    let keys = warp::any().map(|| format!("TODO: get keys from db"));

    let routes = subscription.or(rest).or(keys);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
