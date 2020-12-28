extern crate base64;
extern crate pretty_env_logger;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
  atomic::{AtomicUsize, Ordering},
  Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::delay_for;
use tokio::time::Duration;
use warp::ws::{Message, WebSocket};
use warp::{http, Filter};

#[macro_use]
extern crate log;

#[derive(Serialize, Deserialize, Debug)]
struct Object {
  created: u128,
  updated: u128,
  data: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WriteMessage {
  data: String,
}

// Global unique user id counter
static NEXT_CONNECTION_ID: AtomicUsize = AtomicUsize::new(1);

// A websocket connection sender
type Connection = mpsc::UnboundedSender<Result<Message, warp::Error>>;
// Vector of connections
type Connections = HashMap<usize, Connection>;
// Hash map key -> active connections
type Pool = HashMap<String, Connections>;
// Arc with lock of Pool
type Pools = Arc<tokio::sync::Mutex<Pool>>;
// Arc with lock for sled db
type Storage = Arc<Mutex<sled::Db>>;

fn now() -> u128 {
  let start = SystemTime::now();
  return start.duration_since(UNIX_EPOCH).unwrap().as_nanos();
}

async fn clock_writer(pools: Pools) {
  let key = "".to_string();
  let pkey = key.clone();

  loop {
    let mut mpools = pools.lock().await;
    let pool = mpools.get_mut(&pkey).unwrap();
    for (_index, connection) in pool.iter() {
      connection.send(Ok(Message::binary(now().to_string()))).ok();
    }
    drop(mpools);
    delay_for(Duration::from_millis(1000)).await;
  }
}

async fn clock_reader(ws: WebSocket, pools: Pools) {
  let key = "".to_string();
  let pkey = key.clone();
  let exit_key = key.clone();
  let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);

  info!("subscription to the clock");

  let (ws_tx, mut ws_rx) = ws.split();
  // Use an unbounded channel to handle buffering and flushing of messages
  // to the websocket...
  let (tx, rx) = mpsc::unbounded_channel();
  tokio::task::spawn(rx.forward(ws_tx));
  tx.send(Ok(Message::binary(now().to_string()))).ok();
  info!("clock handshake sent");

  // add connection to the corresponding pool
  let mut mpools = pools.lock().await;
  let pool = mpools.get_mut(&pkey).unwrap();
  pool.insert(connection_id, tx);
  info!("clock pool size {}", mpools.get(&pkey).unwrap().len());
  // drop the lock before entering the connection read block
  drop(mpools);

  while let Some(result) = ws_rx.next().await {
    let _msg = match result {
      Ok(msg) => msg,
      Err(e) => {
        warn!("websocket error: {}", e);
        break;
      }
    };
  }

  info!("leaving clock {:?}", connection_id);
  let mut pools = pools.lock().await;
  let pool = pools.get_mut(&exit_key).unwrap();
  pool.remove(&connection_id);
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
  let exit_key = key.clone();
  let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);

  info!("subscription to \"{}\"", key);
  let result = db.lock().unwrap().get(fkey).unwrap();
  if result.is_some() {
    let raw = &result.unwrap();
    let serialized = String::from_utf8_lossy(raw);
    data = serde_json::from_str(&serialized).unwrap();
  }

  let (ws_tx, mut ws_rx) = ws.split();
  // Use an unbounded channel to handle buffering and flushing of messages
  // to the websocket...
  let (tx, rx) = mpsc::unbounded_channel();
  tokio::task::spawn(rx.forward(ws_tx));

  let j = serde_json::to_string(&data);
  tx.send(Ok(Message::text(j.unwrap()))).ok();
  info!("handshake of \"{}\" sent", key);

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
  info!(
    "\"{}\" pool size {}",
    pkey,
    mpools.get(&pkey).unwrap().len()
  );
  // drop the lock before entering the connection read block
  drop(mpools);

  while let Some(result) = ws_rx.next().await {
    let _msg = match result {
      Ok(msg) => msg,
      Err(e) => {
        warn!("websocket error: {}", e);
        break;
      }
    };
  }

  info!("leaving: \"{}\" {:?}", exit_key, connection_id);
  let mut pools = pools.lock().await;
  let pool = pools.get_mut(&exit_key).unwrap();
  pool.remove(&connection_id);
}

fn storage() -> Storage {
  // Wrap the database as an arc(mutex)
  let db = sled::open("db").expect("open");
  return Arc::new(Mutex::new(db));
}

fn json_body() -> impl Filter<Extract = (WriteMessage,), Error = warp::Rejection> + Clone {
  // When accepting a body, we want a JSON body, (and to reject huge payloads)
  warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}

fn storage_read(db: Storage, key: String) -> Object {
  let result = db.lock().unwrap().get(key).unwrap();
  if !result.is_some() {
    return Object {
      created: 0,
      updated: 0,
      data: "".to_string(),
    };
  }
  let raw = &result.unwrap();
  let serialized = String::from_utf8_lossy(raw);
  let serialized: Object = serde_json::from_str(&serialized).unwrap();
  return serialized;
}

fn storage_write(db: Storage, key: String, msg: WriteMessage) -> Object {
  let pkey = key.clone();
  let _db = db.clone();
  let obj = storage_read(db, key);
  let mut created = obj.created;
  if created == 0 {
    created = now();
  }
  let mut updated = 0;
  if obj.created != 0 {
    updated = now();
  }
  let data = Object {
    created: created,
    updated: updated,
    data: msg.data,
  };
  let serialized = serde_json::to_string(&data).unwrap();
  _db
    .lock()
    .unwrap()
    .insert(pkey, serialized.as_bytes())
    .unwrap();
  return data;
}

async fn _write(
  key: String,
  msg: WriteMessage,
  db: Storage,
  pools: Pools,
) -> Result<impl warp::Reply, warp::Rejection> {
  info!("writing \"{}\"", key);
  let pkey = key.clone();
  let data = storage_write(db, key, msg);
  let pools = pools.lock().await;
  for (socket_key, connections) in pools.iter() {
    // TODO: glob pattern match
    if socket_key.to_string() != pkey {
      continue;
    }
    for (_index, connection) in connections.iter() {
      let j = serde_json::to_string(&data);
      connection.send(Ok(Message::text(j.unwrap()))).ok();
    }
  }

  Ok(warp::reply::with_status("{}", http::StatusCode::CREATED))
}

pub async fn run() {
  pretty_env_logger::init();
  // pool
  // Keep track of connections, key is usize, value
  // is a websocket sender.
  let pools: Pools = Arc::new(tokio::sync::Mutex::new(Pool::new()));
  // initialize the pools with the clock connections list
  let mut mpools = pools.lock().await;
  mpools.insert("".to_string(), Connections::default());
  drop(mpools);

  // https://users.rust-lang.org/t/why-closure-outlives-variables-owned-by-main/16725
  // even if the variable is Arc<Mutex> you need to clone
  let _db = storage();
  let _rest_read_db = _db.clone();
  let _rest_write_db = _db.clone();
  let _keys_db = _db.clone();
  let _rest_pools = pools.clone();
  let _subscription_pools = pools.clone();
  let _clock_pools = pools.clone();
  // db filters cloning
  let subscription_db = warp::any().map(move || _db.clone());
  let rest_read_db = warp::any().map(move || _rest_read_db.clone());
  let rest_write_db = warp::any().map(move || _rest_write_db.clone());
  let keys_db = warp::any().map(move || _keys_db.clone());
  // Turn our "state" into a new Filter...
  let subscription_pools = warp::any().map(move || _subscription_pools.clone());
  let rest_write_pools = warp::any().map(move || _rest_pools.clone());
  let clock_pools = warp::any().map(move || _clock_pools.clone());

  // routes
  let subscription = warp::path::param()
    .and(warp::ws())
    .and(subscription_pools)
    .and(subscription_db)
    .map(|key: String, ws: warp::ws::Ws, pools: Pools, db: Storage| {
      ws.on_upgrade(move |websocket| ws_reader(websocket, key, pools, db))
    });

  let clock = warp::ws()
    .and(clock_pools)
    .map(|ws: warp::ws::Ws, pools: Pools| {
      ws.on_upgrade(move |websocket| clock_reader(websocket, pools))
    });

  let read =
    warp::get()
      .and(warp::path::param())
      .and(rest_read_db)
      .map(|key: String, db: Storage| {
        let obj = storage_read(db, key);
        warp::reply::json(&obj)
      });

  let write = warp::post()
    .and(warp::path::param())
    .and(json_body())
    .and(rest_write_db)
    .and(rest_write_pools)
    .and_then(_write);

  let keys = warp::get().and(keys_db).map(|db: Storage| {
    // iterating over keys https://github.com/spacejam/sled/blob/master/tests/test_tree.rs#L306
    let mut result = db.lock().unwrap().iter().keys();
    let mut keys: Vec<String> = vec![];
    info!("iterating through keys");
    loop {
      let _k = match result.next() {
        Some(_content) => {
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

  let routes = subscription.or(clock).or(write).or(read).or(keys);

  tokio::spawn(async { clock_writer(pools).await });
  warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
