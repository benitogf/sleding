// use base64::encode;
use hyper::body::HttpBody;
use hyper::{Body, Client, Request};
use tokio::time::delay_for;
use tokio::time::Duration;

#[tokio::test]
async fn start_server() {
  tokio::spawn(async move { sleding::run().await });
  delay_for(Duration::from_millis(600)).await;
}

#[tokio::test]
async fn test_route_get() {
  let client = Client::new();
  let uri = "http://localhost:3030/test".parse().unwrap();
  let mut resp = client.get(uri).await.unwrap();
  println!("Response test: {}", resp.status());
  assert_eq!(resp.status(), 200);
  while let Some(chunk) = resp.body_mut().data().await {
    println!("{:?}", &chunk.unwrap())
  }
}

#[tokio::test]
async fn test_route_post() {
  let client = Client::new();
  let uri: String = "http://localhost:3030/test".parse().unwrap();
  let req = Request::builder()
    .method("POST")
    .uri(uri)
    .body(Body::from("{\"data\": \"🛷\"}"))
    .expect("request builder");
  println!("test post");
  let mut resp = client.request(req).await.unwrap();
  println!("Response post test: {}", resp.status());
  assert_eq!(resp.status(), 201);
  while let Some(chunk) = resp.body_mut().data().await {
    println!("{:?}", &chunk.unwrap())
  }
}

#[tokio::test]
async fn root_route() {
  let client = Client::new();
  let uri = "http://localhost:3030".parse().unwrap();
  let mut resp = client.get(uri).await.unwrap();
  println!("Response root: {}", resp.status());
  assert_eq!(resp.status(), 200);
  while let Some(chunk) = resp.body_mut().data().await {
    println!("{:?}", &chunk.unwrap())
  }
}
