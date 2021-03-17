// use base64::encode;
use hyper::body::HttpBody;
use hyper::{Body, Client, Request};
use tokio::time::sleep;
use tokio::time::Duration;
#[macro_use]

extern crate log;

async fn start_server() {
  tokio::spawn(async move { sleding::run().await });
  sleep(Duration::from_millis(300)).await;
}
#[tokio::test]
async fn keys() {
  
  
}

#[tokio::test]
async fn root_route() {
  std::env::set_var("RUST_LOG", "info");
  std::env::set_var("RUST_BACKTRACE", "1");
  start_server().await;
  let client = Client::new();

  // test post
  let uri: String = "http://127.0.0.1:3030/test".parse().unwrap();
  let req = Request::builder()
    .method("POST")
    .uri(uri)
    .body(Body::from("{\"data\": \"🛷\"}"))
    .expect("request builder");
  info!("test post");
  let mut resp = client.request(req).await.unwrap();
  info!("Response post test: {}", resp.status());
  assert_eq!(resp.status(), 200);
  while let Some(chunk) = resp.body_mut().data().await {
    info!("{:?}", &chunk.unwrap())
  }

  // test get
  let uri = "http://127.0.0.1:3030/test".parse().unwrap();
  let mut resp = client.get(uri).await.unwrap();
  info!("Response test: {}", resp.status());
  assert_eq!(resp.status(), 200);
  while let Some(chunk) = resp.body_mut().data().await {
    info!("{:?}", &chunk.unwrap())
  }

  // test invalid get
  let uri = "http://127.0.0.1:3030/test//one".parse().unwrap();
  let mut resp = client.get(uri).await.unwrap();
  info!("Response test: {}", resp.status());
  assert_eq!(resp.status(), 400);
  while let Some(chunk) = resp.body_mut().data().await {
    info!("{:?}", &chunk.unwrap())
  }

  // test keys
  let uri = "http://127.0.0.1:3030".parse().unwrap();
  let mut resp = client.get(uri).await.unwrap();
  info!("Response root: {}", resp.status());
  assert_eq!(resp.status(), 200);
  while let Some(chunk) = resp.body_mut().data().await {
    info!("{:?}", &chunk.unwrap())
  }
}
