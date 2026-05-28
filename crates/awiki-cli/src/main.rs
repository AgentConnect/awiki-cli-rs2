#[tokio::main]
async fn main() {
    std::process::exit(awiki_cli::execute_async().await);
}
