extern crate base64;
extern crate pretty_env_logger;
use base64::encode;
use futures::future;
use futures::{Future, FutureExt, StreamExt};
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

type Connections = Vec<mpsc::UnboundedSender<Result<Message, warp::Error>>>;
type Pool = HashMap<String, Connections>;
type Pools = Arc<Mutex<Vec<Pool>>>;

// type Pool = []Users

fn now() -> u128 {
    let start = SystemTime::now();
    let result = start.duration_since(UNIX_EPOCH);
    match result {
        Ok(content) => content.as_nanos(),
        Err(error) => {
            error!(
                "this is not the way, the writter failed to insert: {0}",
                error.to_string()
            );
            0
        }
    }
}

fn ws_connected(ws: WebSocket, _pools: Pools) -> impl Future<Output = Result<(), ()>> {
    // Use a counter to assign a new unique ID for this user.

    // Split the socket into a sender and receive of messages.
    let (user_ws_tx, user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (_tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            eprintln!("websocket send error: {}", e);
        }
    }));

    // Save the sender in our list of connected users.
    // users.lock().unwrap().insert(0, tx);

    // Return a `Future` that is basically a state machine managing

    // Make an extra clone to give to our disconnection handler...
    // let users2 = users.clone();

    user_ws_rx
        // Every time the user sends a message, broadcast it to
        // all other users...
        .for_each(move |_msg| {
            // user_message(my_id, msg.unwrap(), &users);
            future::ready(())
        })
        // for_each will keep processing as long as the user stays
        // connected. Once they disconnect, then...
        .then(move |result| {
            // user_disconnected(my_id, &users2);
            future::ok(result)
        })
    // If at any time, there was a websocket error, log here...
    // .map_err(move |e| {
    //     eprintln!("websocket error(uid={}): {}", my_id, e);
    // })
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // Database
    let t = sled::open("db").unwrap();
    let test = Object {
        created: now(),
        updated: 0,
        data: encode("{\"data\": \"🎁\"}"),
    };
    let serialized = serde_json::to_string(&test).unwrap();
    let result = t.insert("test", serialized.as_bytes());
    let _status = match result {
        Ok(_content) => {
            info!("such info");
        }
        Err(error) => {
            error!(
                "this is not the way, the writter failed to insert: {0}",
                error.to_string()
            );
        }
    };

    // pool
    // Keep track of all connected users, key is usize, value
    // is a websocket sender.
    let pools = Arc::new(Mutex::new(vec![]));
    // Turn our "state" into a new Filter...
    let pools = warp::any().map(move || pools.clone());

    // routes
    let ws = warp::path::param()
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(|key: String, ws: warp::ws::Ws| {
            // And then our closure will be called when it completes...
            info!("Hello {}!", key);
            ws.on_upgrade(|websocket| {
                // Just echo all messages back...
                let (tx, rx) = websocket.split();
                rx.forward(tx).map(|result| {
                    if let Err(e) = result {
                        eprintln!("websocket error: {:?}", e);
                    }
                })
            })
        });

    let clock = warp::path::param()
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .and(pools)
        .map(|key: String, ws: warp::ws::Ws, pools: Pools| {
            info!("Hello {}!", key);
            ws.on_upgrade(move |websocket| {
                ws_connected(websocket, pools).map(|result| result.unwrap())
            })
        });

    let rest = warp::path::param().map(move |key: String| {
        let result = t.get(key).unwrap();
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

    let keys = warp::any().map(|| format!("Hello!"));

    let routes = ws.or(clock).or(rest).or(keys);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
