#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "info");
    // std::env::set_var("RUST_BACKTRACE", "1");
    sleding::run().await;
}
