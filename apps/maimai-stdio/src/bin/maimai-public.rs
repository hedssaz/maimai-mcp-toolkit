use maimai_stdio::run_public;

#[tokio::main]
async fn main() -> Result<(), maimai_stdio::PublicProcessError> {
    run_public().await
}
