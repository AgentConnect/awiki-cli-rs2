#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("__self-check") {
        match awiki_cli::build_info::client_version_info() {
            Ok(client) => println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "version": awiki_cli::build_info::VERSION,
                    "client_version": client.map(|value| value.header_value()),
                })
            ),
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    std::process::exit(awiki_cli::execute_async().await);
}
