extern crate base64;
extern crate pretty_env_logger;
use base64::encode;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
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

/// Our global unique user id counter.
static NEXT_CONNECTION_ID: AtomicUsize = AtomicUsize::new(1);

// A websocket connection sender
type Connection = mpsc::UnboundedSender<Result<Message, warp::Error>>;
// Vector of connections
type Connections = HashMap<usize, Connection>;
// Hash map key -> active connections
type Pool = HashMap<String, Connections>;
// ??
type Pools = Arc<tokio::sync::Mutex<Pool>>;
//
type Storage = Arc<Mutex<sled::Db>>;

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

async fn ws_reader(ws: WebSocket, key: String, pools: Pools, db: Storage) {
    let mut data = Object {
        created: 0,
        updated: 0,
        data: "".to_string(),
    };
    // https://stackoverflow.com/a/38918024
    let fkey = &key;
    let pkey = key.clone();

    let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);

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
        let result = db.lock().unwrap().get(fkey).unwrap();
        if result.is_some() {
            let raw = &result.unwrap();
            let serialized = String::from_utf8_lossy(raw);
            data = serde_json::from_str(&serialized).unwrap();
        }
    }

    let (ws_tx, mut ws_rx) = ws.split();
    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(ws_tx).map(|result| {
        if let Err(e) = result {
            error!("websocket send error: {}", e);
            // https://github.com/rust-lang/rust/issues/62290
            // TODO: remove connection from pool on disconnect
            // let mut pools = pools.lock().await;
            // let pool = pools.get_mut(&pkey).unwrap();
            // pool.remove(&connection_id);
        }
    }));

    let j = serde_json::to_string(&data);
    tx.send(Ok(Message::text(j.unwrap()))).ok();
    info!("handshake {} sent", key);

    // add connection to the corresponding pool
    let mut mpools = pools.lock().await;
    if !mpools.contains_key(fkey) {
        // create the pool for the key if it doesn't exists
        let mut new_pool = Connections::default();
        new_pool.insert(connection_id, tx);
        mpools.insert(key, new_pool);
    } else {
        let pool = mpools.get_mut(&key).unwrap();
        pool.insert(connection_id, tx);
    }

    info!("pool {}", mpools.get(&pkey).unwrap().len());

    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error: {}", e);
                break;
            }
        };
        // user_message(my_id, msg, &users).await;
        info!("received: {:?}", msg);
    }
}

fn storage() -> Storage {
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
    // pool
    // Keep track of connections, key is usize, value
    // is a websocket sender.
    let pools: Pools = Arc::new(tokio::sync::Mutex::new(Pool::new()));

    // https://users.rust-lang.org/t/why-closure-outlives-variables-owned-by-main/16725
    // even if the variable is Arc<Mutex> you need to clone
    // let subscription_pools = pools.clone();
    let _db = storage();
    let _rest_db = _db.clone();
    let _keys_db = _db.clone();
    // Turn our "state" into a new Filter...
    let subscription_pools = warp::any().map(move || pools.clone());
    let subscription_db = warp::any().map(move || _db.clone());
    let rest_db = warp::any().map(move || _rest_db.clone());
    let keys_db = warp::any().map(move || _keys_db.clone());

    // routes
    let subscription = warp::path::param()
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .and(subscription_pools)
        .and(subscription_db)
        .map(|key: String, ws: warp::ws::Ws, pools: Pools, db: Storage| {
            ws.on_upgrade(move |websocket| ws_reader(websocket, key, pools, db))
        });

    let rest = warp::path::param()
        .and(rest_db)
        .map(|key: String, db: Storage| {
            let result = db.lock().unwrap().get(key).unwrap();
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

    let keys = warp::any().and(keys_db).map(|db: Storage| {
        // iterating over keys https://github.com/spacejam/sled/blob/master/tests/test_tree.rs#L306
        let mut result = db.lock().unwrap().iter().keys();
        let mut keys: Vec<String> = vec![];
        loop {
            let _k = match result.next() {
                Some(_content) => {
                    info!("sled iterating");
                    let raw = &_content.unwrap();
                    let serialized = String::from_utf8_lossy(raw);
                    keys.push(serialized.to_string());
                }
                None => {
                    break;
                }
            };
        }
        warp::reply::json(&keys)
    });

    let routes = subscription.or(rest).or(keys);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
