use hyper::body::HttpBody;
use hyper::Client;
use tokio::time::delay_for;
use tokio::time::Duration;

#[tokio::test]
async fn start_server() {
  tokio::spawn(async move { sleding::run().await });
  delay_for(Duration::from_millis(600)).await;
}

#[tokio::test]
async fn test_route() {
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
