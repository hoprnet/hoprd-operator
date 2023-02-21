pub mod crd;
mod hoprd;
mod servicemonitor;
mod operator;
mod actions;
mod utils;
mod finalizer;
mod constants;

#[tokio::main]
async fn main() {    
    operator::run_operator().await
}

