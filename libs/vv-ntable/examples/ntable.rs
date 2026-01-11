use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let mut client = vv_ntable::NtClient::new("tester".to_string());
    let publisher = client.handle().publish("/counter");
    let counter_loop = async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        let mut count = 0;
        loop {
            timer.tick().await;
            publisher.set(&count);
            count += 1;
        }
    };
    tokio::select! {
        _ = counter_loop => (),
        res = client.connect("localhost", false) => res.unwrap()
    }
}
